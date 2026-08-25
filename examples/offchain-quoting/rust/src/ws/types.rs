use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use alloy::primitives::B256;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum ChainEvent {
    /// The confirmed-head subscription was acknowledged. The consumer must take
    /// a fresh pinned snapshot before quotes can become healthy.
    Connected,
    /// The transport or subscription failed. Cached quotes must fail closed.
    Disconnected,
    Head {
        number: u64,
        hash: B256,
    },
}

/// Associates a chain event with the Redis-backed connection epoch that
/// produced it. A queued event from an old WebSocket connection must never be
/// allowed to make quotes healthy after that connection has failed.
#[derive(Debug, Clone)]
pub struct ConnectionEvent {
    pub epoch: u64,
    /// Redis snapshot generation allocated when this head was observed. It is
    /// `None` for connection lifecycle events.
    pub generation: Option<u64>,
    pub event: ChainEvent,
}

impl ConnectionEvent {
    pub const fn new(epoch: u64, event: ChainEvent) -> Self {
        Self {
            epoch,
            generation: None,
            event,
        }
    }

    pub const fn head(epoch: u64, generation: u64, number: u64, hash: B256) -> Self {
        Self {
            epoch,
            generation: Some(generation),
            event: ChainEvent::Head { number, hash },
        }
    }
}

/// Fast in-process guard for queued WebSocket events. Redis remains the final
/// atomic publication authority, but this guard prevents known-stale heads
/// from starting unnecessary RPC work after a disconnect or reconnect.
#[derive(Default)]
struct ActiveConnectionState {
    epoch: AtomicU64,
    head_generation: AtomicU64,
}

#[derive(Clone, Default)]
pub struct ActiveConnectionEpoch(Arc<ActiveConnectionState>);

impl ActiveConnectionEpoch {
    pub fn activate(&self, epoch: u64) {
        debug_assert_ne!(epoch, 0, "connection epoch zero is reserved");
        self.0.head_generation.store(0, Ordering::Release);
        self.0.epoch.store(epoch, Ordering::Release);
    }

    pub fn invalidate(&self, epoch: u64) {
        if self
            .0
            .epoch
            .compare_exchange(epoch, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.0.head_generation.store(0, Ordering::Release);
        }
    }

    pub fn is_active(&self, epoch: u64) -> bool {
        epoch != 0 && self.0.epoch.load(Ordering::Acquire) == epoch
    }

    pub fn observe_head(&self, epoch: u64, generation: u64) -> bool {
        if !self.is_active(epoch) {
            return false;
        }
        self.0.head_generation.store(generation, Ordering::Release);
        self.is_active(epoch)
    }

    pub fn is_current_head(&self, epoch: u64, generation: u64) -> bool {
        self.is_active(epoch)
            && generation != 0
            && self.0.head_generation.load(Ordering::Acquire) == generation
    }
}

pub fn head(result: &Value) -> Option<ChainEvent> {
    Some(ChainEvent::Head {
        number: block_number(result)?,
        hash: block_hash(result)?,
    })
}

fn block_number(result: &Value) -> Option<u64> {
    let number_hex = result.get("number").and_then(|n| n.as_str())?;
    parse_hex_u64(number_hex)
}

fn block_hash(result: &Value) -> Option<B256> {
    let hash_hex = result.get("hash").and_then(Value::as_str)?;
    if !hash_hex.starts_with("0x") || hash_hex.len() != 66 {
        return None;
    }
    hash_hex.parse().ok()
}

pub fn parse_hex_u64(s: &str) -> Option<u64> {
    let digits = s.strip_prefix("0x")?;
    if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_heads_with_an_explicit_number_and_hash() {
        let value = serde_json::json!({
            "number": "0x2a",
            "hash": "0x1111111111111111111111111111111111111111111111111111111111111111"
        });
        assert!(matches!(
            head(&value),
            Some(ChainEvent::Head { number: 42, hash })
                if hash == B256::repeat_byte(0x11)
        ));
    }

    #[test]
    fn rejects_missing_or_malformed_head_identity() {
        let valid_number = "0x2a";
        let valid_hash = "0x1111111111111111111111111111111111111111111111111111111111111111";

        for value in [
            serde_json::json!({ "hash": valid_hash }),
            serde_json::json!({ "number": valid_number }),
            serde_json::json!({ "number": "latest", "hash": valid_hash }),
            serde_json::json!({ "number": "0x02a", "hash": valid_hash }),
            serde_json::json!({ "number": valid_number, "hash": "0x1234" }),
            serde_json::json!({
                "number": valid_number,
                "hash": "0xzz11111111111111111111111111111111111111111111111111111111111111"
            }),
            serde_json::json!({ "number": null, "hash": valid_hash }),
            serde_json::json!({ "number": valid_number, "hash": null }),
        ] {
            assert!(head(&value).is_none(), "unexpected valid head: {value}");
        }
    }

    #[test]
    fn invalidating_an_old_epoch_cannot_disable_or_revive_the_new_epoch() {
        let active = ActiveConnectionEpoch::default();
        active.activate(7);
        let queued_old_head = ConnectionEvent::new(
            7,
            ChainEvent::Head {
                number: 42,
                hash: B256::repeat_byte(0x11),
            },
        );

        active.invalidate(7);
        active.activate(8);

        assert!(!active.is_active(queued_old_head.epoch));
        assert!(active.is_active(8));
        active.invalidate(7);
        assert!(active.is_active(8));
    }

    #[test]
    fn a_newly_observed_head_supersedes_the_previous_generation_immediately() {
        let active = ActiveConnectionEpoch::default();
        active.activate(7);
        assert!(active.observe_head(7, 101));
        assert!(active.is_current_head(7, 101));

        assert!(active.observe_head(7, 102));
        assert!(!active.is_current_head(7, 101));
        assert!(active.is_current_head(7, 102));
    }
}
