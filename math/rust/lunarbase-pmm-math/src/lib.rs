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
//! use lunarbase_pmm_math::{quote_x_to_y, PoolParams, U256};
//!
//! let params = PoolParams {
//!     sqrt_price_x48: 1u128 << 48,
//!     anchor_sqrt_price_x48: 1u128 << 48,
//!     fee_q48: 1u64 << 44,
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

pub use curve_pmm::{quote_x_to_y, quote_y_to_x, PoolParams, QuoteResult};
pub use uint256::{U256Ext, U256};
