//! Pure Rust mirror of LunarBase linear anchor quotes and directional
//! punishment state transitions.
//!
//! The crate is bit-for-bit identical with the on-chain Solidity reference
//! (current Q64.96 `uint160` sqrt-price design), validated by
//! focused unit vectors derived from the contract. It has no `unsafe`, no FFI,
//! no allocations on the hot path, and depends only on [`ruint`] for
//! fixed-width integers.
//!
//! # Quick start
//!
//! ```
//! use lunarbase_pmm_math::{
//!     quote_x_to_y, quote_x_to_y_with_multiplier, PoolParams, Q24, Q96, U256,
//! };
//!
//! let params = PoolParams {
//!     sqrt_price_x96: Q96, // price = 1.0
//!     fee_ask_x24: 0,
//!     fee_bid_x24: (1u32 << 24) / 1000, // 0.10% bid fee
//!     reserve_x: 1_000_000,
//!     reserve_y: 1_000_000,
//!     max_punishment_x24: Q24 / 1_000, // maximum 0.10% increment
//! };
//! let result = quote_x_to_y(&params, U256::from(1_000u64));
//! let non_whitelisted = quote_x_to_y_with_multiplier(&params, U256::from(1_000u64), U256::from(100u64));
//! let _ = result.amount_out;
//! let _ = non_whitelisted.amount_out;
//! ```

pub mod fee_accounting;
pub mod mechanism;
pub mod order_book;
pub mod order_book_policy;
pub mod order_book_precision;
pub mod uint256;

/// Backward-compatible module path for consumers that imported the former
/// quote kernel through `curve_pmm`. Its contents now implement linear quotes
/// and immediate punishment; no concentration curve remains.
#[deprecated(
    since = "0.4.0",
    note = "use `mechanism`; concentration and slippage APIs were removed"
)]
pub mod curve_pmm {
    pub use crate::mechanism::*;
}

pub use fee_accounting::{
    try_validate_fee_accounting_capacity, FeeAccountingError, FeeAccountingState, FeeBucket,
    PARTNER_FEE_SCALE,
};
pub use mechanism::{
    apply_fee, apply_punishment, apply_update, price_to_sqrt_price_x96, punishment_x24,
    quote_x_to_y, quote_x_to_y_with_multiplier, quote_y_to_x, quote_y_to_x_with_multiplier,
    simulate_successful_swap, sqrt_price_x96_to_price, try_apply_fee, try_apply_punishment,
    try_apply_update, try_punishment_transition, try_punishment_x24, try_quote_x_to_y,
    try_quote_x_to_y_with_multiplier, try_quote_y_to_x, try_quote_y_to_x_with_multiplier,
    try_simulate_successful_swap, try_x_value_in_y, x_value_in_y, Direction, MathError, PoolParams,
    PunishmentTransition, QuoteResult, RollbackReason, SimulationStatus, SwapSimulation,
    MAX_PUNISHMENT_X24, MAX_U112, MAX_U160, MAX_U24, Q24, Q96,
};
pub use order_book::{
    geometric_sizes, try_build_order_book, try_ladder_amount_out, try_ladder_amount_out_at_cursor,
    DirectionalLadder, OrderBook, OrderBookConfig, OrderBookError, OrderBookLevel, OrderBookSafety,
    OrderBookState, OrderBookStatus, MAX_ORDER_BOOK_LEVELS, PRICE_SCALE_X18,
};
pub use order_book_policy::{
    try_build_validated_order_book, FillPolicy, FillPolicyError, ValidatedOrderBook,
    ValidatedOrderBookConfig, MAX_VALIDATION_TRANSITIONS,
};
pub use order_book_precision::{
    try_build_precise_order_book, OrderBookPrecision, PreciseOrderBook, PreciseOrderBookError,
    MAX_PRECISION_WORK,
};
pub use uint256::{U256Ext, U256};
