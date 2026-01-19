//! TOS Kernel(TAKO) Executor Integration Tests
//!
//! Tests the complete TOS Kernel(TAKO) execution pipeline with real ELF contracts.

#![allow(clippy::disallowed_methods)]

use std::collections::HashMap;
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractExecutor, ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
};
use tos_daemon::tako_integration::TakoContractExecutor;
use tos_kernel::ValueCell;

/// Mock provider for testing
struct MockProvider {
    balances: HashMap<(PublicKey, Hash), u64>,
    storage: HashMap<(Hash, Vec<u8>), Vec<u8>>,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
            storage: HashMap::new(),
        }
    }

    #[allow(dead_code)]
    fn with_balance(mut self, key: PublicKey, asset: Hash, amount: u64) -> Self {
        self.balances.insert((key, asset), amount);
        self
    }
}

impl ContractProvider for MockProvider {
    fn get_contract_balance_for_asset(
        &self,
        _contract: &Hash,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, u64)>> {
        Ok(Some((100, 0)))
    }

    fn get_account_balance_for_asset(
        &self,
        key: &PublicKey,
        asset: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, u64)>> {
        Ok(self
            .balances
            .get(&(key.clone(), asset.clone()))
            .map(|&amount| (100, amount)))
    }

    fn asset_exists(&self, _asset: &Hash, _topoheight: TopoHeight) -> anyhow::Result<bool> {
        Ok(true)
    }

    fn load_asset_data(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, AssetData)>> {
        Ok(None)
    }

    fn load_asset_supply(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, u64)>> {
        Ok(None)
    }

    fn account_exists(&self, _key: &PublicKey, _topoheight: TopoHeight) -> anyhow::Result<bool> {
        Ok(true)
    }

    fn load_contract_module(
        &self,
        _contract: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

impl ContractStorage for MockProvider {
    fn load_data(
        &self,
        contract: &Hash,
        key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, Option<ValueCell>)>> {
        let key_bytes = bincode::serialize(key)?;
        match self.storage.get(&(contract.clone(), key_bytes)) {
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
    ) -> anyhow::Result<Option<TopoHeight>> {
        Ok(Some(100))
    }

    fn has_data(
        &self,
        contract: &Hash,
        key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<bool> {
        let key_bytes = bincode::serialize(key)?;
        Ok(self.storage.contains_key(&(contract.clone(), key_bytes)))
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> anyhow::Result<bool> {
        Ok(true)
    }
}

#[tokio::test]
async fn test_tako_executor_hello_world() {
    // Load the hello-world ELF contract from test fixtures
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let contract_path = format!("{manifest_dir}/tests/fixtures/hello_world.so");
    let bytecode =
        std::fs::read(&contract_path).expect("Failed to load hello-world contract from fixtures");

    // Create mock provider
    let provider = MockProvider::new();

    // Create TAKO executor
    let executor = TakoContractExecutor::new();

    // Verify it recognizes ELF format
    assert!(
        executor.supports_format(&bytecode),
        "Should recognize ELF format"
    );

    // Execute the contract
    let result = executor
        .execute(
            &bytecode,
            &provider,
            100,           // topoheight
            &Hash::zero(), // contract_hash
            &Hash::zero(), // block_hash
            0,             // block_height
            0,             // block_timestamp
            &Hash::zero(), // tx_hash
            &Hash::zero(), // tx_sender
            200_000,       // max_gas
            None,          // parameters
            None,          // nft_provider
        )
        .await
        .expect("Execution should succeed");

    // Verify success
    assert_eq!(
        result.exit_code,
        Some(0),
        "Contract should return success (0)"
    );
    assert!(result.gas_used > 0, "Should have consumed some gas");
    assert!(result.gas_used <= 200_000, "Should not exceed gas limit");

    println!("✅ Hello-world contract executed successfully!");
    println!("   Exit code: {:?}", result.exit_code);
    println!("   Gas used: {}", result.gas_used);
    println!("   Return data: {:?}", result.return_data);
}

#[tokio::test]
async fn test_multi_executor_format_detection() {
    // Load ELF contract from test fixtures
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let contract_path = format!("{manifest_dir}/tests/fixtures/hello_world.so");
    let elf_bytecode =
        std::fs::read(&contract_path).expect("Failed to load hello-world contract from fixtures");

    // Create multi-executor
    let executor = TakoContractExecutor::new();

    // Should support ELF format
    assert!(
        executor.supports_format(&elf_bytecode),
        "Should support ELF"
    );

    println!("✅ Format detection working correctly!");
}

#[tokio::test]
async fn test_multi_executor_execution() {
    // Load ELF contract from test fixtures
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let contract_path = format!("{manifest_dir}/tests/fixtures/hello_world.so");
    let bytecode =
        std::fs::read(&contract_path).expect("Failed to load hello-world contract from fixtures");

    let provider = MockProvider::new();
    let executor = TakoContractExecutor::new();

    // Execute via TakoContractExecutor (should auto-dispatch to TAKO)
    let result = executor
        .execute(
            &bytecode,
            &provider,
            100,
            &Hash::zero(),
            &Hash::zero(),
            0,
            0,
            &Hash::zero(),
            &Hash::zero(),
            200_000,
            None,
            None,
        )
        .await
        .expect("TakoContractExecutor should execute ELF contract");

    assert_eq!(result.exit_code, Some(0), "Should succeed");
    assert!(result.gas_used > 0, "Should consume gas");

    println!("✅ TakoContractExecutor auto-dispatch working!");
    println!("   Gas used: {}", result.gas_used);
}

#[tokio::test]
async fn test_gas_metering() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let contract_path = format!("{manifest_dir}/tests/fixtures/hello_world.so");
    let bytecode =
        std::fs::read(&contract_path).expect("Failed to load hello-world contract from fixtures");

    let provider = MockProvider::new();
    let executor = TakoContractExecutor::new();

    // Test with different gas limits
    let limits = vec![200_000, 500_000, 1_000_000];

    for limit in limits {
        let result = executor
            .execute(
                &bytecode,
                &provider,
                100,
                &Hash::zero(),
                &Hash::zero(),
                0,
                0,
                &Hash::zero(),
                &Hash::zero(),
                limit,
                None,
                None,
            )
            .await
            .expect("Should execute");

        assert!(result.gas_used <= limit, "Should not exceed limit");
        assert!(result.gas_used > 0, "Should use some gas");

        println!(
            "✅ Gas limit {}: used {} ({}%)",
            limit,
            result.gas_used,
            (result.gas_used * 100) / limit
        );
    }
}

#[tokio::test]
async fn test_contract_with_storage() {
    // This test will work once we have a contract that uses storage
    // For now, just verify the hello-world contract can execute

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let contract_path = format!("{manifest_dir}/tests/fixtures/hello_world.so");
    let bytecode = std::fs::read(&contract_path).expect("Failed to load contract from fixtures");

    let provider = MockProvider::new();
    let executor = TakoContractExecutor::new();

    let result = executor
        .execute(
            &bytecode,
            &provider,
            100,
            &Hash::zero(),
            &Hash::zero(),
            0,
            0,
            &Hash::zero(),
            &Hash::zero(),
            200_000,
            None,
            None,
        )
        .await
        .expect("Should execute");

    // Verify basic execution
    assert_eq!(result.exit_code, Some(0));

    println!("✅ Contract with storage operations ready for testing");
}

#[test]
fn test_elf_format_detection() {
    // Test ELF magic number detection
    let elf_header = b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    assert!(tos_common::contract::is_elf_bytecode(elf_header));

    // Test non-ELF
    let non_elf = b"not an ELF file";
    assert!(!tos_common::contract::is_elf_bytecode(non_elf));

    // Test empty
    let empty: &[u8] = &[];
    assert!(!tos_common::contract::is_elf_bytecode(empty));

    // Test partial ELF header (too short)
    let partial = b"\x7FEL";
    assert!(!tos_common::contract::is_elf_bytecode(partial));

    println!("✅ ELF format detection working!");
}
