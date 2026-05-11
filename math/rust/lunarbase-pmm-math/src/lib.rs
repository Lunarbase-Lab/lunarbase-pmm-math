//! Pure Rust port of the LunarBase Curve PMM quoting math.
//!
//! The crate is bit-for-bit identical with the on-chain Solidity reference,
//! validated by deterministic and fuzz vectors generated from the contract.
//! It has no `unsafe`, no FFI, no allocations on the hot path, and depends
//! only on [`ruint`] for fixed-width integers.
//!
//! # Quick start
//!
//! ```
//! use lunarbase_pmm_math::{quote_x_to_y, PoolParams, U256, U256Ext};
//!
//! let params = PoolParams {
//!     sqrt_price_x96: U256::Q96, // price = 1.0
//!     fee_ask_x24: 0,
//!     fee_bid_x24: (1u32 << 24) / 1000, // 0.10% bid fee
//!     reserve_x: 1_000_000,
//!     reserve_y: 1_000_000,
//!     concentration_k: 5_000,
//! };
//! let result = quote_x_to_y(&params, U256::from(1_000u64));
//! let _ = result.amount_out;
//! ```

pub mod curve_pmm;
pub mod sqrt_price_math;
pub mod uint256;

#[cfg(test)]
mod fuzz_tests;
#[cfg(test)]
mod tests;

pub use curve_pmm::{
    plain_to_q12_concentration_k, q12_to_plain_concentration_k, quote_x_to_y, quote_y_to_x,
    sqrt_price_x48_to_x96, sqrt_price_x96_to_x48, PoolParams, QuoteResult,
};
pub use uint256::{U256Ext, U256};
