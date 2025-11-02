// GHOSTDAG JSON Test Suite
//
// This module implements tests using JSON test data from reference GHOSTDAG implementations.
// These tests verify that we can load and parse the JSON test format correctly.
//
// NOTE: These tests currently verify JSON structure and expected data format.
// Full GHOSTDAG computation testing requires a complete Storage implementation,
// which is beyond the scope of this test infrastructure. For full validation,
// see integration tests that use actual consensus.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tos_common::crypto::Hash;

use crate::core::ghostdag::KType;

// ============================================================================
// Reference JSON Format Structures
// ============================================================================

/// Reference test DAG format (from reference GHOSTDAG implementations)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReferenceDag {
    #[serde(rename = "K")]
    k: KType,

    #[serde(rename = "GenesisID")]
    genesis_id: String,

    #[serde(rename = "ExpectedReds", default)]
    expected_reds: Vec<String>,

    #[serde(rename = "Blocks")]
    blocks: Vec<ReferenceBlock>,
}

/// Reference block definition
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReferenceBlock {
    #[serde(rename = "ID")]
    id: String,

    #[serde(rename = "ExpectedScore")]
    expected_score: u64,

    #[serde(rename = "ExpectedSelectedParent")]
    expected_selected_parent: String,

    #[serde(rename = "ExpectedReds")]
    expected_reds: Vec<String>,

    #[serde(rename = "ExpectedBlues")]
    expected_blues: Vec<String>,

    #[serde(rename = "Parents")]
    parents: Vec<String>,
}

// ============================================================================
// Test Execution Functions
// ============================================================================

/// Convert string ID to hash (reference format uses string IDs)
fn string_to_hash(s: &str) -> Hash {
    let mut data = s.as_bytes().to_vec();
    data.resize(32, 0);
    let mut bytes = [0u8; 32];
    bytes[..data.len().min(32)].copy_from_slice(&data[..data.len().min(32)]);
    Hash::new(bytes)
}

/// Load and parse a reference format JSON test file
fn load_reference_test<P: AsRef<Path>>(
    path: P,
) -> Result<ReferenceDag, Box<dyn std::error::Error>> {
    let json_str = fs::read_to_string(path)?;
    let dag: ReferenceDag = serde_json::from_str(&json_str)?;
    Ok(dag)
}

/// Validate reference test structure
fn validate_reference_test(
    test_name: &str,
    dag_def: &ReferenceDag,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Validating test: {} ===", test_name);
    println!("K parameter: {}", dag_def.k);
    println!("Genesis ID: {}", dag_def.genesis_id);
    println!("Total blocks: {}", dag_def.blocks.len());

    // Create ID to hash mapping
    let mut id_to_hash = HashMap::new();
    let genesis_hash = string_to_hash(&dag_def.genesis_id);
    id_to_hash.insert(dag_def.genesis_id.clone(), genesis_hash);

    // Validate each block
    for (idx, block_def) in dag_def.blocks.iter().enumerate() {
        println!(
            "\n--- Validating block {} ({}/{}) ---",
            block_def.id,
            idx + 1,
            dag_def.blocks.len()
        );

        // Create hash for this block
        let block_hash = string_to_hash(&block_def.id);
        id_to_hash.insert(block_def.id.clone(), block_hash);

        // Verify all parents exist
        for parent_id in &block_def.parents {
            if !id_to_hash.contains_key(parent_id) {
                return Err(format!("Block {}: Unknown parent {}", block_def.id, parent_id).into());
            }
        }

        // Verify expected_selected_parent exists
        if !id_to_hash.contains_key(&block_def.expected_selected_parent) {
            return Err(format!(
                "Block {}: Unknown selected parent {}",
                block_def.id, block_def.expected_selected_parent
            )
            .into());
        }

        // Verify all expected blues exist
        for blue_id in &block_def.expected_blues {
            if !id_to_hash.contains_key(blue_id) {
                return Err(
                    format!("Block {}: Unknown blue block {}", block_def.id, blue_id).into(),
                );
            }
        }

        // Verify all expected reds exist
        for red_id in &block_def.expected_reds {
            if !id_to_hash.contains_key(red_id) {
                return Err(format!("Block {}: Unknown red block {}", block_def.id, red_id).into());
            }
        }

        println!("✓ Block {} structure valid", block_def.id);
        println!("  - Parents: {}", block_def.parents.len());
        println!("  - Expected score: {}", block_def.expected_score);
        println!(
            "  - Expected selected parent: {}",
            block_def.expected_selected_parent
        );
        println!("  - Expected blues: {}", block_def.expected_blues.len());
        println!("  - Expected reds: {}", block_def.expected_reds.len());
    }

    println!("\n=== Test {} structure validation PASSED ===\n", test_name);
    Ok(())
}

// ============================================================================
// Individual Test Functions
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_all_json_tests() {
        let testdata_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/dags");
        println!("Loading reference format JSON tests from: {}", testdata_dir);

        let entries = fs::read_dir(testdata_dir).expect("Failed to read testdata directory");

        let mut json_files = Vec::new();
        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let filename = path.file_name().unwrap().to_str().unwrap();
                // Only load reference format files (dag*.json)
                // TOS native format files (simple_*.json) are tested in ghostdag_json_loader
                if filename.starts_with("dag") {
                    json_files.push(path);
                }
            }
        }

        assert!(
            !json_files.is_empty(),
            "No reference format JSON test files found"
        );
        println!(
            "Found {} reference format JSON test files",
            json_files.len()
        );

        for path in json_files {
            let filename = path.file_name().unwrap().to_str().unwrap();
            println!("Verifying can load: {}", filename);
            let dag = load_reference_test(&path).expect(&format!("Failed to load {}", filename));
            println!("  ✓ K={}, {} blocks", dag.k, dag.blocks.len());
        }
    }

    #[tokio::test]
    async fn test_dag0_json() {
        let testdata_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/dags");
        let test_path = format!("{}/dag0.json", testdata_dir);
        let dag = load_reference_test(&test_path).expect("Failed to load dag0.json");
        validate_reference_test("dag0", &dag).expect("dag0 validation failed");
    }

    #[tokio::test]
    async fn test_dag1_json() {
        let testdata_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/dags");
        let test_path = format!("{}/dag1.json", testdata_dir);
        let dag = load_reference_test(&test_path).expect("Failed to load dag1.json");
        validate_reference_test("dag1", &dag).expect("dag1 validation failed");
    }

    #[tokio::test]
    async fn test_dag2_json() {
        let testdata_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/dags");
        let test_path = format!("{}/dag2.json", testdata_dir);
        let dag = load_reference_test(&test_path).expect("Failed to load dag2.json");
        validate_reference_test("dag2", &dag).expect("dag2 validation failed");
    }

    #[tokio::test]
    async fn test_dag3_json() {
        let testdata_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/dags");
        let test_path = format!("{}/dag3.json", testdata_dir);
        let dag = load_reference_test(&test_path).expect("Failed to load dag3.json");
        validate_reference_test("dag3", &dag).expect("dag3 validation failed");
    }

    #[tokio::test]
    async fn test_dag4_json() {
        let testdata_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/dags");
        let test_path = format!("{}/dag4.json", testdata_dir);
        let dag = load_reference_test(&test_path).expect("Failed to load dag4.json");
        validate_reference_test("dag4", &dag).expect("dag4 validation failed");
    }

    #[tokio::test]
    async fn test_dag5_json() {
        let testdata_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/dags");
        let test_path = format!("{}/dag5.json", testdata_dir);
        let dag = load_reference_test(&test_path).expect("Failed to load dag5.json");
        validate_reference_test("dag5", &dag).expect("dag5 validation failed");
    }

    #[test]
    fn test_summary() {
        println!();
        println!("=== GHOSTDAG JSON TEST SUITE SUMMARY ===");
        println!();
        println!("This test suite validates reference format JSON files (dag*.json).");
        println!(
            "TOS native format files (simple_*.json) are tested in ghostdag_json_loader module."
        );
        println!();
        println!("Test Coverage:");
        println!("  [✓] test_load_all_json_tests - Verifies all dag*.json files can be loaded");
        println!("  [✓] test_dag0_json - DAG with K=4, 19 blocks, complex merge");
        println!("  [✓] test_dag1_json - DAG with K=4, 30 blocks, large-scale conflicts");
        println!("  [✓] test_dag2_json - DAG with K=18, 9 blocks, high-K linear chain");
        println!("  [✓] test_dag3_json - DAG with K=3, 10 blocks, K-constraint violation");
        println!("  [✓] test_dag4_json - DAG with K=2, 9 blocks, low-K stress test");
        println!("  [✓] test_dag5_json - DAG with K=3, 7 blocks, multi-parent merge");
        println!();
        println!("Each test verifies:");
        println!("  - JSON file can be loaded and parsed");
        println!("  - Block structure is valid (all parent references exist)");
        println!("  - Expected data format matches reference implementation");
        println!();
        println!("NOTE: Full GHOSTDAG computation testing requires Storage implementation.");
        println!("For full validation, use integration tests with actual consensus.");
        println!();
    }
}
