//! Bounded multi-level fitting against exact, cursor-aware fill constraints.
//!
//! The reference for precision is the minimum Pool output over every reachable
//! state for the same direction/cursor/input, NOT the current snapshot quote.
//! Each candidate and final book is replayed with exact per-tranche floors.
//! A heuristic target miss is reported explicitly; it does not prove global
//! infeasibility. Fitting-budget exhaustion returns no partial certificate.
//! Fee-accounting, reserve reconciliation and standard-token preconditions are
//! identical to [`crate::try_build_validated_order_book`].
//! Coverage applies to these exact raw levels. Executor packing, quantization,
//! boundary rounding or level merging requires a separate exact revalidation;
//! removing a tranche floor can increase output even when prices are lower.

use core::fmt;
use std::collections::BTreeSet;

use crate::order_book_policy::{explore_fill_policy, FillConstraint};
use crate::{
    try_build_order_book, try_quote_x_to_y_with_multiplier, try_quote_y_to_x_with_multiplier,
    Direction, DirectionalLadder, FillPolicyError, OrderBookConfig, OrderBookError, OrderBookLevel,
    OrderBookSafety, OrderBookState, OrderBookStatus, U256Ext, ValidatedOrderBook,
    ValidatedOrderBookConfig, MAX_ORDER_BOOK_LEVELS, MAX_VALIDATION_TRANSITIONS, PRICE_SCALE_X18,
    U256,
};

/// Hard ceiling on fitting/replay work, independent of state-graph transitions.
pub const MAX_PRECISION_WORK: usize = 5_000_000;

/// Accuracy and work policy for a multi-level book.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderBookPrecision {
    /// Maximum levels PER direction, in 1..=20. The result may use fewer.
    pub max_levels: usize,
    /// Maximum ceil-rounded relative output shortfall versus the minimum
    /// reachable output for each cursor/input; in 0..=10_000 basis points.
    pub target_underquote_bps: u32,
    /// Separate fitting/replay budget in 1..=MAX_PRECISION_WORK. One work unit
    /// covers a constraint record or a tranche evaluation; every replay counts.
    pub max_work: usize,
}

/// Safe fitted book and measured precision. Publishing must additionally require
/// `target_met`, active status, and the documented accounting/state preconditions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreciseOrderBook {
    /// Same finite state-domain certificate as the flat builder, with fitted levels.
    pub validated: ValidatedOrderBook,
    /// Requested accuracy/work policy.
    pub precision: OrderBookPrecision,
    /// False on a heuristic target miss, inactive snapshot, or empty domain.
    pub target_met: bool,
    /// Worst ceil-rounded relative shortfall versus minimum reachable output.
    pub worst_underquote_bps: u32,
    /// Separately measured worst discount versus quoting the same input at the
    /// original snapshot, ignoring its lifetime cursor. Negative gaps count as zero.
    pub worst_fresh_snapshot_discount_bps: u32,
    /// Actual fitting/replay work, excluding independently bounded graph traversal.
    pub work_used: usize,
    /// Distinct direction/cursor/input constraints after taking minima across states.
    pub constraint_count: usize,
}

/// Invalid request, unavailable model, or bounded fitting failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreciseOrderBookError {
    /// Underlying finite-policy state exploration failure.
    Policy(FillPolicyError),
    /// Precision fields are outside their documented integer ranges.
    InvalidPrecision,
    /// Work exhausted; no partial fitted certificate is returned.
    WorkBudgetExceeded,
    /// Checked arithmetic or final ladder validation failed.
    Arithmetic(OrderBookError),
    /// Internal candidate violated an exact output constraint; fail closed.
    UnsafeCandidate,
}

impl fmt::Display for PreciseOrderBookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => error.fmt(f),
            Self::InvalidPrecision => f.write_str("precision requires max_levels in 1..=20, target_underquote_bps in 0..=10000, max_work in 1..=5000000"),
            Self::WorkBudgetExceeded => f.write_str("precision fitting/replay work budget exceeded; no partial certificate"),
            Self::Arithmetic(error) => error.fmt(f),
            Self::UnsafeCandidate => f.write_str("fitted candidate exceeds an exact cursor-aware output constraint"),
        }
    }
}

impl std::error::Error for PreciseOrderBookError {}
impl From<FillPolicyError> for PreciseOrderBookError {
    fn from(error: FillPolicyError) -> Self {
        Self::Policy(error)
    }
}
impl From<OrderBookError> for PreciseOrderBookError {
    fn from(error: OrderBookError) -> Self {
        Self::Arithmetic(error)
    }
}

struct Work {
    used: usize,
    limit: usize,
}
impl Work {
    fn charge(&mut self, units: usize) -> Result<(), PreciseOrderBookError> {
        if units > self.limit - self.used {
            return Err(PreciseOrderBookError::WorkBudgetExceeded);
        }
        self.used += units;
        Ok(())
    }
}

fn overlap(constraint: &FillConstraint, start: U256, end: U256) -> U256 {
    let from = start.max(constraint.cursor);
    let to = end.min(constraint.cursor + constraint.amount_in);
    to.saturating_sub(from)
}

fn sweep(
    levels: &[OrderBookLevel],
    constraint: &FillConstraint,
    skip: Option<usize>,
    work: &mut Work,
) -> Result<U256, PreciseOrderBookError> {
    let mut output = U256::ZERO;
    let mut start = U256::ZERO;
    for (index, level) in levels.iter().enumerate() {
        work.charge(1)?;
        if skip != Some(index) {
            let input = overlap(constraint, start, level.size);
            let tranche = U256::checked_mul_div(input, level.price, PRICE_SCALE_X18)
                .ok_or(OrderBookError::PriceOverflow)?;
            output = output
                .checked_add(tranche)
                .ok_or(OrderBookError::OutputOverflow)?;
        }
        start = level.size;
    }
    Ok(output)
}

// Raise each marginal price in top-to-bottom order. Other prices remain fixed,
// so every edge gives an EXACT cap: floor(overlap*p/S) <= q - other_output.
// This is equivalent to p <= floor(((q-other_output+1)*S-1)/overlap).
// It avoids a 256-iteration price search and preserves every floor inequality.
fn fit(
    levels: &mut [OrderBookLevel],
    direction: usize,
    constraints: &[FillConstraint],
    work: &mut Work,
) -> Result<(), PreciseOrderBookError> {
    for index in 0..levels.len() {
        let start = if index == 0 {
            U256::ZERO
        } else {
            levels[index - 1].size
        };
        let mut ceiling = if index == 0 {
            U256::MAX
        } else {
            levels[index - 1].price
        };
        for constraint in constraints {
            work.charge(1)?;
            if constraint.direction_index != direction {
                continue;
            }
            let input = overlap(constraint, start, levels[index].size);
            if input.is_zero() {
                continue;
            }
            let other = sweep(levels, constraint, Some(index), work)?;
            let residual = constraint
                .amount_out
                .checked_sub(other)
                .ok_or(PreciseOrderBookError::UnsafeCandidate)?;
            let numerator = residual
                .checked_add(U256::from(1))
                .and_then(|value| value.checked_mul(PRICE_SCALE_X18))
                .and_then(|value| value.checked_sub(U256::from(1)))
                .ok_or(OrderBookError::PriceOverflow)?;
            ceiling = ceiling.min(numerator / input);
        }
        if ceiling < levels[index].price {
            return Err(PreciseOrderBookError::UnsafeCandidate);
        }
        levels[index].price = ceiling;
    }
    Ok(())
}

fn shortfall_bps(reference: U256, output: U256) -> u32 {
    if output >= reference || reference.is_zero() {
        return 0;
    }
    // Both are Pool outputs bounded by uint112. Product plus denominator fits
    // uint256; preserve full precision instead of converting through floats.
    let numerator = (reference - output) * U256::from(10_000) + reference - U256::from(1);
    (numerator / reference).to::<u32>()
}

struct Metrics {
    worst: u32,
    worst_relative_gap: U256,
    sum_relative_gap: U256,
    worst_indices: [Option<usize>; 2],
    positive: bool,
    first_zero_direction: Option<usize>,
}
fn measure(
    ladders: &[Vec<OrderBookLevel>; 2],
    constraints: &[FillConstraint],
    work: &mut Work,
) -> Result<Metrics, PreciseOrderBookError> {
    let mut result = Metrics {
        worst: 0,
        worst_relative_gap: U256::ZERO,
        sum_relative_gap: U256::ZERO,
        worst_indices: [None; 2],
        positive: true,
        first_zero_direction: None,
    };
    let mut directional_worst = [U256::ZERO; 2];
    for (index, constraint) in constraints.iter().enumerate() {
        work.charge(1)?;
        let output = sweep(&ladders[constraint.direction_index], constraint, None, work)?;
        if output > constraint.amount_out {
            return Err(PreciseOrderBookError::UnsafeCandidate);
        }
        result.positive &= !output.is_zero();
        if output.is_zero() && result.first_zero_direction.is_none() {
            result.first_zero_direction = Some(constraint.direction_index);
        }
        let bps = shortfall_bps(constraint.amount_out, output);
        result.worst = result.worst.max(bps);
        // Compare dimensionless errors at 1e-18 resolution, not raw token units
        // or coarse bps. Otherwise 18-decimal sides dominate 6-decimal sides.
        let gap = constraint.amount_out - output;
        let relative =
            (gap * PRICE_SCALE_X18 + constraint.amount_out - U256::from(1)) / constraint.amount_out;
        result.worst_relative_gap = result.worst_relative_gap.max(relative);
        result.sum_relative_gap = result
            .sum_relative_gap
            .checked_add(relative)
            .ok_or(OrderBookError::OutputOverflow)?;
        if result.worst_indices[constraint.direction_index].is_none()
            || relative > directional_worst[constraint.direction_index]
        {
            directional_worst[constraint.direction_index] = relative;
            result.worst_indices[constraint.direction_index] = Some(index);
        }
    }
    Ok(result)
}

fn improves(candidate: &Metrics, incumbent: &Metrics) -> bool {
    (candidate.worst_relative_gap, candidate.sum_relative_gap)
        < (incumbent.worst_relative_gap, incumbent.sum_relative_gap)
}

/// Fit up to 20 distinct-depth levels per direction while preserving every exact
/// reachable fill constraint. Adaptive lot-aligned splits prioritize the current
/// worst-error fill and geometric/midpoint splits; accepted candidates strictly
/// improve worst relative error, or summed relative error when the worst ties.
/// Fitting uses dimensionless ceil-rounded 1e-18 errors, avoiding token-decimal
/// bias; public precision metrics remain conservative integer basis points.
/// No returned refinement has worse worst-error than the fitted one-level book.
///
/// `target_met == false` means this bounded heuristic did not meet the requested
/// precision. It is not a proof that no possible ladder could meet the target.
/// Production publishers must gate on it. Budget exhaustion instead returns an
/// error and never a partial result. Bounds are conditional on the same initial
/// balance/accounting/token preconditions as the flat finite-policy builder.
pub fn try_build_precise_order_book(
    state: &OrderBookState,
    config: &ValidatedOrderBookConfig,
    precision: &OrderBookPrecision,
) -> Result<PreciseOrderBook, PreciseOrderBookError> {
    if precision.max_levels == 0
        || precision.max_levels > MAX_ORDER_BOOK_LEVELS
        || precision.target_underquote_bps > 10_000
        || precision.max_work == 0
        || precision.max_work > MAX_PRECISION_WORK
    {
        return Err(PreciseOrderBookError::InvalidPrecision);
    }
    if config.max_transitions == 0 || config.max_transitions > MAX_VALIDATION_TRANSITIONS {
        return Err(FillPolicyError::InvalidBudget.into());
    }
    let policies = [config.x_to_y, config.y_to_x];
    for policy in policies.iter().flatten() {
        policy.validate()?;
    }
    let mut book = try_build_order_book(
        state,
        &OrderBookConfig {
            x_to_y_sizes: &[],
            y_to_x_sizes: &[],
        },
    )?;
    let mut result = PreciseOrderBook {
        validated: ValidatedOrderBook {
            book: book.clone(),
            state: *state,
            config: *config,
            checked_states: 0,
            checked_transitions: 0,
        },
        precision: *precision,
        target_met: false,
        worst_underquote_bps: 0,
        worst_fresh_snapshot_discount_bps: 0,
        work_used: 0,
        constraint_count: 0,
    };
    if book.status != OrderBookStatus::Active {
        return Ok(result);
    }
    let exploration = explore_fill_policy(state, config)?;
    let constraints = exploration.constraints;
    let mut work = Work {
        used: 0,
        limit: precision.max_work,
    };
    let mut prices = [U256::MAX; 2];
    for constraint in &constraints {
        work.charge(1)?;
        let price =
            U256::checked_mul_div(constraint.amount_out, PRICE_SCALE_X18, constraint.amount_in)
                .ok_or(OrderBookError::PriceOverflow)?;
        prices[constraint.direction_index] = prices[constraint.direction_index].min(price);
    }
    let mut ladders: [Vec<OrderBookLevel>; 2] = core::array::from_fn(|index| {
        policies[index].map_or_else(Vec::new, |policy| {
            vec![OrderBookLevel {
                size: policy.total_input,
                price: prices[index],
            }]
        })
    });
    for (index, levels) in ladders.iter_mut().enumerate() {
        fit(levels, index, &constraints, &mut work)?;
    }
    let mut metrics = measure(&ladders, &constraints, &mut work)?;

    while metrics.worst > precision.target_underquote_bps || !metrics.positive {
        let mut candidates = BTreeSet::new();
        for direction in 0..2 {
            let Some(policy) = policies[direction] else {
                continue;
            };
            if ladders[direction].len() >= precision.max_levels {
                continue;
            }
            let mut points = Vec::new();
            if let Some(index) = metrics.worst_indices[direction] {
                let edge = &constraints[index];
                points.extend([edge.cursor, edge.cursor + edge.amount_in]);
            }
            // Reference-style geometric cuts plus current segment midpoints.
            for shift in 1..=3 {
                points.push(policy.total_input >> shift);
            }
            let mut start = U256::ZERO;
            for level in &ladders[direction] {
                points.push(start + (level.size - start) / U256::from(2));
                start = level.size;
            }
            for point in points {
                work.charge(1)?;
                let aligned = point / policy.lot_input * policy.lot_input;
                if !aligned.is_zero()
                    && aligned < policy.total_input
                    && !ladders[direction].iter().any(|level| level.size == aligned)
                {
                    candidates.insert((direction, aligned));
                }
            }
        }
        let mut best: Option<([Vec<OrderBookLevel>; 2], Metrics)> = None;
        for (direction, point) in candidates {
            work.charge(1)?;
            let mut proposal = ladders.clone();
            let index = proposal[direction]
                .iter()
                .position(|level| level.size > point)
                .expect("interior split");
            let price = proposal[direction][index].price;
            proposal[direction].insert(index, OrderBookLevel { size: point, price });
            // Splitting equal prices can only LOWER the tranche sum initially.
            fit(&mut proposal[direction], direction, &constraints, &mut work)?;
            let proposed_metrics = measure(&proposal, &constraints, &mut work)?;
            let incumbent_metrics = best.as_ref().map_or(&metrics, |(_, metrics)| metrics);
            if improves(&proposed_metrics, incumbent_metrics) {
                best = Some((proposal, proposed_metrics));
            }
        }
        let Some((next, next_metrics)) = best else {
            break;
        };
        ladders = next;
        metrics = next_metrics;
    }

    // Final replay is deliberately separate and charged, even if a candidate
    // was already measured. Never merge equal-price levels afterwards: doing
    // so can remove a floor and increase output.
    metrics = measure(&ladders, &constraints, &mut work)?;
    for (index, levels) in ladders.iter().enumerate() {
        if levels.iter().any(|level| level.price.is_zero()) {
            return Err(FillPolicyError::ZeroPrice {
                direction: if index == 0 {
                    Direction::XToY
                } else {
                    Direction::YToX
                },
            }
            .into());
        }
        if levels.windows(2).any(|pair| pair[0].price < pair[1].price) {
            return Err(PreciseOrderBookError::UnsafeCandidate);
        }
    }
    if !metrics.positive {
        return Err(FillPolicyError::ZeroPrice {
            direction: if metrics.first_zero_direction == Some(1) {
                Direction::YToX
            } else {
                Direction::XToY
            },
        }
        .into());
    }
    for constraint in &constraints {
        work.charge(1)?;
        let fresh = if constraint.direction_index == 0 {
            try_quote_x_to_y_with_multiplier(
                &state.pool,
                constraint.amount_in,
                state.fee_multiplier,
            )
        } else {
            try_quote_y_to_x_with_multiplier(
                &state.pool,
                constraint.amount_in,
                state.fee_multiplier,
            )
        }
        .map_err(|error| PreciseOrderBookError::Arithmetic(OrderBookError::Math(error)))?;
        let output = sweep(
            &ladders[constraint.direction_index],
            constraint,
            None,
            &mut work,
        )?;
        result.worst_fresh_snapshot_discount_bps = result
            .worst_fresh_snapshot_discount_bps
            .max(shortfall_bps(fresh.amount_out, output));
    }
    let [x_to_y, y_to_x] = ladders;
    book.x_to_y = DirectionalLadder {
        levels: x_to_y,
        truncated: false,
    };
    book.y_to_x = DirectionalLadder {
        levels: y_to_x,
        truncated: false,
    };
    book.safety = OrderBookSafety::ExhaustiveLotPolicy;
    result.validated = ValidatedOrderBook {
        book,
        state: *state,
        config: *config,
        checked_states: exploration.checked_states,
        checked_transitions: exploration.checked_transitions,
    };
    result.target_met = !constraints.is_empty() && metrics.worst <= precision.target_underquote_bps;
    result.worst_underquote_bps = metrics.worst;
    result.work_used = work.used;
    result.constraint_count = constraints.len();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        try_build_validated_order_book, try_ladder_amount_out_at_cursor,
        try_simulate_successful_swap, FillPolicy, PoolParams, SimulationStatus, Q24, Q96,
    };

    fn snapshot() -> OrderBookState {
        OrderBookState {
            pool: PoolParams {
                sqrt_price_x96: Q96,
                fee_ask_x24: 0,
                fee_bid_x24: 0,
                reserve_x: 1_000_000,
                reserve_y: 1_000_000,
                max_punishment_x24: Q24 / 2,
            },
            fee_multiplier: U256::from(1),
            snapshot_block: 100,
            max_execution_block: 101,
            latest_update_block: 100,
            block_delay: 3,
            paused: false,
        }
    }

    fn policy(lot: u128, max: u128, total: u128) -> FillPolicy {
        FillPolicy {
            min_input: U256::from(lot),
            lot_input: U256::from(lot),
            max_input: U256::from(lot * max),
            total_input: U256::from(lot * total),
        }
    }

    fn precision(levels: usize, target: u32) -> OrderBookPrecision {
        OrderBookPrecision {
            max_levels: levels,
            target_underquote_bps: target,
            max_work: MAX_PRECISION_WORK,
        }
    }

    // Independently visit every sequence without sharing the implementation's
    // state/key deduplication, fitting, sweep or metric code.
    fn replay(result: &PreciseOrderBook, pool: PoolParams, cursors: [U256; 2]) -> usize {
        let mut count = 0;
        for (index, direction, policy, ladder) in [
            (
                0,
                Direction::XToY,
                result.validated.config.x_to_y,
                &result.validated.book.x_to_y,
            ),
            (
                1,
                Direction::YToX,
                result.validated.config.y_to_x,
                &result.validated.book.y_to_x,
            ),
        ] {
            let Some(policy) = policy else {
                continue;
            };
            let mut amount = policy.min_input;
            while amount <= policy.max_input && amount <= policy.total_input - cursors[index] {
                let actual = try_simulate_successful_swap(
                    &pool,
                    amount,
                    direction,
                    result.validated.state.fee_multiplier,
                )
                .unwrap();
                assert_eq!(actual.status, SimulationStatus::Applied);
                let promised = try_ladder_amount_out_at_cursor(ladder, cursors[index], amount)
                    .unwrap()
                    .unwrap();
                assert!(promised > U256::ZERO);
                assert!(
                    promised <= actual.quote.amount_out,
                    "{direction:?} cursor={} amount={amount}: {promised} > {}",
                    cursors[index],
                    actual.quote.amount_out
                );
                let mut next = cursors;
                next[index] += amount;
                count += 1 + replay(result, actual.post_swap, next);
                amount += policy.lot_input;
            }
        }
        count
    }

    #[test]
    fn real_distinct_levels_reduce_error_instead_of_repeating_flat_prices() {
        let state = snapshot();
        let config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(50_000, 1, 4)),
            y_to_x: None,
            max_transitions: 1000,
        };
        let flat = try_build_precise_order_book(&state, &config, &precision(1, 0)).unwrap();
        let fitted = try_build_precise_order_book(&state, &config, &precision(4, 0)).unwrap();
        assert!(flat.worst_underquote_bps > 0);
        assert!(fitted.target_met);
        assert_eq!(fitted.worst_underquote_bps, 0);
        assert!(fitted.worst_fresh_snapshot_discount_bps > 0);
        assert!(fitted
            .validated
            .book
            .x_to_y
            .levels
            .windows(2)
            .any(|pair| pair[0].price != pair[1].price));
        assert_eq!(fitted.validated.book.x_to_y.levels.len(), 4);
        assert!(fitted.work_used <= fitted.precision.max_work);
        assert!(replay(&fitted, state.pool, [U256::ZERO; 2]) > 0);
    }

    #[test]
    fn mixed_partial_sequences_are_safe_and_refinement_never_worsens_flat_error() {
        for seed in 1u32..=6 {
            let mut state = snapshot();
            state.pool.sqrt_price_x96 = Q96 * U256::from(8 + seed) / U256::from(10);
            state.pool.max_punishment_x24 = Q24 / (2 + seed);
            let config = ValidatedOrderBookConfig {
                x_to_y: Some(policy(1000, 2, 3)),
                y_to_x: Some(policy(1000, 2, 3)),
                max_transitions: 50_000,
            };
            let flat = try_build_precise_order_book(&state, &config, &precision(1, 0)).unwrap();
            for levels in [2, 3, 20] {
                let fitted =
                    try_build_precise_order_book(&state, &config, &precision(levels, 0)).unwrap();
                assert!(fitted.worst_underquote_bps <= flat.worst_underquote_bps);
                for ladder in [&fitted.validated.book.x_to_y, &fitted.validated.book.y_to_x] {
                    assert!(ladder.levels.len() <= levels);
                    assert!(ladder
                        .levels
                        .windows(2)
                        .all(|pair| pair[0].size < pair[1].size && pair[0].price >= pair[1].price));
                }
                assert!(
                    replay(&fitted, state.pool, [U256::ZERO; 2])
                        >= fitted.validated.checked_transitions
                );
            }
        }
    }

    #[test]
    fn liquid_mixed_decimals_fixture_meets_one_basis_point() {
        let mut state = snapshot();
        state.pool.sqrt_price_x96 = Q96 / U256::from(1_000_000);
        state.pool.reserve_x = 100_000_000_000_000_000_000;
        state.pool.reserve_y = 100_000_000;
        state.pool.fee_ask_x24 = Q24 / 10_000;
        state.pool.fee_bid_x24 = Q24 / 20_000;
        state.pool.max_punishment_x24 = Q24 / 100;
        let config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(1_000_000_000_000_000_000, 2, 4)),
            y_to_x: Some(policy(1_000_000, 2, 4)),
            max_transitions: 100_000,
        };
        let flat = try_build_precise_order_book(&state, &config, &precision(1, 1)).unwrap();
        let fitted = try_build_precise_order_book(&state, &config, &precision(20, 1)).unwrap();
        eprintln!("mixed fixture: flat={}bps precise={}bps levels={}/{} work={} states={} edges={} constraints={}", flat.worst_underquote_bps, fitted.worst_underquote_bps,
            fitted.validated.book.x_to_y.levels.len(), fitted.validated.book.y_to_x.levels.len(), fitted.work_used,
            fitted.validated.checked_states, fitted.validated.checked_transitions, fitted.constraint_count);
        assert!(fitted.target_met);
        assert!(fitted.worst_underquote_bps < flat.worst_underquote_bps);
        for ladder in [&fitted.validated.book.x_to_y, &fitted.validated.book.y_to_x] {
            assert!(ladder
                .levels
                .windows(2)
                .any(|pair| pair[0].price != pair[1].price));
        }
        replay(&fitted, state.pool, [U256::ZERO; 2]);
    }

    #[test]
    fn tiny_partial_counterexample_is_safe_but_precision_target_is_not_promised() {
        let mut state = snapshot();
        state.pool.sqrt_price_x96 = Q96 * U256::from(3) / U256::from(2);
        state.pool.max_punishment_x24 = 0;
        let config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(1, 2, 2)),
            y_to_x: None,
            max_transitions: 100,
        };
        let fitted = try_build_precise_order_book(&state, &config, &precision(20, 0)).unwrap();
        assert!(!fitted.target_met);
        assert!(fitted.worst_underquote_bps > 0);
        assert_eq!(
            try_ladder_amount_out_at_cursor(
                &fitted.validated.book.x_to_y,
                U256::ZERO,
                U256::from(1)
            )
            .unwrap(),
            Some(U256::from(1))
        );
        replay(&fitted, state.pool, [U256::ZERO; 2]);
    }

    #[test]
    fn exact_floor_slack_can_recover_positive_output_without_changing_flat_api() {
        let mut state = snapshot();
        state.pool.max_punishment_x24 = 0;
        state.pool.fee_bid_x24 = Q24 * 3 / 4;
        let config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(3, 1, 1)),
            y_to_x: None,
            max_transitions: 10,
        };
        assert!(matches!(
            try_build_validated_order_book(&state, &config),
            Err(FillPolicyError::ZeroPrice { .. })
        ));
        let fitted = try_build_precise_order_book(&state, &config, &precision(20, 0)).unwrap();
        assert!(fitted.target_met);
        assert_eq!(
            try_ladder_amount_out_at_cursor(
                &fitted.validated.book.x_to_y,
                U256::ZERO,
                U256::from(3)
            )
            .unwrap(),
            Some(U256::from(1))
        );
    }

    #[test]
    fn budgets_and_inactive_states_never_return_a_partial_or_false_certificate() {
        let mut state = snapshot();
        let config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(1000, 2, 4)),
            y_to_x: None,
            max_transitions: 1000,
        };
        let invalid = OrderBookPrecision {
            max_levels: 21,
            ..precision(20, 0)
        };
        assert_eq!(
            try_build_precise_order_book(&state, &config, &invalid),
            Err(PreciseOrderBookError::InvalidPrecision)
        );
        let limited = OrderBookPrecision {
            max_work: 1,
            ..precision(20, 0)
        };
        assert_eq!(
            try_build_precise_order_book(&state, &config, &limited),
            Err(PreciseOrderBookError::WorkBudgetExceeded)
        );
        let mut limited_graph = config;
        limited_graph.max_transitions = 1;
        assert_eq!(
            try_build_precise_order_book(&state, &limited_graph, &precision(20, 0)),
            Err(PreciseOrderBookError::Policy(
                FillPolicyError::BudgetExceeded
            ))
        );
        state.paused = true;
        let paused = try_build_precise_order_book(&state, &config, &precision(20, 0)).unwrap();
        assert!(!paused.target_met);
        assert_eq!(paused.validated.book.safety, OrderBookSafety::Indicative);
        assert_eq!(paused.work_used, 0);
        assert_eq!(paused.constraint_count, 0);
        state.paused = false;
        let empty = ValidatedOrderBookConfig {
            x_to_y: None,
            y_to_x: None,
            max_transitions: 100,
        };
        assert!(
            !try_build_precise_order_book(&state, &empty, &precision(20, 0))
                .unwrap()
                .target_met
        );
    }
}
