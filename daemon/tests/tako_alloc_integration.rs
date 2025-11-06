//! Integration tests for tos-alloc library
//!
//! Tests dynamic memory allocation in actual TAKO VM with heap regions

#![cfg(test)]

use tos_daemon::tako_integration::TakoExecutor;
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
};
use tos_vm::ValueCell;
use anyhow::Result;

/// Mock ContractProvider for testing
///
/// Complete implementation of ContractProvider and ContractStorage traits
/// Based on the actual working implementation from tako_hello_world_test.rs
struct MockProvider;

impl ContractProvider for MockProvider {
    fn get_contract_balance_for_asset(
        &self,
        _contract: &Hash,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        // Return mock balance: topoheight 100, balance 1,000,000
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
        // No CPI modules loaded in this test
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
        // No storage data for test (contracts can still use heap!)
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

/// Helper: Load compiled example contract
fn load_example_contract(name: &str) -> Vec<u8> {
    let path = format!(
        "../../tos-alloc/examples/{}/target/tbpf-tos-tos/release/ex.so",
        name
    );
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to load {}: {}\n\n\
            Build the example first:\n  \
            cd ../../tos-alloc && ./build-example.sh {}",
            path, e, name
        )
    })
}

#[test]
fn test_alloc_basic_vec_operations() {
    // Load compiled contract
    let bytecode = load_example_contract("basic");

    // Create mock provider
    let mut provider = MockProvider;

    // Prepare execution parameters
    let contract_hash = Hash::zero();
    let block_hash = Hash::zero();
    let tx_hash = Hash::zero();
    let tx_sender = Hash::zero();

    // Execute contract
    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        0,               // topoheight
        &contract_hash,
        &block_hash,
        0,               // block_height
        &tx_hash,
        &tx_sender,
        &[],             // input_data
        None,            // compute_budget (use default)
    );

    // Verify success
    assert!(result.is_ok(), "Contract execution failed: {:?}", result.err());

    let exec_result = result.unwrap();
    assert_eq!(
        exec_result.return_value, 0,
        "Contract returned error code: {}",
        exec_result.return_value
    );

    // Check logs
    println!("Contract logs:");
    for log in &exec_result.log_messages {
        println!("  {}", log);
    }

    assert!(
        exec_result
            .log_messages
            .iter()
            .any(|log| log.contains("All tests passed")),
        "Expected success message not found in logs"
    );
}

#[test]
fn test_alloc_heap_usage() {
    let bytecode = load_example_contract("basic");
    let mut provider = MockProvider;

    let contract_hash = Hash::zero();
    let block_hash = Hash::zero();
    let tx_hash = Hash::zero();
    let tx_sender = Hash::zero();

    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        0,
        &contract_hash,
        &block_hash,
        0,
        &tx_hash,
        &tx_sender,
        &[],
        None,
    );

    assert!(result.is_ok());
    let exec_result = result.unwrap();

    // Check that heap was used (compute units > baseline)
    println!("Compute units used: {}", exec_result.compute_units_used);
    assert!(
        exec_result.compute_units_used > 1000,
        "Expected some compute usage from allocations"
    );

    // Verify logs show heap usage
    let has_heap_log = exec_result
        .log_messages
        .iter()
        .any(|log| log.contains("Heap usage") || log.contains("Test 3"));

    assert!(has_heap_log, "Expected heap usage log message");
}

#[test]
#[ignore] // Run manually: cargo test --test tako_alloc_integration test_alloc_out_of_memory -- --ignored
fn test_alloc_out_of_memory() {
    // TODO: Create contract that allocates until OOM
    // Verify it returns null_mut() and handles gracefully instead of panicking
    unimplemented!("OOM test contract not yet created");
}
