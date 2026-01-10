//! Native NFT Example Contract
//!
//! This contract demonstrates how to use the native NFT syscalls provided by
//! the TOS blockchain. Unlike ERC-721 contracts that manage their own state,
//! this contract leverages the blockchain's built-in NFT infrastructure.
//!
//! # Features
//!
//! - Query collection existence
//! - Mint new NFTs with metadata URIs
//! - Transfer NFTs between addresses
//! - Burn NFTs
//! - Approve operators for single tokens or all tokens
//! - Query token ownership and balances
//!
//! # Entrypoints
//!
//! Each function is exposed as a separate entrypoint for testing:
//! - `test_collection_exists` - Test collection existence check
//! - `test_mint` - Test minting an NFT
//! - `test_transfer` - Test transferring an NFT
//! - `test_burn` - Test burning an NFT
//! - `test_approvals` - Test approval mechanisms
//! - `test_queries` - Test query functions
//! - `entrypoint` - Run all tests

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{
    get_caller, log, nft_approve, nft_balance_of, nft_burn, nft_collection_exists, nft_exists,
    nft_get_approved, nft_is_approved_for_all, nft_mint, nft_owner_of, nft_set_approval_for_all,
    nft_token_uri, nft_transfer,
};

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Test collection ID (would be provided by the blockchain in real usage)
const TEST_COLLECTION: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

/// Test recipient address
const TEST_RECIPIENT: [u8; 32] = [
    0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
    0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
];

/// Test operator address
const TEST_OPERATOR: [u8; 32] = [
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
];

// ============================================================================
// Test 1: Collection Existence
// ============================================================================

/// Test checking if a collection exists
#[no_mangle]
pub extern "C" fn test_collection_exists() -> u64 {
    log("Test 1: nft_collection_exists");

    match nft_collection_exists(&TEST_COLLECTION) {
        Ok(exists) => {
            if exists {
                log("Collection exists");
            } else {
                log("Collection does not exist");
            }
            log("nft_collection_exists: PASS");
            0
        }
        Err(code) => {
            log("nft_collection_exists: FAIL");
            100 + code
        }
    }
}

// ============================================================================
// Test 2: Minting
// ============================================================================

/// Test minting a new NFT
#[no_mangle]
pub extern "C" fn test_mint() -> u64 {
    log("Test 2: nft_mint");

    let caller = get_caller();
    let uri = b"ipfs://QmExampleMetadataHash123456789";

    match nft_mint(&TEST_COLLECTION, &TEST_RECIPIENT, uri, &caller) {
        Ok(token_id) => {
            log("NFT minted successfully");
            // Log the token ID (would use log_u64 in real code)
            let _ = token_id;
            log("nft_mint: PASS");
            0
        }
        Err(code) => {
            log("nft_mint: FAIL");
            200 + code
        }
    }
}

// ============================================================================
// Test 3: Transfer
// ============================================================================

/// Test transferring an NFT
#[no_mangle]
pub extern "C" fn test_transfer() -> u64 {
    log("Test 3: nft_transfer");

    let caller = get_caller();
    let token_id = 1u64; // Assuming token 1 exists

    match nft_transfer(
        &TEST_COLLECTION,
        token_id,
        &caller,
        &TEST_RECIPIENT,
        &caller,
    ) {
        Ok(()) => {
            log("NFT transferred successfully");
            log("nft_transfer: PASS");
            0
        }
        Err(code) => {
            log("nft_transfer: FAIL");
            300 + code
        }
    }
}

// ============================================================================
// Test 4: Burn
// ============================================================================

/// Test burning an NFT
#[no_mangle]
pub extern "C" fn test_burn() -> u64 {
    log("Test 4: nft_burn");

    let caller = get_caller();
    let token_id = 1u64; // Assuming token 1 exists

    match nft_burn(&TEST_COLLECTION, token_id, &caller) {
        Ok(()) => {
            log("NFT burned successfully");
            log("nft_burn: PASS");
            0
        }
        Err(code) => {
            log("nft_burn: FAIL");
            400 + code
        }
    }
}

// ============================================================================
// Test 5: Approvals
// ============================================================================

/// Test approval mechanisms
#[no_mangle]
pub extern "C" fn test_approvals() -> u64 {
    log("Test 5: NFT Approvals");

    let caller = get_caller();
    let token_id = 1u64;

    // Test single token approval
    log("5a: nft_approve (set)");
    match nft_approve(&TEST_COLLECTION, token_id, Some(&TEST_OPERATOR), &caller) {
        Ok(()) => {
            log("Single token approval set");
        }
        Err(code) => {
            log("nft_approve (set): FAIL");
            return 500 + code;
        }
    }

    // Test get approved
    log("5b: nft_get_approved");
    match nft_get_approved(&TEST_COLLECTION, token_id) {
        Ok(approved_opt) => {
            if approved_opt.is_some() {
                log("Got approved operator");
            } else {
                log("No approved operator");
            }
        }
        Err(code) => {
            log("nft_get_approved: FAIL");
            return 510 + code;
        }
    }

    // Test clear approval
    log("5c: nft_approve (clear)");
    match nft_approve(&TEST_COLLECTION, token_id, None, &caller) {
        Ok(()) => {
            log("Single token approval cleared");
        }
        Err(code) => {
            log("nft_approve (clear): FAIL");
            return 520 + code;
        }
    }

    // Test approval for all
    log("5d: nft_set_approval_for_all");
    match nft_set_approval_for_all(&TEST_COLLECTION, &TEST_OPERATOR, true, &caller) {
        Ok(()) => {
            log("Approval for all set");
        }
        Err(code) => {
            log("nft_set_approval_for_all: FAIL");
            return 530 + code;
        }
    }

    // Test is approved for all
    log("5e: nft_is_approved_for_all");
    match nft_is_approved_for_all(&TEST_COLLECTION, &caller, &TEST_OPERATOR) {
        Ok(is_approved) => {
            if is_approved {
                log("Operator is approved for all");
            } else {
                log("Operator is NOT approved for all");
            }
        }
        Err(code) => {
            log("nft_is_approved_for_all: FAIL");
            return 540 + code;
        }
    }

    // Revoke approval for all
    log("5f: nft_set_approval_for_all (revoke)");
    match nft_set_approval_for_all(&TEST_COLLECTION, &TEST_OPERATOR, false, &caller) {
        Ok(()) => {
            log("Approval for all revoked");
        }
        Err(code) => {
            log("nft_set_approval_for_all (revoke): FAIL");
            return 550 + code;
        }
    }

    log("NFT Approvals: PASS");
    0
}

// ============================================================================
// Test 6: Query Functions
// ============================================================================

/// Test query functions
#[no_mangle]
pub extern "C" fn test_queries() -> u64 {
    log("Test 6: NFT Queries");

    let caller = get_caller();
    let token_id = 1u64;

    // Test token exists
    log("6a: nft_exists");
    match nft_exists(&TEST_COLLECTION, token_id) {
        Ok(exists) => {
            if exists {
                log("Token exists");
            } else {
                log("Token does not exist");
            }
        }
        Err(code) => {
            log("nft_exists: FAIL");
            return 600 + code;
        }
    }

    // Test owner_of
    log("6b: nft_owner_of");
    match nft_owner_of(&TEST_COLLECTION, token_id) {
        Ok(owner_opt) => {
            if owner_opt.is_some() {
                log("Got token owner");
            } else {
                log("Token has no owner (burned or not minted)");
            }
        }
        Err(code) => {
            log("nft_owner_of: FAIL");
            return 610 + code;
        }
    }

    // Test balance_of
    log("6c: nft_balance_of");
    match nft_balance_of(&TEST_COLLECTION, &caller) {
        Ok(balance) => {
            let _ = balance; // Would log the actual balance
            log("Got balance");
        }
        Err(code) => {
            log("nft_balance_of: FAIL");
            return 620 + code;
        }
    }

    // Test token_uri
    log("6d: nft_token_uri");
    match nft_token_uri(&TEST_COLLECTION, token_id) {
        Ok(uri_opt) => {
            if let Some(result) = uri_opt {
                if result.as_str().is_some() {
                    log("Got token URI");
                } else {
                    log("Token URI is not valid UTF-8");
                }
            } else {
                log("Token has no URI");
            }
        }
        Err(code) => {
            log("nft_token_uri: FAIL");
            return 630 + code;
        }
    }

    log("NFT Queries: PASS");
    0
}

// ============================================================================
// Main Entrypoint
// ============================================================================

/// Main entrypoint - runs all tests sequentially
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Native NFT Syscalls Test Suite ===");

    let test1 = test_collection_exists();
    if test1 != 0 {
        return test1;
    }

    let test2 = test_mint();
    if test2 != 0 {
        return test2;
    }

    let test3 = test_transfer();
    if test3 != 0 {
        return test3;
    }

    let test4 = test_burn();
    if test4 != 0 {
        return test4;
    }

    let test5 = test_approvals();
    if test5 != 0 {
        return test5;
    }

    let test6 = test_queries();
    if test6 != 0 {
        return test6;
    }

    log("=== All Tests PASSED ===");
    0
}

// ============================================================================
// Usage Example Functions (Not Test Entrypoints)
// ============================================================================

/// Example: Mint an NFT to a specific address
///
/// This function shows a typical pattern for minting NFTs in a contract.
#[allow(dead_code)]
fn example_mint_nft(collection: &[u8; 32], to: &[u8; 32], metadata_uri: &[u8]) -> Result<u64, u64> {
    let caller = get_caller();

    // Mint the NFT
    let token_id = nft_mint(collection, to, metadata_uri, &caller)?;

    // Log success
    log("Minted NFT");

    Ok(token_id)
}

/// Example: Safe transfer with ownership check
///
/// This function shows how to safely transfer an NFT with proper checks.
#[allow(dead_code)]
fn example_safe_transfer(collection: &[u8; 32], token_id: u64, to: &[u8; 32]) -> Result<(), u64> {
    let caller = get_caller();

    // Check if token exists
    if !nft_exists(collection, token_id)? {
        log("Token does not exist");
        return Err(1);
    }

    // Check ownership
    let owner = nft_owner_of(collection, token_id)?;
    match owner {
        Some(current_owner) => {
            // Verify caller is owner or approved
            if current_owner != caller {
                // Check if caller is approved for this token
                let approved = nft_get_approved(collection, token_id)?;
                let is_single_approved = approved.map(|a| a == caller).unwrap_or(false);

                // Check if caller is approved for all
                let is_approved_all = nft_is_approved_for_all(collection, &current_owner, &caller)?;

                if !is_single_approved && !is_approved_all {
                    log("Caller not authorized");
                    return Err(2);
                }
            }

            // Perform transfer
            nft_transfer(collection, token_id, &current_owner, to, &caller)?;
            log("Transfer successful");
            Ok(())
        }
        None => {
            log("Token has no owner");
            Err(3)
        }
    }
}

/// Example: Batch mint multiple NFTs
///
/// This function shows how to mint multiple NFTs in sequence.
#[allow(dead_code)]
fn example_batch_mint(
    collection: &[u8; 32],
    to: &[u8; 32],
    count: u8,
    base_uri: &[u8],
) -> Result<u64, u64> {
    let caller = get_caller();
    let mut last_token_id = 0u64;

    for _ in 0..count {
        // In real usage, you'd generate unique URIs for each token
        last_token_id = nft_mint(collection, to, base_uri, &caller)?;
    }

    Ok(last_token_id)
}

/// Example: Check if address owns any NFTs in collection
#[allow(dead_code)]
fn example_has_nfts(collection: &[u8; 32], owner: &[u8; 32]) -> Result<bool, u64> {
    let balance = nft_balance_of(collection, owner)?;
    Ok(balance > 0)
}
