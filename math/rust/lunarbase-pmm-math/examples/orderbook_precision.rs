//! Reproducible synthetic precision measurements, not live-market benchmarks.
//! Run: cargo run -p lunarbase-pmm-math --release --example orderbook_precision

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::time::Instant;

use lunarbase_pmm_math::{
    geometric_sizes, try_build_order_book, try_build_precise_order_book, try_ladder_amount_out,
    try_ladder_amount_out_at_cursor, try_quote_x_to_y_with_multiplier,
    try_simulate_successful_swap, try_validate_fee_accounting_capacity, Direction,
    FeeAccountingState, FillPolicy, OrderBook, OrderBookConfig, OrderBookPrecision, OrderBookState,
    PoolParams, SimulationStatus, U256Ext, ValidatedOrderBookConfig, Q24, Q96, U256,
};
use serde_json::json;

fn snapshot(max_punishment_x24: u32) -> OrderBookState {
    OrderBookState {
        pool: PoolParams {
            // floor(sqrt(2500 * 10^(6-18)) * Q96): no floating point input.
            sqrt_price_x96: Q96 / U256::from(20_000u64),
            fee_ask_x24: Q24 / 10_000,
            fee_bid_x24: Q24 / 10_000,
            reserve_x: 100 * 10u128.pow(18),
            reserve_y: 250_000 * 10u128.pow(6),
            max_punishment_x24,
        },
        fee_multiplier: U256::from(1),
        snapshot_block: 100,
        max_execution_block: 105,
        latest_update_block: 100,
        block_delay: 20,
        paused: false,
    }
}

fn policy() -> ValidatedOrderBookConfig {
    let side = |lot: U256| {
        Some(FillPolicy {
            min_input: lot,
            lot_input: lot,
            max_input: lot * U256::from(2),
            total_input: lot * U256::from(8),
        })
    };
    ValidatedOrderBookConfig {
        x_to_y: side(U256::from(10u64.pow(18))),
        y_to_x: side(U256::from(2_500_000_000u64)),
        max_transitions: 100_000,
    }
}

type ConstraintKey = (usize, U256, U256);

// Independent breadth-first enumeration for the measurement report. The
// builder uses its own state/constraint collector, so this also checks its
// final safety and rounded error metrics rather than trusting them blindly.
fn constraints(
    state: &OrderBookState,
    config: &ValidatedOrderBookConfig,
) -> Result<BTreeMap<ConstraintKey, U256>, Box<dyn Error>> {
    let key = |p: PoolParams, cursors: [U256; 2]| {
        (
            p.reserve_x,
            p.reserve_y,
            p.fee_ask_x24,
            p.fee_bid_x24,
            cursors[0],
            cursors[1],
        )
    };
    let zero = [U256::ZERO; 2];
    let mut seen = BTreeSet::from([key(state.pool, zero)]);
    let mut queue = VecDeque::from([(state.pool, zero)]);
    let mut result: BTreeMap<ConstraintKey, U256> = BTreeMap::new();
    let mut transitions = 0;
    while let Some((pool, cursors)) = queue.pop_front() {
        for (index, direction, policy) in [
            (0, Direction::XToY, config.x_to_y),
            (1, Direction::YToX, config.y_to_x),
        ] {
            let Some(policy) = policy else {
                continue;
            };
            let upper = policy.max_input.min(policy.total_input - cursors[index]);
            let mut amount = policy.min_input;
            while amount <= upper {
                transitions += 1;
                if transitions > config.max_transitions {
                    return Err("report graph budget exhausted".into());
                }
                let swap =
                    try_simulate_successful_swap(&pool, amount, direction, state.fee_multiplier)?;
                if swap.status != SimulationStatus::Applied {
                    return Err("sample has an unexecutable edge".into());
                }
                result
                    .entry((index, cursors[index], amount))
                    .and_modify(|out| *out = (*out).min(swap.quote.amount_out))
                    .or_insert(swap.quote.amount_out);
                let mut next = cursors;
                next[index] += amount;
                if seen.insert(key(swap.post_swap, next)) {
                    queue.push_back((swap.post_swap, next));
                }
                if upper - amount < policy.lot_input {
                    break;
                }
                amount += policy.lot_input;
            }
        }
    }
    Ok(result)
}

fn measure(
    book: &OrderBook,
    constraints: &BTreeMap<ConstraintKey, U256>,
) -> Result<(u32, [U256; 2], u64), Box<dyn Error>> {
    let mut worst_underquote_bps = 0;
    let mut worst_underquote_microbps = 0;
    let mut excess = [U256::ZERO; 2];
    for (&(index, cursor, input), &actual) in constraints {
        let side = if index == 0 {
            &book.x_to_y
        } else {
            &book.y_to_x
        };
        let promised = try_ladder_amount_out_at_cursor(side, cursor, input)?
            .ok_or("book omitted required depth")?;
        if promised > actual {
            excess[index] = excess[index].max(promised - actual);
        } else {
            let bps: u32 =
                U256::checked_mul_div_ceil(actual - promised, U256::from(10_000), actual)
                    .ok_or("metric overflow")?
                    .to();
            worst_underquote_bps = worst_underquote_bps.max(bps);
            let microbps: u64 = U256::checked_mul_div_ceil(
                actual - promised,
                U256::from(10_000_000_000u64),
                actual,
            )
            .ok_or("metric overflow")?
            .to();
            worst_underquote_microbps = worst_underquote_microbps.max(microbps);
        }
    }
    Ok((worst_underquote_bps, excess, worst_underquote_microbps))
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = policy();
    for (name, punishment) in [
        ("flat_no_punishment", 0),
        ("weth_usdc_moderate_punishment", Q24 / 100),
        ("weth_usdc_high_punishment", Q24 / 10),
    ] {
        let state = snapshot(punishment);
        try_validate_fee_accounting_capacity(&state, &config, &FeeAccountingState::default())?;
        let edges = constraints(&state, &config)?;
        let x_sizes = geometric_sizes(config.x_to_y.unwrap().total_input, 20)?;
        let y_sizes = geometric_sizes(config.y_to_x.unwrap().total_input, 20)?;
        let reference = try_build_order_book(
            &state,
            &OrderBookConfig {
                x_to_y_sizes: &x_sizes,
                y_to_x_sizes: &y_sizes,
            },
        )?;
        let (reference_error, reference_excess, reference_microbps) = measure(&reference, &edges)?;
        println!(
            "{}",
            json!({"sample":name,"synthetic":true,"builder":"reference_geometric_20",
            "worstUnderquoteBpsCeil":reference_error,
            "worstUnderquoteMicroBpsCeil":reference_microbps,
            "maxExcessRawOutputX":reference_excess[1].to_string(),
            "maxExcessRawOutputY":reference_excess[0].to_string(),
            "xLevels":reference.x_to_y.levels.len(),"yLevels":reference.y_to_x.levels.len()})
        );
        for max_levels in [1, 4, 8, 20] {
            let precision = OrderBookPrecision {
                max_levels,
                target_underquote_bps: 1,
                max_work: 5_000_000,
            };
            let started = Instant::now();
            match try_build_precise_order_book(&state, &config, &precision) {
                Ok(result) => {
                    let elapsed = started.elapsed();
                    let (observed_error, observed_excess, observed_microbps) =
                        measure(&result.validated.book, &edges)?;
                    assert_eq!(
                        observed_excess,
                        [U256::ZERO; 2],
                        "precise ladder overpromises"
                    );
                    assert_eq!(observed_error, result.worst_underquote_bps);
                    let levels = |side: &lunarbase_pmm_math::DirectionalLadder| {
                        side.levels.iter().map(|level| {
                            json!({"size": level.size.to_string(), "price": level.price.to_string()})
                        }).collect::<Vec<_>>()
                    };
                    println!(
                        "{}",
                        json!({
                            "sample": name,
                            "synthetic": true,
                            "anchorPriceX96": state.pool.sqrt_price_x96.to_string(),
                            "reserveX": state.pool.reserve_x.to_string(),
                            "reserveY": state.pool.reserve_y.to_string(),
                            "feeAskX24": state.pool.fee_ask_x24,
                            "feeBidX24": state.pool.fee_bid_x24,
                            "maxPunishmentX24": punishment,
                            "lotsPerDirection": 8,
                            "maxFillLots": 2,
                            "maxLevels": max_levels,
                            "targetUnderquoteBps": precision.target_underquote_bps,
                            "targetMet": result.target_met,
                            "worstUnderquoteBpsCeil": result.worst_underquote_bps,
                            "worstUnderquoteMicroBpsCeil": observed_microbps,
                            "worstFreshSnapshotDiscountBpsCeil": result.worst_fresh_snapshot_discount_bps,
                            "checkedStates": result.validated.checked_states,
                            "checkedTransitions": result.validated.checked_transitions,
                            "constraints": result.constraint_count,
                            "workUsed": result.work_used,
                            "elapsedMicros": elapsed.as_micros(),
                            "independentReplayMaxExcessRawOutput": "0",
                            "xToY": levels(&result.validated.book.x_to_y),
                            "yToX": levels(&result.validated.book.y_to_x),
                        })
                    );
                }
                Err(error) => println!(
                    "{}",
                    json!({
                        "sample": name, "synthetic": true, "maxLevels": max_levels,
                        "error": error.to_string(), "elapsedMicros": started.elapsed().as_micros(),
                    })
                ),
            }
        }
    }

    // Existing raw-unit counterexample to applying the continuous-concavity
    // proof to the integer Pool without additional validation.
    let mut integer = snapshot(0);
    integer.pool.sqrt_price_x96 = Q96 * U256::from(3) / U256::from(2);
    integer.pool.fee_ask_x24 = 0;
    integer.pool.fee_bid_x24 = 0;
    integer.pool.reserve_x = 1_000_000;
    integer.pool.reserve_y = 1_000_000;
    let samples = [U256::from(2)];
    let book = try_build_order_book(
        &integer,
        &OrderBookConfig {
            x_to_y_sizes: &samples,
            y_to_x_sizes: &[],
        },
    )?;
    let amount = U256::from(1);
    let promised = try_ladder_amount_out(&book.x_to_y, amount)?.unwrap();
    let actual = try_quote_x_to_y_with_multiplier(&integer.pool, amount, U256::from(1))?.amount_out;
    assert_eq!((promised, actual), (U256::from(2), U256::from(1)));
    println!(
        "{}",
        json!({"sample":"raw_unit_rounding_counterexample", "amountIn":"1",
        "sampledPromise":promised.to_string(), "actualPoolOutput":actual.to_string(),
        "excessRawOutputUnits":(promised-actual).to_string()})
    );
    Ok(())
}
