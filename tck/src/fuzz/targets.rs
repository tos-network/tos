//! Fuzz target definitions

use super::*;

/// Get all available fuzz targets
pub fn all_targets() -> Vec<Box<dyn FuzzTarget>> {
    vec![
        Box::new(TransactionFuzzer),
        Box::new(BlockFuzzer),
        Box::new(SyscallFuzzer),
        Box::new(P2pMessageFuzzer),
        Box::new(RpcRequestFuzzer),
    ]
}

/// Get a fuzz target by name
pub fn get_target(name: &str) -> Option<Box<dyn FuzzTarget>> {
    match name {
        "fuzz_transaction" => Some(Box::new(TransactionFuzzer)),
        "fuzz_block" => Some(Box::new(BlockFuzzer)),
        "fuzz_syscall" => Some(Box::new(SyscallFuzzer)),
        "fuzz_p2p_message" => Some(Box::new(P2pMessageFuzzer)),
        "fuzz_rpc_request" => Some(Box::new(RpcRequestFuzzer)),
        _ => None,
    }
}

/// List all target names
pub fn target_names() -> Vec<&'static str> {
    vec![
        "fuzz_transaction",
        "fuzz_block",
        "fuzz_syscall",
        "fuzz_p2p_message",
        "fuzz_rpc_request",
    ]
}
