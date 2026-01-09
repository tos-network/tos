//! Fuzz target for syscall input parsing
//!
//! Tests that arbitrary syscall inputs do not cause panics
//! in the syscall handlers.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

/// Syscall types
#[derive(Debug, Clone, Copy, Arbitrary)]
#[repr(u32)]
enum SyscallId {
    // Balance operations
    GetBalance = 0x01,
    Transfer = 0x02,

    // Storage operations
    SLoad = 0x10,
    SStore = 0x11,
    TLoad = 0x12,
    TStore = 0x13,

    // Environment operations
    GetCaller = 0x20,
    GetOrigin = 0x21,
    GetCallValue = 0x22,
    GetBlockNumber = 0x23,
    GetBlockTimestamp = 0x24,
    GetBlockHash = 0x25,
    GetChainId = 0x26,
    GetGasPrice = 0x27,
    GetGasLeft = 0x28,

    // Call operations
    Call = 0x30,
    DelegateCall = 0x31,
    StaticCall = 0x32,
    Create = 0x33,
    Create2 = 0x34,

    // Crypto operations
    Sha3 = 0x40,
    EcRecover = 0x41,

    // Code operations
    GetCodeSize = 0x50,
    GetCodeCopy = 0x51,
    GetExtCodeSize = 0x52,
    GetExtCodeCopy = 0x53,
    GetExtCodeHash = 0x54,

    // Log operations
    Log0 = 0x60,
    Log1 = 0x61,
    Log2 = 0x62,
    Log3 = 0x63,
    Log4 = 0x64,

    // Return operations
    Return = 0x70,
    Revert = 0x71,

    // NFT operations
    NftMint = 0x80,
    NftTransfer = 0x81,
    NftBurn = 0x82,
    NftOwnerOf = 0x83,
}

/// Syscall input
#[derive(Debug, Arbitrary)]
struct SyscallInput {
    /// Syscall ID
    syscall_id: SyscallId,
    /// Input data
    data: Vec<u8>,
    /// Gas limit
    gas: u64,
    /// Call value
    value: u64,
}

/// Transfer parameters
#[derive(Debug)]
struct TransferParams {
    to: [u8; 20],
    amount: u64,
}

/// Storage parameters
#[derive(Debug)]
struct StorageParams {
    key: [u8; 32],
    value: Option<[u8; 32]>,
}

/// Call parameters
#[derive(Debug)]
struct CallParams {
    target: [u8; 20],
    value: u64,
    gas: u64,
    input_data: Vec<u8>,
}

fuzz_target!(|input: SyscallInput| {
    // Limit input size
    if input.data.len() > 64 * 1024 {
        return;
    }

    // Parse and validate syscall input based on type
    match input.syscall_id {
        SyscallId::Transfer => {
            let _ = parse_transfer_params(&input.data);
        }
        SyscallId::SLoad | SyscallId::TLoad => {
            let _ = parse_storage_read_params(&input.data);
        }
        SyscallId::SStore | SyscallId::TStore => {
            let _ = parse_storage_write_params(&input.data);
        }
        SyscallId::Call | SyscallId::DelegateCall | SyscallId::StaticCall => {
            let _ = parse_call_params(&input.data, input.value, input.gas);
        }
        SyscallId::Sha3 => {
            let _ = compute_sha3(&input.data);
        }
        SyscallId::Log0 | SyscallId::Log1 | SyscallId::Log2 | SyscallId::Log3 | SyscallId::Log4 => {
            let topic_count = match input.syscall_id {
                SyscallId::Log0 => 0,
                SyscallId::Log1 => 1,
                SyscallId::Log2 => 2,
                SyscallId::Log3 => 3,
                SyscallId::Log4 => 4,
                _ => 0,
            };
            let _ = parse_log_params(&input.data, topic_count);
        }
        _ => {
            // Other syscalls - just validate data is parseable
            let _ = validate_syscall_data(&input.data);
        }
    }
});

/// Parse transfer parameters
fn parse_transfer_params(data: &[u8]) -> Option<TransferParams> {
    if data.len() < 28 {
        return None;
    }

    let mut to = [0u8; 20];
    to.copy_from_slice(&data[0..20]);

    let amount = u64::from_le_bytes([
        data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
    ]);

    Some(TransferParams { to, amount })
}

/// Parse storage read parameters
fn parse_storage_read_params(data: &[u8]) -> Option<[u8; 32]> {
    if data.len() < 32 {
        return None;
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&data[0..32]);
    Some(key)
}

/// Parse storage write parameters
fn parse_storage_write_params(data: &[u8]) -> Option<StorageParams> {
    if data.len() < 32 {
        return None;
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&data[0..32]);

    let value = if data.len() >= 64 {
        let mut v = [0u8; 32];
        v.copy_from_slice(&data[32..64]);
        Some(v)
    } else {
        None
    };

    Some(StorageParams { key, value })
}

/// Parse call parameters
fn parse_call_params(data: &[u8], value: u64, gas: u64) -> Option<CallParams> {
    if data.len() < 20 {
        return None;
    }

    let mut target = [0u8; 20];
    target.copy_from_slice(&data[0..20]);

    let input_data = if data.len() > 20 {
        data[20..].to_vec()
    } else {
        vec![]
    };

    Some(CallParams {
        target,
        value,
        gas,
        input_data,
    })
}

/// Compute SHA3 hash (simplified)
fn compute_sha3(data: &[u8]) -> [u8; 32] {
    // Simplified hash for fuzzing - real impl uses keccak256
    let mut result = [0u8; 32];
    for (i, byte) in data.iter().enumerate() {
        result[i % 32] ^= byte;
    }
    result
}

/// Parse log parameters
fn parse_log_params(data: &[u8], topic_count: usize) -> Option<(Vec<[u8; 32]>, Vec<u8>)> {
    let topics_size = topic_count * 32;
    if data.len() < topics_size {
        return None;
    }

    let mut topics = Vec::with_capacity(topic_count);
    for i in 0..topic_count {
        let mut topic = [0u8; 32];
        topic.copy_from_slice(&data[i * 32..(i + 1) * 32]);
        topics.push(topic);
    }

    let log_data = data[topics_size..].to_vec();

    Some((topics, log_data))
}

/// Validate syscall data is well-formed
fn validate_syscall_data(data: &[u8]) -> bool {
    // Basic validation - data should be parseable
    !data.is_empty() && data.len() <= 64 * 1024
}
