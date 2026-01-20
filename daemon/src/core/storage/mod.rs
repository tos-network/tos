mod cache;
mod constants;
mod providers;

pub mod rocksdb;
pub mod snapshot;

pub use self::{cache::*, providers::*, rocksdb::RocksStorage};

use crate::{config::PRUNE_SAFETY_LIMIT, core::error::BlockchainError};
use async_trait::async_trait;
use log::{debug, trace, warn};
use std::collections::HashSet;
use tos_common::{
    block::{BlockHeader, TopoHeight},
    contract::ContractProvider as ContractInfoProvider,
    crypto::Hash,
    immutable::Immutable,
    nft::NftStorageProvider,
    transaction::Transaction,
};

// Represents the tips of the chain or of a block
pub type Tips = HashSet<Hash>;

#[async_trait]
pub trait Storage:
    BlockExecutionOrderProvider
    + DagOrderProvider
    + PrunedTopoheightProvider
    + NonceProvider
    + AccountProvider
    + AgentAccountProvider
    + A2ANonceProvider
    + ClientProtocolProvider
    + BlockDagProvider
    + MerkleHashProvider
    + NetworkProvider
    + MultiSigProvider
    + TipsProvider
    + SnapshotProvider
    + ContractProvider
    + ContractDataProvider
    + ContractOutputsProvider
    + ContractInfoProvider
    + ContractBalanceProvider
    + ContractAssetExtProvider
    + ContractEventProvider
    + ContractScheduledExecutionProvider
    + VersionedProvider
    + SupplyProvider
    + CacheProvider
    + StateProvider
    + EnergyProvider
    + ReferralProvider
    + EscrowProvider
    + ArbiterProvider
    + ArbitrationCommitProvider
    + KycProvider
    + CommitteeProvider
    + NftProvider
    + NftStorageProvider
    + UnoBalanceProvider
    + TnsProvider
    + Sync
    + Send
    + 'static
{
    // delete block at topoheight, and all pointers (hash_at_topo, topo_by_hash, reward, supply, diff, cumulative diff...)
    async fn delete_block_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<
        (
            Hash,
            Immutable<BlockHeader>,
            Vec<(Hash, Immutable<Transaction>)>,
        ),
        BlockchainError,
    >;

    // Count is the number of blocks (topoheight) to rewind
    async fn pop_blocks(
        &mut self,
        mut height: u64,
        mut topoheight: TopoHeight,
        count: u64,
        until_topo_height: TopoHeight,
    ) -> Result<(u64, TopoHeight, Vec<(Hash, Immutable<Transaction>)>), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "pop blocks from height: {}, topoheight: {}, count: {}",
                height,
                topoheight,
                count
            );
        }
        if topoheight < count as u64 {
            // also prevent removing genesis block
            return Err(BlockchainError::NotEnoughBlocks);
        }

        let start_topoheight = topoheight;
        // search the lowest topo height available based on count + 1
        // (last lowest topo height accepted)
        let mut lowest_topo = topoheight - count;
        if log::log_enabled!(log::Level::Trace) {
            trace!("Lowest topoheight for rewind: {}", lowest_topo);
        }

        let pruned_topoheight = self.get_pruned_topoheight().await?.unwrap_or(0);

        if pruned_topoheight != 0 {
            let safety_pruned_topoheight = pruned_topoheight + PRUNE_SAFETY_LIMIT;
            if lowest_topo <= safety_pruned_topoheight && until_topo_height != 0 {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "Pruned topoheight is {}, lowest topoheight is {}, rewind only until {}",
                        pruned_topoheight, lowest_topo, safety_pruned_topoheight
                    );
                }
                lowest_topo = safety_pruned_topoheight;
            }
        }

        // new TIPS for chain
        let mut tips = self.get_tips().await?;

        // Delete all orphaned blocks tips
        for tip in tips.clone() {
            if !self.is_block_topological_ordered(&tip).await? {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Tip {} is not ordered, removing", tip);
                }
                tips.remove(&tip);
            }
        }

        // all txs to be rewinded
        let mut txs = Vec::new();
        let mut done = 0;
        'main: loop {
            // stop rewinding if its genesis block or if we reached the lowest topo
            if topoheight <= lowest_topo || topoheight <= until_topo_height || topoheight == 0 {
                // prevent removing genesis block
                if log::log_enabled!(log::Level::Trace) {
                    trace!("Done: {done}, count: {count}, height: {height}, topoheight: {topoheight}, lowest topo: {lowest_topo}, stable topo: {until_topo_height}");
                }
                break 'main;
            }

            // Delete the hash at topoheight
            let (hash, block, block_txs) = self.delete_block_at_topoheight(topoheight).await?;
            // Delete versioned data per-topoheight for data consistency during rewind
            self.delete_versioned_data_at_topoheight(topoheight).await?;

            if log::log_enabled!(log::Level::Debug) {
                debug!("Block {} at topoheight {} deleted", hash, topoheight);
            }
            txs.extend(block_txs);

            // generate new tips
            if log::log_enabled!(log::Level::Trace) {
                trace!("Removing {} from {} tips", hash, tips.len());
            }
            tips.remove(&hash);

            for hash in block.get_tips().iter() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("Adding {} to {} tips", hash, tips.len());
                }
                tips.insert(hash.clone());
            }

            if topoheight <= pruned_topoheight {
                warn!("Pruned topoheight is reached, this is not healthy, starting from 0");
                topoheight = 0;
                height = 0;

                // Remove total blocks
                done = start_topoheight;

                tips.clear();
                tips.insert(self.get_hash_at_topo_height(0).await?);

                self.set_pruned_topoheight(None).await?;

                // Clear out ALL data
                self.delete_versioned_data_above_topoheight(0).await?;

                break 'main;
            }

            topoheight -= 1;
            // height of old block become new height
            if block.get_height() < height {
                height = block.get_height();
            }
            done += 1;
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!("removing versioned data above topoheight {}", topoheight);
        }
        // Batch deletion disabled - per-topoheight deletion ensures data consistency
        // self.delete_versioned_data_above_topoheight(topoheight).await?;

        if log::log_enabled!(log::Level::Warn) {
            warn!(
                "Blocks rewinded: {}, new topoheight: {}, new height: {}",
                done, topoheight, height
            );
        }

        trace!("Cleaning caches");
        // Clear all caches to not have old data after rewind
        self.clear_objects_cache().await?;

        trace!("Storing new pointers");
        // store the new tips and topo topoheight
        self.store_tips(&tips).await?;
        self.set_top_topoheight(topoheight).await?;
        self.set_top_height(height).await?;

        // Reduce the count of blocks stored
        self.decrease_blocks_count(done).await?;

        Ok((height, topoheight, txs))
    }

    // Get the size of the chain on disk in bytes
    async fn get_size_on_disk(&self) -> Result<u64, BlockchainError>;

    // Estimate the size of the DB in bytes
    async fn estimate_size(&self) -> Result<u64, BlockchainError>;

    // Stop the storage and wait for it to finish
    async fn stop(&mut self) -> Result<(), BlockchainError>;

    // Flush the inner DB after a block being written
    async fn flush(&mut self) -> Result<(), BlockchainError>;
}
