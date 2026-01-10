//! Proxy Contract Example
//!
//! Demonstrates the proxy pattern using delegatecall (via call with DELEGATE flag).
//! This example shows how to create an upgradeable contract where:
//! - The proxy contract holds state and delegates logic to an implementation contract
//! - The implementation can be upgraded by changing the implementation address
//! - State remains in the proxy contract (not the implementation)
//!
//! # Architecture
//!
//! ```
//! User → Proxy Contract → (DELEGATECALL) → Implementation Contract
//!        [State Storage]                    [Logic Only]
//! ```
//!
//! # How DELEGATECALL Works
//!
//! Unlike a regular CALL:
//! - Code from implementation contract is executed
//! - But storage, caller, and value come from the proxy's context
//! - Implementation sees proxy's storage, not its own
//!
//! This is equivalent to EVM's DELEGATECALL (0xF4).

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{log, storage_read, storage_write, call, get_caller, get_input_data, set_return_data};

// Storage keys
const IMPLEMENTATION_KEY: &[u8] = b"impl_addr"; // Address of implementation contract
const COUNTER_KEY: &[u8] = b"counter"; // Example state (stored in proxy)
const OWNER_KEY: &[u8] = b"owner"; // Contract owner

// Call flags
const CALL_FLAG_DELEGATE: u32 = 0x2;

// Operation codes
const OP_GET_COUNTER: u8 = 0;
const OP_INCREMENT: u8 = 1;
const OP_SET_IMPLEMENTATION: u8 = 2;
const OP_GET_IMPLEMENTATION: u8 = 3;

/// Read implementation address from storage
fn read_implementation() -> Option<[u8; 32]> {
    let mut buffer = [0u8; 32];
    let len = storage_read(IMPLEMENTATION_KEY, &mut buffer);

    if len == 32 {
        Some(buffer)
    } else {
        None
    }
}

/// Write implementation address to storage
fn write_implementation(addr: &[u8; 32]) -> bool {
    storage_write(IMPLEMENTATION_KEY, addr).is_ok()
}

/// Check if caller is owner
fn is_owner() -> bool {
    let caller = get_caller();

    let mut owner = [0u8; 32];
    let len = storage_read(OWNER_KEY, &mut owner);

    len == 32 && caller == owner
}

/// Initialize owner (only if not set)
fn initialize_owner() {
    let mut owner = [0u8; 32];
    let len = storage_read(OWNER_KEY, &mut owner);

    // If owner not set, set caller as owner
    if len != 32 {
        let caller = get_caller();
        let _ = storage_write(OWNER_KEY, &caller);
        log("Owner initialized");
    }
}

/// Read counter from storage
fn read_counter() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(COUNTER_KEY, &mut buffer);

    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Write counter to storage
fn write_counter(value: u64) -> bool {
    let bytes = value.to_le_bytes();
    storage_write(COUNTER_KEY, &bytes).is_ok()
}

/// Delegate call to implementation contract
///
/// This is the key feature: DELEGATECALL preserves the proxy's context
/// - Storage operations affect the proxy's storage
/// - msg.sender remains the original caller
/// - msg.value remains the original value
fn delegate_to_implementation(input_data: &[u8]) -> u64 {
    log("Delegating to implementation contract");

    // Read implementation address
    let impl_addr = match read_implementation() {
        Some(addr) => addr,
        None => {
            log("ERROR: No implementation set");
            return 1;
        }
    };

    // Make DELEGATECALL to implementation
    // Note: CALL_FLAG_DELEGATE (0x2) tells call to use DELEGATECALL semantics
    let result = call(&impl_addr, input_data, 0, CALL_FLAG_DELEGATE);

    if result.is_ok() {
        log("Delegation succeeded");
        log("IMPORTANT: Implementation executed in proxy's context");
        log("- Storage changes affected proxy's storage");
        log("- Caller remained the original caller");
        log("- This is how upgradeable contracts work!");
        0
    } else {
        log("ERROR: Delegation failed");
        result.unwrap_err()
    }
}

/// Main contract entrypoint - PROXY SIDE
///
/// This contract acts as a proxy that delegates all logic to an implementation contract.
/// It demonstrates:
/// 1. How to use DELEGATECALL to execute code from another contract
/// 2. How state remains in the proxy (this contract)
/// 3. How to implement upgradeable contracts
///
/// Instruction format:
/// - Byte 0: operation code
/// - Bytes 1+: operation-specific data
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Proxy Contract ===");
    log("This contract delegates logic to an implementation");

    // Initialize owner on first call
    initialize_owner();

    // Read input data
    let mut input = [0u8; 256];
    let input_len = get_input_data(&mut input);

    if input_len == 0 {
        log("Running demo mode (no input data)");
        return demo_mode();
    }

    let op = input[0];

    match op {
        OP_GET_COUNTER => {
            log("Operation: Get Counter (handled by proxy)");
            let counter = read_counter();
            let bytes = counter.to_le_bytes();
            let _ = set_return_data(&bytes);
            log("Returning counter value");
            0
        }

        OP_INCREMENT => {
            log("Operation: Increment (delegated to implementation)");
            // Delegate to implementation contract
            // The implementation will increment the counter in the proxy's storage
            delegate_to_implementation(&input[..input_len as usize])
        }

        OP_SET_IMPLEMENTATION => {
            log("Operation: Set Implementation");

            // Only owner can change implementation
            if !is_owner() {
                log("ERROR: Only owner can set implementation");
                return 2;
            }

            if input_len < 33 {
                log("ERROR: Invalid input length");
                return 3;
            }

            let mut new_impl = [0u8; 32];
            new_impl.copy_from_slice(&input[1..33]);

            if write_implementation(&new_impl) {
                log("Implementation updated successfully");
                log("Contract is now upgraded!");
                0
            } else {
                log("ERROR: Failed to update implementation");
                4
            }
        }

        OP_GET_IMPLEMENTATION => {
            log("Operation: Get Implementation");
            match read_implementation() {
                Some(addr) => {
                    let _ = set_return_data(&addr);
                    log("Returning implementation address");
                    0
                }
                None => {
                    log("No implementation set");
                    5
                }
            }
        }

        _ => {
            log("Unknown operation - delegating to implementation");
            delegate_to_implementation(&input[..input_len as usize])
        }
    }
}

/// Demo mode - shows how the proxy pattern works
fn demo_mode() -> u64 {
    log("");
    log("=== PROXY PATTERN DEMONSTRATION ===");
    log("");
    log("1. PROXY CONTRACT (this contract):");
    log("   - Holds all state (counter, implementation address, owner)");
    log("   - Delegates logic execution to implementation");
    log("   - Can be upgraded by changing implementation address");
    log("");
    log("2. IMPLEMENTATION CONTRACT:");
    log("   - Contains the business logic (increment, decrement, etc)");
    log("   - NO state storage of its own");
    log("   - Operates on proxy's storage via DELEGATECALL");
    log("");
    log("3. DELEGATECALL SEMANTICS:");
    log("   - Code: from implementation contract");
    log("   - Storage: from proxy contract");
    log("   - msg.sender: original caller (not proxy)");
    log("   - msg.value: original value");
    log("");
    log("4. UPGRADEABILITY:");
    log("   - Change implementation address → new logic");
    log("   - State in proxy remains intact");
    log("   - No data migration needed!");
    log("");
    log("5. SECURITY CONSIDERATIONS:");
    log("   - Only owner can change implementation");
    log("   - Implementation must use same storage layout");
    log("   - Careful with storage collisions");
    log("");

    // Show current state
    let counter = read_counter();
    log("Current counter value in proxy storage:");
    // (would log the value here with proper formatting)

    match read_implementation() {
        Some(_) => {
            log("Implementation contract: SET");
            log("Ready to delegate calls!");
        }
        None => {
            log("Implementation contract: NOT SET");
            log("Use OP_SET_IMPLEMENTATION to set it");
        }
    }

    log("");
    log("To use this contract:");
    log("1. Deploy implementation contract");
    log("2. Call SET_IMPLEMENTATION with impl address");
    log("3. Call INCREMENT - will delegatecall to impl");
    log("4. Counter in proxy storage will be incremented!");

    0
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
