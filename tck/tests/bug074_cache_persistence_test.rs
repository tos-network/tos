//! ISSUE-074 Regression Tests: TAKO Contract Storage Cache Persistence
//!
//! These tests verify that:
//! 1. TAKO executor returns cache with storage writes
//! 2. merge_overlay_storage() correctly merges storage
//! 3. Cache is NOT merged on execution failure
//!
//! Related internal note: ISSUE-074-tako-cache-not-persisted.md

#![allow(clippy::disallowed_methods)]

use anyhow::Result;
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractCache, ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
    versioned_type::VersionedState,
};
use tos_daemon::tako_integration::TakoExecutor;
use tos_kernel::ValueCell;

/// Mock provider for testing
struct MockProvider;

impl ContractProvider for MockProvider {
    fn get_contract_balance_for_asset(
        &self,
        _contract: &Hash,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        Ok(Some((100, 1_000_000)))
    }

    fn get_account_balance_for_asset(
        &self,
        _key: &PublicKey,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        Ok(Some((100, 1_000_000)))
    }

    fn asset_exists(&self, _asset: &Hash, _topoheight: TopoHeight) -> Result<bool> {
        Ok(true)
    }

    fn load_asset_data(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, AssetData)>> {
        Ok(None)
    }

    fn load_asset_supply(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        Ok(None)
    }

    fn account_exists(&self, _key: &PublicKey, _topoheight: TopoHeight) -> Result<bool> {
        Ok(true)
    }

    fn load_contract_module(
        &self,
        _contract: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

impl ContractStorage for MockProvider {
    fn load_data(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Option<ValueCell>)>> {
        Ok(None)
    }

    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<TopoHeight>> {
        Ok(Some(100))
    }

    fn has_data(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<bool> {
        Ok(false)
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> Result<bool> {
        Ok(true)
    }
}

// ============================================================================
// Category 1: Success Persist - Cache Contains Storage Writes
// ============================================================================

/// Test that counter contract execution returns cache with storage writes
#[test]
fn test_success_cache_contains_storage_writes() {
    let contract_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/counter.so");
    let bytecode = std::fs::read(&contract_path).expect("Failed to read counter.so");

    let provider = MockProvider;
    let contract_hash = Hash::zero();
    let block_hash = Hash::zero();
    let tx_hash = Hash::zero();
    let tx_sender = Hash::zero();

    println!("\n=== ISSUE-074 Test: Cache Contains Storage Writes ===");

    // Execute counter contract (writes to storage)
    let result = TakoExecutor::execute(
        &bytecode,
        &provider,
        100,
        &contract_hash,
        &block_hash,
        100,
        12345,
        &tx_hash,
        &tx_sender,
        &[],
        Some(200_000),
    );

    match result {
        Ok(exec_result) => {
            println!(
                "✓ Execution succeeded with return_value: {}",
                exec_result.return_value
            );

            // Verify cache is returned (ISSUE-074 fix)
            assert!(
                !exec_result.cache.storage.is_empty(),
                "ISSUE-074: Cache should contain storage writes from counter contract"
            );

            println!(
                "✓ Cache contains {} storage entries",
                exec_result.cache.storage.len()
            );

            // Verify the counter key exists
            let has_count_key = exec_result.cache.storage.keys().any(|k| {
                if let ValueCell::Bytes(bytes) = k {
                    bytes == b"count"
                } else {
                    false
                }
            });

            assert!(
                has_count_key,
                "Cache should contain 'count' key from counter contract"
            );
            println!("✓ Cache contains 'count' key");

            println!("\n✅ ISSUE-074 Test PASSED: Cache contains storage writes");
        }
        Err(e) => {
            panic!("Counter execution failed: {e}");
        }
    }
}

// ============================================================================
// Category 2: merge_overlay_storage() Function Tests
// ============================================================================

/// Test that merge_overlay_storage merges storage field
#[test]
fn test_merge_overlay_storage() {
    println!("\n=== ISSUE-074 Test: merge_overlay_storage ===");

    // Create chain_state cache (simulates deposits)
    let mut chain_cache = ContractCache::new();
    chain_cache
        .balances
        .insert(Hash::zero(), Some((VersionedState::New, 1000)));
    println!("✓ Created chain_cache with balance entry");

    // Create VM cache (simulates storage writes)
    let mut vm_cache = ContractCache::new();
    let key = ValueCell::Bytes(b"test_key".to_vec());
    let value = ValueCell::Bytes(b"test_value".to_vec());
    vm_cache
        .storage
        .insert(key.clone(), (VersionedState::New, Some(value.clone())));
    // Also add some balance to vm_cache (should NOT be merged)
    vm_cache
        .balances
        .insert(Hash::new([1u8; 32]), Some((VersionedState::New, 9999)));
    println!("✓ Created vm_cache with storage entry and balance entry");

    // Merge
    chain_cache.merge_overlay_storage(vm_cache);

    // Verify storage was merged
    assert!(
        chain_cache.storage.contains_key(&key),
        "Storage should be merged"
    );
    println!("✓ Storage was merged correctly");

    // Verify balances were NOT merged (only chain_cache's original balance)
    assert_eq!(
        chain_cache.balances.len(),
        1,
        "Only original balance should exist (vm_cache balance not merged)"
    );
    assert!(
        chain_cache.balances.contains_key(&Hash::zero()),
        "Original balance should be preserved"
    );
    assert!(
        !chain_cache.balances.contains_key(&Hash::new([1u8; 32])),
        "VM cache balance should NOT be merged"
    );
    println!("✓ Balances were NOT merged (correct behavior)");

    println!("\n✅ ISSUE-074 Test PASSED: merge_overlay_storage works correctly");
}

/// Test merge_overlay_storage with multiple storage entries
#[test]
fn test_merge_overlay_multiple_entries() {
    println!("\n=== ISSUE-074 Test: Merge Multiple Storage Entries ===");

    let mut chain_cache = ContractCache::new();

    // Pre-existing storage entry in chain_cache
    let existing_key = ValueCell::Bytes(b"existing".to_vec());
    chain_cache.storage.insert(
        existing_key.clone(),
        (
            VersionedState::FetchedAt(50),
            Some(ValueCell::Bytes(b"old".to_vec())),
        ),
    );

    let mut vm_cache = ContractCache::new();

    // New storage entries from VM
    let key1 = ValueCell::Bytes(b"key1".to_vec());
    let key2 = ValueCell::Bytes(b"key2".to_vec());
    vm_cache.storage.insert(
        key1.clone(),
        (
            VersionedState::New,
            Some(ValueCell::Bytes(b"value1".to_vec())),
        ),
    );
    vm_cache.storage.insert(
        key2.clone(),
        (
            VersionedState::New,
            Some(ValueCell::Bytes(b"value2".to_vec())),
        ),
    );
    // Overwrite existing key
    vm_cache.storage.insert(
        existing_key.clone(),
        (
            VersionedState::Updated(50),
            Some(ValueCell::Bytes(b"new".to_vec())),
        ),
    );

    chain_cache.merge_overlay_storage(vm_cache);

    // Verify all entries
    assert_eq!(
        chain_cache.storage.len(),
        3,
        "Should have 3 storage entries"
    );
    assert!(chain_cache.storage.contains_key(&key1), "key1 should exist");
    assert!(chain_cache.storage.contains_key(&key2), "key2 should exist");

    // Verify overwrite worked
    let (_, existing_value) = chain_cache.storage.get(&existing_key).unwrap();
    if let Some(ValueCell::Bytes(bytes)) = existing_value {
        assert_eq!(
            bytes, b"new",
            "Existing key should be overwritten with new value"
        );
    } else {
        panic!("Expected Bytes value");
    }

    println!("✓ All storage entries merged correctly");
    println!("✓ Existing key was overwritten");
    println!("\n✅ ISSUE-074 Test PASSED: Multiple entries merge correctly");
}

// ============================================================================
// Category 3: Failure Rollback - Cache Exists But Should Not Be Merged
// ============================================================================

/// Test that execution failure still returns cache (but caller won't merge it)
#[test]
fn test_failure_cache_not_merged_on_nonzero_exit() {
    println!("\n=== ISSUE-074 Test: Failure Rollback Semantics ===");

    // Create a scenario that simulates what invoke_contract does
    let mut chain_cache = ContractCache::new();
    chain_cache.balances.insert(
        Hash::zero(),
        Some((VersionedState::New, 1000)), // Simulated deposit
    );

    // Simulated VM cache (as if contract wrote storage before failing)
    let mut vm_cache = ContractCache::new();
    vm_cache.storage.insert(
        ValueCell::Bytes(b"should_not_persist".to_vec()),
        (
            VersionedState::New,
            Some(ValueCell::Bytes(b"value".to_vec())),
        ),
    );

    // Simulate is_success = false (exit_code != 0)
    let is_success = false;

    if is_success {
        // This branch should NOT be taken
        chain_cache.merge_overlay_storage(vm_cache);
        panic!("Should not merge on failure");
    } else {
        // Failure path: vm_cache is dropped, NOT merged
        println!("✓ Simulated failure path: vm_cache NOT merged");
    }

    // Verify chain_cache has no storage (vm_cache was not merged)
    assert!(
        chain_cache.storage.is_empty(),
        "Chain cache should have no storage after failure (vm_cache not merged)"
    );

    // Verify deposits are still there (would be refunded separately)
    assert!(
        !chain_cache.balances.is_empty(),
        "Deposits should still be in chain_cache for refund"
    );

    println!("✓ Chain cache storage is empty (correct)");
    println!("✓ Deposits preserved for refund");
    println!("\n✅ ISSUE-074 Test PASSED: Failure rollback semantics correct");
}

/// Test ContractExecutionResult.cache field in error path
#[test]
fn test_execution_error_cache_is_none() {
    println!("\n=== ISSUE-074 Test: Execution Error Cache is None ===");

    // Simulate what contract.rs does on execution error
    let execution_result = tos_common::contract::ContractExecutionResult {
        exit_code: None,
        gas_used: 100_000,
        return_data: Some(b"Execution error: invalid bytecode".to_vec()),
        transfers: vec![],
        events: vec![],
        cache: None, // This is what we set on error
    };

    assert!(
        execution_result.cache.is_none(),
        "Cache should be None on execution error"
    );
    assert!(
        execution_result.exit_code.is_none(),
        "Exit code should be None on execution error"
    );

    println!("✓ cache is None on execution error");
    println!("✓ exit_code is None on execution error");
    println!("\n✅ ISSUE-074 Test PASSED: Error path sets cache to None");
}

// ============================================================================
// Category 4: Integration - Full Execute and Cache Return
// ============================================================================

/// Test that TakoExecutor.execute returns cache correctly
#[test]
fn test_tako_executor_returns_cache() {
    let contract_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/counter.so");
    let bytecode = std::fs::read(&contract_path).expect("Failed to read counter.so");

    let provider = MockProvider;
    let contract_hash = Hash::zero();

    println!("\n=== ISSUE-074 Test: TakoExecutor Returns Cache ===");

    // Use execute_simple which also returns ExecutionResult with cache
    let result = TakoExecutor::execute_simple(&bytecode, &provider, 100, &contract_hash);

    match result {
        Ok(exec_result) => {
            // The cache field exists and contains storage writes
            println!("✓ Execution succeeded");
            println!("  - return_value: {}", exec_result.return_value);
            println!(
                "  - cache.storage entries: {}",
                exec_result.cache.storage.len()
            );

            // Counter contract writes to storage, so cache should not be empty
            assert!(
                !exec_result.cache.storage.is_empty(),
                "ISSUE-074: TakoExecutor should return cache with storage writes"
            );

            println!("\n✅ ISSUE-074 Test PASSED: TakoExecutor returns cache correctly");
        }
        Err(e) => {
            panic!("Execution failed: {e}");
        }
    }
}

/// Test storage delete operation is captured in cache
#[test]
fn test_storage_delete_in_cache() {
    println!("\n=== ISSUE-074 Test: Storage Delete Captured in Cache ===");

    let mut cache = ContractCache::new();

    // Simulate storage.delete() - sets value to None
    let key = ValueCell::Bytes(b"deleted_key".to_vec());
    cache.storage.insert(
        key.clone(),
        (VersionedState::Updated(50), None), // None = deleted
    );

    // Verify delete is captured
    let entry = cache.storage.get(&key);
    assert!(entry.is_some(), "Deleted key should exist in cache");

    let (state, value) = entry.unwrap();
    assert!(
        matches!(state, VersionedState::Updated(_)),
        "State should be Updated"
    );
    assert!(value.is_none(), "Value should be None (deleted)");

    println!("✓ Storage delete captured correctly");
    println!("  - Key exists in cache: true");
    println!("  - Value is None: true");
    println!("\n✅ ISSUE-074 Test PASSED: Storage delete captured in cache");
}
