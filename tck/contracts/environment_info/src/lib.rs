//! Environment Information Example
//!
//! Demonstrates Gateway 2 environment syscalls that provide access to execution context:
//! - get_caller: Get the direct caller address (EVM: CALLER)
//! - get_call_value: Get value sent with call (EVM: CALLVALUE)
//! - get_timestamp: Get block timestamp (EVM: TIMESTAMP)
//! - get_chain_id: Get chain ID (EVM: CHAINID)
//! - get_coinbase: Get block producer address (EVM: COINBASE)
//!
//! This example shows:
//! 1. How to access environment information
//! 2. How to use this info for access control
//! 3. How to validate call parameters
//! 4. Real-world use cases for each syscall

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{log, log_u64, log_pubkey, get_caller, get_call_value, get_timestamp, get_chain_id, get_coinbase, get_tx_sender, storage_read, storage_write, get_input_data};

// Storage keys
const OWNER_KEY: &[u8] = b"owner";
const MIN_VALUE_KEY: &[u8] = b"min_value";
const LAST_CALLER_KEY: &[u8] = b"last_caller";
const CALL_COUNT_KEY: &[u8] = b"call_count";

// Operation codes
const OP_SHOW_ENV: u8 = 0;
const OP_REQUIRE_OWNER: u8 = 1;
const OP_REQUIRE_VALUE: u8 = 2;
const OP_TIME_LOCK: u8 = 3;
const OP_CHAIN_SPECIFIC: u8 = 4;

/// Read owner from storage
fn read_owner() -> Option<[u8; 32]> {
    let mut buffer = [0u8; 32];
    let len = storage_read(OWNER_KEY, &mut buffer);

    if len == 32 {
        Some(buffer)
    } else {
        None
    }
}

/// Initialize owner if not set
fn initialize_owner() {
    if read_owner().is_none() {
        let caller = get_caller();
        let _ = storage_write(OWNER_KEY, &caller);
        log("Owner initialized to first caller");
    }
}

/// Check if caller is owner
fn is_owner() -> bool {
    let caller = get_caller();
    match read_owner() {
        Some(owner) => caller == owner,
        None => false,
    }
}

/// Display all environment information
fn show_environment() -> u64 {
    log("");
    log("=== ENVIRONMENT INFORMATION ===");
    log("");

    // 1. CALLER (get_caller)
    log("1. CALLER (direct caller of this contract):");
    let caller = get_caller();
    log_pubkey(&caller);
    log("   Use case: Access control, audit logs");
    log("   EVM equivalent: CALLER (0x33)");
    log("   Cost: 2 CU");
    log("");

    // 2. TX SENDER (get_tx_sender)
    log("2. TX SENDER (original transaction signer):");
    let tx_sender = get_tx_sender();
    log_pubkey(&tx_sender);
    log("   Use case: Track original user in CPI chains");
    log("   Note: CALLER != TX_SENDER in delegatecall");
    log("");

    // 3. CALL VALUE (get_call_value)
    log("3. CALL VALUE (tokens sent with this call):");
    let value = get_call_value();
    log_u64(value, 0, 0, 0, 0);
    log("   Use case: Payable functions, deposit tracking");
    log("   EVM equivalent: CALLVALUE (0x34)");
    log("   Cost: 2 CU");
    log("");

    // 4. TIMESTAMP (get_timestamp)
    log("4. BLOCK TIMESTAMP (current block time):");
    let timestamp = get_timestamp();
    log_u64(timestamp, 0, 0, 0, 0);
    log("   Use case: Time-based logic, vesting, locks");
    log("   EVM equivalent: TIMESTAMP (0x42)");
    log("   Cost: 2 CU");
    log("   WARNING: Block producers can manipulate slightly");
    log("");

    // 5. CHAIN ID (get_chain_id)
    log("5. CHAIN ID (blockchain identifier):");
    let chain_id = get_chain_id();
    log_u64(chain_id, 0, 0, 0, 0);
    log("   Use case: Prevent replay attacks across chains");
    log("   EVM equivalent: CHAINID (0x46)");
    log("   Cost: 2 CU");
    log("");

    // 6. COINBASE (get_coinbase)
    log("6. COINBASE (block producer/validator address):");
    let coinbase = get_coinbase();
    log_pubkey(&coinbase);
    log("   Use case: Validator rewards, MEV tracking");
    log("   EVM equivalent: COINBASE (0x41)");
    log("   Cost: 2 CU");
    log("");

    0
}

/// Example: Require caller to be owner
fn require_owner() -> u64 {
    log("=== OWNER-ONLY FUNCTION ===");

    let caller = get_caller();
    log("Caller address:");
    log_pubkey(&caller);

    if is_owner() {
        log("SUCCESS: Caller is owner");
        log("Using get_caller() for access control");
        0
    } else {
        log("ERROR: Caller is not owner");
        log("Access denied!");
        1
    }
}

/// Example: Require minimum value sent with call
fn require_minimum_value(min_value: u64) -> u64 {
    log("=== PAYABLE FUNCTION WITH MINIMUM VALUE ===");

    let value = get_call_value();
    log("Value sent with call:");
    log_u64(value, 0, 0, 0, 0);

    log("Minimum required:");
    log_u64(min_value, 0, 0, 0, 0);

    if value >= min_value {
        log("SUCCESS: Sufficient value sent");
        log("Using get_call_value() for payment validation");
        0
    } else {
        log("ERROR: Insufficient value");
        log("Transaction rejected!");
        2
    }
}

/// Example: Time-locked function (only callable after certain time)
fn time_locked_function(unlock_time: u64) -> u64 {
    log("=== TIME-LOCKED FUNCTION ===");

    let current_time = get_timestamp();
    log("Current timestamp:");
    log_u64(current_time, 0, 0, 0, 0);

    log("Unlock time:");
    log_u64(unlock_time, 0, 0, 0, 0);

    if current_time >= unlock_time {
        log("SUCCESS: Time lock expired");
        log("Using get_timestamp() for time-based logic");
        log("Use cases: Vesting, delayed execution, voting periods");
        0
    } else {
        let remaining = unlock_time - current_time;
        log("ERROR: Still time-locked");
        log("Seconds remaining:");
        log_u64(remaining, 0, 0, 0, 0);
        3
    }
}

/// Example: Chain-specific behavior
fn chain_specific_logic() -> u64 {
    log("=== CHAIN-SPECIFIC LOGIC ===");

    let chain_id = get_chain_id();
    log("Current chain ID:");
    log_u64(chain_id, 0, 0, 0, 0);

    // Example: Different behavior on different chains
    match chain_id {
        1 => {
            log("Running on TOS Mainnet (chain 1)");
            log("Using mainnet parameters");
        }
        2 => {
            log("Running on TOS Testnet (chain 2)");
            log("Using testnet parameters");
        }
        _ => {
            log("Running on unknown chain");
            log("Using default parameters");
        }
    }

    log("Use cases:");
    log("- Prevent replay attacks across chains");
    log("- Enable/disable features per chain");
    log("- Cross-chain message verification");

    0
}

/// Record caller information for analytics
fn record_call_info() {
    let caller = get_caller();
    let value = get_call_value();
    let timestamp = get_timestamp();

    // Store last caller
    let _ = storage_write(LAST_CALLER_KEY, &caller);

    // Increment call count
    let mut count_buf = [0u8; 8];
    let count_len = storage_read(CALL_COUNT_KEY, &mut count_buf);

    let count = if count_len == 8 {
        u64::from_le_bytes(count_buf)
    } else {
        0
    };

    let new_count = count + 1;
    let count_bytes = new_count.to_le_bytes();
    let _ = storage_write(CALL_COUNT_KEY, &count_bytes);

    log("Call information recorded:");
    log("- Caller address stored");
    log("- Call count incremented");
    log_u64(new_count, value, timestamp, 0, 0);
}

/// Main contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Environment Information Contract ===");
    log("Demonstrates Gateway 2 environment syscalls");

    // Initialize owner on first call
    initialize_owner();

    // Record call information
    record_call_info();

    // Read input
    let mut input = [0u8; 256];
    let input_len = get_input_data(&mut input);

    if input_len == 0 {
        log("No input - showing all environment info");
        return show_environment();
    }

    let op = input[0];

    match op {
        OP_SHOW_ENV => show_environment(),

        OP_REQUIRE_OWNER => require_owner(),

        OP_REQUIRE_VALUE => {
            let min_value = 1000u64; // Example: require 1000 tokens
            require_minimum_value(min_value)
        }

        OP_TIME_LOCK => {
            // Example: unlock after 1 hour from now
            let current_time = get_timestamp();
            let unlock_time = current_time + 3600;
            time_locked_function(unlock_time)
        }

        OP_CHAIN_SPECIFIC => chain_specific_logic(),

        _ => {
            log("Unknown operation");
            show_environment()
        }
    }
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
