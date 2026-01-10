#![allow(clippy::disallowed_methods)]

use anyhow::Result;
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
};
/// Integration test for Hello World contract
///
/// This test verifies that the simplest TAKO contract can load and execute successfully.
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
fn test_hello_world_loads() {
    // Load the hello-world contract bytecode
    let contract_path = "tests/fixtures/hello_world.so";

    println!("Loading contract from: {contract_path}");
    let bytecode = std::fs::read(contract_path)
        .expect("Failed to read hello_world.so - ensure it exists in tests/fixtures/");

    println!("Contract loaded: {} bytes", bytecode.len());

    // Verify ELF magic
    assert_eq!(&bytecode[0..4], b"\x7FELF", "Invalid ELF magic number");
    println!("✓ ELF magic verified");

    // Verify it's 64-bit
    assert_eq!(bytecode[4], 2, "Not ELF64");
    println!("✓ ELF64 verified");

    // Verify little-endian
    assert_eq!(bytecode[5], 1, "Not little-endian");
    println!("✓ Little-endian verified");
}

#[test]
fn test_hello_world_executes() {
    // Load the contract
    let contract_path = "tests/fixtures/hello_world.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read hello_world.so");

    println!("\n=== Hello World Execution Test ===");
    println!("Contract size: {} bytes", bytecode.len());

    // Create mock provider
    let mut provider = MockProvider;

    // Contract parameters
    let contract_hash = Hash::zero();
    let topoheight = 100;

    println!("Executing contract...");

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

            if let Some(return_data) = &exec_result.return_data {
                println!("  Return data size: {} bytes", return_data.len());
            }

            // Verify successful execution
            assert_eq!(
                exec_result.return_value, 0,
                "Contract should return 0 for success"
            );
            assert!(
                exec_result.instructions_executed > 0,
                "Should execute some instructions"
            );
            assert!(
                exec_result.compute_units_used > 0,
                "Should use some compute units"
            );

            println!("\n✅ All assertions passed!");
        }
        Err(e) => {
            println!("\n❌ Execution failed: {e:?}");
            println!("Error details: {e}");
            panic!("Hello world execution failed: {e}");
        }
    }
}

#[test]
fn test_hello_world_with_compute_budget() {
    let contract_path = "tests/fixtures/hello_world.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read hello_world.so");

    let mut provider = MockProvider;
    let contract_hash = Hash::zero();
    let block_hash = Hash::zero();
    let tx_hash = Hash::zero();
    let tx_sender = Hash::zero();

    println!("\n=== Hello World with Custom Compute Budget ===");

    // Test with different compute budgets
    let budgets = vec![1000, 10000, 100000];

    for budget in budgets {
        println!("\nTesting with compute budget: {budget}");

        let result = TakoExecutor::execute(
            &bytecode,
            &mut provider,
            100,
            &contract_hash,
            &block_hash,
            0,
            0,
            &tx_hash,
            &tx_sender,
            &[],
            Some(budget),
        );

        match result {
            Ok(exec_result) => {
                println!("  ✓ Success with budget {budget}");
                println!(
                    "    Compute used: {}/{}",
                    exec_result.compute_units_used, budget
                );
                assert!(
                    exec_result.compute_units_used <= budget,
                    "Should not exceed budget"
                );
            }
            Err(e) => {
                println!("  ✗ Failed with budget {budget}: {e}");
                // Low budgets might fail, that's okay
                if budget >= 10000 {
                    panic!("Should succeed with reasonable budget: {e}");
                }
            }
        }
    }
}

#[test]
fn test_hello_world_vs_cpi_callee() {
    println!("\n=== Comparing Hello World vs CPI Callee ===\n");

    let hello_path = "tests/fixtures/hello_world.so";
    let callee_path = "tests/fixtures/cpi_callee.so";

    let hello_bytecode = std::fs::read(hello_path).expect("Failed to read hello_world.so");
    let callee_bytecode = std::fs::read(callee_path).expect("Failed to read cpi_callee.so");

    println!("Hello World: {} bytes", hello_bytecode.len());
    println!("CPI Callee:  {} bytes", callee_bytecode.len());

    let mut provider = MockProvider;
    let contract_hash = Hash::zero();

    // Test hello world
    println!("\nExecuting Hello World...");
    let hello_result =
        TakoExecutor::execute_simple(&hello_bytecode, &mut provider, 100, &contract_hash);

    // Test CPI callee
    println!("Executing CPI Callee...");
    let callee_result =
        TakoExecutor::execute_simple(&callee_bytecode, &mut provider, 100, &contract_hash);

    match (hello_result, callee_result) {
        (Ok(hello), Ok(callee)) => {
            println!("\n✅ Both contracts executed successfully!");
            println!("\nHello World:");
            println!("  Instructions: {}", hello.instructions_executed);
            println!("  Compute units: {}", hello.compute_units_used);

            println!("\nCPI Callee:");
            println!("  Instructions: {}", callee.instructions_executed);
            println!("  Compute units: {}", callee.compute_units_used);
        }
        (Err(e), Ok(_)) => {
            panic!("Hello World failed but CPI Callee succeeded: {e}");
        }
        (Ok(_), Err(e)) => {
            println!("✓ Hello World succeeded");
            println!("✗ CPI Callee failed: {e}");
        }
        (Err(e1), Err(e2)) => {
            panic!("Both failed - Hello: {e1}, Callee: {e2}");
        }
    }
}
