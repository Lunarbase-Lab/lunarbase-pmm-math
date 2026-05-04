//! N-API binding exposing [`lunarbase_pmm_math`] to Node.js.
#![allow(
    missing_docs,
    clippy::needless_pass_by_value,
    clippy::missing_safety_doc
)]

use lunarbase_pmm_math::curve_pmm::{self, PoolParams, QuoteResult as InternalQuoteResult};
use lunarbase_pmm_math::uint256::{U256Ext, U256};
use napi::bindgen_prelude::*;
use napi_derive::napi;

/// Parse a BigInt-compatible string (decimal or 0x hex) into U256.
fn parse_u256(s: &str) -> Result<U256> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        let bytes = hex_mod::decode_padded(hex)?;
        let mut limbs = [0u8; 32];
        let offset = (32usize).saturating_sub(bytes.len());
        limbs[offset..offset + bytes.len()].copy_from_slice(&bytes);
        Ok(U256::from_be_bytes(limbs))
    } else {
        parse_decimal_u256(s)
    }
}

fn parse_decimal_u256(s: &str) -> Result<U256> {
    let mut result = U256::ZERO;
    let ten = U256::from(10u64);
    for &b in s.as_bytes() {
        if !b.is_ascii_digit() {
            return Err(Error::from_reason(format!("invalid digit in '{s}'")));
        }
        result = result
            .wrapping_mul(ten)
            .wrapping_add(U256::from(u64::from(b - b'0')));
    }
    Ok(result)
}

mod hex_mod {
    use napi::Error;
    use napi::Result;

    pub(super) fn decode_padded(hex: &str) -> Result<Vec<u8>> {
        let hex = if hex.len() % 2 == 1 {
            format!("0{hex}")
        } else {
            hex.to_owned()
        };
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        let chars = hex.as_bytes();
        for i in (0..chars.len()).step_by(2) {
            let hi = hex_digit(chars[i])?;
            let lo = hex_digit(chars[i + 1])?;
            bytes.push((hi << 4) | lo);
        }
        Ok(bytes)
    }

    fn hex_digit(c: u8) -> Result<u8> {
        match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            b'A'..=b'F' => Ok(c - b'A' + 10),
            _ => Err(Error::from_reason(format!(
                "invalid hex digit: {}",
                c as char
            ))),
        }
    }
}

fn u256_to_string(v: U256) -> String {
    if v.is_zero() {
        return "0".to_owned();
    }

    if v <= U256::from(u128::MAX) {
        return v.as_u128().to_string();
    }

    let mut digits = Vec::new();
    let mut remaining = v;
    let ten = U256::from(10u64);
    while !remaining.is_zero() {
        let r = remaining % ten;
        digits.push((r.as_u128() as u8) + b'0');
        remaining /= ten;
    }
    digits.reverse();
    String::from_utf8(digits).unwrap()
}

fn parse_u128_field(s: &str) -> Result<u128> {
    let v = parse_u256(s)?;
    if !v.fits_u128() {
        return Err(Error::from_reason(format!("value too large for u128: {s}")));
    }
    Ok(v.as_u128())
}

fn parse_u80_field(s: &str) -> Result<u128> {
    let v = parse_u256(s)?;
    if !v.fits_u80() {
        return Err(Error::from_reason(format!(
            "value too large for uint80: {s}"
        )));
    }
    Ok(v.as_u128())
}

#[napi(object)]
pub struct QuoteParams {
    /// sqrtPriceX48 as decimal or hex string
    pub sqrt_price_x48: String,
    /// anchor sqrtPriceX48 as decimal or hex string. Defaults to sqrtPriceX48 when omitted by callers compiled against older API.
    pub anchor_sqrt_price_x48: Option<String>,
    /// fee in Q48 as decimal or hex string
    pub fee_q48: String,
    /// reserve X as decimal or hex string
    pub reserve_x: String,
    /// reserve Y as decimal or hex string
    pub reserve_y: String,
    /// concentration multiplier k
    pub concentration_k: u32,
    /// input amount as decimal or hex string
    pub amount_in: String,
}

#[napi(object)]
pub struct QuoteResult {
    /// output amount as decimal string
    pub amount_out: String,
    /// new sqrt price as decimal string
    pub sqrt_price_next: String,
    /// fee amount as decimal string
    pub fee: String,
}

fn to_pool_params(p: &QuoteParams) -> Result<(PoolParams, U256)> {
    let sqrt_price = parse_u80_field(&p.sqrt_price_x48)?;
    let anchor_sqrt_price = match &p.anchor_sqrt_price_x48 {
        Some(anchor) => parse_u80_field(anchor)?,
        None => sqrt_price,
    };
    let fee_q48_val = parse_u128_field(&p.fee_q48)?;
    if fee_q48_val > u128::from(u64::MAX) {
        return Err(Error::from_reason("fee_q48 exceeds u48"));
    }
    let reserve_x = parse_u128_field(&p.reserve_x)?;
    let reserve_y = parse_u128_field(&p.reserve_y)?;
    let amount_in = parse_u256(&p.amount_in)?;

    Ok((
        PoolParams {
            sqrt_price_x48: sqrt_price,
            anchor_sqrt_price_x48: anchor_sqrt_price,
            fee_q48: fee_q48_val as u64,
            reserve_x,
            reserve_y,
            concentration_k: p.concentration_k,
        },
        amount_in,
    ))
}

fn from_internal_result(r: InternalQuoteResult) -> QuoteResult {
    QuoteResult {
        amount_out: u256_to_string(r.amount_out),
        sqrt_price_next: r.sqrt_price_next.to_string(),
        fee: u256_to_string(r.fee),
    }
}

#[napi(js_name = "quoteXToY")]
pub fn quote_x_to_y_napi(params: QuoteParams) -> Result<QuoteResult> {
    let (pool, amount_in) = to_pool_params(&params)?;
    let result = curve_pmm::quote_x_to_y(&pool, amount_in);
    Ok(from_internal_result(result))
}

#[napi(js_name = "quoteYToX")]
pub fn quote_y_to_x_napi(params: QuoteParams) -> Result<QuoteResult> {
    let (pool, amount_in) = to_pool_params(&params)?;
    let result = curve_pmm::quote_y_to_x(&pool, amount_in);
    Ok(from_internal_result(result))
}
