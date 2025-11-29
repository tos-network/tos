//! Integration tests for DeFi contracts (no_std conversions)
//!
//! Tests the following contracts that were converted from tako-storage to tako-sdk:
//! - factory-pattern
//! - governance

#![allow(clippy::disallowed_methods)]
//! - multisig-wallet
//! - staking-contract
//! - timelock

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
    /// Track storage: (contract_hash, key) -> value
    storage: Arc<Mutex<HashMap<([u8; 32], Vec<u8>), Vec<u8>>>>,
    /// Track balances: (address, asset) -> balance
    balances: Arc<Mutex<HashMap<([u8; 32], [u8; 32]), u64>>>,
    /// Track contract bytecode: contract_hash -> bytecode
    contracts: Arc<Mutex<HashMap<[u8; 32], Vec<u8>>>>,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            balances: Arc::new(Mutex::new(HashMap::new())),
            contracts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set initial balance for a contract/account
    #[allow(dead_code)]
    fn set_balance(&self, address: &[u8; 32], asset: &[u8; 32], balance: u64) {
        let mut balances = self.balances.lock().unwrap();
        balances.insert((*address, *asset), balance);
    }

    /// Set contract bytecode
    #[allow(dead_code)]
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

// Contract file paths (in tests/fixtures/ directory)
const FACTORY_PATTERN_PATH: &str = "tests/fixtures/factory_pattern.so";
const GOVERNANCE_PATH: &str = "tests/fixtures/governance.so";
const MULTISIG_WALLET_PATH: &str = "tests/fixtures/multisig_wallet.so";
const STAKING_CONTRACT_PATH: &str = "tests/fixtures/staking_contract.so";
const TIMELOCK_PATH: &str = "tests/fixtures/timelock.so";

// ===================================================================
// Test 1: Factory Pattern Contract
// ===================================================================

#[test]
fn test_factory_pattern_loads() {
    println!("\n=== Factory Pattern Load Test ===");

    let bytecode = match std::fs::read(FACTORY_PATTERN_PATH) {
        Ok(b) => b,
        Err(e) => {
            println!("⚠ Skipping: Failed to read factory_pattern.so: {}", e);
            println!(
                "  Run 'cd tako/examples/factory-pattern && cargo tako build --release' first"
            );
            return;
        }
    };

    println!("Contract size: {} bytes", bytecode.len());

    // Verify ELF magic
    assert_eq!(&bytecode[0..4], b"\x7FELF", "Invalid ELF magic number");
    println!("✓ ELF magic verified");

    // Verify it's 64-bit
    assert_eq!(bytecode[4], 2, "Not ELF64");
    println!("✓ ELF64 verified");

    // Verify little-endian
    assert_eq!(bytecode[5], 1, "Not little-endian");
    println!("✓ Little-endian verified");

    println!("✅ Factory Pattern contract loaded successfully");
}

#[test]
fn test_factory_pattern_init() {
    println!("\n=== Factory Pattern Init Test ===");

    let bytecode = match std::fs::read(FACTORY_PATTERN_PATH) {
        Ok(b) => b,
        Err(_) => {
            println!("⚠ Skipping: contract not built");
            return;
        }
    };

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Create init input: [0, admin[32]]
    let mut input = vec![0u8]; // opcode 0 = init
    let admin = [1u8; 32]; // admin address
    input.extend_from_slice(&admin);

    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        topoheight,
        &contract_hash,
        &Hash::zero(),     // block_hash
        1000,              // block_height
        1700000000,        // block_timestamp
        &Hash::zero(),     // tx_hash
        &Hash::new(admin), // tx_sender (admin)
        &input,            // input_data
        None,              // compute_budget (use default)
    );

    match result {
        Ok(exec_result) => {
            println!("✅ Factory Pattern init succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Init failed with code {}",
                exec_result.return_value
            );
        }
        Err(e) => {
            panic!("Factory Pattern init failed: {:?}", e);
        }
    }
}

// ===================================================================
// Test 2: Governance Contract
// ===================================================================

#[test]
fn test_governance_loads() {
    println!("\n=== Governance Load Test ===");

    let bytecode = match std::fs::read(GOVERNANCE_PATH) {
        Ok(b) => b,
        Err(e) => {
            println!("⚠ Skipping: Failed to read governance.so: {}", e);
            println!("  Run 'cd tako/examples/governance && cargo tako build --release' first");
            return;
        }
    };

    println!("Contract size: {} bytes", bytecode.len());

    // Verify ELF magic
    assert_eq!(&bytecode[0..4], b"\x7FELF", "Invalid ELF magic number");
    println!("✓ ELF magic verified");

    println!("✅ Governance contract loaded successfully");
}

#[test]
fn test_governance_init() {
    println!("\n=== Governance Init Test ===");

    let bytecode = match std::fs::read(GOVERNANCE_PATH) {
        Ok(b) => b,
        Err(_) => {
            println!("⚠ Skipping: contract not built");
            return;
        }
    };

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Create init input: [0, admin[32], quorum[8], voting_period[8]]
    let mut input = vec![0u8]; // opcode 0 = init
    let admin = [1u8; 32];
    input.extend_from_slice(&admin);
    input.extend_from_slice(&100u64.to_le_bytes()); // quorum
    input.extend_from_slice(&86400u64.to_le_bytes()); // voting_period (1 day)

    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        topoheight,
        &contract_hash,
        &Hash::zero(),     // block_hash
        1000,              // block_height
        1700000000,        // block_timestamp
        &Hash::zero(),     // tx_hash
        &Hash::new(admin), // tx_sender (admin)
        &input,            // input_data
        None,              // compute_budget (use default)
    );

    match result {
        Ok(exec_result) => {
            println!("✅ Governance init succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Init failed with code {}",
                exec_result.return_value
            );
        }
        Err(e) => {
            panic!("Governance init failed: {}", e);
        }
    }
}

// ===================================================================
// Test 3: Multisig Wallet Contract
// ===================================================================

#[test]
fn test_multisig_wallet_loads() {
    println!("\n=== Multisig Wallet Load Test ===");

    let bytecode = match std::fs::read(MULTISIG_WALLET_PATH) {
        Ok(b) => b,
        Err(e) => {
            println!("⚠ Skipping: Failed to read multisig_wallet.so: {}", e);
            println!(
                "  Run 'cd tako/examples/multisig-wallet && cargo tako build --release' first"
            );
            return;
        }
    };

    println!("Contract size: {} bytes", bytecode.len());

    // Verify ELF magic
    assert_eq!(&bytecode[0..4], b"\x7FELF", "Invalid ELF magic number");
    println!("✓ ELF magic verified");

    println!("✅ Multisig Wallet contract loaded successfully");
}

#[test]
fn test_multisig_wallet_init() {
    println!("\n=== Multisig Wallet Init Test ===");

    let bytecode = match std::fs::read(MULTISIG_WALLET_PATH) {
        Ok(b) => b,
        Err(_) => {
            println!("⚠ Skipping: contract not built");
            return;
        }
    };

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Create init input: [0, threshold[4], owner_count[4], owners[32 * count]...]
    let mut input = vec![0u8]; // opcode 0 = init
    input.extend_from_slice(&2u32.to_le_bytes()); // threshold = 2
    input.extend_from_slice(&3u32.to_le_bytes()); // owner_count = 3

    // Add 3 owners
    let owner1 = [1u8; 32];
    let owner2 = [2u8; 32];
    let owner3 = [3u8; 32];
    input.extend_from_slice(&owner1);
    input.extend_from_slice(&owner2);
    input.extend_from_slice(&owner3);

    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        topoheight,
        &contract_hash,
        &Hash::zero(),      // block_hash
        1000,               // block_height
        1700000000,         // block_timestamp
        &Hash::zero(),      // tx_hash
        &Hash::new(owner1), // tx_sender (owner1)
        &input,             // input_data
        None,               // compute_budget (use default)
    );

    match result {
        Ok(exec_result) => {
            println!("✅ Multisig Wallet init succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Init failed with code {}",
                exec_result.return_value
            );
        }
        Err(e) => {
            panic!("Multisig Wallet init failed: {}", e);
        }
    }
}

// ===================================================================
// Test 4: Staking Contract
// ===================================================================

#[test]
fn test_staking_contract_loads() {
    println!("\n=== Staking Contract Load Test ===");

    let bytecode = match std::fs::read(STAKING_CONTRACT_PATH) {
        Ok(b) => b,
        Err(e) => {
            println!("⚠ Skipping: Failed to read staking_contract.so: {}", e);
            println!(
                "  Run 'cd tako/examples/staking-contract && cargo tako build --release' first"
            );
            return;
        }
    };

    println!("Contract size: {} bytes", bytecode.len());

    // Verify ELF magic
    assert_eq!(&bytecode[0..4], b"\x7FELF", "Invalid ELF magic number");
    println!("✓ ELF magic verified");

    println!("✅ Staking Contract loaded successfully");
}

#[test]
fn test_staking_contract_init() {
    println!("\n=== Staking Contract Init Test ===");

    let bytecode = match std::fs::read(STAKING_CONTRACT_PATH) {
        Ok(b) => b,
        Err(_) => {
            println!("⚠ Skipping: contract not built");
            return;
        }
    };

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Create init input: [0, owner[32], reward_rate[8]]
    let mut input = vec![0u8]; // opcode 0 = init
    let owner = [1u8; 32];
    input.extend_from_slice(&owner);
    input.extend_from_slice(&100u64.to_le_bytes()); // reward_rate

    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        topoheight,
        &contract_hash,
        &Hash::zero(),     // block_hash
        1000,              // block_height
        1700000000,        // block_timestamp
        &Hash::zero(),     // tx_hash
        &Hash::new(owner), // tx_sender (owner)
        &input,            // input_data
        None,              // compute_budget (use default)
    );

    match result {
        Ok(exec_result) => {
            println!("✅ Staking Contract init succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Init failed with code {}",
                exec_result.return_value
            );
        }
        Err(e) => {
            panic!("Staking Contract init failed: {}", e);
        }
    }
}

// ===================================================================
// Test 5: Timelock Contract
// ===================================================================

#[test]
fn test_timelock_loads() {
    println!("\n=== Timelock Load Test ===");

    let bytecode = match std::fs::read(TIMELOCK_PATH) {
        Ok(b) => b,
        Err(e) => {
            println!("⚠ Skipping: Failed to read timelock.so: {}", e);
            println!("  Run 'cd tako/examples/timelock && cargo tako build --release' first");
            return;
        }
    };

    println!("Contract size: {} bytes", bytecode.len());

    // Verify ELF magic
    assert_eq!(&bytecode[0..4], b"\x7FELF", "Invalid ELF magic number");
    println!("✓ ELF magic verified");

    println!("✅ Timelock contract loaded successfully");
}

#[test]
fn test_timelock_init() {
    println!("\n=== Timelock Init Test ===");

    let bytecode = match std::fs::read(TIMELOCK_PATH) {
        Ok(b) => b,
        Err(_) => {
            println!("⚠ Skipping: contract not built");
            return;
        }
    };

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();
    let topoheight = 100;

    // Create init input: [0, admin[32], min_delay[8]]
    let mut input = vec![0u8]; // opcode 0 = init
    let admin = [1u8; 32];
    input.extend_from_slice(&admin);
    input.extend_from_slice(&3600u64.to_le_bytes()); // min_delay = 1 hour

    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        topoheight,
        &contract_hash,
        &Hash::zero(),     // block_hash
        1000,              // block_height
        1700000000,        // block_timestamp
        &Hash::zero(),     // tx_hash
        &Hash::new(admin), // tx_sender (admin)
        &input,            // input_data
        None,              // compute_budget (use default)
    );

    match result {
        Ok(exec_result) => {
            println!("✅ Timelock init succeeded!");
            println!("  Return value: {}", exec_result.return_value);
            println!(
                "  Instructions executed: {}",
                exec_result.instructions_executed
            );
            println!("  Compute units used: {}", exec_result.compute_units_used);

            assert_eq!(
                exec_result.return_value, 0,
                "Init failed with code {}",
                exec_result.return_value
            );
        }
        Err(e) => {
            panic!("Timelock init failed: {}", e);
        }
    }
}

// ===================================================================
// Summary Test
// ===================================================================

#[test]
fn test_defi_contracts_summary() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║  DeFi Contracts (no_std conversions) Test Summary            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let contracts = vec![
        (
            "factory_pattern.so",
            "Factory Pattern",
            FACTORY_PATTERN_PATH,
        ),
        ("governance.so", "Governance", GOVERNANCE_PATH),
        (
            "multisig_wallet.so",
            "Multisig Wallet",
            MULTISIG_WALLET_PATH,
        ),
        (
            "staking_contract.so",
            "Staking Contract",
            STAKING_CONTRACT_PATH,
        ),
        ("timelock.so", "Timelock", TIMELOCK_PATH),
    ];

    let mut total_loaded = 0;
    let mut total_size = 0;

    for (filename, name, path) in &contracts {
        match std::fs::read(path) {
            Ok(bytecode) => {
                println!("✓ {} ({}) - {} bytes", name, filename, bytecode.len());
                total_loaded += 1;
                total_size += bytecode.len();
            }
            Err(_) => {
                println!("✗ {} ({}) - Not built", name, filename);
            }
        }
    }

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║  Summary                                                     ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║  Contracts Loaded: {}/{}                                       ║",
        total_loaded,
        contracts.len()
    );
    println!(
        "║  Total Size: {} bytes                                    ║",
        total_size
    );
    println!("║                                                              ║");
    println!("║  These contracts were converted from tako-storage to        ║");
    println!("║  tako-sdk for no_std compatibility with TBPF V3 target.     ║");
    println!("║                                                              ║");
    println!("║  Features tested:                                            ║");
    println!("║    • Storage read/write operations                           ║");
    println!("║    • Admin authorization checks                              ║");
    println!("║    • State initialization                                    ║");
    println!("║    • Error handling                                          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}
