//! Contract-derived cubic punishment and price-age regression cases.

use lunarbase_pmm_math::{punishment_x24, Direction, PoolParams, MAX_U24, Q96, U256};
use lunarbase_pmm_math::{try_apply_update, try_quote_x_to_y, try_simulate_successful_swap};

fn snapshot() -> PoolParams {
    PoolParams {
        sqrt_price_x96: Q96,
        fee_ask_x24: 10_000,
        fee_bid_x24: 10_000,
        reserve_x: 500_000,
        reserve_y: 500_000,
        max_punishment_x24: 16_778,
        max_decay_multiplier_x24: 0,
        current_block: 0,
        latest_update_block: 0,
        block_delay: 1,
    }
}

#[test]
fn cubic_punishment_rounds_small_positive_trades_up_to_one() {
    let pool = snapshot();
    for direction in [Direction::XToY, Direction::YToX] {
        assert_eq!(punishment_x24(&pool, U256::from(1_000), direction), 1);
    }
}

#[test]
fn cubic_punishment_uses_the_wealth_ratio_cubed_and_clamps_at_one() {
    let mut pool = snapshot();
    pool.reserve_x = 0;
    pool.reserve_y = 1_000;
    pool.max_punishment_x24 = MAX_U24;
    for (amount, expected) in [(500, 2_097_152), (1_000, MAX_U24), (1_001, MAX_U24)] {
        assert_eq!(
            punishment_x24(&pool, U256::from(amount), Direction::YToX),
            expected
        );
    }
}

#[test]
fn wealth_ratio_is_rounded_up_to_q48_before_cubing() {
    let mut pool = snapshot();
    pool.reserve_x = 0;
    pool.reserve_y = 3;
    pool.max_punishment_x24 = 27;
    // Exact rational ceil(27 * (1/3)^3) is 1; the contract's Q48 ceil makes it 2.
    assert_eq!(punishment_x24(&pool, U256::from(1), Direction::XToY), 2);
}

#[test]
fn price_age_has_contract_golden_fees_and_an_inclusive_deadline() {
    let mut pool = snapshot();
    pool.max_punishment_x24 = 0;
    pool.fee_bid_x24 = 10_000;
    pool.reserve_y = 100_000_000;
    pool.latest_update_block = 100;
    pool.block_delay = 10;
    pool.max_decay_multiplier_x24 = 3 * (1 << 24);
    for (age, expected_fee) in [
        (0, 10_000),
        (1, 10_020),
        (5, 12_500),
        (9, 24_580),
        (10, 30_000),
    ] {
        pool.current_block = 100 + age;
        let quote = try_quote_x_to_y(&pool, U256::from(1 << 24)).unwrap();
        assert_eq!(quote.fee, U256::from(expected_fee), "age {age}");
        assert_eq!(quote.effective_fee_x24, 10_000);
    }
    pool.current_block = 111;
    assert_eq!(
        try_quote_x_to_y(&pool, U256::from(1 << 24))
            .unwrap()
            .amount_out,
        U256::ZERO
    );
}

#[test]
fn simulation_persists_only_punishment_and_update_resets_the_price_age() {
    let mut pool = snapshot();
    pool.fee_bid_x24 = 10_000;
    pool.current_block = 105;
    pool.latest_update_block = 100;
    pool.block_delay = 10;
    pool.max_decay_multiplier_x24 = 3 * (1 << 24);
    let simulation =
        try_simulate_successful_swap(&pool, U256::from(1_000), Direction::XToY, U256::from(1))
            .unwrap();
    assert_eq!(simulation.quote.effective_fee_x24, 10_001);
    assert_eq!(simulation.post_swap.fee_bid_x24, 10_001);
    assert_eq!(simulation.post_swap.latest_update_block, 100);
    let updated = try_apply_update(&simulation.post_swap, Q96, 10_000, 10_000).unwrap();
    assert_eq!(updated.latest_update_block, 105);
    assert_eq!(
        updated.max_decay_multiplier_x24,
        pool.max_decay_multiplier_x24
    );
}

#[test]
fn wide_age_math_ceil_saturation_and_stale_rollback_match_the_contract() {
    use lunarbase_pmm_math::{try_decay_fee_x24, RollbackReason, SimulationStatus, MAX_U48, Q24};
    assert_eq!(
        try_decay_fee_x24(10_001, u64::from(Q24) * 3, 5, 10).unwrap(),
        12_502
    );
    assert_eq!(
        try_decay_fee_x24(10_000, u64::MAX, 1, MAX_U48).unwrap(),
        10_001
    );
    assert_eq!(
        try_decay_fee_x24(10_000, u64::MAX, MAX_U48, MAX_U48).unwrap(),
        MAX_U24
    );
    assert_eq!(try_decay_fee_x24(0, u64::MAX, 10, 10).unwrap(), 0);
    assert_eq!(
        try_decay_fee_x24(MAX_U24, u64::MAX, 10, 10).unwrap(),
        MAX_U24
    );
    for maximum in [0, u64::from(Q24)] {
        assert_eq!(try_decay_fee_x24(10_001, maximum, 10, 10).unwrap(), 10_001);
    }
    let mut pool = snapshot();
    pool.current_block = 11;
    pool.block_delay = 10;
    let result =
        try_simulate_successful_swap(&pool, U256::MAX, Direction::XToY, U256::from(1)).unwrap();
    assert_eq!(
        result.status,
        SimulationStatus::RolledBack(RollbackReason::StalePrice)
    );
    assert_eq!(result.post_swap, pool);
    assert_eq!(result.punishment.desired_punishment_x24, 0);
}

#[test]
fn order_book_prices_the_last_inclusive_execution_block() {
    use lunarbase_pmm_math::{
        try_build_order_book, OrderBookConfig, OrderBookState, OrderBookStatus,
    };
    let mut pool = snapshot();
    pool.reserve_y = 100_000_000;
    pool.fee_bid_x24 = 10_000;
    pool.max_punishment_x24 = 0;
    pool.max_decay_multiplier_x24 = 3 * (1 << 24);
    pool.current_block = 100;
    pool.latest_update_block = 100;
    pool.block_delay = 10;
    let mut state = OrderBookState {
        fee_accounting: lunarbase_pmm_math::FeeAccountingState::default(),
        pool,
        max_execution_block: 110,
        fee_multiplier: U256::from(1),
        paused: false,
    };
    let sizes = [U256::from(1 << 24)];
    let config = OrderBookConfig {
        x_to_y_sizes: &sizes,
        y_to_x_sizes: &[],
    };
    let book = try_build_order_book(&state, &config).unwrap();
    assert_eq!(book.status, OrderBookStatus::Active);
    assert_eq!(
        book.x_to_y.levels[0].price,
        U256::from(998_211_860_656_738_281u64)
    );
    state.max_execution_block = 111;
    assert_eq!(
        try_build_order_book(&state, &config).unwrap().status,
        OrderBookStatus::Stale
    );
}

#[test]
fn operator_update_preserves_the_contracts_uint48_block_truncation() {
    let mut pool = snapshot();
    pool.current_block = (1u64 << 48) + 7;
    let updated = try_apply_update(&pool, Q96, 1, 2).unwrap();
    assert_eq!(updated.latest_update_block, 7);
}
