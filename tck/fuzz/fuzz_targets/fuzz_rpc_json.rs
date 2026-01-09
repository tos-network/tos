//! Fuzz target for RPC JSON parsing
//!
//! Tests that arbitrary JSON inputs do not cause panics
//! when parsed as RPC requests.

#![no_main]

use libfuzzer_sys::fuzz_target;

/// RPC request structure for fuzzing
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct RpcRequest {
    jsonrpc: Option<String>,
    method: Option<String>,
    params: Option<serde_json::Value>,
    id: Option<serde_json::Value>,
}

fuzz_target!(|data: &[u8]| {
    // Try to parse as UTF-8 string first
    if let Ok(json_str) = std::str::from_utf8(data) {
        // Attempt to parse as RPC request
        // Should never panic, only return errors
        let _ = serde_json::from_str::<RpcRequest>(json_str);

        // Also try parsing as generic JSON value
        let _ = serde_json::from_str::<serde_json::Value>(json_str);
    }
});
