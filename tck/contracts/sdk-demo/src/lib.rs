//! SDK Demo Contract
//!
//! This contract demonstrates the usage of TAKO SDK features:
//! - entrypoint! macro
//! - Type wrappers (Address, Balance, Hash)
//! - Helper macros (require!, define_errors!)
//! - Storage operations with types

#![no_std]
#![no_main]

use tako_sdk::*;

// Define custom error codes
define_errors! {
    InvalidAmount = 1001,
    InsufficientBalance = 1002,
    Unauthorized = 1003,
    TransferFailed = 1004,
}

// Use the entrypoint macro
entrypoint!(process_instruction);

/// Main contract logic
///
/// Demonstrates:
/// - Parameter validation with require! macro
/// - Type-safe operations with Balance
/// - Storage operations
/// - Transfer operations
fn process_instruction(_input: &[u8]) -> entrypoint::Result<()> {
    log("SDK Demo: Starting");

    // Get contract information using type wrappers
    let contract_hash = Hash::from(get_contract_hash());
    let block_height = get_block_height();
    let sender = Address::from(get_tx_sender());

    debug_log!("Contract initialized");

    // Demonstrate balance operations
    let balance = Balance::from(get_balance(&sender.0));
    require!(
        balance.as_u64() > 0,
        InvalidAmount,
        "Balance must be positive"
    );

    log("SDK Demo: Balance check passed");

    // Demonstrate storage operations with const keys
    const COUNTER_KEY: &[u8] = b"counter";

    // Read counter from storage
    let mut buffer = [0u8; 8];
    let len = storage_read(COUNTER_KEY, &mut buffer);

    let counter = if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0u64
    };

    // Increment counter
    let new_counter = counter.saturating_add(1);
    let counter_bytes = new_counter.to_le_bytes();

    // Write back to storage
    storage_write(COUNTER_KEY, &counter_bytes).map_err(|_| TransferFailed)?;

    log("SDK Demo: Counter incremented");

    // Demonstrate Balance operations
    let amount1 = Balance::new(100);
    let amount2 = Balance::new(50);
    let total = amount1.saturating_add(amount2);

    require!(total.as_u64() == 150, InvalidAmount);

    log("SDK Demo: Completed successfully");

    Ok(())
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
