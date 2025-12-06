// ============================================================================
// SYNC BLOCK VALIDATOR (P0-3)
//
// Provides additional security validation during chain synchronization.
//
// IMPLEMENTATION STATUS:
// ✅ Mergeset size limit (4*k + 16) - IMPLEMENTED in blockchain.rs:add_new_block
//    This protects ALL block additions, not just sync, which is more comprehensive.
//
// ✅ Parent wait loop with per-block timeout - IMPLEMENTED in mod.rs:handle_chain_response
//    When ParentNotFound is returned, block is deferred and retried with timeout.
//    Uses P2pError::SyncMaxRetriesExceeded when max retries exceeded.
//
// ✅ Blue work monotonicity checking - Available in this module for optional use.
//    The sync flow processes blocks by topoheight order, so blue_work should
//    naturally increase. Monitoring is optional but available.
//
// This module provides additional utilities that can be used for enhanced
// monitoring or alternative sync implementations.
//
// Reference: TOS_FORK_PREVENTION_IMPLEMENTATION_V2.md
// ============================================================================

use crate::core::{blockchain::Blockchain, ghostdag::BlueWorkType, storage::Storage};
use crate::p2p::error::P2pError;
use log::{debug, info, warn};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tos_common::{
    block::Block,
    crypto::{Hash, Hashable},
    tokio::time::sleep,
};

/// Configuration for sync validation
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct SyncValidatorConfig {
    /// Multiplier for k to calculate max mergeset (default: 4)
    /// Formula: max_mergeset = k_multiplier * k + safety_margin
    pub k_multiplier: usize,
    /// Safety margin added to mergeset limit (default: 16)
    pub safety_margin: usize,
    /// Timeout for waiting on parent fetches per block (default: 30s)
    pub parent_fetch_timeout: Duration,
    /// Maximum retries for parent fetch rounds (default: 3)
    pub max_parent_retries: u32,
}

impl Default for SyncValidatorConfig {
    fn default() -> Self {
        Self {
            // 4*k + 16 is more permissive than 2*k + 8
            // For k=10: max_mergeset = 56 (vs 28 before)
            // This handles bursty/high-latency conditions better
            k_multiplier: 4,
            safety_margin: 16,
            parent_fetch_timeout: Duration::from_secs(30),
            max_parent_retries: 3,
        }
    }
}

/// Result of validating a sync block
#[allow(dead_code)]
pub enum SyncValidationResult {
    /// Block is valid and ready to be added
    Ready,
    /// Block needs parents that are not yet available
    NeedsParents(Vec<Hash>),
}

/// Validates blocks during chain synchronization with parent tracking.
///
/// This validator tracks blocks that are waiting for their parents and
/// provides methods to handle deferred block processing with per-block
/// timeout windows.
#[allow(dead_code)]
pub struct SyncBlockValidator<'a, S: Storage> {
    /// Reference to the blockchain
    blockchain: &'a Blockchain<S>,
    /// Last validated blue_work for monotonicity checking
    last_validated_blue_work: BlueWorkType,
    /// Blocks waiting for parents: child_hash -> set of missing parent hashes
    blocks_waiting_for_parents: HashMap<Hash, HashSet<Hash>>,
    /// All pending parent requests (for deduplication)
    pending_parent_requests: HashSet<Hash>,
    /// K parameter for dynamic mergeset limit
    k_parameter: u8,
    /// Configuration
    pub config: SyncValidatorConfig,
}

#[allow(dead_code)]
impl<'a, S: Storage> SyncBlockValidator<'a, S> {
    /// Create a new SyncBlockValidator with default config
    pub fn new(blockchain: &'a Blockchain<S>, k_parameter: u8) -> Self {
        Self::with_config(blockchain, k_parameter, SyncValidatorConfig::default())
    }

    /// Create a new SyncBlockValidator with custom config
    pub fn with_config(
        blockchain: &'a Blockchain<S>,
        k_parameter: u8,
        config: SyncValidatorConfig,
    ) -> Self {
        Self {
            blockchain,
            last_validated_blue_work: BlueWorkType::zero(),
            blocks_waiting_for_parents: HashMap::new(),
            pending_parent_requests: HashSet::new(),
            k_parameter,
            config,
        }
    }

    /// Calculate dynamic maximum mergeset size based on k parameter
    /// Formula: max_mergeset = k_multiplier * k + safety_margin
    pub fn max_mergeset_size(&self) -> usize {
        self.config.k_multiplier * (self.k_parameter as usize) + self.config.safety_margin
    }

    /// Notify validator that a parent block has arrived
    /// Call this when blocks are added to storage to unblock waiting children
    pub fn on_parent_arrived(&mut self, parent_hash: &Hash) {
        // Remove from pending requests
        self.pending_parent_requests.remove(parent_hash);

        // Update all blocks waiting for this parent
        let mut unblocked_children = Vec::new();
        for (child_hash, missing_parents) in self.blocks_waiting_for_parents.iter_mut() {
            missing_parents.remove(parent_hash);
            if missing_parents.is_empty() {
                unblocked_children.push(child_hash.clone());
            }
        }

        // Remove children that now have all parents
        for child_hash in unblocked_children {
            self.blocks_waiting_for_parents.remove(&child_hash);
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Block {} unblocked after parent {} arrived",
                    child_hash, parent_hash
                );
            }
        }
    }

    /// Check if a block is ready for validation (all parents present)
    pub fn is_block_ready(&self, block_hash: &Hash) -> bool {
        !self.blocks_waiting_for_parents.contains_key(block_hash)
    }

    /// Get missing parents for a block
    pub fn get_missing_parents(&self, block_hash: &Hash) -> Option<HashSet<Hash>> {
        self.blocks_waiting_for_parents.get(block_hash).cloned()
    }

    /// Get all pending parent requests (for deduplication when fetching)
    pub fn get_pending_parent_requests(&self) -> &HashSet<Hash> {
        &self.pending_parent_requests
    }

    /// Check if a parent is already being requested
    pub fn is_parent_pending(&self, parent_hash: &Hash) -> bool {
        self.pending_parent_requests.contains(parent_hash)
    }

    /// Get count of blocks waiting for parents
    pub fn deferred_count(&self) -> usize {
        self.blocks_waiting_for_parents.len()
    }

    /// Check if block has all parents available in blockchain
    pub async fn check_parents(&mut self, block: &Block) -> Result<SyncValidationResult, P2pError> {
        let block_hash = block.hash();

        // Check all parents
        let mut missing_parents = Vec::new();
        for parent_hash in block.get_parents() {
            if !self.blockchain.has_block(parent_hash).await? {
                missing_parents.push(parent_hash.clone());
                self.pending_parent_requests.insert(parent_hash.clone());
            }
        }

        if !missing_parents.is_empty() {
            let missing_set: HashSet<Hash> = missing_parents.iter().cloned().collect();
            let missing_count = missing_parents.len();
            self.blocks_waiting_for_parents
                .insert(block_hash.clone(), missing_set);

            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Sync validation: block {} missing {} parent(s), deferring",
                    block_hash, missing_count
                );
            }

            return Ok(SyncValidationResult::NeedsParents(missing_parents));
        }

        Ok(SyncValidationResult::Ready)
    }

    /// Validate mergeset size for a block
    /// This should be called after GHOSTDAG computation
    pub fn validate_mergeset(
        &self,
        block_hash: &Hash,
        blues_count: usize,
        reds_count: usize,
    ) -> Result<(), P2pError> {
        let mergeset_size = blues_count + reds_count;
        let max_mergeset = self.max_mergeset_size();

        if mergeset_size > max_mergeset {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "SYNC SAFETY: Block {} rejected - mergeset too large ({} > max {}). \
                     This may indicate a 'wide but light' attack. k={}, blues={}, reds={}",
                    block_hash,
                    mergeset_size,
                    max_mergeset,
                    self.k_parameter,
                    blues_count,
                    reds_count
                );
            }
            return Err(P2pError::SyncMergesetTooLarge {
                block_hash: block_hash.clone(),
                size: mergeset_size,
                max_size: max_mergeset,
            });
        }

        Ok(())
    }

    /// Check blue_work monotonicity (warning only, don't reject)
    pub fn check_blue_work_monotonicity(&mut self, block_hash: &Hash, blue_work: BlueWorkType) {
        if blue_work < self.last_validated_blue_work {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "SYNC SAFETY: Blue work decreased during sync. \
                     Block {} blue_work={} < previous blue_work={}. \
                     This may indicate sync ordering issue.",
                    block_hash, blue_work, self.last_validated_blue_work
                );
            }
        }

        // Update if higher
        if blue_work > self.last_validated_blue_work {
            self.last_validated_blue_work = blue_work;
        }
    }

    /// Mark a block as processed (remove from waiting lists)
    pub fn mark_processed(&mut self, block_hash: &Hash) {
        self.blocks_waiting_for_parents.remove(block_hash);
    }

    /// Reset the validator state
    pub fn reset(&mut self) {
        self.last_validated_blue_work = BlueWorkType::zero();
        self.blocks_waiting_for_parents.clear();
        self.pending_parent_requests.clear();
    }
}

/// Process deferred blocks with per-block timeout
///
/// This implements the parent wait loop from the fork prevention design.
/// Each block gets its own timeout window (not shared per-batch).
///
/// # Arguments
/// * `validator` - The sync block validator
/// * `deferred_blocks` - Blocks waiting for parents
/// * `blockchain` - Blockchain reference for checking parent availability
/// * `on_parent_needed` - Callback to request missing parents
/// * `on_block_ready` - Callback when block is ready to process
///
/// # Returns
/// * Ok(processed_count) if all deferred blocks were processed
/// * Err if timeout or max retries exceeded
#[allow(dead_code)]
pub async fn process_deferred_blocks<S, FParent, FBlock, FutParent, FutBlock>(
    validator: &mut SyncBlockValidator<'_, S>,
    mut deferred_blocks: Vec<Arc<Block>>,
    blockchain: &Blockchain<S>,
    mut on_parent_needed: FParent,
    mut on_block_ready: FBlock,
) -> Result<usize, P2pError>
where
    S: Storage,
    FParent: FnMut(&Hash) -> FutParent,
    FutParent: std::future::Future<Output = Result<(), P2pError>>,
    FBlock: FnMut(Arc<Block>) -> FutBlock,
    FutBlock: std::future::Future<Output = Result<(), P2pError>>,
{
    if deferred_blocks.is_empty() {
        return Ok(0);
    }

    if log::log_enabled!(log::Level::Info) {
        info!(
            "Processing {} deferred blocks (waiting for parents)...",
            deferred_blocks.len()
        );
    }

    let timeout = validator.config.parent_fetch_timeout;
    let max_retries = validator.config.max_parent_retries;
    let mut retry_count = 0;
    let mut total_processed = 0;

    while !deferred_blocks.is_empty() && retry_count < max_retries {
        let mut still_deferred = Vec::new();

        for block in deferred_blocks.drain(..) {
            let block_hash = block.hash();

            // V2.3 FIX: Each block gets its own timeout window
            let block_start = Instant::now();

            // Poll until all parents arrive or this block's timeout expires
            loop {
                if block_start.elapsed() > timeout {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!(
                            "Timeout waiting for parents of block {} ({}s), retry {}/{}",
                            block_hash,
                            timeout.as_secs(),
                            retry_count + 1,
                            max_retries
                        );
                    }
                    break;
                }

                // Check if block is ready now
                if validator.is_block_ready(&block_hash) {
                    break;
                }

                // Check if any parents have arrived
                let mut any_arrived = false;
                if let Some(missing) = validator.get_missing_parents(&block_hash) {
                    for parent_hash in missing {
                        if blockchain.has_block(&parent_hash).await.unwrap_or(false) {
                            validator.on_parent_arrived(&parent_hash);
                            any_arrived = true;
                        }
                    }
                }

                if !any_arrived {
                    // Wait a bit before checking again
                    sleep(Duration::from_millis(100)).await;
                }
            }

            // Try to process the block now
            match validator.check_parents(&block).await? {
                SyncValidationResult::Ready => {
                    // Block is ready, call the handler
                    on_block_ready(block.clone()).await?;

                    // Notify that this block is now available as a parent
                    validator.on_parent_arrived(&block_hash);

                    total_processed += 1;
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Processed deferred block {}", block_hash);
                    }
                }
                SyncValidationResult::NeedsParents(missing_parents) => {
                    // Still missing parents, request them and defer again
                    for parent_hash in &missing_parents {
                        if !validator.is_parent_pending(parent_hash) {
                            on_parent_needed(parent_hash).await?;
                        }
                    }
                    still_deferred.push(block);
                }
            }
        }

        deferred_blocks = still_deferred;

        if !deferred_blocks.is_empty() {
            retry_count += 1;
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "{} blocks still waiting for parents after round {}/{}",
                    deferred_blocks.len(),
                    retry_count,
                    max_retries
                );
            }
        }
    }

    if !deferred_blocks.is_empty() {
        if log::log_enabled!(log::Level::Warn) {
            warn!(
                "Failed to process {} blocks after {} retries - max retries exceeded",
                deferred_blocks.len(),
                max_retries
            );
        }
        return Err(P2pError::SyncMaxRetriesExceeded(deferred_blocks.len()));
    }

    if log::log_enabled!(log::Level::Info) {
        info!("Completed processing {} deferred blocks", total_processed);
    }

    Ok(total_processed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SyncValidatorConfig::default();
        assert_eq!(config.k_multiplier, 4);
        assert_eq!(config.safety_margin, 16);
        assert_eq!(config.parent_fetch_timeout, Duration::from_secs(30));
        assert_eq!(config.max_parent_retries, 3);
    }

    #[test]
    fn test_max_mergeset_calculation() {
        // Test with k=10: max = 4*10 + 16 = 56
        let config = SyncValidatorConfig::default();
        let max = config.k_multiplier * 10 + config.safety_margin;
        assert_eq!(max, 56);

        // Test with k=18: max = 4*18 + 16 = 88
        let max = config.k_multiplier * 18 + config.safety_margin;
        assert_eq!(max, 88);

        // Test with k=32: max = 4*32 + 16 = 144
        let max = config.k_multiplier * 32 + config.safety_margin;
        assert_eq!(max, 144);
    }
}
