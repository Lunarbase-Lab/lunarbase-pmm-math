use alloy::primitives::{Bytes, B256};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum ChainEvent {
    Head { number: u64 },
    Flashblock { number: u64 },
    Log(LogEvent),
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogEvent {
    pub topics: Vec<B256>,
    pub data: Bytes,
    #[serde(rename = "blockNumber", default, with = "hex_u64_opt")]
    pub block_number: Option<u64>,
    #[serde(rename = "logIndex", default, with = "hex_u64_opt")]
    pub log_index: Option<u64>,
    #[serde(rename = "transactionHash", default)]
    pub transaction_hash: Option<B256>,
}

pub fn route(_subscription_id: &str, result: Value) -> Option<ChainEvent> {
    if result.is_null() {
        return None;
    }

    if result.get("topics").is_some() && result.get("address").is_some() {
        let log: LogEvent = serde_json::from_value(result).ok()?;
        return Some(ChainEvent::Log(log));
    }

    let number_hex = result.get("number").and_then(|n| n.as_str())?;
    let number = parse_hex_u64(number_hex)?;

    if result.get("transactions").is_some() {
        Some(ChainEvent::Flashblock { number })
    } else {
        Some(ChainEvent::Head { number })
    }
}

pub fn parse_hex_u64(s: &str) -> Option<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).ok()
}

mod hex_u64_opt {
    use super::parse_hex_u64;
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        let opt: Option<String> = Option::deserialize(d)?;
        Ok(opt.and_then(|s| parse_hex_u64(&s)))
    }
}
