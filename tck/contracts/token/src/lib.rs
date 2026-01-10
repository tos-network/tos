//! Simple Token Contract
//!
//! Demonstrates token operations with balance management and transfers.
//!
//! This is a simplified token contract that showcases:
//! - Balance storage per address
//! - Minting tokens to caller
//! - Transfer validation and execution
//! - Total supply tracking
//!
//! Note: This example uses a simplified design for demonstration.
//! A production token contract would need:
//! - Proper instruction parsing with recipient addresses
//! - Owner/authority checks
//! - Decimal precision handling
//! - Events/logging for transfers
//! - Allowance mechanism

#![no_std]
#![no_main]

use tako_sdk::*;

/// Amount to mint per call (for demonstration)
const MINT_AMOUNT: u64 = 100;

/// Demonstration recipient address for transfers
const DEMO_RECIPIENT: [u8; 32] = [0xFF; 32];

/// Transfer amount (for demonstration)
const TRANSFER_AMOUNT: u64 = 10;

/// Main contract entrypoint
///
/// This demo contract mints tokens to the caller and demonstrates
/// a transfer to a fixed recipient address.
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("Token: Starting operation");

    // Get caller address (transaction sender)
    let caller = get_tx_sender();

    // Step 1: Mint tokens to caller
    log("Token: Minting tokens to caller");
    let mint_result = mint_to_caller(&caller);
    if mint_result != SUCCESS {
        log("Token: Mint failed");
        return mint_result;
    }

    // Step 2: Perform a demonstration transfer
    log("Token: Executing demo transfer");
    let transfer_result = transfer(&caller, &DEMO_RECIPIENT, TRANSFER_AMOUNT);
    if transfer_result != SUCCESS {
        log("Token: Transfer failed");
        return transfer_result;
    }

    // Step 3: Log final balances
    log("Token: Querying final balances");
    let _caller_balance = get_balance_of(&caller);
    let _recipient_balance = get_balance_of(&DEMO_RECIPIENT);

    log("Token: Operation completed successfully");
    log("Caller balance updated");
    log("Recipient balance updated");

    SUCCESS
}

/// Mint tokens to the caller
fn mint_to_caller(address: &[u8; 32]) -> u64 {
    // Read current balance
    let current_balance = get_balance_of(address);

    // Calculate new balance
    let new_balance = current_balance.saturating_add(MINT_AMOUNT);

    // Write new balance
    if set_balance_of(address, new_balance) != SUCCESS {
        return ERROR;
    }

    // Update total supply
    let current_supply = get_total_supply();
    let new_supply = current_supply.saturating_add(MINT_AMOUNT);
    if set_total_supply(new_supply) != SUCCESS {
        return ERROR;
    }

    log("Token: Minted 100 tokens");
    SUCCESS
}

/// Transfer tokens from one address to another
fn transfer(from: &[u8; 32], to: &[u8; 32], amount: u64) -> u64 {
    // Check sender has sufficient balance
    let from_balance = get_balance_of(from);
    if from_balance < amount {
        log("Token: Insufficient balance");
        return ERROR;
    }

    // Deduct from sender
    let new_from_balance = from_balance - amount;
    if set_balance_of(from, new_from_balance) != SUCCESS {
        return ERROR;
    }

    // Add to recipient
    let to_balance = get_balance_of(to);
    let new_to_balance = to_balance.saturating_add(amount);
    if set_balance_of(to, new_to_balance) != SUCCESS {
        // Rollback: this is simplified, production needs atomic operations
        set_balance_of(from, from_balance);
        return ERROR;
    }

    log("Token: Transferred 10 tokens");
    SUCCESS
}

/// Get balance of an address
fn get_balance_of(address: &[u8; 32]) -> u64 {
    // Create storage key: "balance:" + address
    let mut key = [0u8; 40]; // 8 bytes prefix + 32 bytes address
    key[0..8].copy_from_slice(b"balance:");
    key[8..40].copy_from_slice(address);

    // Read from storage
    let mut buffer = [0u8; 8];
    let len = storage_read(&key, &mut buffer);

    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0u64
    }
}

/// Set balance of an address
fn set_balance_of(address: &[u8; 32], balance: u64) -> u64 {
    // Create storage key: "balance:" + address
    let mut key = [0u8; 40];
    key[0..8].copy_from_slice(b"balance:");
    key[8..40].copy_from_slice(address);

    // Write to storage
    let value_bytes = balance.to_le_bytes();
    match storage_write(&key, &value_bytes) {
        Ok(_) => SUCCESS,
        Err(_) => ERROR,
    }
}

/// Get total supply
fn get_total_supply() -> u64 {
    let key = b"total_supply";
    let mut buffer = [0u8; 8];
    let len = storage_read(key, &mut buffer);

    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0u64
    }
}

/// Set total supply
fn set_total_supply(supply: u64) -> u64 {
    let key = b"total_supply";
    let value_bytes = supply.to_le_bytes();
    match storage_write(key, &value_bytes) {
        Ok(_) => SUCCESS,
        Err(_) => ERROR,
    }
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
