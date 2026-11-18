//! Active treasury reserve and partner-accounting regressions.
use lunarbase_pmm_math::{
    try_simulate_successful_swap, try_simulate_successful_swap_with_fee_accounting,
    try_validate_fee_accounting_capacity, Direction, FeeAccountingError, FeeAccountingState,
    FeeBucket, FillPolicy, MathError, OrderBookState, PoolParams, RollbackReason, SimulationStatus,
    ValidatedOrderBookConfig, MAX_U112, PARTNER_FEE_SCALE, Q24, Q96, U256,
};

fn params() -> PoolParams {
    PoolParams {
        sqrt_price_x96: Q96,
        fee_ask_x24: Q24 / 10,
        fee_bid_x24: Q24 / 10,
        reserve_x: 1_000_000,
        reserve_y: 1_000_000,
        max_punishment_x24: Q24 / 10,
        max_decay_multiplier_x24: 0,
        current_block: 100,
        latest_update_block: 100,
        block_delay: 3,
    }
}

#[test]
fn only_credited_partner_fees_leave_reserves_with_per_swap_flooring() {
    let before = params();
    for direction in [Direction::XToY, Direction::YToX] {
        for partner_fee in [0, 1, 333_333, 500_000, PARTNER_FEE_SCALE] {
            for operator_present in [false, true] {
                let accounting = FeeAccountingState {
                    partner_fee,
                    partner_operator_present: operator_present,
                    ..FeeAccountingState::default()
                };
                let result = try_simulate_successful_swap_with_fee_accounting(
                    &before,
                    U256::from(100_003),
                    direction,
                    U256::from(1),
                    &accounting,
                )
                .unwrap();
                let partner = if operator_present {
                    result.quote.fee * U256::from(partner_fee) / U256::from(PARTNER_FEE_SCALE)
                } else {
                    U256::ZERO
                };
                let output_reserve = if direction == Direction::XToY {
                    result.post_swap.reserve_y
                } else {
                    result.post_swap.reserve_x
                };
                assert_eq!(
                    output_reserve,
                    1_000_000 - (result.quote.amount_out + partner).to::<u128>()
                );
            }
        }
    }
}

#[test]
fn partner_configuration_is_validated_and_later_reverts_restore_reserves() {
    let before = params();
    let mut accounting = FeeAccountingState {
        partner_fee: 500_000,
        partner_operator_present: true,
        ..FeeAccountingState::default()
    };
    let mut result = try_simulate_successful_swap_with_fee_accounting(
        &before,
        U256::from(100_003),
        Direction::XToY,
        U256::from(1),
        &accounting,
    )
    .unwrap();
    result.mark_rolled_back(RollbackReason::LaterRevert);
    assert_eq!(*result.effective_params(), before);
    accounting.partner_fee = PARTNER_FEE_SCALE + 1;
    assert_eq!(
        try_simulate_successful_swap_with_fee_accounting(
            &before,
            U256::from(100),
            Direction::XToY,
            U256::from(1),
            &accounting,
        ),
        Err(MathError::InvalidPartnerFee)
    );
}

#[test]
fn treasury_headroom_covers_repeated_fees_on_retained_assets() {
    let mut pool = params();
    pool.reserve_x = 200;
    pool.reserve_y = 200;
    pool.fee_bid_x24 = Q24 * 99 / 100;
    pool.max_punishment_x24 = 0;
    let state = OrderBookState {
        pool,
        fee_multiplier: U256::from(1),
        max_execution_block: 100,
        paused: false,
        fee_accounting: FeeAccountingState {
            treasury_y: MAX_U112 - 500,
            ..FeeAccountingState::default()
        },
    };
    let config = ValidatedOrderBookConfig {
        x_to_y: Some(FillPolicy {
            min_input: U256::from(100),
            lot_input: U256::from(100),
            max_input: U256::from(100),
            total_input: U256::from(1000),
        }),
        y_to_x: None,
        max_transitions: 100,
    };
    let mut total_fee = U256::ZERO;
    for _ in 0..10 {
        let step =
            try_simulate_successful_swap(&pool, U256::from(100), Direction::XToY, U256::from(1))
                .unwrap();
        assert_eq!(step.status, SimulationStatus::Applied);
        total_fee += step.quote.fee;
        pool = step.post_swap;
    }
    assert!(total_fee > U256::from(state.pool.reserve_y));
    assert_eq!(
        try_validate_fee_accounting_capacity(&state, &config),
        Err(FeeAccountingError::CapacityExceeded(FeeBucket::TreasuryY))
    );
}

#[test]
fn treasury_fees_remain_in_both_trading_reserves() {
    let before = params();
    for direction in [Direction::XToY, Direction::YToX] {
        let result =
            try_simulate_successful_swap(&before, U256::from(100_000), direction, U256::from(1))
                .unwrap();
        assert_eq!(result.status, SimulationStatus::Applied);
        assert!(result.quote.fee > U256::ZERO);
        let output_reserve = if direction == Direction::XToY {
            result.post_swap.reserve_y
        } else {
            result.post_swap.reserve_x
        };
        assert_eq!(
            output_reserve,
            1_000_000 - result.quote.amount_out.to::<u128>()
        );
        assert_eq!(
            result.post_swap.reserve_x + result.post_swap.reserve_y,
            before.reserve_x + before.reserve_y + result.quote.fee.to::<u128>()
        );
    }
}

#[test]
fn partner_books_cover_every_mixed_direction_sequence() {
    use lunarbase_pmm_math::{
        try_build_validated_order_book, try_ladder_amount_out_at_cursor, ValidatedOrderBook,
    };
    fn replay(book: &ValidatedOrderBook, pool: PoolParams, cursors: [u32; 2]) {
        for (side, direction) in [Direction::XToY, Direction::YToX].into_iter().enumerate() {
            if cursors[side] == 3 {
                continue;
            }
            let amount = U256::from(100_000);
            let actual = try_simulate_successful_swap_with_fee_accounting(
                &pool,
                amount,
                direction,
                U256::from(1),
                &book.state.fee_accounting,
            )
            .unwrap();
            assert_eq!(actual.status, SimulationStatus::Applied);
            let ladder = if side == 0 {
                &book.book.x_to_y
            } else {
                &book.book.y_to_x
            };
            let promised =
                try_ladder_amount_out_at_cursor(ladder, U256::from(cursors[side]) * amount, amount)
                    .unwrap()
                    .unwrap();
            assert!(promised <= actual.quote.amount_out);
            let mut next_cursors = cursors;
            next_cursors[side] += 1;
            replay(book, actual.post_swap, next_cursors);
        }
    }
    let fill = FillPolicy {
        min_input: U256::from(100_000),
        lot_input: U256::from(100_000),
        max_input: U256::from(100_000),
        total_input: U256::from(300_000),
    };
    let config = ValidatedOrderBookConfig {
        x_to_y: Some(fill),
        y_to_x: Some(fill),
        max_transitions: 10_000,
    };
    for share in [0, 500_000, PARTNER_FEE_SCALE] {
        let state = OrderBookState {
            pool: params(),
            fee_multiplier: U256::from(1),
            max_execution_block: 100,
            paused: false,
            fee_accounting: FeeAccountingState {
                partner_fee: share,
                partner_operator_present: true,
                ..FeeAccountingState::default()
            },
        };
        let book = try_build_validated_order_book(&state, &config).unwrap();
        replay(&book, state.pool, [0, 0]);
    }
}
