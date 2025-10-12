// TOS Optimized Block Template Generator
// Implements caching and optimizations for mining block template generation

use std::sync::Arc;
use std::time::Instant;
use log::{debug, warn};
use tos_common::{
    block::{BlockHeader, EXTRA_NONCE_SIZE, get_combined_hash_for_tips},
    crypto::{Hash, PublicKey},
    config::TIPS_LIMIT,
    time::get_current_time_in_millis,
    network::Network,
};
use crate::core::{
    error::BlockchainError,
    storage::Storage,
    tx_selector::TxSelectorEntry,
    blockdag,
    hard_fork::get_version_at_height,
};
use super::{
    cache::{GhostdagCache, BlockTemplateCache, TipSelectionCache},
    stats::MiningStats,
};

/// Optimized block template generator with caching
pub struct BlockTemplateGenerator {
    /// GHOSTDAG data cache
    ghostdag_cache: GhostdagCache,

    /// Block template cache (to avoid regeneration)
    template_cache: BlockTemplateCache,

    /// Tip selection cache
    tip_cache: TipSelectionCache,

    /// Mining statistics
    stats: Arc<MiningStats>,

    /// Template cache TTL in milliseconds
    template_ttl_ms: u64,

    /// Network (for version calculation)
    network: Network,
}

impl BlockTemplateGenerator {
    /// Create a new block template generator with caching
    ///
    /// # Arguments
    /// * `ghostdag_cache_size` - Size of GHOSTDAG data cache
    /// * `tip_cache_size` - Size of tip selection cache
    /// * `template_ttl_ms` - How long to cache templates (milliseconds)
    /// * `stats` - Mining statistics tracker
    /// * `network` - Network (for version calculation)
    pub fn new(
        ghostdag_cache_size: usize,
        tip_cache_size: usize,
        template_ttl_ms: u64,
        stats: Arc<MiningStats>,
        network: Network,
    ) -> Self {
        Self {
            ghostdag_cache: GhostdagCache::new(ghostdag_cache_size),
            template_cache: BlockTemplateCache::new(template_ttl_ms),
            tip_cache: TipSelectionCache::new(tip_cache_size),
            stats,
            template_ttl_ms,
            network,
        }
    }

    /// Get GHOSTDAG data with caching
    async fn get_ghostdag_data_cached<S: Storage>(
        &self,
        storage: &S,
        hash: &Hash,
    ) -> Result<Arc<crate::core::ghostdag::TosGhostdagData>, BlockchainError> {
        // Check cache first
        if let Some(data) = self.ghostdag_cache.get(hash).await {
            self.stats.record_cache_hit();
            return Ok(data);
        }

        // Cache miss - fetch from storage
        self.stats.record_cache_miss();
        let start = Instant::now();

        let data = storage.get_ghostdag_data(hash).await?;

        // Store in cache for next time
        self.ghostdag_cache.put(hash.clone(), data.clone()).await;

        self.stats.record_ghostdag_calculation(start.elapsed());

        Ok(data)
    }

    /// Validate that a tip is acceptable for mining
    async fn validate_tip<S: Storage>(
        &self,
        storage: &S,
        best_tip: &Hash,
        tip: &Hash,
    ) -> Result<bool, BlockchainError> {
        if best_tip == tip {
            return Ok(true);
        }

        // Get difficulties with caching
        let best_diff = storage.get_difficulty_for_block_hash(best_tip).await?;
        let tip_diff = storage.get_difficulty_for_block_hash(tip).await?;

        // Tip difficulty must be at least 91% of best tip
        let min_diff = (best_diff.as_ref() * 91u64) / 100u64;
        Ok(tip_diff.as_ref() >= &min_diff)
    }

    /// Check if a tip is near enough to main chain
    async fn is_near_enough_from_main_chain<S: Storage>(
        &self,
        storage: &S,
        tip: &Hash,
        current_height: u64,
    ) -> Result<bool, BlockchainError> {
        let tip_data = self.get_ghostdag_data_cached(storage, tip).await?;

        // Allow tips within 10 blocks of current height
        const MAX_DISTANCE: u64 = 10;
        Ok(current_height.saturating_sub(tip_data.blue_score) <= MAX_DISTANCE)
    }

    /// Select and validate tips for block template
    async fn select_and_validate_tips<S: Storage>(
        &self,
        storage: &S,
        tips: Vec<Hash>,
        current_height: u64,
    ) -> Result<Vec<Hash>, BlockchainError> {
        if tips.len() <= 1 {
            return Ok(tips);
        }

        // Generate cache key from tips
        let tips_hash = get_combined_hash_for_tips(tips.iter().cloned());

        // Check if we have cached validated tips
        if let Some(cached_tips) = self.tip_cache.get(&tips_hash).await {
            debug!("Using cached tip selection");
            return Ok((*cached_tips).clone());
        }

        // Find best tip by blue_work
        let mut best_tip = tips[0].clone();
        let mut best_blue_work = self.get_ghostdag_data_cached(storage, &best_tip).await?.blue_work;

        for tip in tips.iter().skip(1) {
            let data = self.get_ghostdag_data_cached(storage, tip).await?;
            if data.blue_work > best_blue_work {
                best_blue_work = data.blue_work;
                best_tip = tip.clone();
            }
        }

        debug!("Best tip selected by GHOSTDAG (blue_work={}): {}", best_blue_work, best_tip);

        // Validate other tips
        let mut selected_tips = Vec::with_capacity(tips.len());
        for hash in tips {
            if best_tip != hash {
                if !self.validate_tip(storage, &best_tip, &hash).await? {
                    warn!("Tip {} is invalid, not selecting it because difficulty can't be less than 91% of {}", hash, best_tip);
                    continue;
                }

                if !self.is_near_enough_from_main_chain(storage, &hash, current_height).await? {
                    warn!("Tip {} is not selected for mining: too far from mainchain at height: {}", hash, current_height);
                    continue;
                }
            }
            selected_tips.push(hash);
        }

        if selected_tips.is_empty() {
            warn!("No valid tips found for block template, using best tip {}", best_tip);
            selected_tips.push(best_tip);
        }

        // Cache the validated tips
        let selected_arc = Arc::new(selected_tips.clone());
        self.tip_cache.put(tips_hash, selected_arc).await;

        Ok(selected_tips)
    }

    /// Generate an optimized block header template
    pub async fn generate_header_template<S: Storage>(
        &self,
        storage: &S,
        address: PublicKey,
        current_height: u64,
    ) -> Result<BlockHeader, BlockchainError> {
        let start = Instant::now();

        let extra_nonce: [u8; EXTRA_NONCE_SIZE] = rand::random();
        let tips_set = storage.get_tips().await?;
        let mut tips: Vec<Hash> = tips_set.into_iter().collect();

        // Select and validate tips with caching
        tips = self.select_and_validate_tips(storage, tips, current_height).await?;

        // Sort tips by blue work
        let mut sorted_tips = blockdag::sort_tips(storage, tips.into_iter()).await?;
        if sorted_tips.len() > TIPS_LIMIT {
            let dropped_tips = sorted_tips.drain(TIPS_LIMIT..);
            warn!("Dropping tips {} because they are not in the first {} heavier tips",
                  dropped_tips.map(|h| h.to_string()).collect::<Vec<String>>().join(", "),
                  TIPS_LIMIT);
        }

        // Find the newest timestamp
        let mut timestamp = 0;
        for tip in sorted_tips.iter() {
            let tip_timestamp = storage.get_timestamp_for_block_hash(tip).await?;
            if tip_timestamp > timestamp {
                timestamp = tip_timestamp;
            }
        }

        // Check that our current timestamp is correct
        let current_timestamp = get_current_time_in_millis();
        if current_timestamp < timestamp {
            warn!("Current timestamp is less than the newest tip timestamp, using newest timestamp from tips");
        } else {
            timestamp = current_timestamp;
        }

        let height = blockdag::calculate_height_at_tips(storage, sorted_tips.iter()).await?;
        let sorted_tips_vec: Vec<Hash> = sorted_tips.into_iter().collect();
        let version = get_version_at_height(&self.network, height);

        let block = BlockHeader::new_simple(
            version,
            sorted_tips_vec,
            timestamp,
            extra_nonce,
            address,
            Hash::zero()
        );

        self.stats.record_template_generation(start.elapsed());

        Ok(block)
    }

    /// Clear all caches
    pub async fn clear_caches(&self) {
        self.ghostdag_cache.clear().await;
        self.template_cache.clear().await;
        self.tip_cache.clear().await;
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> CacheStats {
        CacheStats {
            ghostdag_cache_size: self.ghostdag_cache.len().await,
            tip_cache_size: 0, // TipSelectionCache doesn't expose size
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub ghostdag_cache_size: usize,
    pub tip_cache_size: usize,
}

/// Transaction selector with pre-computed fee ordering
pub struct OptimizedTxSelector {
    /// Pre-sorted transaction entries by fee
    entries: Vec<TxSelectorEntry<'static>>,
    /// Current index in the sorted list
    index: usize,
}

impl OptimizedTxSelector {
    /// Create a new optimized transaction selector
    /// Pre-computes and caches fee information for faster selection
    pub fn new<'a, I>(iter: I) -> Self
    where
        I: Iterator<Item = (usize, &'a Arc<Hash>, &'a Arc<tos_common::transaction::Transaction>)>
    {
        let mut entries: Vec<TxSelectorEntry> = iter.map(|(size, hash, tx)| {
            TxSelectorEntry {
                hash: unsafe { std::mem::transmute(hash) },
                tx: unsafe { std::mem::transmute(tx) },
                size,
            }
        }).collect();

        // Sort by fee in descending order
        entries.sort_by(|a, b| {
            b.tx.get_fee().cmp(&a.tx.get_fee())
                .then_with(|| a.tx.get_nonce().cmp(&b.tx.get_nonce()))
        });

        Self {
            entries,
            index: 0,
        }
    }

    /// Get the next transaction with highest fee
    pub fn next(&mut self) -> Option<&TxSelectorEntry> {
        if self.index < self.entries.len() {
            let entry = &self.entries[self.index];
            self.index += 1;
            Some(entry)
        } else {
            None
        }
    }

    /// Check if selector is empty
    pub fn is_empty(&self) -> bool {
        self.index >= self.entries.len()
    }

    /// Get total number of transactions
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Get remaining transactions count
    pub fn remaining(&self) -> usize {
        self.entries.len().saturating_sub(self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::tokio;

    #[tokio::test]
    async fn test_block_template_generator_creation() {
        let stats = MiningStats::new(100);
        let network = tos_common::network::Network::Mainnet;
        let generator = BlockTemplateGenerator::new(1000, 100, 5000, stats, network);

        let cache_stats = generator.get_cache_stats().await;
        assert_eq!(cache_stats.ghostdag_cache_size, 0);
    }

    #[tokio::test]
    async fn test_clear_caches() {
        let stats = MiningStats::new(100);
        let network = tos_common::network::Network::Mainnet;
        let generator = BlockTemplateGenerator::new(1000, 100, 5000, stats, network);

        generator.clear_caches().await;

        let cache_stats = generator.get_cache_stats().await;
        assert_eq!(cache_stats.ghostdag_cache_size, 0);
    }
}
