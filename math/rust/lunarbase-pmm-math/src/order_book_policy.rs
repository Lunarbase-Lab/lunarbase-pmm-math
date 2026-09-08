//! Exhaustively bounded lot-policy books, including mixed-direction sequences.
//!
//! Each enabled side is a single flat-price level. Its price is the minimum
//! floor-rounded output/input ratio over EVERY reachable allowed fill from the
//! supplied snapshot, within the Pool quote/reserve/punishment model.
//! Thus all allowed cursor fills are covered without assuming integer concavity.
//! This is a finite-model guarantee: the adapter must enforce the policy, both
//! published directional caps must share this generation, and no unmodeled Pool
//! swap, update, deposit, withdrawal or parameter change may intervene. Always
//! retain the adapter's exact `amountOutMinimum` guard for changes after signing.
//! The model also assumes standard ERC20 behavior, fully credited fees
//! (`partner_fee == 0` or a configured partner operator), and sufficient global
//! treasury/partner and per-router uint112 fee-bucket headroom. Call
//! [`crate::try_validate_fee_accounting_capacity`] on the same cached snapshot.
//! Initial active reserves must also reconcile with token balances minus pending
//! deposit escrow and global fee buckets: a preexisting unsynced token donation
//! can change the next `Sync` despite unchanged stored reserves. This library
//! does not fetch or certify that raw-balance partition.
//! Neither check is a guarantee of arbitrary token calls or full EVM execution.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    try_build_order_book, try_simulate_successful_swap, Direction, DirectionalLadder, MathError,
    OrderBook, OrderBookConfig, OrderBookError, OrderBookLevel, OrderBookSafety, OrderBookState,
    OrderBookStatus, PoolParams, SimulationStatus, U256Ext, PRICE_SCALE_X18, U256,
};

/// Hard limit on explored transitions, bounding both CPU work and stored states.
pub const MAX_VALIDATION_TRANSITIONS: usize = 100_000;

/// On-chain-enforced raw token-input constraints for one direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FillPolicy {
    /// Smallest allowed fill; a positive multiple of `lot_input`.
    pub min_input: U256,
    /// Every fill must be divisible by this positive lot.
    pub lot_input: U256,
    /// Largest allowed individual fill; a multiple of `lot_input`.
    pub max_input: U256,
    /// Cumulative generation depth; a multiple of `lot_input`.
    pub total_input: U256,
}

impl FillPolicy {
    /// Reject malformed or unreachable bounds before enumerating fills.
    pub fn validate(&self) -> Result<(), FillPolicyError> {
        if self.lot_input.is_zero()
            || self.min_input.is_zero()
            || self.min_input > self.max_input
            || self.max_input > self.total_input
            || self.min_input % self.lot_input != U256::ZERO
            || self.max_input % self.lot_input != U256::ZERO
            || self.total_input % self.lot_input != U256::ZERO
        {
            return Err(FillPolicyError::InvalidPolicy);
        }
        Ok(())
    }

    /// Whether an individual fill satisfies the policy. The consumer must also
    /// ensure `cursor + amount_in <= total_input` for its signed generation.
    #[must_use]
    pub fn allows(&self, amount_in: U256) -> bool {
        self.validate().is_ok()
            && amount_in >= self.min_input
            && amount_in <= self.max_input
            && amount_in % self.lot_input == U256::ZERO
    }
}

/// Finite domain to certify. `None` disables one direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedOrderBookConfig {
    /// X-input constraints, enforced by the X→Y adapter route.
    pub x_to_y: Option<FillPolicy>,
    /// Y-input constraints, enforced by the Y→X adapter route.
    pub y_to_x: Option<FillPolicy>,
    /// Caller-selected work budget in `1..=MAX_VALIDATION_TRANSITIONS`.
    /// Exhaustion returns an error, never a partially certified book.
    pub max_transitions: usize,
}

/// Book together with the complete input state, policy and coverage counters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedOrderBook {
    /// Directional books; active results certify the finite price/reserve/
    /// punishment model, conditional on fee accounting and token behavior.
    /// The flat builder emits one level; the precision builder may emit more.
    pub book: OrderBook,
    /// Exact state from which the guarantee begins.
    pub state: OrderBookState,
    /// Policy and caps that must match the on-chain adapter and signed ladders.
    pub config: ValidatedOrderBookConfig,
    /// Distinct Pool-state/cursor pairs examined, including the initial state.
    pub checked_states: usize,
    /// Every permitted outgoing fill is counted, including deduplicated states.
    pub checked_transitions: usize,
}

/// Fail-closed validation or exhaustive-pricing failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillPolicyError {
    /// Snapshot or checked order-book arithmetic is invalid.
    OrderBook(OrderBookError),
    /// Nonpositive, misaligned or incorrectly ordered input constraints.
    InvalidPolicy,
    /// Budget is zero or exceeds the hard implementation limit.
    InvalidBudget,
    /// Exploration exceeded the explicit budget; no guarantee was produced.
    BudgetExceeded,
    /// An allowed fill in a reachable state cannot commit in the Pool model.
    UnexecutableFill {
        /// Direction of the failing edge.
        direction: Direction,
        /// Input of the failing edge.
        amount_in: U256,
    },
    /// A positive fixed-point price/output cannot represent the safe bound.
    ZeroPrice {
        /// Direction whose conservative quote rounds to zero.
        direction: Direction,
    },
}

impl fmt::Display for FillPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OrderBook(error) => error.fmt(f),
            Self::InvalidPolicy => {
                f.write_str("fill policy requires positive aligned lot/min/max/depth bounds")
            }
            Self::InvalidBudget => write!(
                f,
                "max_transitions must be in 1..={MAX_VALIDATION_TRANSITIONS}"
            ),
            Self::BudgetExceeded => {
                f.write_str("exhaustive fill-policy budget exceeded; book is not certified")
            }
            Self::UnexecutableFill {
                direction,
                amount_in,
            } => write!(
                f,
                "allowed {direction:?} fill {amount_in} cannot execute in a reachable state"
            ),
            Self::ZeroPrice { direction } => write!(
                f,
                "{direction:?} conservative price or minimum output rounds to zero"
            ),
        }
    }
}

impl std::error::Error for FillPolicyError {}

impl From<OrderBookError> for FillPolicyError {
    fn from(value: OrderBookError) -> Self {
        Self::OrderBook(value)
    }
}

impl From<MathError> for FillPolicyError {
    fn from(value: MathError) -> Self {
        Self::OrderBook(OrderBookError::Math(value))
    }
}

#[derive(Clone, Copy)]
struct ReachableState {
    pool: PoolParams,
    cursors: [U256; 2],
}

/// Minimum output over all states sharing one externally observable fill key.
pub(crate) struct FillConstraint {
    pub(crate) direction_index: usize,
    pub(crate) cursor: U256,
    pub(crate) amount_in: U256,
    pub(crate) amount_out: U256,
}

pub(crate) struct FillExploration {
    pub(crate) constraints: Vec<FillConstraint>,
    pub(crate) checked_states: usize,
    pub(crate) checked_transitions: usize,
}

impl ReachableState {
    fn key(&self) -> (u128, u128, u32, u32, U256, U256) {
        // Anchor and max punishment are immutable across this entire domain.
        (
            self.pool.reserve_x,
            self.pool.reserve_y,
            self.pool.fee_ask_x24,
            self.pool.fee_bid_x24,
            self.cursors[0],
            self.cursors[1],
        )
    }
}

/// Build conservative prices for all allowed finite fill sequences, including
/// arbitrary direction interleavings and partial fills on the enforced lot grid.
///
/// The exhaustive state graph can grow exponentially. Choose shallow total/lot
/// ratios (e.g. four lots per direction), then increase only within a measured
/// budget. A budget error or any unexecutable allowed fill yields no book.
/// Fee accounting must be preflighted separately with
/// [`crate::try_validate_fee_accounting_capacity`]. `Applied` is the modeled
/// transition, not a guarantee that arbitrary token transfers or EVM calls succeed.
pub fn try_build_validated_order_book(
    state: &OrderBookState,
    config: &ValidatedOrderBookConfig,
) -> Result<ValidatedOrderBook, FillPolicyError> {
    if config.max_transitions == 0 || config.max_transitions > MAX_VALIDATION_TRANSITIONS {
        return Err(FillPolicyError::InvalidBudget);
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
    if book.status != OrderBookStatus::Active {
        return Ok(ValidatedOrderBook {
            book,
            state: *state,
            config: *config,
            checked_states: 0,
            checked_transitions: 0,
        });
    }

    let exploration = explore_fill_policy(state, config)?;
    let mut prices = [U256::MAX; 2];
    for constraint in &exploration.constraints {
        let price =
            U256::checked_mul_div(constraint.amount_out, PRICE_SCALE_X18, constraint.amount_in)
                .ok_or(OrderBookError::PriceOverflow)?;
        prices[constraint.direction_index] = prices[constraint.direction_index].min(price);
    }

    for (index, direction) in [Direction::XToY, Direction::YToX].into_iter().enumerate() {
        let Some(policy) = policies[index] else {
            continue;
        };
        let minimum_output =
            U256::checked_mul_div(policy.min_input, prices[index], PRICE_SCALE_X18)
                .ok_or(OrderBookError::PriceOverflow)?;
        if minimum_output.is_zero() {
            return Err(FillPolicyError::ZeroPrice { direction });
        }
        let ladder = DirectionalLadder {
            levels: vec![OrderBookLevel {
                size: policy.total_input,
                price: prices[index],
            }],
            truncated: false,
        };
        match direction {
            Direction::XToY => book.x_to_y = ladder,
            Direction::YToX => book.y_to_x = ladder,
        }
    }
    book.safety = OrderBookSafety::ExhaustiveLotPolicy;
    Ok(ValidatedOrderBook {
        book,
        state: *state,
        config: *config,
        checked_states: exploration.checked_states,
        checked_transitions: exploration.checked_transitions,
    })
}

/// Explore the complete finite state graph once. Callers validate the snapshot,
/// policies and budget before entering; all successful states use actual Pool
/// outputs, never the promised ladder output.
pub(crate) fn explore_fill_policy(
    state: &OrderBookState,
    config: &ValidatedOrderBookConfig,
) -> Result<FillExploration, FillPolicyError> {
    let policies = [config.x_to_y, config.y_to_x];
    let initial = ReachableState {
        pool: state.pool,
        cursors: [U256::ZERO; 2],
    };
    let mut seen = BTreeSet::from([initial.key()]);
    let mut pending = vec![initial];
    let mut constraints = BTreeMap::<(usize, U256, U256), U256>::new();
    let mut checked_transitions = 0usize;
    while let Some(current) = pending.pop() {
        for (index, direction) in [Direction::XToY, Direction::YToX].into_iter().enumerate() {
            let Some(policy) = policies[index] else {
                continue;
            };
            let remaining = policy.total_input - current.cursors[index];
            let upper = policy.max_input.min(remaining);
            let mut amount_in = policy.min_input;
            while amount_in <= upper {
                if checked_transitions == config.max_transitions {
                    return Err(FillPolicyError::BudgetExceeded);
                }
                checked_transitions += 1;
                let simulation = try_simulate_successful_swap(
                    &current.pool,
                    amount_in,
                    direction,
                    state.fee_multiplier,
                )?;
                if simulation.status != SimulationStatus::Applied {
                    return Err(FillPolicyError::UnexecutableFill {
                        direction,
                        amount_in,
                    });
                }
                constraints
                    .entry((index, current.cursors[index], amount_in))
                    .and_modify(|output| *output = (*output).min(simulation.quote.amount_out))
                    .or_insert(simulation.quote.amount_out);
                let mut next = ReachableState {
                    pool: simulation.post_swap,
                    cursors: current.cursors,
                };
                // bounded above by the validated total; cannot overflow.
                next.cursors[index] += amount_in;
                if seen.insert(next.key()) {
                    pending.push(next);
                }
                if upper - amount_in < policy.lot_input {
                    break;
                }
                amount_in += policy.lot_input;
            }
        }
    }

    Ok(FillExploration {
        constraints: constraints
            .into_iter()
            .map(
                |((direction_index, cursor, amount_in), amount_out)| FillConstraint {
                    direction_index,
                    cursor,
                    amount_in,
                    amount_out,
                },
            )
            .collect(),
        checked_states: seen.len(),
        checked_transitions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{try_ladder_amount_out_at_cursor, MAX_U112, Q24, Q96};

    fn state() -> OrderBookState {
        OrderBookState {
            pool: PoolParams {
                sqrt_price_x96: Q96,
                fee_ask_x24: Q24 / 1_000,
                fee_bid_x24: Q24 / 1_000,
                reserve_x: 10_000,
                reserve_y: 10_000,
                max_punishment_x24: Q24 / 10,
            },
            fee_multiplier: U256::from(1),
            snapshot_block: 100,
            max_execution_block: 101,
            latest_update_block: 100,
            block_delay: 3,
            paused: false,
        }
    }

    fn policy(lot: u64, max_lots: u64, total_lots: u64) -> FillPolicy {
        FillPolicy {
            min_input: U256::from(lot),
            lot_input: U256::from(lot),
            max_input: U256::from(lot * max_lots),
            total_input: U256::from(lot * total_lots),
        }
    }

    // Independent tree walk deliberately avoids the implementation's state
    // deduplication and computes each actual promise through the public sweep.
    fn check_every_sequence(
        validated: &ValidatedOrderBook,
        pool: PoolParams,
        cursors: [U256; 2],
    ) -> usize {
        let mut edges = 0;
        for (index, direction, configured, ladder) in [
            (
                0,
                Direction::XToY,
                validated.config.x_to_y,
                &validated.book.x_to_y,
            ),
            (
                1,
                Direction::YToX,
                validated.config.y_to_x,
                &validated.book.y_to_x,
            ),
        ] {
            if let Some(policy) = configured {
                let mut amount = policy.min_input;
                while amount <= policy.max_input && amount <= policy.total_input - cursors[index] {
                    let promised = try_ladder_amount_out_at_cursor(ladder, cursors[index], amount)
                        .unwrap()
                        .unwrap();
                    let actual = try_simulate_successful_swap(
                        &pool,
                        amount,
                        direction,
                        validated.state.fee_multiplier,
                    )
                    .unwrap();
                    assert_eq!(actual.status, SimulationStatus::Applied);
                    assert!(promised > U256::ZERO);
                    assert!(
                        promised <= actual.quote.amount_out,
                        "{direction:?} {amount} at {cursors:?}: {promised} > {}",
                        actual.quote.amount_out
                    );
                    let mut next_cursors = cursors;
                    next_cursors[index] += amount;
                    edges += 1 + check_every_sequence(validated, actual.post_swap, next_cursors);
                    amount += policy.lot_input;
                }
            }
        }
        edges
    }

    #[test]
    fn certifies_all_partial_and_mixed_direction_sequences() {
        // Vary non-integral anchors, reserves, caller multipliers and punishment.
        // Every book is independently replayed over all sequence orderings.
        for seed in 1u64..=24 {
            let mut snapshot = state();
            snapshot.pool.sqrt_price_x96 = Q96 * U256::from(8 + seed % 7) / U256::from(10);
            snapshot.pool.reserve_x += u128::from(seed * 73);
            snapshot.pool.reserve_y += u128::from(seed * 53);
            snapshot.pool.max_punishment_x24 = (Q24 / 50) * (seed as u32 % 4);
            snapshot.fee_multiplier = U256::from(1 + seed % 3);
            let config = ValidatedOrderBookConfig {
                x_to_y: Some(policy(10, 2, 3)),
                y_to_x: Some(policy(10, 2, 3)),
                max_transitions: 10_000,
            };
            let book = try_build_validated_order_book(&snapshot, &config).unwrap();
            assert_eq!(book.book.safety, OrderBookSafety::ExhaustiveLotPolicy);
            assert!(book.book.requires_amount_out_minimum);
            assert!(book.checked_states > 1);
            assert!(
                check_every_sequence(&book, snapshot.pool, [U256::ZERO; 2])
                    >= book.checked_transitions
            );
        }
    }

    #[test]
    fn covers_nested_floor_partial_and_cursor_counterexamples() {
        let mut snapshot = state();
        snapshot.pool.fee_bid_x24 = 0;
        snapshot.pool.max_punishment_x24 = 0;
        snapshot.pool.sqrt_price_x96 = Q96 * U256::from(3) / U256::from(2);
        let partial = try_build_validated_order_book(
            &snapshot,
            &ValidatedOrderBookConfig {
                x_to_y: Some(policy(1, 2, 2)),
                y_to_x: None,
                max_transitions: 100,
            },
        )
        .unwrap();
        assert_eq!(partial.book.x_to_y.levels[0].price, PRICE_SCALE_X18);
        check_every_sequence(&partial, snapshot.pool, [U256::ZERO; 2]);

        snapshot.pool.sqrt_price_x96 = Q96 / U256::from(3);
        snapshot.pool.reserve_x = 1;
        snapshot.pool.reserve_y = 10;
        let cursor = try_build_validated_order_book(
            &snapshot,
            &ValidatedOrderBookConfig {
                x_to_y: Some(policy(20, 2, 3)),
                y_to_x: None,
                max_transitions: 100,
            },
        )
        .unwrap();
        assert_eq!(
            cursor.book.x_to_y.levels[0].price,
            PRICE_SCALE_X18 / U256::from(20)
        );
        check_every_sequence(&cursor, snapshot.pool, [U256::ZERO; 2]);
    }

    #[test]
    fn fails_closed_on_budget_invalid_policy_and_reachable_reverts() {
        let mut config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(10, 2, 3)),
            y_to_x: Some(policy(10, 2, 3)),
            max_transitions: 1,
        };
        assert_eq!(
            try_build_validated_order_book(&state(), &config),
            Err(FillPolicyError::BudgetExceeded)
        );
        config.max_transitions = MAX_VALIDATION_TRANSITIONS + 1;
        assert_eq!(
            try_build_validated_order_book(&state(), &config),
            Err(FillPolicyError::InvalidBudget)
        );
        config.max_transitions = 100;
        config.x_to_y.as_mut().unwrap().min_input = U256::from(11);
        assert_eq!(
            try_build_validated_order_book(&state(), &config),
            Err(FillPolicyError::InvalidPolicy)
        );
        config.x_to_y = Some(policy(10, 1, 3));
        config.y_to_x = None;
        let mut snapshot = state();
        snapshot.pool.reserve_x = MAX_U112 - 15;
        assert!(matches!(
            try_build_validated_order_book(&snapshot, &config),
            Err(FillPolicyError::UnexecutableFill { .. })
        ));
        snapshot = state();
        snapshot.pool.fee_bid_x24 = Q24 - 1;
        assert!(matches!(
            try_build_validated_order_book(&snapshot, &config),
            Err(FillPolicyError::UnexecutableFill { .. })
        ));
    }

    #[test]
    fn rejects_zero_encoded_output_even_if_pool_quote_is_nonzero() {
        let mut snapshot = state();
        snapshot.pool.sqrt_price_x96 = Q96 / U256::from(3);
        snapshot.pool.max_punishment_x24 = 0;
        snapshot.pool.fee_bid_x24 = 0;
        let config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(10, 1, 1)),
            y_to_x: None,
            max_transitions: 10,
        };
        // Pool quote(10)=0 due nested floors; don't publish zero liquidity.
        assert!(matches!(
            try_build_validated_order_book(&snapshot, &config),
            Err(FillPolicyError::UnexecutableFill { .. }) | Err(FillPolicyError::ZeroPrice { .. })
        ));
        snapshot.pool.sqrt_price_x96 = Q96;
        snapshot.pool.fee_bid_x24 = Q24 * 3 / 4;
        let config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(3, 1, 1)),
            y_to_x: None,
            max_transitions: 10,
        };
        // Pool quote(3)=1, but floor(3 * floor(1e18 / 3) / 1e18)=0.
        assert!(matches!(
            try_build_validated_order_book(&snapshot, &config),
            Err(FillPolicyError::ZeroPrice { .. })
        ));
    }

    #[test]
    fn paused_or_expired_results_do_not_claim_coverage() {
        let mut snapshot = state();
        snapshot.paused = true;
        let config = ValidatedOrderBookConfig {
            x_to_y: Some(policy(10, 1, 3)),
            y_to_x: None,
            max_transitions: 100,
        };
        let paused = try_build_validated_order_book(&snapshot, &config).unwrap();
        assert_eq!(paused.book.status, OrderBookStatus::Paused);
        assert_eq!(paused.book.safety, OrderBookSafety::Indicative);
        assert_eq!(paused.checked_transitions, 0);
        snapshot.paused = false;
        snapshot.max_execution_block = 103;
        assert_eq!(
            try_build_validated_order_book(&snapshot, &config)
                .unwrap()
                .book
                .status,
            OrderBookStatus::Stale
        );
        assert!(!policy(10, 2, 3).allows(U256::from(15)));
        assert!(policy(10, 2, 3).allows(U256::from(20)));
    }
}
