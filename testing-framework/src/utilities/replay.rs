// File: testing-framework/src/utilities/replay.rs
//
// Artifact Replay Utilities
//
// This module provides utilities for loading and replaying test artifacts
// to reproduce failures and debug issues.

use super::artifacts::{ArtifactCollector, TestArtifact};
use anyhow::Result;
use std::path::Path;

/// Load artifact from disk
///
/// # Arguments
///
/// * `filepath` - Path to the artifact JSON file
///
/// # Examples
///
/// ```rust,ignore
/// use tos_testing_framework::utilities::replay::load_artifact;
///
/// #[tokio::test]
/// async fn test_replay_from_artifact() -> Result<()> {
///     let artifact = load_artifact("./artifacts/test_example_20251115.json").await?;
///     println!("Test: {}", artifact.metadata.test_name);
///     println!("RNG Seed: {:?}", artifact.metadata.rng_seed);
///     println!("Failure: {:?}", artifact.metadata.failure_reason);
///     Ok(())
/// }
/// ```
pub async fn load_artifact(filepath: impl AsRef<Path>) -> Result<TestArtifact> {
    ArtifactCollector::load(filepath).await
}

/// Print artifact summary to stdout
///
/// Displays key information from the artifact in a human-readable format.
///
/// # Examples
///
/// ```rust,ignore
/// use tos_testing_framework::utilities::replay::{load_artifact, print_artifact_summary};
///
/// #[tokio::test]
/// async fn test_inspect_artifact() -> Result<()> {
///     let artifact = load_artifact("./artifacts/test_example.json").await?;
///     print_artifact_summary(&artifact);
///     Ok(())
/// }
/// ```
pub fn print_artifact_summary(artifact: &TestArtifact) {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║              TEST FAILURE ARTIFACT SUMMARY                     ║");
    println!("╠════════════════════════════════════════════════════════════════╣");
    println!("║ Test Name:     {:44} ║", artifact.metadata.test_name);
    println!("║ Timestamp:     {:44} ║", artifact.metadata.timestamp);
    println!(
        "║ Duration:      {:44} ║",
        format!("{} ms", artifact.metadata.duration_ms)
    );

    if let Some(seed) = artifact.metadata.rng_seed {
        println!("║ RNG Seed:      {:44} ║", format!("0x{:016x}", seed));
    } else {
        println!("║ RNG Seed:      {:44} ║", "N/A");
    }

    if let Some(ref reason) = artifact.metadata.failure_reason {
        println!("╠════════════════════════════════════════════════════════════════╣");
        println!("║ FAILURE REASON:                                                ║");
        for line in textwrap::wrap(reason, 62) {
            println!("║ {:62} ║", line);
        }
    }

    println!("╠════════════════════════════════════════════════════════════════╣");

    if let Some(ref topology) = artifact.topology {
        println!(
            "║ Network:       {:44} ║",
            format!("{} nodes", topology.node_count)
        );
        if !topology.partitions.is_empty() {
            println!(
                "║ Partitions:    {:44} ║",
                format!("{} active", topology.partitions.len())
            );
        }
    }

    println!(
        "║ Node States:   {:44} ║",
        format!("{} captured", artifact.blockchain_states.len())
    );
    println!(
        "║ Transactions:  {:44} ║",
        format!("{} recorded", artifact.transactions.len())
    );
    println!(
        "║ Log Entries:   {:44} ║",
        format!("{} captured", artifact.logs.len())
    );

    println!("╠════════════════════════════════════════════════════════════════╣");
    println!("║ BLOCKCHAIN STATES:                                             ║");
    for state in &artifact.blockchain_states {
        println!(
            "║   Node {}: height={}, supply={}, accounts={}{}║",
            state.node_id,
            state.tip_height,
            state.total_supply,
            state.balances.len(),
            " ".repeat(30 - format!("{}", state.balances.len()).len())
        );
    }

    if !artifact.logs.is_empty() {
        println!("╠════════════════════════════════════════════════════════════════╣");
        println!("║ RECENT LOGS (last 5):                                          ║");
        for log in artifact.logs.iter().rev().take(5).rev() {
            let msg = if log.message.len() > 50 {
                format!("{}...", &log.message[..47])
            } else {
                log.message.clone()
            };
            println!("║ [{:5}] {:52} ║", log.level, msg);
        }
    }

    println!("╠════════════════════════════════════════════════════════════════╣");
    println!("║ REPLAY COMMAND:                                                ║");
    if let Some(seed) = artifact.metadata.rng_seed {
        println!(
            "║ TOS_TEST_SEED=0x{:016x} cargo test {}  ║",
            seed,
            if artifact.metadata.test_name.len() <= 25 {
                format!("{:25}", artifact.metadata.test_name)
            } else {
                format!("{}...", &artifact.metadata.test_name[..22])
            }
        );
    } else {
        println!("║ cargo test {:48} ║", artifact.metadata.test_name);
    }
    println!("╚════════════════════════════════════════════════════════════════╝");
}

/// Extract replay command from artifact
///
/// Returns the shell command needed to replay the test with the same seed.
///
/// # Examples
///
/// ```rust,ignore
/// use tos_testing_framework::utilities::replay::{load_artifact, get_replay_command};
///
/// #[tokio::test]
/// async fn test_get_replay_cmd() -> Result<()> {
///     let artifact = load_artifact("./artifacts/test_example.json").await?;
///     let cmd = get_replay_command(&artifact);
///     println!("Replay with: {}", cmd);
///     Ok(())
/// }
/// ```
pub fn get_replay_command(artifact: &TestArtifact) -> String {
    if let Some(seed) = artifact.metadata.rng_seed {
        format!(
            "TOS_TEST_SEED=0x{:016x} cargo test {}",
            seed, artifact.metadata.test_name
        )
    } else {
        format!("cargo test {}", artifact.metadata.test_name)
    }
}

/// Validate artifact integrity
///
/// Checks that the artifact has consistent data and all required fields.
///
/// # Examples
///
/// ```rust,ignore
/// use tos_testing_framework::utilities::replay::{load_artifact, validate_artifact};
///
/// #[tokio::test]
/// async fn test_validate() -> Result<()> {
///     let artifact = load_artifact("./artifacts/test_example.json").await?;
///     validate_artifact(&artifact)?;
///     println!("Artifact is valid!");
///     Ok(())
/// }
/// ```
pub fn validate_artifact(artifact: &TestArtifact) -> Result<()> {
    // Check test name is not empty
    if artifact.metadata.test_name.is_empty() {
        anyhow::bail!("Artifact has empty test name");
    }

    // Check timestamp is valid
    if artifact.metadata.timestamp.is_empty() {
        anyhow::bail!("Artifact has empty timestamp");
    }

    // If topology exists, validate it
    if let Some(ref topology) = artifact.topology {
        if topology.node_count == 0 {
            anyhow::bail!("Topology has zero nodes");
        }

        // Validate blockchain states match topology
        if artifact.blockchain_states.len() > topology.node_count {
            anyhow::bail!(
                "More blockchain states ({}) than nodes ({})",
                artifact.blockchain_states.len(),
                topology.node_count
            );
        }
    }

    // Validate blockchain states
    for state in &artifact.blockchain_states {
        // Check balances sum to total_supply + fees_burned
        let balances_sum: u128 = state.balances.values().map(|&b| b as u128).sum();
        let expected_total = balances_sum + state.fees_burned as u128;

        if state.total_supply != expected_total {
            anyhow::bail!(
                "Node {} supply mismatch: total_supply={}, balances+burned={}",
                state.node_id,
                state.total_supply,
                expected_total
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utilities::artifacts::{
        ArtifactCollector, BlockchainStateSnapshot, TopologySnapshot,
    };
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_load_artifact() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let mut collector = ArtifactCollector::new("test_load");
        collector.set_rng_seed(0x12345);

        let filepath = collector.save(temp_dir.path()).await?;
        let loaded = load_artifact(&filepath).await?;

        assert_eq!(loaded.metadata.test_name, "test_load");
        assert_eq!(loaded.metadata.rng_seed, Some(0x12345));

        Ok(())
    }

    #[test]
    fn test_get_replay_command_with_seed() {
        let artifact = TestArtifact {
            metadata: crate::utilities::artifacts::TestMetadata {
                test_name: "test_example".to_string(),
                rng_seed: Some(0xdeadbeefcafebabe),
                timestamp: "2025-11-15T12:00:00Z".to_string(),
                duration_ms: 1000,
                failure_reason: None,
            },
            topology: None,
            blockchain_states: vec![],
            transactions: vec![],
            logs: vec![],
        };

        let cmd = get_replay_command(&artifact);
        assert!(cmd.contains("TOS_TEST_SEED=0xdeadbeefcafebabe"));
        assert!(cmd.contains("cargo test test_example"));
    }

    #[test]
    fn test_get_replay_command_without_seed() {
        let artifact = TestArtifact {
            metadata: crate::utilities::artifacts::TestMetadata {
                test_name: "test_no_seed".to_string(),
                rng_seed: None,
                timestamp: "2025-11-15T12:00:00Z".to_string(),
                duration_ms: 500,
                failure_reason: None,
            },
            topology: None,
            blockchain_states: vec![],
            transactions: vec![],
            logs: vec![],
        };

        let cmd = get_replay_command(&artifact);
        assert!(!cmd.contains("TOS_TEST_SEED"));
        assert_eq!(cmd, "cargo test test_no_seed");
    }

    #[test]
    fn test_validate_artifact_success() {
        let artifact = TestArtifact {
            metadata: crate::utilities::artifacts::TestMetadata {
                test_name: "test_valid".to_string(),
                rng_seed: Some(123),
                timestamp: "2025-11-15T12:00:00Z".to_string(),
                duration_ms: 1000,
                failure_reason: None,
            },
            topology: Some(TopologySnapshot {
                node_count: 2,
                connections: HashMap::new(),
                partitions: vec![],
            }),
            blockchain_states: vec![BlockchainStateSnapshot {
                node_id: 0,
                tip_height: 5,
                tip_hash: "0xabc".to_string(),
                balances: HashMap::from([("0xaddr1".to_string(), 1000)]),
                nonces: HashMap::new(),
                total_supply: 1000, // balances (1000) + fees_burned (0)
                fees_burned: 0,
            }],
            transactions: vec![],
            logs: vec![],
        };

        assert!(validate_artifact(&artifact).is_ok());
    }

    #[test]
    fn test_validate_artifact_empty_test_name() {
        let artifact = TestArtifact {
            metadata: crate::utilities::artifacts::TestMetadata {
                test_name: "".to_string(),
                rng_seed: None,
                timestamp: "2025-11-15T12:00:00Z".to_string(),
                duration_ms: 0,
                failure_reason: None,
            },
            topology: None,
            blockchain_states: vec![],
            transactions: vec![],
            logs: vec![],
        };

        assert!(validate_artifact(&artifact).is_err());
    }

    #[test]
    fn test_validate_artifact_supply_mismatch() {
        let artifact = TestArtifact {
            metadata: crate::utilities::artifacts::TestMetadata {
                test_name: "test_mismatch".to_string(),
                rng_seed: None,
                timestamp: "2025-11-15T12:00:00Z".to_string(),
                duration_ms: 0,
                failure_reason: None,
            },
            topology: None,
            blockchain_states: vec![BlockchainStateSnapshot {
                node_id: 0,
                tip_height: 1,
                tip_hash: "0x123".to_string(),
                balances: HashMap::from([("0xaddr".to_string(), 1000)]),
                nonces: HashMap::new(),
                total_supply: 9999, // WRONG: should be 1000
                fees_burned: 0,
            }],
            transactions: vec![],
            logs: vec![],
        };

        let result = validate_artifact(&artifact);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("supply mismatch"));
    }

    #[test]
    fn test_print_artifact_summary() {
        let artifact = TestArtifact {
            metadata: crate::utilities::artifacts::TestMetadata {
                test_name: "test_print".to_string(),
                rng_seed: Some(0xabcd1234),
                timestamp: "2025-11-15T12:00:00Z".to_string(),
                duration_ms: 5000,
                failure_reason: Some("Height mismatch".to_string()),
            },
            topology: Some(TopologySnapshot {
                node_count: 3,
                connections: HashMap::new(),
                partitions: vec![],
            }),
            blockchain_states: vec![],
            transactions: vec![],
            logs: vec![],
        };

        // This should not panic
        print_artifact_summary(&artifact);
    }
}
