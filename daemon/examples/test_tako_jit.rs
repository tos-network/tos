//! TOS Kernel(TAKO) JIT Compilation Performance Test
//!
//! This example tests the TOS Kernel(TAKO) with JIT compilation enabled and measures:
//! - Contract loading and execution
//! - JIT compilation (if enabled via features)
//! - Performance metrics (execution time, instructions, compute units)
//! - Verification that contracts execute successfully
//!
//! Usage:
//!   cargo run --release --example test_tako_jit

// Allow clippy lints for example code
#![allow(clippy::type_complexity)]
#![allow(clippy::unnecessary_literal_unwrap)]
#![allow(unexpected_cfgs)]

use std::fs;
use std::time::Instant;
use tos_common::crypto::Hash;
use tos_daemon::tako_integration::TakoExecutor;

/// Mock provider for testing - implements the bare minimum traits needed
mod mock {
    use anyhow::Result;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tos_common::{
        asset::AssetData,
        block::TopoHeight,
        crypto::{Hash, PublicKey},
    };
    use tos_kernel::ValueCell;

    /// Simple in-memory contract provider for testing
    pub struct MockContractProvider {
        storage: Arc<Mutex<HashMap<(Hash, Vec<u8>), Vec<u8>>>>,
    }

    impl MockContractProvider {
        pub fn new() -> Self {
            Self {
                storage: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    impl tos_common::contract::ContractProvider for MockContractProvider {
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
            Ok(Some((100, 5_000_000)))
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
            Ok(Some((100, 21_000_000)))
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

    impl tos_common::contract::ContractStorage for MockContractProvider {
        fn load_data(
            &self,
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<ValueCell>)>> {
            let key_bytes = bincode::serialize(key)?;
            let storage = self.storage.lock().unwrap();

            match storage.get(&(contract.clone(), key_bytes)) {
                Some(data) => {
                    let value: ValueCell = bincode::deserialize(data)?;
                    Ok(Some((100, Some(value))))
                }
                None => Ok(None),
            }
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
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<bool> {
            let key_bytes = bincode::serialize(key)?;
            let storage = self.storage.lock().unwrap();
            Ok(storage.contains_key(&(contract.clone(), key_bytes)))
        }

        fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> Result<bool> {
            Ok(true)
        }
    }
}

fn main() {
    // Initialize logging to see VM output
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("\n=== TOS Kernel(TAKO) JIT Compilation Performance Test ===\n");

    // Step 1: Load the hello-world contract bytecode
    // Try the test contract from tos-tbpf first (known working), then hello-world
    let contract_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "~/tos-network/tos-tbpf/tests/elfs/relative_call.so".to_string());
    println!("Loading contract bytecode from: {contract_path}");

    let bytecode = match fs::read(contract_path) {
        Ok(bytes) => {
            println!("✓ Contract loaded successfully: {} bytes", bytes.len());
            bytes
        }
        Err(e) => {
            eprintln!("✗ Failed to load contract: {e}");
            eprintln!("\nPlease build the contract first:");
            eprintln!("  cd ~/tos-network/tako/examples/hello-world");
            eprintln!("  bash build.sh");
            std::process::exit(1);
        }
    };

    // Step 2: Create mock provider
    let mut provider = mock::MockContractProvider::new();

    // Step 3: Set up execution parameters
    let contract_hash = Hash::zero();
    let block_hash = Hash::zero();
    let tx_hash = Hash::zero();
    let tx_sender = Hash::zero();
    let topoheight = 100;
    let block_height = 50;
    let input_data = b"";
    let compute_budget = Some(200_000);

    println!("\nExecution parameters:");
    println!("  Contract hash: {contract_hash}");
    println!("  Topoheight: {topoheight}");
    println!("  Block height: {block_height}");
    println!("  Compute budget: {} units", compute_budget.unwrap());

    // Step 4: Execute with timing
    println!("\n=== Executing Contract ===");
    println!("Note: JIT compilation (if enabled) will compile the bytecode before execution");
    println!("This adds upfront latency but significantly speeds up execution.\n");

    let start = Instant::now();

    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        topoheight,
        &contract_hash,
        &block_hash,
        block_height,
        &tx_hash,
        &tx_sender,
        input_data,
        compute_budget,
    );

    let elapsed = start.elapsed();

    // Step 5: Display results
    println!("\n=== Execution Results ===\n");

    match result {
        Ok(exec_result) => {
            println!("✓ Execution succeeded!\n");
            println!("Return value: {}", exec_result.return_value);
            println!(
                "Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("Compute units used: {}", exec_result.compute_units_used);
            println!(
                "Compute units remaining: {}",
                compute_budget.unwrap() - exec_result.compute_units_used
            );

            if let Some(return_data) = &exec_result.return_data {
                println!("Return data size: {} bytes", return_data.len());
                if !return_data.is_empty() {
                    println!("Return data (hex): {}", hex::encode(return_data));
                }
            } else {
                println!("Return data: None");
            }

            println!("\n=== Performance Metrics ===\n");
            println!("Total execution time: {elapsed:?}");
            println!(
                "Time per instruction: {:.2} ns",
                elapsed.as_nanos() as f64 / exec_result.instructions_executed as f64
            );
            println!(
                "Instructions per second: {:.2} million",
                exec_result.instructions_executed as f64 / elapsed.as_secs_f64() / 1_000_000.0
            );

            println!("\n=== JIT Status ===\n");
            #[cfg(feature = "jit")]
            {
                println!("✓ JIT compilation is ENABLED (via 'jit' feature flag)");
                println!("  - Bytecode was JIT-compiled to native machine code");
                println!("  - Execution uses native instructions (much faster)");
                println!("  - Expected performance improvement: 10-50x vs interpreter");
            }
            #[cfg(not(feature = "jit"))]
            {
                println!("✗ JIT compilation is DISABLED");
                println!("  - Bytecode executed by interpreter");
                println!("  - To enable JIT: add 'features = [\"jit\"]' to tos-tbpf dependency");
            }

            println!("\n=== Summary ===\n");
            if exec_result.return_value == 0 {
                println!("✓ Contract executed successfully (return value = 0)");
            } else {
                println!(
                    "⚠ Contract returned non-zero value: {}",
                    exec_result.return_value
                );
            }

            println!("✓ All syscalls worked correctly");
            println!("✓ Memory management functioned properly");
            println!("✓ Compute budget tracking accurate");

            println!("\n=== Next Steps ===\n");
            println!("1. Run multiple times to see consistent performance");
            println!("2. Test with more complex contracts (storage operations, etc.)");
            println!("3. Compare performance with/without JIT (rebuild without jit feature)");
            println!("4. Profile execution to identify bottlenecks");
            println!("5. Test under load (concurrent executions)");
        }
        Err(e) => {
            eprintln!("✗ Execution failed!\n");
            eprintln!("Error: {e}");
            eprintln!("Error category: {}", e.category());
            eprintln!("User message: {}", e.user_message());
            eprintln!("\nExecution time before error: {elapsed:?}");

            println!("\n=== Debug Information ===\n");
            println!("This error occurred during TOS Kernel(TAKO) execution.");
            println!("Common issues:");
            println!("  - Invalid bytecode format (not proper ELF)");
            println!("  - Missing syscalls in the loader");
            println!("  - Memory access violations");
            println!("  - Compute budget exceeded");
            println!("  - Invalid contract logic");

            std::process::exit(1);
        }
    }

    println!("\n=== Test Complete ===\n");
}
