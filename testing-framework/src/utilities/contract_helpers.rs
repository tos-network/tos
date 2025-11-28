// File: testing-framework/src/utilities/contract_helpers.rs
//
// Smart Contract Testing Helpers
//
// This module provides simplified utilities for testing TAKO smart contracts.
// For testing purposes, we use the simpler execute_simple approach rather than
// full contract deployment, as it's faster and doesn't require Module parsing.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use tos_common::{
    block::TopoHeight,
    contract::ContractStorage,
    crypto::{Hash, KeyPair},
};
use tos_daemon::{
    core::{error::BlockchainError, storage::rocksdb::RocksStorage},
    tako_integration::{ExecutionResult, TakoExecutor},
};
use tos_kernel::ValueCell;

use super::{create_test_rocksdb_storage, setup_account_rocksdb};

/// Execute a TAKO contract for testing
///
/// This uses `TakoExecutor::execute_simple` which is designed for testing.
/// It doesn't require prior deployment - just executes the bytecode directly.
///
/// # Arguments
///
/// * `bytecode` - The compiled contract bytecode (ELF format)
/// * `storage` - The RocksDB storage instance (acts as ContractProvider)
/// * `topoheight` - Current topoheight for versioned reads
/// * `contract_hash` - Hash identifier for the contract (can be any Hash for testing)
///
/// # Returns
///
/// * `ExecutionResult` - Result with return_value, compute_units_used, logs, etc.
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::execute_test_contract;
/// use tos_common::crypto::Hash;
///
/// #[tokio::test]
/// async fn test_hello_world() {
///     let bytecode = include_bytes!("../../tests/fixtures/hello_world.so");
///     let storage = create_test_rocksdb_storage().await;
///
///     let result = execute_test_contract(bytecode, &storage, 1, &Hash::zero())
///         .await
///         .unwrap();
///
///     assert_eq!(result.return_value, 0); // Success
///     println!("Gas used: {}", result.compute_units_used);
/// }
/// ```
pub async fn execute_test_contract(
    bytecode: &[u8],
    storage: &Arc<RwLock<RocksStorage>>,
    topoheight: TopoHeight,
    contract_hash: &Hash,
) -> Result<ExecutionResult> {
    execute_test_contract_with_input(
        bytecode,
        storage,
        topoheight,
        contract_hash,
        &Hash::zero(),
        &[],
    )
    .await
}

/// Execute a TAKO contract for testing with custom input data
///
/// This is an extended version of `execute_test_contract` that allows passing
/// input data to the contract. Use this when testing contracts that accept parameters.
///
/// # Arguments
///
/// * `bytecode` - The compiled contract bytecode (ELF format)
/// * `storage` - The RocksDB storage instance
/// * `topoheight` - Current topoheight for versioned reads
/// * `contract_hash` - Hash identifier for the contract
/// * `tx_sender` - Transaction sender address (caller of the contract)
/// * `input_data` - Input data to pass to the contract (can be empty &[])
///
/// # Returns
///
/// * `ExecutionResult` - Result with return_value, compute_units_used, logs, etc.
pub async fn execute_test_contract_with_input(
    bytecode: &[u8],
    storage: &Arc<RwLock<RocksStorage>>,
    topoheight: TopoHeight,
    contract_hash: &Hash,
    tx_sender: &Hash,
    input_data: &[u8],
) -> Result<ExecutionResult> {
    let mut storage_write = storage.write().await;

    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            "Executing contract {} at topoheight {} ({} bytes bytecode, {} bytes input)",
            contract_hash,
            topoheight,
            bytecode.len(),
            input_data.len()
        );
    }

    // Use TakoExecutor::execute directly to pass input_data
    // Use a reasonable default timestamp for testing (2024-01-01 00:00:00 UTC = 1704067200)
    let block_timestamp = 1704067200u64;

    let mut result = TakoExecutor::execute(
        bytecode,
        &mut *storage_write,
        topoheight,
        contract_hash,
        &Hash::zero(),   // block_hash
        0,               // block_height
        block_timestamp, // block_timestamp
        &Hash::zero(),   // tx_hash
        tx_sender,       // tx_sender
        input_data,      // input_data
        None,            // compute_budget (use default)
    )
    .context("Contract execution failed")?;

    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            "Contract execution completed: return_value={}, compute_units={}",
            result.return_value,
            result.compute_units_used
        );
    }

    // Persist contract storage cache to storage (CRITICAL for test execution!)
    // NOTE: In production, cache persistence is handled by the transaction apply phase.
    // For tests, we must manually persist the cache here so subsequent executions can read the data.
    //
    // For testing purposes, we deploy a minimal contract entry if it doesn't exist yet.
    use std::borrow::Cow;
    use tos_common::versioned_type::Versioned;
    use tos_daemon::core::storage::ContractDataProvider;

    // Check if contract exists, if not create a minimal entry for testing
    use tos_common::contract::ContractStorage;
    if !ContractStorage::has_contract(&*storage_write, contract_hash, topoheight).unwrap_or(false) {
        // Create minimal TAKO contract for testing (just so we can save contract data)
        use tos_daemon::core::storage::ContractProvider;
        use tos_kernel::Module;

        let test_module = Module::from_bytecode(bytecode.to_vec());
        storage_write
            .set_last_contract_to(
                contract_hash,
                topoheight,
                &Versioned::new(Some(Cow::Owned(test_module)), None),
            )
            .await
            .context("Failed to create test contract entry")?;
    }

    for (key, (state, value)) in result.cache.storage.drain() {
        if state.should_be_stored() {
            storage_write
                .set_last_contract_data_to(
                    contract_hash,
                    &key,
                    topoheight,
                    &Versioned::new(value, state.get_topoheight()),
                )
                .await
                .context("Failed to persist contract storage")?;
        }
    }

    Ok(result)
}

/// Create storage with a funded account for contract testing
///
/// This is a convenience function that creates test storage and funds an account
/// so it can deploy/call contracts.
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::create_contract_test_storage;
///
/// let account = KeyPair::new();
/// let storage = create_contract_test_storage(&account, 1000 * COIN_VALUE).await.unwrap();
/// ```
pub async fn create_contract_test_storage(
    account: &KeyPair,
    balance: u64,
) -> Result<Arc<RwLock<RocksStorage>>, BlockchainError> {
    let storage = create_test_rocksdb_storage().await;
    setup_account_rocksdb(&storage, &account.get_public_key().compress(), balance, 0).await?;
    Ok(storage)
}

/// Get a value from contract storage
///
/// This reads a value from the contract's persistent storage at the specified topoheight.
///
/// # Arguments
///
/// * `storage` - The RocksDB storage instance
/// * `contract_hash` - Hash of the contract
/// * `key` - Storage key (as bytes)
/// * `topoheight` - Topoheight for versioned read
///
/// # Returns
///
/// * `Option<Vec<u8>>` - The stored value, or None if key doesn't exist
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::get_contract_storage;
///
/// let count = get_contract_storage(&storage, contract_hash, b"count", 10)
///     .await
///     .unwrap();
///
/// if let Some(value) = count {
///     println!("Counter value: {:?}", value);
/// }
/// ```
pub async fn get_contract_storage(
    storage: &Arc<RwLock<RocksStorage>>,
    contract_hash: Hash,
    key: &[u8],
    topoheight: TopoHeight,
) -> Result<Option<Vec<u8>>> {
    let storage_read = storage.read().await;

    // Convert bytes to ValueCell
    let key_cell = ValueCell::Bytes(key.to_vec());

    let result = storage_read
        .load_data(&contract_hash, &key_cell, topoheight)
        .context("Failed to load contract storage")?;

    // Extract the value from Option<(TopoHeight, Option<ValueCell>)>
    Ok(result.and_then(|(_, value_opt)| value_opt.and_then(|v| v.as_bytes().ok().cloned())))
}

/// Check if a contract exists at a given topoheight
///
/// # Example
///
/// ```ignore
/// let exists = contract_exists(&storage, contract_hash, 10).await.unwrap();
/// assert!(exists);
/// ```
pub async fn contract_exists(
    storage: &Arc<RwLock<RocksStorage>>,
    contract_hash: Hash,
    topoheight: TopoHeight,
) -> Result<bool> {
    let storage_read = storage.read().await;
    // Use ContractStorage trait method explicitly
    match <RocksStorage as ContractStorage>::has_contract(
        &*storage_read,
        &contract_hash,
        topoheight,
    ) {
        Ok(exists) => Ok(exists),
        // ContractNotFound means the contract doesn't exist, return false
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("Contract not found") {
                Ok(false)
            } else {
                Err(e).context("Failed to check contract existence")
            }
        }
    }
}

/// Fund a test account for contract testing
///
/// This is a convenience wrapper around `setup_account_rocksdb`.
///
/// # Example
///
/// ```ignore
/// let user = KeyPair::new();
/// fund_test_account(&storage, &user, 1000 * COIN_VALUE).await.unwrap();
/// ```
pub async fn fund_test_account(
    storage: &Arc<RwLock<RocksStorage>>,
    account: &KeyPair,
    balance: u64,
) -> Result<(), BlockchainError> {
    setup_account_rocksdb(storage, &account.get_public_key().compress(), balance, 0).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::config::COIN_VALUE;
    use tos_daemon::core::storage::BalanceProvider;

    /// Test creating storage with funded account
    #[allow(clippy::unwrap_used, clippy::assertions_on_constants)]
    #[tokio::test]
    async fn test_create_contract_test_storage() {
        let account = KeyPair::new();

        let storage = create_contract_test_storage(&account, 500 * COIN_VALUE)
            .await
            .unwrap();

        let storage_read = storage.read().await;
        let (_, balance) = storage_read
            .get_last_balance(
                &account.get_public_key().compress(),
                &tos_common::config::TOS_ASSET,
            )
            .await
            .unwrap();
        assert_eq!(balance.get_balance(), 500 * COIN_VALUE);
    }

    /// Test funding accounts
    #[allow(clippy::unwrap_used, clippy::assertions_on_constants)]
    #[tokio::test]
    async fn test_fund_account() {
        let storage = create_test_rocksdb_storage().await;
        let user = KeyPair::new();

        fund_test_account(&storage, &user, 500 * COIN_VALUE)
            .await
            .unwrap();

        let storage_read = storage.read().await;
        let (_, balance) = storage_read
            .get_last_balance(
                &user.get_public_key().compress(),
                &tos_common::config::TOS_ASSET,
            )
            .await
            .unwrap();
        assert_eq!(balance.get_balance(), 500 * COIN_VALUE);
    }

    /// Test contract existence check
    #[allow(clippy::unwrap_used, clippy::assertions_on_constants)]
    #[tokio::test]
    async fn test_contract_exists() {
        let storage = create_test_rocksdb_storage().await;
        let fake_hash = Hash::zero();

        // Non-existent contract should return false
        let exists = contract_exists(&storage, fake_hash, 1).await.unwrap();
        assert!(!exists, "Non-existent contract should not exist");
    }

    /// Test contract execution with minimal ELF
    #[test]
    fn test_execute_minimal_elf() {
        // Note: This test doesn't actually execute because we need a valid TAKO contract
        // For real tests, use fixtures from daemon/tests/fixtures/

        // Create minimal ELF header (for demonstration only - not executable)
        let mut bytecode = vec![0x7F, b'E', b'L', b'F']; // ELF magic
        bytecode.extend_from_slice(&[
            2, // 64-bit
            1, // little-endian
            1, // ELF version
            0, 0, 0, 0, 0, 0, 0, 0, 0, // padding
        ]);
        bytecode.resize(128, 0);

        // Verify it looks like an ELF
        assert_eq!(&bytecode[0..4], b"\x7FELF");
        assert_eq!(bytecode[4], 2); // 64-bit
        assert_eq!(bytecode[5], 1); // little-endian
    }
}
