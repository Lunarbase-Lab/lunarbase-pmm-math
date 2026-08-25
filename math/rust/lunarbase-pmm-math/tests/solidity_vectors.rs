//! Bit-for-bit replay of schema-v4 vectors produced by the Solidity oracle.

use serde::Deserialize;

use lunarbase_pmm_math::{
    try_apply_update, try_quote_x_to_y_with_multiplier, try_quote_y_to_x_with_multiplier,
    try_simulate_successful_swap, Direction, MathError, PoolParams, RollbackReason,
    SimulationStatus, U256,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
enum VectorOutcome {
    Applied,
    SwapImpossible,
    ReserveTransitionOverflow,
    MathMulDivRevert,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateVector {
    anchor_price: String,
    fee_ask_x24: String,
    fee_bid_x24: String,
    anchor_price_after: String,
    fee_ask_x24_after: String,
    fee_bid_x24_after: String,
    reserve_x_after: String,
    reserve_y_after: String,
    max_punishment_x24_after: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vector {
    schema_version: u32,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    seed: Option<String>,
    dir: String,
    anchor_price: String,
    fee_ask_x24: String,
    fee_bid_x24: String,
    reserve_x: String,
    reserve_y: String,
    max_punishment_x24: String,
    fee_multiplier: String,
    amount_in: String,
    amount_out: String,
    p_next: String,
    fee_amount: String,
    outcome: VectorOutcome,
    revert_selector: String,
    revert_class: String,
    desired_punishment_x24: String,
    effective_fee_x24: String,
    applied_punishment_x24: String,
    fee_ask_x24_after: String,
    fee_bid_x24_after: String,
    reserve_x_after: String,
    reserve_y_after: String,
    #[serde(default)]
    update: Option<UpdateVector>,
}

impl Vector {
    fn label(&self, line: usize) -> String {
        format!(
            "line {line} ({})",
            self.name
                .as_deref()
                .or(self.seed.as_deref())
                .unwrap_or(self.dir.as_str())
        )
    }

    fn direction(&self) -> Direction {
        match self.dir.as_str() {
            "xToY" => Direction::XToY,
            "yToX" => Direction::YToX,
            other => panic!("invalid vector direction: {other}"),
        }
    }
}

fn parse_u256(value: &str) -> U256 {
    U256::from_str_radix(value, 10).unwrap_or_else(|error| {
        panic!("invalid uint256 vector value {value:?}: {error}");
    })
}

fn parse_u128(value: &str) -> u128 {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid u128 vector value {value:?}: {error}"))
}

fn parse_u32(value: &str) -> u32 {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid u32 vector value {value:?}: {error}"))
}

fn expected_revert(outcome: VectorOutcome) -> (&'static str, &'static str) {
    match outcome {
        VectorOutcome::Applied => ("0x00000000", "None"),
        VectorOutcome::SwapImpossible => ("0x4a45e749", "SwapImpossible()"),
        VectorOutcome::ReserveTransitionOverflow => (
            "0x6dfcc650",
            "SafeCastOverflowedUintDowncast(uint8,uint256)",
        ),
        VectorOutcome::MathMulDivRevert => ("0x4e487b71", "Panic(0x11)"),
    }
}

fn replay(path: &str, data: &str) {
    let mut total = 0usize;
    let mut x_to_y = 0usize;
    let mut y_to_x = 0usize;
    let mut applied = 0usize;
    let mut swap_impossible = 0usize;
    let mut reserve_overflow = 0usize;
    let mut math_revert = 0usize;
    let mut updates = 0usize;

    for (index, line) in data.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = index + 1;
        let vector: Vector = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("{path}:{line_number}: invalid JSON: {error}"));
        assert_eq!(
            vector.schema_version, 4,
            "{path}:{line_number}: unsupported schema"
        );
        let label = format!("{path}:{}", vector.label(line_number));
        let direction = vector.direction();
        match direction {
            Direction::XToY => x_to_y += 1,
            Direction::YToX => y_to_x += 1,
        }

        let (expected_selector, expected_class) = expected_revert(vector.outcome);
        assert_eq!(
            vector.revert_selector, expected_selector,
            "{label}: revertSelector"
        );
        assert_eq!(vector.revert_class, expected_class, "{label}: revertClass");

        let params = PoolParams {
            sqrt_price_x96: parse_u256(&vector.anchor_price),
            fee_ask_x24: parse_u32(&vector.fee_ask_x24),
            fee_bid_x24: parse_u32(&vector.fee_bid_x24),
            reserve_x: parse_u128(&vector.reserve_x),
            reserve_y: parse_u128(&vector.reserve_y),
            max_punishment_x24: parse_u32(&vector.max_punishment_x24),
        };
        let amount_in = parse_u256(&vector.amount_in);
        let fee_multiplier = parse_u256(&vector.fee_multiplier);
        let quote_result = match direction {
            Direction::XToY => try_quote_x_to_y_with_multiplier(&params, amount_in, fee_multiplier),
            Direction::YToX => try_quote_y_to_x_with_multiplier(&params, amount_in, fee_multiplier),
        };
        let simulation_result =
            try_simulate_successful_swap(&params, amount_in, direction, fee_multiplier);

        if vector.outcome == VectorOutcome::MathMulDivRevert {
            assert_eq!(
                quote_result,
                Err(MathError::MulDivOverflow),
                "{label}: quote error"
            );
            assert_eq!(
                simulation_result,
                Err(MathError::MulDivOverflow),
                "{label}: simulation error"
            );
            assert!(
                vector.update.is_none(),
                "{label}: reverted vector has update"
            );
            assert_eq!(
                vector.desired_punishment_x24, "0",
                "{label}: desired punishment"
            );
            assert_eq!(
                vector.applied_punishment_x24, "0",
                "{label}: applied punishment"
            );
            assert_eq!(vector.effective_fee_x24, "0", "{label}: effective fee");
            assert_eq!(
                parse_u32(&vector.fee_ask_x24_after),
                params.fee_ask_x24,
                "{label}: ask"
            );
            assert_eq!(
                parse_u32(&vector.fee_bid_x24_after),
                params.fee_bid_x24,
                "{label}: bid"
            );
            assert_eq!(
                parse_u128(&vector.reserve_x_after),
                params.reserve_x,
                "{label}: reserve X"
            );
            assert_eq!(
                parse_u128(&vector.reserve_y_after),
                params.reserve_y,
                "{label}: reserve Y"
            );
            math_revert += 1;
            total += 1;
            continue;
        }

        let quote =
            quote_result.unwrap_or_else(|error| panic!("{label}: Rust quote reverted: {error}"));
        let simulation = simulation_result
            .unwrap_or_else(|error| panic!("{label}: Rust simulation reverted: {error}"));
        let expected_status = match vector.outcome {
            VectorOutcome::Applied => {
                applied += 1;
                SimulationStatus::Applied
            }
            VectorOutcome::SwapImpossible => {
                swap_impossible += 1;
                SimulationStatus::RolledBack(RollbackReason::SwapImpossible)
            }
            VectorOutcome::ReserveTransitionOverflow => {
                reserve_overflow += 1;
                SimulationStatus::RolledBack(RollbackReason::ReserveTransitionOverflow)
            }
            VectorOutcome::MathMulDivRevert => unreachable!(),
        };
        assert_eq!(simulation.status, expected_status, "{label}: exact outcome");
        let effective = simulation.effective_params();

        assert_eq!(quote, simulation.quote, "{label}: quote/simulation quote");
        assert_eq!(
            quote.amount_out,
            parse_u256(&vector.amount_out),
            "{label}: amountOut"
        );
        assert_eq!(
            quote.sqrt_price_next,
            parse_u256(&vector.p_next),
            "{label}: pNext"
        );
        assert_eq!(
            quote.fee,
            parse_u256(&vector.fee_amount),
            "{label}: feeAmount"
        );
        assert_eq!(
            quote.effective_fee_x24,
            parse_u32(&vector.effective_fee_x24),
            "{label}: effectiveFeeX24"
        );
        assert_eq!(
            simulation.punishment.desired_punishment_x24,
            parse_u32(&vector.desired_punishment_x24),
            "{label}: desiredPunishmentX24"
        );
        assert_eq!(
            simulation.punishment.applied_punishment_x24,
            parse_u32(&vector.applied_punishment_x24),
            "{label}: appliedPunishmentX24"
        );
        assert_eq!(
            effective.fee_ask_x24,
            parse_u32(&vector.fee_ask_x24_after),
            "{label}: ask after"
        );
        assert_eq!(
            effective.fee_bid_x24,
            parse_u32(&vector.fee_bid_x24_after),
            "{label}: bid after"
        );
        assert_eq!(
            effective.reserve_x,
            parse_u128(&vector.reserve_x_after),
            "{label}: reserve X after"
        );
        assert_eq!(
            effective.reserve_y,
            parse_u128(&vector.reserve_y_after),
            "{label}: reserve Y after"
        );

        if let Some(update) = &vector.update {
            assert_eq!(
                vector.outcome,
                VectorOutcome::Applied,
                "{label}: update outcome"
            );
            let updated = try_apply_update(
                effective,
                parse_u256(&update.anchor_price),
                parse_u32(&update.fee_ask_x24),
                parse_u32(&update.fee_bid_x24),
            )
            .unwrap_or_else(|error| panic!("{label}: Rust update reverted: {error}"));
            assert_eq!(
                updated.sqrt_price_x96,
                parse_u256(&update.anchor_price_after),
                "{label}: update anchor"
            );
            assert_eq!(
                updated.fee_ask_x24,
                parse_u32(&update.fee_ask_x24_after),
                "{label}: update ask"
            );
            assert_eq!(
                updated.fee_bid_x24,
                parse_u32(&update.fee_bid_x24_after),
                "{label}: update bid"
            );
            assert_eq!(
                updated.reserve_x,
                parse_u128(&update.reserve_x_after),
                "{label}: update reserve X"
            );
            assert_eq!(
                updated.reserve_y,
                parse_u128(&update.reserve_y_after),
                "{label}: update reserve Y"
            );
            assert_eq!(
                updated.max_punishment_x24,
                parse_u32(&update.max_punishment_x24_after),
                "{label}: update max punishment"
            );
            updates += 1;
        }
        total += 1;
    }

    assert!(total > 0, "{path}: empty vector corpus");
    assert!(x_to_y > 0, "{path}: no X -> Y rows");
    assert!(y_to_x > 0, "{path}: no Y -> X rows");
    assert!(applied > 0, "{path}: no applied rows");
    assert!(
        swap_impossible + reserve_overflow > 0,
        "{path}: no rollback rows"
    );
    if path == "deterministic_vectors.jsonl" {
        assert!(math_revert > 0, "{path}: no mulDiv revert rows");
        assert!(updates > 0, "{path}: no swap-punishment-update row");
    }
    eprintln!(
        "{path}: {total} exact matches ({x_to_y} X->Y, {y_to_x} Y->X, \
         {applied} applied, {swap_impossible} SwapImpossible, \
         {reserve_overflow} reserve overflow, {math_revert} mulDiv revert, {updates} update)"
    );
}

#[test]
fn deterministic_solidity_vectors_match_bit_for_bit() {
    replay(
        "deterministic_vectors.jsonl",
        include_str!("../deterministic_vectors.jsonl"),
    );
}

#[test]
fn fuzz_solidity_vectors_match_bit_for_bit() {
    replay("fuzz_vectors.jsonl", include_str!("../fuzz_vectors.jsonl"));
}
