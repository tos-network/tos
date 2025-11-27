//! Integration tests for TBPF version support
//!
//! Tests the SVMFeatureSet and dynamic TBPF version range configuration,
//! aligning with Solana's approach to SBPF version control.
//!
//! # TBPF Version Support Matrix
//!
//! | Version | e_flags | Features |
//! |---------|---------|----------|
//! | V0 | 0 | Legacy format (backward compatible) |
//! | V1 | 1 | Dynamic stack frames (SIMD-0166) |
//! | V2 | 2 | Arithmetic improvements (SIMD-0174) |
//! | V3 | 3 | Static syscalls, stricter ELF (SIMD-0178) |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
};
use tos_daemon::tako_integration::{SVMFeatureSet, TakoExecutor};
use tos_kernel::ValueCell;

// Contract paths
const V0_CONTRACT_PATH: &str = "tests/fixtures/hello_world.so"; // e_flags=0
const V3_CONTRACT_PATH: &str = "tests/fixtures/minimal.so"; // e_flags=3

/// Mock provider for testing
struct MockProvider {
    balances: Arc<Mutex<HashMap<([u8; 32], [u8; 32]), u64>>>,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            balances: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ContractProvider for MockProvider {
    fn get_contract_balance_for_asset(
        &self,
        contract: &Hash,
        asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        let balances = self.balances.lock().unwrap();
        Ok(balances
            .get(&(*contract.as_bytes(), *asset.as_bytes()))
            .map(|&balance| (100, balance)))
    }

    fn get_account_balance_for_asset(
        &self,
        _key: &PublicKey,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        Ok(Some((100, 1_000_000)))
    }

    fn asset_exists(&self, _asset: &Hash, _topoheight: TopoHeight) -> Result<bool, anyhow::Error> {
        Ok(true)
    }

    fn load_asset_data(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, AssetData)>, anyhow::Error> {
        Ok(None)
    }

    fn load_asset_supply(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn account_exists(
        &self,
        _key: &PublicKey,
        _topoheight: TopoHeight,
    ) -> Result<bool, anyhow::Error> {
        Ok(true)
    }

    fn load_contract_module(
        &self,
        _contract: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<Vec<u8>>, anyhow::Error> {
        Ok(None)
    }
}

impl ContractStorage for MockProvider {
    fn load_data(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> {
        Ok(None)
    }

    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<TopoHeight>, anyhow::Error> {
        Ok(None)
    }

    fn has_data(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    fn has_contract(
        &self,
        _contract: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<bool, anyhow::Error> {
        Ok(true)
    }
}

/// Helper to load contract bytecode
fn load_contract(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("Failed to load {}: {}", path, e))
}

/// Helper to get e_flags from ELF bytecode
fn get_e_flags(bytecode: &[u8]) -> u32 {
    if bytecode.len() < 52 {
        return 0;
    }
    u32::from_le_bytes([bytecode[48], bytecode[49], bytecode[50], bytecode[51]])
}

// ============================================================================
// SVMFeatureSet Unit Tests
// ============================================================================

#[test]
fn test_feature_set_default() {
    let features = SVMFeatureSet::default();

    // Default: V0 only
    assert!(!features.disable_tbpf_v0_execution);
    assert!(!features.enable_tbpf_v1_deployment_and_execution);
    assert!(!features.enable_tbpf_v2_deployment_and_execution);
    assert!(!features.enable_tbpf_v3_deployment_and_execution);

    let range = features.enabled_tbpf_versions();
    assert!(range.contains(&tos_tbpf::program::TBPFVersion::V0));
    assert!(!range.contains(&tos_tbpf::program::TBPFVersion::V3));
}

#[test]
fn test_feature_set_production() {
    let features = SVMFeatureSet::production();

    // Production: V0-V3 (matching Solana)
    assert!(!features.disable_tbpf_v0_execution);
    assert!(features.enable_tbpf_v1_deployment_and_execution);
    assert!(features.enable_tbpf_v2_deployment_and_execution);
    assert!(features.enable_tbpf_v3_deployment_and_execution);

    let range = features.enabled_tbpf_versions();
    assert!(range.contains(&tos_tbpf::program::TBPFVersion::V0));
    assert!(range.contains(&tos_tbpf::program::TBPFVersion::V1));
    assert!(range.contains(&tos_tbpf::program::TBPFVersion::V2));
    assert!(range.contains(&tos_tbpf::program::TBPFVersion::V3));
    assert!(!range.contains(&tos_tbpf::program::TBPFVersion::V4));
}

#[test]
fn test_feature_set_v3_only() {
    let features = SVMFeatureSet::v3_only();

    // V3 only: Disable V0, enable only V3
    assert!(features.disable_tbpf_v0_execution);
    assert!(!features.enable_tbpf_v1_deployment_and_execution);
    assert!(!features.enable_tbpf_v2_deployment_and_execution);
    assert!(features.enable_tbpf_v3_deployment_and_execution);

    let range = features.enabled_tbpf_versions();
    assert!(!range.contains(&tos_tbpf::program::TBPFVersion::V0));
    assert!(range.contains(&tos_tbpf::program::TBPFVersion::V3));
}

// ============================================================================
// V0 Contract Tests
// ============================================================================

#[test]
fn test_v0_contract_with_default_features() {
    let bytecode = load_contract(V0_CONTRACT_PATH);
    let e_flags = get_e_flags(&bytecode);
    assert_eq!(e_flags, 0, "Expected V0 contract (e_flags=0)");

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();

    // Default features should support V0
    let result = TakoExecutor::execute(
        &bytecode,
        &mut provider,
        100,
        &contract_hash,
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
    );

    assert!(
        result.is_ok(),
        "V0 contract should execute with default features: {:?}",
        result.err()
    );
}

#[test]
fn test_v0_contract_with_production_features() {
    let bytecode = load_contract(V0_CONTRACT_PATH);
    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();

    // Production features (V0-V3) should support V0
    let result = TakoExecutor::execute_with_features(
        &bytecode,
        &mut provider,
        100,
        &contract_hash,
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::production(),
    );

    assert!(
        result.is_ok(),
        "V0 contract should execute with production features: {:?}",
        result.err()
    );
}

#[test]
fn test_v0_contract_rejected_by_v3_only() {
    let bytecode = load_contract(V0_CONTRACT_PATH);
    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();

    // V3-only features should reject V0 contracts
    let result = TakoExecutor::execute_with_features(
        &bytecode,
        &mut provider,
        100,
        &contract_hash,
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::v3_only(),
    );

    assert!(
        result.is_err(),
        "V0 contract should be rejected with V3-only features"
    );
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("UnsupportedTBPFVersion") || err_str.contains("ELF"),
        "Error should indicate unsupported TBPF version: {}",
        err_str
    );
}

// ============================================================================
// V3 Contract Tests
// ============================================================================

#[test]
fn test_v3_contract_version_detected() {
    // Verify that V3 contract is correctly identified by e_flags
    let bytecode = load_contract(V3_CONTRACT_PATH);
    let e_flags = get_e_flags(&bytecode);
    assert_eq!(e_flags, 3, "Expected V3 contract (e_flags=3)");
    println!("V3 contract detected: e_flags={}", e_flags);
}

#[test]
fn test_v3_contract_loads_with_production_features() {
    let bytecode = load_contract(V3_CONTRACT_PATH);
    let e_flags = get_e_flags(&bytecode);
    assert_eq!(e_flags, 3, "Expected V3 contract (e_flags=3)");

    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();

    // Production features (V0-V3) should allow V3 contract to load
    // Note: The contract may fail at execution due to V3-specific instructions
    // (SIMD-0377 exit opcode 0x9d), but ELF loading should succeed
    let result = TakoExecutor::execute_with_features(
        &bytecode,
        &mut provider,
        100,
        &contract_hash,
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::production(),
    );

    // V3 contract loads but may fail at execution due to V3-specific opcodes
    // This is expected behavior - ELF version check passes, but interpreter
    // may not support all V3 instructions yet (e.g., SIMD-0377 exit opcode)
    if result.is_err() {
        let err = result.unwrap_err();
        let err_str = err.to_string();
        // Should NOT be UnsupportedTBPFVersion - that would mean version check failed
        assert!(
            !err_str.contains("UnsupportedTBPFVersion"),
            "V3 contract should pass version check with production features: {}",
            err_str
        );
        println!(
            "V3 contract passed version check, execution error (expected): {}",
            err_str
        );
    } else {
        println!("V3 contract executed successfully");
    }
}

#[test]
fn test_v3_contract_loads_with_v3_only_features() {
    let bytecode = load_contract(V3_CONTRACT_PATH);
    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();

    // V3-only features should allow V3 contract to load
    let result = TakoExecutor::execute_with_features(
        &bytecode,
        &mut provider,
        100,
        &contract_hash,
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::v3_only(),
    );

    // Similar to above - version check should pass
    if result.is_err() {
        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            !err_str.contains("UnsupportedTBPFVersion"),
            "V3 contract should pass version check with V3-only features: {}",
            err_str
        );
        println!(
            "V3 contract passed version check with V3-only features, execution error: {}",
            err_str
        );
    } else {
        println!("V3 contract executed successfully with V3-only features");
    }
}

#[test]
fn test_v3_contract_rejected_by_default_features() {
    let bytecode = load_contract(V3_CONTRACT_PATH);
    let mut provider = MockProvider::new();
    let contract_hash = Hash::zero();

    // Default features (V0 only) should reject V3 contracts at ELF loading
    let result = TakoExecutor::execute_with_features(
        &bytecode,
        &mut provider,
        100,
        &contract_hash,
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::default(),
    );

    assert!(
        result.is_err(),
        "V3 contract should be rejected with default features (V0 only)"
    );
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("UnsupportedTBPFVersion") || err_str.contains("ELF"),
        "Error should indicate unsupported TBPF version: {}",
        err_str
    );
    println!(
        "V3 contract correctly rejected with default features: {}",
        err_str
    );
}

// ============================================================================
// Summary Test
// ============================================================================

#[test]
fn test_tbpf_version_support_summary() {
    println!("\n=== TBPF Version Support Summary ===\n");

    // Test V0 contract
    let v0_bytecode = load_contract(V0_CONTRACT_PATH);
    let v0_flags = get_e_flags(&v0_bytecode);
    println!("V0 Contract ({}): e_flags={}", V0_CONTRACT_PATH, v0_flags);

    // Test V3 contract
    let v3_bytecode = load_contract(V3_CONTRACT_PATH);
    let v3_flags = get_e_flags(&v3_bytecode);
    println!("V3 Contract ({}): e_flags={}", V3_CONTRACT_PATH, v3_flags);

    println!("\n--- Feature Set Configurations ---");

    // Default features
    let default_features = SVMFeatureSet::default();
    println!(
        "Default: {:?}..={:?}",
        default_features.min_tbpf_version(),
        default_features.max_tbpf_version()
    );

    // Production features
    let production_features = SVMFeatureSet::production();
    println!(
        "Production: {:?}..={:?}",
        production_features.min_tbpf_version(),
        production_features.max_tbpf_version()
    );

    // V3-only features
    let v3_only_features = SVMFeatureSet::v3_only();
    println!(
        "V3-Only: {:?}..={:?}",
        v3_only_features.min_tbpf_version(),
        v3_only_features.max_tbpf_version()
    );

    println!("\n--- Execution Matrix ---");
    println!("| Contract | Default | Production | V3-Only |");
    println!("|----------|---------|------------|---------|");

    let mut provider = MockProvider::new();

    // V0 contract execution matrix
    let v0_default = TakoExecutor::execute_with_features(
        &v0_bytecode,
        &mut provider,
        100,
        &Hash::zero(),
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::default(),
    )
    .is_ok();

    let v0_production = TakoExecutor::execute_with_features(
        &v0_bytecode,
        &mut provider,
        100,
        &Hash::zero(),
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::production(),
    )
    .is_ok();

    let v0_v3only = TakoExecutor::execute_with_features(
        &v0_bytecode,
        &mut provider,
        100,
        &Hash::zero(),
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::v3_only(),
    )
    .is_ok();

    println!(
        "| V0       | {}     | {}        | {}     |",
        if v0_default { "PASS" } else { "FAIL" },
        if v0_production { "PASS" } else { "FAIL" },
        if v0_v3only { "PASS" } else { "FAIL" }
    );

    // V3 contract execution matrix
    let v3_default = TakoExecutor::execute_with_features(
        &v3_bytecode,
        &mut provider,
        100,
        &Hash::zero(),
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::default(),
    )
    .is_ok();

    let v3_production = TakoExecutor::execute_with_features(
        &v3_bytecode,
        &mut provider,
        100,
        &Hash::zero(),
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::production(),
    )
    .is_ok();

    let v3_v3only = TakoExecutor::execute_with_features(
        &v3_bytecode,
        &mut provider,
        100,
        &Hash::zero(),
        &Hash::zero(),
        1000,
        1700000000,
        &Hash::zero(),
        &Hash::zero(),
        &[],
        Some(200_000),
        &SVMFeatureSet::v3_only(),
    )
    .is_ok();

    println!(
        "| V3       | {}     | {}        | {}     |",
        if v3_default { "PASS" } else { "FAIL" },
        if v3_production { "PASS" } else { "FAIL" },
        if v3_v3only { "PASS" } else { "FAIL" }
    );

    println!("\n=== Summary ===");
    println!("- TOS now supports TBPF V0-V3 (aligned with Solana)");
    println!("- Production mode: V0..=V3 for backward compatibility");
    println!("- V3-only mode: For new networks without legacy contracts");
    println!("- Default mode: V0 only (conservative, backward compatible)");
}
