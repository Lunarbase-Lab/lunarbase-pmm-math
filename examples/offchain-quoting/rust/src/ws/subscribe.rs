use serde_json::json;

pub const NEW_HEADS_REQUEST_ID: &str = "newHeads";

pub fn subscription_messages() -> Vec<String> {
    vec![json!({
        "jsonrpc": "2.0",
        "id": NEW_HEADS_REQUEST_ID,
        "method": "eth_subscribe",
        "params": ["newHeads"],
    })
    .to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribes_only_to_confirmed_heads() {
        let messages = subscription_messages();
        assert_eq!(messages.len(), 1);
        assert!(messages.iter().any(|message| message.contains("newHeads")));
        assert!(messages
            .iter()
            .all(|message| !message.contains("newFlashblocks")));
        assert!(messages
            .iter()
            .all(|message| !message.contains("pendingLogs")));
    }
}
