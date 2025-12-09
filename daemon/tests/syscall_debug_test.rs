//! Debug test to verify syscall registration
#![allow(clippy::disallowed_methods)] // Allow expect/unwrap in tests

use std::sync::Arc;
use tos_daemon::tako_integration::SVMFeatureSet;
use tos_program_runtime::invoke_context::InvokeContext;
use tos_tbpf::{ebpf, elf::Executable, program::BuiltinProgram, vm::Config};

#[test]
fn test_syscall_registration_debug() {
    // Create loader with production config (V0-V3)
    let feature_set = SVMFeatureSet::production();

    let config = Config {
        enabled_tbpf_versions: feature_set.enabled_tbpf_versions(),
        ..Default::default()
    };

    let mut loader = BuiltinProgram::<InvokeContext>::new_loader(config.clone());

    // Register syscalls
    tos_syscalls::register_syscalls(&mut loader).expect("Failed to register syscalls");

    // Check if tos_log is registered
    let tos_log_hash = ebpf::hash_symbol_name(b"tos_log");
    println!("tos_log hash: 0x{:08x}", tos_log_hash);

    // Verify it's the expected value
    assert_eq!(tos_log_hash, 0x25715484, "tos_log hash mismatch");

    // Check if the syscall is in the registry
    let registry = loader.get_function_registry();
    let result = registry.lookup_by_key(tos_log_hash);

    if result.is_some() {
        println!("tos_log syscall found in registry!");
    } else {
        println!("tos_log syscall NOT found in registry!");

        // Print all registered keys
        println!("Registered keys:");
        for key in registry.keys() {
            println!("  0x{:08x}", key);
        }
    }

    assert!(result.is_some(), "tos_log syscall should be registered");

    // Now load the access_control binary and check the executable
    let bytecode = include_bytes!("fixtures/access_control.so");
    let loader = Arc::new(loader);

    let executable = Executable::load(bytecode, loader.clone()).expect("Failed to load executable");

    println!(
        "Executable TBPF version: {:?}",
        executable.get_tbpf_version()
    );
    println!(
        "static_syscalls: {}",
        executable.get_tbpf_version().static_syscalls()
    );

    // Verify via executable's loader
    let exec_registry = executable.get_loader().get_function_registry();
    let result2 = exec_registry.lookup_by_key(tos_log_hash);

    if result2.is_some() {
        println!("tos_log syscall found in executable's loader registry!");
    } else {
        println!("tos_log syscall NOT found in executable's loader registry!");
    }

    assert!(
        result2.is_some(),
        "tos_log should be in executable's loader"
    );
}

/// Now test actual execution to see where it fails
#[test]
fn test_actual_execution() {
    use tos_common::asset::AssetData;
    use tos_common::block::TopoHeight;
    use tos_common::contract::ContractProvider;
    use tos_common::crypto::Hash;
    use tos_common::crypto::PublicKey;
    use tos_kernel::ValueCell;

    // Mock provider
    #[allow(dead_code)]
    struct MockProvider;
    impl MockProvider {
        fn new() -> Self {
            Self
        }
    }
    impl ContractProvider for MockProvider {
        fn get_contract_balance_for_asset(
            &self,
            _: &Hash,
            _: &Hash,
            _: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }
        fn get_account_balance_for_asset(
            &self,
            _: &PublicKey,
            _: &Hash,
            _: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(Some((100, 1000000)))
        }
        fn asset_exists(&self, _: &Hash, _: TopoHeight) -> Result<bool, anyhow::Error> {
            Ok(true)
        }
        fn load_asset_data(
            &self,
            _: &Hash,
            _: TopoHeight,
        ) -> Result<Option<(TopoHeight, AssetData)>, anyhow::Error> {
            Ok(None)
        }
        fn load_asset_supply(
            &self,
            _: &Hash,
            _: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }
        fn account_exists(&self, _: &PublicKey, _: TopoHeight) -> Result<bool, anyhow::Error> {
            Ok(true)
        }
        fn load_contract_module(
            &self,
            _: &Hash,
            _: TopoHeight,
        ) -> Result<Option<Vec<u8>>, anyhow::Error> {
            Ok(None)
        }
    }
    impl tos_common::contract::ContractStorage for MockProvider {
        fn load_data(
            &self,
            _: &Hash,
            _: &ValueCell,
            _: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> {
            Ok(None)
        }
        fn load_data_latest_topoheight(
            &self,
            _: &Hash,
            _: &ValueCell,
            _: TopoHeight,
        ) -> Result<Option<TopoHeight>, anyhow::Error> {
            Ok(Some(100))
        }
        fn has_data(&self, _: &Hash, _: &ValueCell, _: TopoHeight) -> Result<bool, anyhow::Error> {
            Ok(false)
        }
        fn has_contract(&self, _: &Hash, _: TopoHeight) -> Result<bool, anyhow::Error> {
            Ok(true)
        }
    }

    let bytecode = include_bytes!("fixtures/access_control.so");
    let mut provider = MockProvider::new();

    println!("Running actual execution...");

    let result = tos_daemon::tako_integration::TakoExecutor::execute(
        bytecode,
        &mut provider,
        100,
        &Hash::zero(),
        &Hash::zero(),
        0,
        1704067200,
        &Hash::zero(),
        &Hash::zero(),
        &[0x00], // OP_INITIALIZE
        Some(1_000_000),
    );

    match result {
        Ok(res) => println!("Execution succeeded: return_value={}", res.return_value),
        Err(e) => {
            println!("Execution failed: {:?}", e);
            panic!("Execution should succeed: {:?}", e);
        }
    }
}
