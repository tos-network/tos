// TOS Reachability Rebuild Module
//
// Provides functionality to rebuild missing reachability data from GHOSTDAG data.
// This is used during node startup to recover from database inconsistencies,
// out-of-order block reception, or version upgrades.

use crate::core::error::BlockchainError;
use crate::core::reachability::TosReachability;
use crate::core::storage::Storage;
use tos_common::crypto::Hash;

/// Statistics from a reachability rebuild operation
#[derive(Debug, Default)]
pub struct RebuildStats {
    /// Total blocks checked
    pub blocks_checked: u64,
    /// Blocks that were missing reachability data
    pub blocks_rebuilt: u64,
    /// Blocks that already had reachability data
    pub blocks_skipped: u64,
}

/// Check if reachability data needs to be rebuilt
///
/// Returns true if any blocks are missing reachability data.
/// This is a fast check that can be run at startup.
pub async fn needs_rebuild<S: Storage>(
    storage: &S,
    genesis_hash: &Hash,
) -> Result<bool, BlockchainError> {
    // Check if genesis has reachability data
    if !storage.has_reachability_data(genesis_hash).await? {
        return Ok(true);
    }

    // Check reindex root - if missing, we need to rebuild
    if storage.get_reindex_root().await.is_err() {
        return Ok(true);
    }

    // Sample a few blocks at different topoheights to check for gaps
    let top_topoheight = storage.get_top_topoheight().await?;
    let sample_points = [
        0,
        top_topoheight / 4,
        top_topoheight / 2,
        3 * top_topoheight / 4,
        top_topoheight,
    ];

    for topo in sample_points {
        if topo > top_topoheight {
            continue;
        }
        if let Ok(hash) = storage.get_hash_at_topo_height(topo).await {
            if !storage.has_reachability_data(&hash).await? {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Rebuild all missing reachability data from GHOSTDAG data
///
/// This function walks through all blocks in topoheight order and rebuilds
/// reachability data for any blocks that are missing it.
///
/// # Algorithm
/// 1. Ensure genesis has reachability data (initialize if missing)
/// 2. Walk all blocks from topoheight 0 to top
/// 3. For each block without reachability data:
///    a. Get its GHOSTDAG selected_parent
///    b. Build reachability data using add_tree_block
///    c. Update future covering sets using add_dag_block
///
/// # Arguments
/// * `storage` - Mutable storage reference
/// * `genesis_hash` - Genesis block hash
///
/// # Returns
/// Statistics about the rebuild operation
pub async fn rebuild_missing_reachability<S: Storage>(
    storage: &mut S,
    genesis_hash: &Hash,
) -> Result<RebuildStats, BlockchainError> {
    let mut stats = RebuildStats::default();

    // Step 1: Ensure genesis has reachability data
    if !storage.has_reachability_data(genesis_hash).await? {
        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Initializing genesis reachability data for {}",
                genesis_hash
            );
        }
        let reachability = TosReachability::new(genesis_hash.clone());
        let genesis_data = reachability.genesis_reachability_data();
        storage
            .set_reachability_data(genesis_hash, &genesis_data)
            .await?;
        storage.set_reindex_root(genesis_hash.clone()).await?;
        stats.blocks_rebuilt += 1;
    } else {
        stats.blocks_skipped += 1;
    }
    stats.blocks_checked += 1;

    // Step 2: Get the top topoheight
    let top_topoheight = storage.get_top_topoheight().await?;
    if top_topoheight == 0 {
        // Only genesis exists, nothing more to do
        return Ok(stats);
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Checking reachability data for {} blocks (topoheight 1 to {})",
            top_topoheight,
            top_topoheight
        );
    }

    // Step 3: Walk all blocks in topoheight order
    let mut reachability = TosReachability::new(genesis_hash.clone());

    for topo in 1..=top_topoheight {
        stats.blocks_checked += 1;

        // Get block hash at this topoheight
        let hash = match storage.get_hash_at_topo_height(topo).await {
            Ok(h) => h,
            Err(_) => {
                // Gap in topoheight sequence - skip
                continue;
            }
        };

        // Check if reachability data exists
        if storage.has_reachability_data(&hash).await? {
            stats.blocks_skipped += 1;
            continue;
        }

        // Need to rebuild - get GHOSTDAG data
        let ghostdag_data = match storage.get_ghostdag_data(&hash).await {
            Ok(data) => data,
            Err(e) => {
                if log::log_enabled!(log::Level::Warn) {
                    log::warn!(
                        "Cannot rebuild reachability for block {} at topo {}: no GHOSTDAG data ({})",
                        hash, topo, e
                    );
                }
                continue;
            }
        };

        let selected_parent = ghostdag_data.selected_parent.clone();

        // Verify parent has reachability data
        if !storage.has_reachability_data(&selected_parent).await? {
            if log::log_enabled!(log::Level::Warn) {
                log::warn!(
                    "Cannot rebuild reachability for block {} at topo {}: parent {} missing reachability",
                    hash, topo, selected_parent
                );
            }
            continue;
        }

        // Add to reachability tree
        reachability
            .add_tree_block(storage, hash.clone(), selected_parent)
            .await?;

        // Update future covering sets for mergeset blues
        let mergeset_blues: Vec<Hash> = ghostdag_data.mergeset_blues.iter().cloned().collect();
        reachability
            .add_dag_block(storage, &hash, &mergeset_blues)
            .await?;

        stats.blocks_rebuilt += 1;

        // Log progress every 10000 blocks
        if stats.blocks_rebuilt % 10000 == 0 && log::log_enabled!(log::Level::Info) {
            log::info!(
                "Rebuilt reachability data for {} blocks (at topo {})",
                stats.blocks_rebuilt,
                topo
            );
        }
    }

    // Step 4: Ensure reindex root is set correctly
    // Set it to ~100 blocks behind the tip
    let reindex_root_topo = if top_topoheight > 100 {
        top_topoheight - 100
    } else {
        0
    };

    if let Ok(reindex_root_hash) = storage.get_hash_at_topo_height(reindex_root_topo).await {
        storage.set_reindex_root(reindex_root_hash).await?;
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Reachability rebuild complete: checked={}, rebuilt={}, skipped={}",
            stats.blocks_checked,
            stats.blocks_rebuilt,
            stats.blocks_skipped
        );
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Integration tests would require a full storage mock
    // Unit tests here focus on the RebuildStats structure

    #[test]
    fn test_rebuild_stats_default() {
        let stats = RebuildStats::default();
        assert_eq!(stats.blocks_checked, 0);
        assert_eq!(stats.blocks_rebuilt, 0);
        assert_eq!(stats.blocks_skipped, 0);
    }
}
