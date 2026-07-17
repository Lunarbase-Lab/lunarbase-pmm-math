use tracing::info;

use crate::abi::Pool;

pub fn observe(ev: &Pool::SwapExecuted) {
    let direction = if ev.xToY { "X->Y" } else { "Y->X" };
    info!(
        direction,
        recipient = %ev.recipient,
        dx = %ev.dx,
        dy = %ev.dy,
        fee = %ev.fee,
        "SwapExecuted observed; authoritative reserves come from Sync"
    );
}
