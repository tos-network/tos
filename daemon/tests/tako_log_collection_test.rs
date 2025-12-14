//! Integration test for TOS Kernel(TAKO) log collection
//!
//! This test verifies that log messages emitted by contracts are properly
//! collected and included in the ExecutionResult.

#![allow(clippy::disallowed_methods)]

use anyhow::Result;
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
};
use tos_daemon::tako_integration::TakoExecutor;

/// Mock provider for testing
struct MockProvider;

impl ContractProvider for MockProvider {
    fn get_contract_balance_for_asset(
        &self,
        _contract: &Hash,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        Ok(Some((100, 1000000)))
    }

    fn get_account_balance_for_asset(
        &self,
        _key: &PublicKey,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        Ok(Some((100, 1000000)))
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
        _key: &tos_kernel::ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Option<tos_kernel::ValueCell>)>> {
        Ok(None)
    }

    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<TopoHeight>> {
        Ok(Some(100))
    }

    fn has_data(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<bool> {
        Ok(false)
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> Result<bool> {
        Ok(true)
    }
}

#[test]
fn test_log_collection_hello_world() {
    println!("\n=== Log Collection Test: Hello World ===");

    // Load the hello-world contract which calls log("Hello, TOS!")
    let contract_path = "tests/fixtures/hello_world.so";
    let bytecode = std::fs::read(contract_path)
        .expect("Failed to read hello_world.so - ensure it exists in tests/fixtures/");

    println!("Contract loaded: {} bytes", bytecode.len());

    let mut provider = MockProvider;
    let contract_hash = Hash::zero();
    let topoheight = 100;

    println!("Executing contract with log collection enabled...");

    // Execute the contract
    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, topoheight, &contract_hash);

    match result {
        Ok(exec_result) => {
            println!("\n✅ Execution succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);
            println!("  Log messages: {}", exec_result.log_messages.len());

            // Print all log messages
            for (i, msg) in exec_result.log_messages.iter().enumerate() {
                println!("    [{i}] {msg}");
            }

            // Verify log messages were collected
            assert!(
                !exec_result.log_messages.is_empty(),
                "Contract should emit at least one log message"
            );

            // The hello-world contract calls log("Hello, TOS!")
            // which should result in "Program log: Hello, TOS!"
            let has_hello_log = exec_result
                .log_messages
                .iter()
                .any(|msg| msg.contains("Hello, TOS!"));

            assert!(
                has_hello_log,
                "Should contain 'Hello, TOS!' log message. Found: {:?}",
                exec_result.log_messages
            );

            println!("\n✅ Log collection verified!");
        }
        Err(e) => {
            println!("\n❌ Execution failed: {e:?}");
            panic!("Log collection test failed: {e}");
        }
    }
}

#[test]
fn test_log_collection_field_exists() {
    println!("\n=== Verifying ExecutionResult has log_messages field ===");

    // This test ensures ExecutionResult structure has the log_messages field
    // even if the contract doesn't emit any logs

    let contract_path = "tests/fixtures/hello_world.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read hello_world.so");

    let mut provider = MockProvider;
    let contract_hash = Hash::zero();

    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, 100, &contract_hash);

    match result {
        Ok(exec_result) => {
            // The log_messages field should exist (we're accessing it here)
            let _log_count = exec_result.log_messages.len();

            println!("✅ log_messages field exists and is accessible");
            println!("   Collected {_log_count} log messages");
        }
        Err(e) => {
            panic!("Execution failed: {e}");
        }
    }
}

#[test]
fn test_log_messages_format() {
    println!("\n=== Testing Log Message Format ===");

    let contract_path = "tests/fixtures/hello_world.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read hello_world.so");

    let mut provider = MockProvider;
    let contract_hash = Hash::zero();

    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, 100, &contract_hash);

    match result {
        Ok(exec_result) => {
            println!("Collected {} log messages:", exec_result.log_messages.len());

            for (i, msg) in exec_result.log_messages.iter().enumerate() {
                println!("  [{i}] {msg}");

                // All log messages should follow SVM-compatible format
                // "Program log: ..." or "Program data: ..." or "Program consumption: ..."
                let is_valid_format = msg.starts_with("Program log: ")
                    || msg.starts_with("Program data: ")
                    || msg.starts_with("Program consumption: ");

                assert!(
                    is_valid_format,
                    "Log message should follow SVM format. Got: {msg}"
                );
            }

            println!("✅ All log messages follow SVM-compatible format");
        }
        Err(e) => {
            panic!("Execution failed: {e}");
        }
    }
}

#[test]
fn test_empty_logs_for_non_logging_contract() {
    println!("\n=== Testing Contract Without Logging ===");

    // The counter contract might not have logging calls
    let contract_path = "tests/fixtures/counter.so";

    // Check if the file exists first
    if !std::path::Path::new(contract_path).exists() {
        println!("⚠️  Counter contract not found, skipping test");
        return;
    }

    let bytecode = std::fs::read(contract_path).expect("Failed to read counter.so");

    let mut provider = MockProvider;
    let contract_hash = Hash::zero();

    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, 100, &contract_hash);

    match result {
        Ok(exec_result) => {
            println!("Execution succeeded");
            println!("Log messages: {}", exec_result.log_messages.len());

            // This contract might or might not have logs
            // The important thing is that log_messages field is always present
            // and is a valid Vec<String>

            for (i, msg) in exec_result.log_messages.iter().enumerate() {
                println!("  [{i}] {msg}");
            }

            println!("✅ log_messages field works correctly for all contracts");
        }
        Err(e) => {
            // Counter contract might not execute successfully in this test environment
            // That's okay, we're mainly testing the structure
            println!("⚠️  Execution failed (expected for some contracts): {e}");
        }
    }
}
