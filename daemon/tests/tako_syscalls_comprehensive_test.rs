#![allow(clippy::disallowed_methods)]

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
};
use tos_daemon::tako_integration::TakoExecutor;

/// Mock provider for testing with state tracking
struct MockProvider {
    /// Track balances for contracts and accounts: (address, asset) -> balance
    balances: Arc<Mutex<HashMap<([u8; 32], [u8; 32]), u64>>>,
    /// Track contract bytecode: contract_hash -> bytecode
    contracts: Arc<Mutex<HashMap<[u8; 32], Vec<u8>>>>,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            balances: Arc::new(Mutex::new(HashMap::new())),
            contracts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set initial balance for a contract/account
    fn set_balance(&self, address: &[u8; 32], asset: &[u8; 32], balance: u64) {
        let mut balances = self.balances.lock().unwrap();
        balances.insert((*address, *asset), balance);
    }

    /// Set contract bytecode
    fn set_contract(&self, contract_hash: &[u8; 32], bytecode: Vec<u8>) {
        let mut contracts = self.contracts.lock().unwrap();
        contracts.insert(*contract_hash, bytecode);
    }
}

impl ContractProvider for MockProvider {
    fn get_contract_balance_for_asset(
        &self,
        contract: &Hash,
        asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        let balances = self.balances.lock().unwrap();
        let balance = balances
            .get(&(*contract.as_bytes(), *asset.as_bytes()))
            .copied()
            .unwrap_or(1000000);
        Ok(Some((100, balance)))
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
        contract: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<Vec<u8>>> {
        let contracts = self.contracts.lock().unwrap();
        Ok(contracts.get(contract.as_bytes()).cloned())
    }
}

impl ContractStorage for MockProvider {
    fn load_data(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Option<tos_kernel::ValueCell>)>> {
        // Return None - contract storage is empty at start
        // Writes during execution go to ContractCache (handled by TosStorageAdapter)
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
        // No pre-existing data
        Ok(false)
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> Result<bool> {
        Ok(true)
    }
}

// ===================================================================
// Test 1: Transient Storage (EIP-1153)
// ===================================================================

#[test]
fn test_transient_storage_loads() {
    let contract_path = "tests/fixtures/test_transient_storage.so";

    if log::log_enabled!(log::Level::Info) {
        log::info!("Loading Transient Storage test contract from: {contract_path}");
    }
    let bytecode = std::fs::read(contract_path)
        .expect("Failed to read test_transient_storage.so - ensure it exists in tests/fixtures/");

    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Transient Storage contract loaded: {} bytes",
            bytecode.len()
        );
    }

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
fn test_transient_storage_execution() {
    let contract_path = "tests/fixtures/test_transient_storage.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read test_transient_storage.so");

    println!("\n=== Transient Storage Execution Test ===");
    println!("Contract size: {} bytes", bytecode.len());

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Execute entrypoint (runs all 5 tests)
    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, topoheight, &contract_hash);

    match result {
        Ok(exec_result) => {
            println!("✅ Transient Storage tests succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Transient storage tests failed with code {}",
                exec_result.return_value
            );
            println!("✓ All transient storage tests passed");
        }
        Err(e) => {
            eprintln!("❌ Transient storage execution failed!");
            eprintln!("Error: {}", e);
            eprintln!("Error debug: {:?}", e);
            panic!("Transient storage execution failed: {}", e);
        }
    }
}

// ===================================================================
// Test 2: Balance and Transfer Operations
// ===================================================================

#[test]
fn test_balance_transfer_loads() {
    let contract_path = "tests/fixtures/test_balance_transfer.so";

    if log::log_enabled!(log::Level::Info) {
        log::info!("Loading Balance/Transfer test contract from: {contract_path}");
    }
    let bytecode = std::fs::read(contract_path)
        .expect("Failed to read test_balance_transfer.so - ensure it exists in tests/fixtures/");

    if log::log_enabled!(log::Level::Info) {
        log::info!("Balance/Transfer contract loaded: {} bytes", bytecode.len());
    }

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
fn test_balance_transfer_execution() {
    let contract_path = "tests/fixtures/test_balance_transfer.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read test_balance_transfer.so");

    println!("\n=== Balance/Transfer Execution Test ===");
    println!("Contract size: {} bytes", bytecode.len());

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let native_asset = Hash::zero();
    let topoheight = 100;

    // Set initial balance for the contract
    provider.set_balance(contract_hash.as_bytes(), native_asset.as_bytes(), 10000);
    println!("✓ Set initial contract balance: 10000");

    // Execute entrypoint (runs all 6 tests)
    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, topoheight, &contract_hash);

    match result {
        Ok(exec_result) => {
            println!("✅ Balance/Transfer tests succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Balance/transfer tests failed with code {}",
                exec_result.return_value
            );
            println!("✓ All balance/transfer tests passed");
        }
        Err(e) => {
            panic!("Balance/transfer execution failed: {}", e);
        }
    }
}

// ===================================================================
// Test 3: Code Operations (EXTCODESIZE, EXTCODEHASH, EXTCODECOPY)
// ===================================================================

#[test]
fn test_code_ops_loads() {
    let contract_path = "tests/fixtures/test_code_ops.so";

    if log::log_enabled!(log::Level::Info) {
        log::info!("Loading Code Operations test contract from: {contract_path}");
    }
    let bytecode = std::fs::read(contract_path)
        .expect("Failed to read test_code_ops.so - ensure it exists in tests/fixtures/");

    if log::log_enabled!(log::Level::Info) {
        log::info!("Code Operations contract loaded: {} bytes", bytecode.len());
    }

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
fn test_code_ops_execution() {
    let contract_path = "tests/fixtures/test_code_ops.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read test_code_ops.so");

    println!("\n=== Code Operations Execution Test ===");
    println!("Contract size: {} bytes", bytecode.len());

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Register the contract bytecode so get_external_code_size returns actual size
    provider.set_contract(contract_hash.as_bytes(), bytecode.clone());
    println!("✓ Registered contract bytecode: {} bytes", bytecode.len());

    // Execute entrypoint (runs all 7 tests)
    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, topoheight, &contract_hash);

    match result {
        Ok(exec_result) => {
            println!("✅ Code Operations tests succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Code operations tests failed with code {}",
                exec_result.return_value
            );
            println!("✓ All code operations tests passed");
        }
        Err(e) => {
            panic!("Code operations execution failed: {}", e);
        }
    }
}

// ===================================================================
// Test 4: Event Emission (LOG0-LOG4)
// ===================================================================

#[test]
fn test_events_loads() {
    let contract_path = "tests/fixtures/test_events.so";

    if log::log_enabled!(log::Level::Info) {
        log::info!("Loading Events test contract from: {contract_path}");
    }
    let bytecode = std::fs::read(contract_path)
        .expect("Failed to read test_events.so - ensure it exists in tests/fixtures/");

    if log::log_enabled!(log::Level::Info) {
        log::info!("Events contract loaded: {} bytes", bytecode.len());
    }

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
fn test_events_execution() {
    let contract_path = "tests/fixtures/test_events.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read test_events.so");

    println!("\n=== Events Execution Test ===");
    println!("Contract size: {} bytes", bytecode.len());

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Execute entrypoint (runs all 9 tests)
    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, topoheight, &contract_hash);

    match result {
        Ok(exec_result) => {
            println!("✅ Event Emission tests succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Event emission tests failed with code {}",
                exec_result.return_value
            );
            println!("✓ All event emission tests passed");
        }
        Err(e) => {
            panic!("Event emission execution failed: {}", e);
        }
    }
}

// ===================================================================
// Test 5: Environment (Caller/msg.sender)
// ===================================================================

#[test]
fn test_environment_loads() {
    let contract_path = "tests/fixtures/test_environment.so";

    if log::log_enabled!(log::Level::Info) {
        log::info!("Loading Environment test contract from: {contract_path}");
    }
    let bytecode = std::fs::read(contract_path)
        .expect("Failed to read test_environment.so - ensure it exists in tests/fixtures/");

    if log::log_enabled!(log::Level::Info) {
        log::info!("Environment contract loaded: {} bytes", bytecode.len());
    }

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
fn test_environment_execution() {
    let contract_path = "tests/fixtures/test_environment.so";
    let bytecode = std::fs::read(contract_path).expect("Failed to read test_environment.so");

    println!("\n=== Environment Execution Test ===");
    println!("Contract size: {} bytes", bytecode.len());

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Execute entrypoint (runs all 7 tests)
    let result = TakoExecutor::execute_simple(&bytecode, &mut provider, topoheight, &contract_hash);

    match result {
        Ok(exec_result) => {
            println!("✅ Environment tests succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Environment tests failed with code {}",
                exec_result.return_value
            );
            println!("✓ All environment tests passed");
        }
        Err(e) => {
            panic!("Environment execution failed: {}", e);
        }
    }
}

// ===================================================================
// Summary Test
// ===================================================================

#[test]
fn test_all_syscalls_summary() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║  TAKO Syscalls Comprehensive Test Coverage Summary          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let test_contracts = vec![
        (
            "test_transient_storage.so",
            "Transient Storage (EIP-1153)",
            5,
        ),
        ("test_balance_transfer.so", "Balance/Transfer Operations", 6),
        ("test_code_ops.so", "Code Operations (EXTCODE*)", 7),
        ("test_events.so", "Event Emission (LOG0-LOG4)", 9),
        ("test_environment.so", "Environment (msg.sender)", 7),
    ];

    let mut total_tests = 0;
    let mut passed_contracts = 0;

    for (filename, name, test_count) in &test_contracts {
        let path = format!("tests/fixtures/{}", filename);
        match std::fs::read(&path) {
            Ok(bytecode) => {
                println!(
                    "✓ {} ({} tests) - {} bytes",
                    name,
                    test_count,
                    bytecode.len()
                );
                total_tests += test_count;
                passed_contracts += 1;
            }
            Err(e) => {
                println!("✗ {} - Failed to load: {}", name, e);
            }
        }
    }

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║  Summary                                                     ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║  Contracts Loaded: {}/{}                                    ║",
        passed_contracts,
        test_contracts.len()
    );
    println!(
        "║  Total Test Cases: {}                                        ║",
        total_tests
    );
    println!("║                                                              ║");
    println!("║  Syscalls Tested:                                            ║");
    println!("║    • tos_tstore, tos_tload                                   ║");
    println!("║    • tos_get_balance, tos_transfer                           ║");
    println!("║    • tos_ext_code_size, tos_ext_code_hash                    ║");
    println!("║    • tos_ext_code_copy                                       ║");
    println!("║    • tos_emit_log (LOG0-LOG4)                                ║");
    println!("║    • tos_get_caller                                          ║");
    println!("║    • tos_get_contract_address                                ║");
    println!("║    • tos_storage_read, tos_storage_write                     ║");
    println!("║    • tos_log, tos_log_u64                                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}
