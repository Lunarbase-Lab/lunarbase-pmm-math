use alloy::primitives::Address;
use serde_json::json;

pub fn subscription_messages(pool: Address) -> Vec<String> {
    let pool_str = format!("{:#x}", pool);
    vec![
        json!({
            "jsonrpc": "2.0",
            "id": "newHeads",
            "method": "eth_subscribe",
            "params": ["newHeads"],
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "id": "newFlashblocks",
            "method": "eth_subscribe",
            "params": ["newFlashblocks"],
        })
        .to_string(),
        json!({
            "jsonrpc": "2.0",
            "id": "pendingLogs",
            "method": "eth_subscribe",
            "params": ["pendingLogs", { "address": pool_str }],
        })
        .to_string(),
    ]
}
