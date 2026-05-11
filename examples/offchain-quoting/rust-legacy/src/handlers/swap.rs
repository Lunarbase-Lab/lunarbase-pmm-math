use eyre::Result;
use lunarbase_pmm_math::{quote_x_to_y, quote_y_to_x, U256};
use tracing::{info, warn};

use crate::abi::Pool;
use crate::cache::Cache;
use crate::pool_state::{u256_to_u128_saturating, PoolState};

pub async fn apply(ev: &Pool::SwapExecuted, snap: &PoolState, cache: &mut Cache) -> Result<()> {
    let params = snap.to_params();
    let (sqrt_price_next, gross_x_in, gross_y_in) = if ev.xToY {
        let q = quote_x_to_y(&params, ev.dx);
        if q.amount_out.is_zero() && q.fee.is_zero() {
            warn!("local quote_x_to_y rejected the on-chain swap; keeping prior state");
            return Ok(());
        }
        sanity_check_fee(q.fee, ev.fee);
        (q.sqrt_price_next, ev.dx, q.amount_out + q.fee)
    } else {
        let q = quote_y_to_x(&params, ev.dy);
        if q.amount_out.is_zero() && q.fee.is_zero() {
            warn!("local quote_y_to_x rejected the on-chain swap; keeping prior state");
            return Ok(());
        }
        sanity_check_fee(q.fee, ev.fee);
        (q.sqrt_price_next, q.amount_out + q.fee, ev.dy)
    };

    let (new_x, new_y) = if ev.xToY {
        let x = snap
            .reserve_x
            .saturating_add(u256_to_u128_saturating(gross_x_in));
        let y = snap
            .reserve_y
            .saturating_sub(u256_to_u128_saturating(gross_y_in));
        (x, y)
    } else {
        let x = snap
            .reserve_x
            .saturating_sub(u256_to_u128_saturating(gross_x_in));
        let y = snap
            .reserve_y
            .saturating_add(u256_to_u128_saturating(gross_y_in));
        (x, y)
    };

    cache.apply_swap(sqrt_price_next, new_x, new_y).await?;

    let direction = if ev.xToY { "X->Y" } else { "Y->X" };
    info!(
        direction,
        dx = %ev.dx,
        dy = %ev.dy,
        sqrt_price_x96 = %sqrt_price_next,
        reserve_x = new_x,
        reserve_y = new_y,
        "swap applied; live price (informational) and reserves updated"
    );
    Ok(())
}

fn sanity_check_fee(local: U256, on_chain: U256) {
    if local != on_chain {
        let diff = if local > on_chain {
            local - on_chain
        } else {
            on_chain - local
        };
        warn!(
            %local,
            %on_chain,
            %diff,
            "fee from local quote diverges from on-chain SwapExecuted.fee"
        );
    }
}
