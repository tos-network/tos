mod bootstrap;
mod chain_validator;
mod sync_validator;

use futures::{stream, StreamExt};
use indexmap::IndexSet;
use log::{debug, error, info, trace, warn};
use std::{
    borrow::Cow,
    sync::Arc,
    time::{Duration, Instant},
};
use tos_common::{
    block::Block,
    crypto::{Hash, Hashable},
    immutable::Immutable,
    time::{get_current_time_in_millis, TimestampMillis},
    tokio::{select, time::interval, Executor, Scheduler},
    transaction::Transaction,
};

use crate::{
    config::{CHAIN_SYNC_TOP_BLOCKS, PEER_OBJECTS_CONCURRENCY, STABLE_LIMIT},
    core::{blockchain::BroadcastOption, error::BlockchainError, storage::Storage},
    p2p::{
        error::P2pError,
        packet::{ChainRequest, ObjectRequest, Packet, PacketWrapper},
    },
};

use super::{
    packet::{BlockId, ChainResponse},
    P2pServer, Peer,
};

pub use chain_validator::*;

// P0-3: SyncBlockValidator exports for chain synchronization security
// NOTE: Core mergeset validation (4*k+16) is implemented in blockchain.rs:add_new_block
// which protects ALL block additions. This module provides additional utilities:
// - SyncBlockValidator: For tracking blue_work monotonicity during sync (optional)
// - process_deferred_blocks: For handling blocks with missing parents (if needed)
// These are available for integration if enhanced sync monitoring is desired.
#[allow(unused_imports)]
pub use sync_validator::{
    process_deferred_blocks, SyncBlockValidator, SyncValidationResult, SyncValidatorConfig,
};

enum ResponseHelper {
    Requested(Block, Immutable<Hash>),
    NotRequested(Immutable<Hash>),
}

/// P0-3: Result of attempting to add a sync block
/// Used to handle blocks that need to be deferred due to missing parents
enum SyncBlockResult {
    /// Block was added successfully
    Added,
    /// Block was already in chain
    AlreadyInChain,
    /// Block needs to be deferred (missing parents)
    Deferred(Block, Immutable<Hash>),
}

impl<S: Storage> P2pServer<S> {
    // this function basically send all our blocks based on topological order (topoheight)
    // we send up to CHAIN_SYNC_REQUEST_MAX_BLOCKS blocks id (combinaison of block hash and topoheight)
    // we add at the end the genesis block to be sure to be on the same chain as others peers
    // its used to find a common point with the peer to which we ask the chain
    // SECURITY FIX: Removed skip_stable_height_check parameter
    // Previously, sync errors could grant the next peer elevated privileges to bypass
    // stable height checks. This was a vulnerability allowing colluding peers to
    // cause deep rewinds. Now only priority peers can bypass stable height checks.
    pub async fn request_sync_chain_for(
        &self,
        peer: &Arc<Peer>,
        last_chain_sync: &mut TimestampMillis,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Requesting chain from {}", peer);
        }

        // This can be configured by the node operator, it will be adjusted between protocol bounds
        // and based on peer configuration
        // This will allow to boost-up syncing for those who want and can be used to use low resources for low devices
        let requested_max_size = self.max_chain_response_size;

        let packet = {
            debug!("locking storage for sync chain request");
            let storage = self.blockchain.get_storage().read().await;
            debug!("locked storage for sync chain request");
            let request = ChainRequest::new(
                self.build_list_of_blocks_id(&*storage).await?,
                requested_max_size as u16,
            );
            if log::log_enabled!(log::Level::Trace) {
                trace!("Built a chain request with {} blocks", request.size());
            }
            let ping = self
                .build_generic_ping_packet_with_storage(&*storage)
                .await?;
            PacketWrapper::new(Cow::Owned(request), Cow::Owned(ping))
        };

        // Update last chain sync time
        // This will be overwritten in case
        // we got the chain response
        // This prevent us from requesting too fast the chain from peer
        *last_chain_sync = get_current_time_in_millis();

        let response = peer.request_sync_chain(packet).await?;
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Received a chain response of {} blocks",
                response.blocks_size()
            );
        }

        // Check that the peer followed our requirements
        if response.blocks_size() > requested_max_size {
            return Err(P2pError::InvalidChainResponseSize(
                response.blocks_size(),
                requested_max_size,
            )
            .into());
        }

        // Update last chain sync time
        *last_chain_sync = get_current_time_in_millis();

        self.handle_chain_response(peer, response, requested_max_size)
            .await
    }

    // search a common point between our blockchain and the peer's one
    // when the common point is found, start sending blocks from this point
    pub async fn handle_chain_request(
        self: &Arc<Self>,
        peer: &Arc<Peer>,
        blocks: IndexSet<BlockId>,
        accepted_response_size: usize,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "handle chain request for {} with {} blocks",
                peer,
                blocks.len()
            );
        }
        let storage = self.blockchain.get_storage().read().await;
        debug!("storage locked for chain request");
        // blocks hashes sent for syncing (topoheight ordered)
        let mut response_blocks = IndexSet::new();
        let mut top_blocks = IndexSet::new();
        // common point used to notify peer if he should rewind or not
        let common_point = self.find_common_point(&*storage, blocks).await?;
        // Lowest height of the blocks sent
        let mut lowest_common_height = None;

        if let Some(common_point) = &common_point {
            let mut topoheight = common_point.get_topoheight();
            // lets add all blocks ordered hash
            let top_topoheight = self.blockchain.get_topo_height();
            // used to detect if we find unstable height for alt tips
            let mut unstable_height = None;
            let top_height = self.blockchain.get_blue_score();
            // check to see if we should search for alt tips (and above unstable height)
            let should_search_alt_tips =
                top_topoheight - topoheight < accepted_response_size as u64;
            if should_search_alt_tips {
                debug!("Peer is near to be synced, will send him alt tips blocks");
                unstable_height = Some(self.blockchain.get_stable_blue_score() + 1);
            }

            // Search the lowest height
            let mut lowest_height = top_height;

            // complete ChainResponse blocks until we are full or that we reach the top topheight
            while response_blocks.len() < accepted_response_size && topoheight <= top_topoheight {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("looking for hash at topoheight {}", topoheight);
                }
                let hash = storage.get_hash_at_topo_height(topoheight).await?;

                // Find the lowest height
                let height = storage.get_blue_score_for_block_hash(&hash).await?;
                if height < lowest_height {
                    lowest_height = height;
                }

                // VERSION UNIFICATION: V0 ordering issues no longer apply with Baseline
                // All blocks use proper ordering from genesis, no swap logic needed
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "for chain request, adding hash {} at topoheight {}",
                        hash,
                        topoheight
                    );
                }
                response_blocks.insert(hash);
                topoheight += 1;
            }
            lowest_common_height = Some(lowest_height);

            // now, lets check if peer is near to be synced, and send him alt tips blocks
            if let Some(mut height) = unstable_height {
                let top_height = self.blockchain.get_blue_score();
                if log::log_enabled!(log::Level::Trace) {
                    trace!("unstable height: {}, top height: {}", height, top_height);
                }
                while height <= top_height && top_blocks.len() < CHAIN_SYNC_TOP_BLOCKS {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!("get blocks at height {} for top blocks", height);
                    }
                    for hash in storage.get_blocks_at_blue_score(height).await? {
                        if !response_blocks.contains(&hash) {
                            if log::log_enabled!(log::Level::Trace) {
                                trace!("Adding top block at height {}: {}", height, hash);
                            }
                            top_blocks.insert(hash);
                        } else {
                            if log::log_enabled!(log::Level::Trace) {
                                trace!("Top block at height {}: {} was skipped because its already present in response blocks", height, hash);
                            }
                        }
                    }
                    height += 1;
                }
            }
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Sending {} blocks & {} top blocks as response to {}",
                response_blocks.len(),
                top_blocks.len(),
                peer
            );
        }
        peer.send_packet(Packet::ChainResponse(ChainResponse::new(
            common_point,
            lowest_common_height,
            response_blocks,
            top_blocks,
        )))
        .await?;
        Ok(())
    }

    // Handle the blocks from chain validator by requesting missing TXs from each header
    // We don't request the full block itself as we already have the block header
    // This may be faster, but we would use slightly more bandwidth
    // NOTE: ChainValidator must check the block hash and not trust it
    // as we are giving it the chain directly to prevent a re-compute
    async fn handle_blocks_from_chain_validator(
        &self,
        peer: &Arc<Peer>,
        mut chain_validator: ChainValidator<'_, S>,
        blocks: IndexSet<Hash>,
    ) -> Result<(), BlockchainError> {
        // now retrieve all txs from all blocks header and add block in chain

        let capacity = if self.allow_boost_sync() {
            debug!("Requesting needed blocks in boost sync mode");
            Some(PEER_OBJECTS_CONCURRENCY)
        } else {
            Some(1)
        };

        let mut scheduler = Scheduler::new(capacity);
        for hash in blocks {
            let hash = Immutable::Arc(Arc::new(hash));
            if log::log_enabled!(log::Level::Trace) {
                trace!("Processing block {} from chain validator", hash);
            }
            let _header = chain_validator.get_block(&hash);

            let future = async move {
                // we don't already have this block, lets retrieve its txs and add in our chain
                if !self.blockchain.has_block(&hash).await? {
                    // Since BlockHeader no longer contains transaction hashes, we need to fetch the complete block
                    let block = peer
                        .request_blocking_object(ObjectRequest::Block(hash.clone()))
                        .await?
                        .into_block()?
                        .0;

                    Ok::<_, BlockchainError>(ResponseHelper::Requested(block, hash))
                } else {
                    Ok(ResponseHelper::NotRequested(hash))
                }
            };

            scheduler.push_back(future);
        }

        let mut blocks_executor = Executor::new();
        loop {
            select! {
                biased;
                Some(res) = blocks_executor.next() => {
                    res?;
                    // Increase by one the limit again
                    // allow to request one new block
                    scheduler.increment_n();
                },
                Some(res) = scheduler.next() => {
                    let future = async move {
                        match res? {
                            ResponseHelper::Requested(block, hash) => {
                                // SECURITY FIX: Verify block hash matches before any processing
                                // This prevents chain poisoning attacks from malicious peers
                                // NOTE: Using warn! here to avoid log spam from malicious peers;
                                // core layer (add_new_block) retains error! for canonical logging
                                // TODO: Add metrics counter p2p_block_hash_mismatch_total
                                let computed_hash = block.hash();
                                if computed_hash != *hash {
                                    if log::log_enabled!(log::Level::Warn) {
                                        warn!(
                                            "Block hash mismatch from peer! Expected: {}, Computed: {}. Rejecting block.",
                                            hash, computed_hash
                                        );
                                    }
                                    return Err(P2pError::BlockHashMismatch {
                                        expected: (*hash).clone(),
                                        actual: computed_hash,
                                    }.into());
                                }
                                self.blockchain.add_new_block(block, Some(hash), BroadcastOption::Miners, false).await
                            },
                            ResponseHelper::NotRequested(hash) => self.try_re_execution_block(hash).await,
                        }
                    };

                    // Decrease by one the limit
                    // This create a backpressure to reduce
                    // requesting too many blocks and keeping them
                    // in memory
                    scheduler.decrement_n();
                    blocks_executor.push_back(future);
                },
                else => {
                    break;
                }
            }
        }

        Ok(())
    }

    // Handle the chain validator by rewinding our current chain first
    // This should only be called with a commit point enabled
    async fn handle_chain_validator_with_rewind(
        &self,
        peer: &Arc<Peer>,
        pop_count: u64,
        chain_validator: ChainValidator<'_, S>,
        blocks: IndexSet<Hash>,
    ) -> Result<
        (
            Vec<(Hash, Immutable<Transaction>)>,
            Result<(), BlockchainError>,
        ),
        BlockchainError,
    > {
        // peer chain looks correct, lets rewind our chain
        if log::log_enabled!(log::Level::Warn) {
            warn!(
                "Rewinding chain because of {} (pop count: {})",
                peer, pop_count
            );
        }
        let (topoheight, txs) = self.blockchain.rewind_chain(pop_count, false).await?;
        if log::log_enabled!(log::Level::Debug) {
            debug!("Rewinded chain until topoheight {}", topoheight);
        }
        let res = self
            .handle_blocks_from_chain_validator(peer, chain_validator, blocks)
            .await;

        Ok((txs, res))
    }

    // Handle a chain response from another peer
    // We receive a list of blocks hashes ordered by their topoheight
    // It also contains a CommonPoint which is a block hash point where we have the same topoheight as our peer
    // Based on the lowest height of the chain sent, we may need to rewind some blocks
    // NOTE: Only a priority node can rewind below the stable height
    // SECURITY FIX: Removed skip_stable_height_check parameter to prevent colluding peer attacks
    async fn handle_chain_response(
        &self,
        peer: &Arc<Peer>,
        mut response: ChainResponse,
        requested_max_size: usize,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("handle chain response from {}", peer);
        }
        let response_size = response.blocks_size();

        let (Some(common_point), Some(lowest_height)) =
            (response.get_common_point(), response.get_lowest_height())
        else {
            if log::log_enabled!(log::Level::Warn) {
                warn!("No common block was found with {}", peer);
            }
            if response.blocks_size() > 0 {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "Peer have no common block but send us {} blocks!",
                        response.blocks_size()
                    );
                }
                return Err(P2pError::InvalidPacket.into());
            }
            return Ok(());
        };

        let common_topoheight = common_point.get_topoheight();
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "{} found a common point with block {} at topo {} for sync, received {} blocks",
                peer.get_outgoing_address(),
                common_point.get_hash(),
                common_topoheight,
                response_size
            );
        }
        let pop_count = {
            let storage = self.blockchain.get_storage().read().await;
            let expected_common_topoheight = storage
                .get_topo_height_for_hash(common_point.get_hash())
                .await?;
            if expected_common_topoheight != common_topoheight {
                if log::log_enabled!(log::Level::Error) {
                    error!("{} sent us a valid block hash, but at invalid topoheight (expected: {}, got: {})!", peer, expected_common_topoheight, common_topoheight);
                }
                return Err(P2pError::InvalidCommonPoint(common_topoheight).into());
            }

            let block_height = storage
                .get_blue_score_for_block_hash(common_point.get_hash())
                .await?;
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "block height: {}, stable height: {}, topoheight: {}, hash: {}",
                    block_height,
                    self.blockchain.get_stable_blue_score(),
                    expected_common_topoheight,
                    common_point.get_hash()
                );
            }
            // We are under the stable height, rewind is necessary
            // SECURITY FIX: Only priority peers can bypass stable height check
            // Previously skip_stable_height_check could be set based on previous sync errors,
            // allowing colluding peers to cause deep rewinds
            let mut count =
                if peer.is_priority() || lowest_height <= self.blockchain.get_stable_blue_score() {
                    let our_topoheight = self.blockchain.get_topo_height();
                    if our_topoheight > expected_common_topoheight {
                        our_topoheight - expected_common_topoheight
                    } else {
                        expected_common_topoheight - our_topoheight
                    }
                } else {
                    0
                };

            if let Some(pruned_topo) = storage.get_pruned_topoheight().await? {
                let available_diff = self.blockchain.get_topo_height() - pruned_topo;
                if count > available_diff && !(available_diff == 0 && peer.is_priority()) {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!(
                            "Peer sent us a pop count of {} but we only have {} blocks available",
                            count, available_diff
                        );
                    }
                    count = available_diff;
                }
            }

            count
        };

        // Packet verification ended, handle the chain response now

        let (mut blocks, top_blocks) = response.consume();
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "handling chain response from {}, {} blocks, {} top blocks, pop count {}",
                peer,
                blocks.len(),
                top_blocks.len(),
                pop_count
            );
        }

        let our_previous_topoheight = self.blockchain.get_topo_height();
        let our_previous_height = self.blockchain.get_blue_score();
        let top_len = top_blocks.len();
        let blocks_len = blocks.len();

        // merge both list together
        blocks.extend(top_blocks);

        if pop_count > 0 {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "{} sent us a pop count request of {} with {} blocks (common point: {} at {})",
                    peer,
                    pop_count,
                    blocks_len,
                    common_point.get_hash(),
                    common_topoheight
                );
            }
        }

        // if node asks us to pop blocks, check that the peer's height/topoheight is in advance on us
        let peer_topoheight = peer.get_topoheight();
        let our_stable_topoheight = self.blockchain.get_stable_topoheight();

        // SECURITY FIX: Removed skip_stable_height_check from the condition
        // Only priority peers or when common_topoheight < our_stable_topoheight can trigger rewind
        if pop_count > 0
            && peer_topoheight > our_previous_topoheight
            && peer.get_height() >= our_previous_height
            && common_topoheight < our_stable_topoheight
            // then, verify if it's a priority node, otherwise, check if we are connected to a priority node so only him can rewind us
            && (peer.is_priority() || !self.is_connected_to_a_synced_priority_node().await)
        {
            // check that if we can trust him
            if peer.is_priority() {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Rewinding chain without checking because {} is a priority node (pop count: {})", peer, pop_count);
                }
                // User trust him as a priority node, rewind chain without checking, allow to go below stable height also
                self.blockchain.rewind_chain(pop_count, false).await?;
            } else {
                // Verify that someone isn't trying to trick us
                if pop_count > blocks_len as u64 {
                    // TODO: maybe we could request its whole chain for comparison until chain validator has_higher_blue_work (GHOSTDAG) ?
                    // If after going through all its chain and we still have higher blue_work, we should not rewind
                    if log::log_enabled!(log::Level::Warn) {
                        warn!(
                            "{} sent us a pop count of {} but only sent us {} blocks, ignoring",
                            peer, pop_count, blocks_len
                        );
                    }
                    return Err(P2pError::InvalidPopCount(pop_count, blocks_len as u64).into());
                }

                let capacity = if self.allow_boost_sync() {
                    debug!("Requesting needed blocks in boost sync mode");
                    Some(PEER_OBJECTS_CONCURRENCY)
                } else {
                    Some(1)
                };

                // request all blocks header and verify basic chain structure
                // Starting topoheight must be the next topoheight after common block
                // Blocks in chain response must be ordered by topoheight otherwise it will give incorrect results
                let mut futures = Scheduler::new(capacity);
                for hash in blocks.iter().cloned() {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!("Request block header for chain validator: {}", hash);
                    }

                    let fut = async {
                        // check if we already have the block to not request it
                        if self.blockchain.has_block(&hash).await? {
                            if log::log_enabled!(log::Level::Trace) {
                                trace!("We already have block {}, skipping", hash);
                            }
                            return Ok(None);
                        }

                        peer.request_blocking_object(ObjectRequest::BlockHeader(Immutable::Owned(
                            hash,
                        )))
                        .await?
                        .into_block_header()
                        .map(Some)
                    };

                    futures.push_back(fut);
                }

                let mut expected_topoheight = common_topoheight + 1;
                let mut chain_validator = ChainValidator::new(&self.blockchain);
                let mut exit_signal = self.exit_sender.subscribe();
                'main: loop {
                    select! {
                        _ = exit_signal.recv() => {
                            debug!("Stopping chain validator due to exit signal");
                            break 'main;
                        },
                        next = futures.next() => {
                            let Some(res) = next else {
                                debug!("No more items in futures for chain validator");
                                break 'main;
                            };

                            if let Some((block, hash)) = res? {
                                chain_validator.insert_block(hash, block, expected_topoheight).await?;
                                expected_topoheight += 1;
                            }
                        }
                    };
                }

                // GHOSTDAG: Verify that peer's chain has higher blue_work than ours
                // blue_work is the correct metric for DAG consensus
                if !chain_validator.has_higher_blue_work().await? {
                    if log::log_enabled!(log::Level::Error) {
                        error!("{} sent us a chain response with lower blue_work than ours (GHOSTDAG consensus)", peer);
                    }
                    return Err(BlockchainError::LowerCumulativeDifficulty); // Error name kept for compatibility
                }

                // Handle the chain validator
                {
                    info!("Starting commit point for chain validator");
                    let mut storage = self.blockchain.get_storage().write().await;
                    storage.start_commit_point().await?;
                    info!("Commit point started for chain validator");
                }
                let mut res = self
                    .handle_chain_validator_with_rewind(peer, pop_count, chain_validator, blocks)
                    .await;
                {
                    info!("Ending commit point for chain validator");
                    let apply = res.as_ref().map_or(false, |(_, v)| v.is_ok());

                    {
                        debug!("locking storage write mode for commit point");
                        let mut storage = self.blockchain.get_storage().write().await;
                        debug!("locked storage write mode for commit point");

                        storage.end_commit_point(apply).await?;
                        if log::log_enabled!(log::Level::Info) {
                            info!("Commit point ended for chain validator, apply: {}", apply);
                        }
                    }

                    if !apply {
                        debug!(
                            "Reloading chain caches from disk due to invalidation of commit point"
                        );
                        self.blockchain.reload_from_disk().await?;

                        // Try to apply any orphaned TX back to our chain
                        // We want to prevent any loss
                        if let Ok((ref mut txs, _)) = res.as_mut() {
                            if log::log_enabled!(log::Level::Debug) {
                                debug!("Applying back orphaned {} TXs", txs.len());
                            }
                            for (hash, tx) in txs.drain(..) {
                                if log::log_enabled!(log::Level::Debug) {
                                    debug!("Trying to apply orphaned TX {}", hash);
                                }
                                if !self.blockchain.is_tx_included(&hash).await? {
                                    if log::log_enabled!(log::Level::Debug) {
                                        debug!("TX {} is not in chain, adding it to mempool", hash);
                                    }
                                    if let Err(e) = self
                                        .blockchain
                                        .add_tx_to_mempool_with_hash(
                                            tx.into_arc(),
                                            Immutable::Owned(hash),
                                            false,
                                        )
                                        .await
                                    {
                                        if log::log_enabled!(log::Level::Debug) {
                                            debug!("Couldn't add back to mempool after commit point rollbacked: {}", e);
                                        }
                                    }
                                } else {
                                    if log::log_enabled!(log::Level::Debug) {
                                        debug!("TX {} is already in chain, skipping", hash);
                                    }
                                }
                            }
                        }
                    }

                    // Return errors if any
                    res?.1?;
                }
            }
        } else {
            // no rewind are needed, process normally
            // it will first add blocks to sync, and then all alt-tips blocks if any (top blocks)
            let mut total_requested = 0;
            let start = Instant::now();

            let capacity = if self.allow_boost_sync() {
                debug!("Requesting needed blocks in boost sync mode");
                Some(PEER_OBJECTS_CONCURRENCY)
            } else {
                Some(1)
            };

            let mut futures = Scheduler::new(capacity);
            let group_id = self.object_tracker.next_group_id();

            // P0-3: Deferred blocks waiting for parents (fork prevention)
            // When a block fails with ParentNotFound, we defer it and retry later
            let mut deferred_blocks: Vec<(Block, Immutable<Hash>)> = Vec::new();

            for hash in blocks {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("processing block request {}", hash);
                }
                let fut = async {
                    let hash = Immutable::Arc(Arc::new(hash));
                    if !self.blockchain.has_block(&hash).await? {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Requesting boost sync block {}", hash);
                        }
                        peer.request_blocking_object(ObjectRequest::Block(hash.clone()))
                            .await?
                            .into_block()
                            .map(|(block, _)| ResponseHelper::Requested(block, hash))
                    } else {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Block {} is already in chain or being processed, verify if its in DAG", hash);
                        }
                        Ok(ResponseHelper::NotRequested(hash))
                    }
                };

                futures.push_back(fut);
            }

            // In case we must shutdown
            let mut exit_signal = self.exit_sender.subscribe();
            // Timer to update the display of our BPS (blocks per second)
            let mut internal_bps = interval(Duration::from_secs(1));
            // All blocks processed during our syncing
            let mut blocks_processed = 0;
            // Blocks executor for sequential processing
            let mut blocks_executor = Executor::new();

            'main: loop {
                select! {
                    biased;
                    _ = exit_signal.recv() => {
                        debug!("Stopping chain sync due to exit signal");
                        break 'main;
                    },
                    _ = internal_bps.tick() => {
                        self.set_chain_sync_rate_bps(blocks_processed);
                        blocks_processed = 0;
                    },
                    Some(res) = blocks_executor.next() => {
                        // P0-3: Handle sync block result including deferred blocks
                        match res? {
                            SyncBlockResult::Added => {
                                total_requested += 1;
                            }
                            SyncBlockResult::AlreadyInChain => {
                                // Block was already in chain, no action needed
                            }
                            SyncBlockResult::Deferred(block, hash) => {
                                // Block needs to wait for parents, add to deferred queue
                                deferred_blocks.push((block, hash));
                            }
                        }

                        futures.increment_n();
                        blocks_processed += 1;
                    },
                    // Even with the biased select & the option future being above
                    // we must ensure we don't miss a block
                    Some(res) = futures.next() => {
                        let future = async {
                            match res {
                                Ok(response) => match response {
                                    ResponseHelper::Requested(block, hash) => {
                                        // SECURITY FIX: Verify block hash matches before any processing
                                        // This prevents chain poisoning attacks from malicious peers
                                        // NOTE: Using warn! here to avoid log spam from malicious peers;
                                        // core layer (add_new_block) retains error! for canonical logging
                                        // TODO: Add metrics counter p2p_block_hash_mismatch_total
                                        let computed_hash = block.hash();
                                        if computed_hash != *hash {
                                            if log::log_enabled!(log::Level::Warn) {
                                                warn!(
                                                    "Block hash mismatch from peer! Expected: {}, Computed: {}. Rejecting block.",
                                                    hash, computed_hash
                                                );
                                            }
                                            self.object_tracker.mark_group_as_fail(group_id).await;
                                            return Err(P2pError::BlockHashMismatch {
                                                expected: (*hash).clone(),
                                                actual: computed_hash,
                                            }.into());
                                        }

                                        // Lets ensure that the block is not already in chain
                                        // This may happen if we try to chain sync with peer
                                        // while we got the block through propagation
                                        if !self.blockchain.has_block(&hash).await? {
                                            match self.blockchain.add_new_block(block.clone(), Some(hash.clone()), BroadcastOption::Miners, false).await {
                                                Ok(()) => Ok(SyncBlockResult::Added),
                                                Err(BlockchainError::ParentNotFound(parent_hash)) => {
                                                    // P0-3: Defer block if parent not found
                                                    if log::log_enabled!(log::Level::Debug) {
                                                        debug!(
                                                            "Block {} deferred: parent {} not found, will retry later",
                                                            hash, parent_hash
                                                        );
                                                    }
                                                    Ok(SyncBlockResult::Deferred(block, hash))
                                                }
                                                Err(e) => {
                                                    self.object_tracker.mark_group_as_fail(group_id).await;
                                                    Err(e)
                                                }
                                            }
                                        } else {
                                            if log::log_enabled!(log::Level::Debug) {
                                                debug!("Block {} is already in chain despite requesting it, skipping it..", hash);
                                            }
                                            Ok(SyncBlockResult::AlreadyInChain)
                                        }
                                    },
                                    ResponseHelper::NotRequested(hash) => {
                                        if let Err(e) = self.try_re_execution_block(hash).await {
                                            self.object_tracker.mark_group_as_fail(group_id).await;
                                            return Err(e)
                                        }

                                        Ok(SyncBlockResult::AlreadyInChain)
                                    }
                                },
                                Err(e) => {
                                    if log::log_enabled!(log::Level::Debug) {
                                        debug!("Unregistering group id {} due to error {}", group_id, e);
                                    }
                                    self.object_tracker.mark_group_as_fail(group_id).await;
                                    Err(e.into())
                                }
                            }
                        };

                        futures.decrement_n();
                        blocks_executor.push_back(future);
                    },
                    else => {
                        break 'main;
                    }
                };

                if blocks_executor.is_empty() && futures.is_empty() {
                    break;
                }
            }

            // P0-3: Process deferred blocks with per-block timeout (CONCURRENT)
            // Each block gets its own independent 30s timeout window running in parallel
            if !deferred_blocks.is_empty() {
                use futures::stream::FuturesUnordered;

                if log::log_enabled!(log::Level::Info) {
                    info!(
                        "Processing {} deferred blocks concurrently (waiting for parents)...",
                        deferred_blocks.len()
                    );
                }

                let parent_timeout = Duration::from_secs(30); // Per-block timeout
                let max_retries: u32 = 3;
                let mut retry_count: u32 = 0;

                while !deferred_blocks.is_empty() && retry_count < max_retries {
                    // Create concurrent futures for all deferred blocks
                    let mut deferred_futures = FuturesUnordered::new();

                    for (block, hash) in deferred_blocks.drain(..) {
                        let blockchain = &self.blockchain;
                        let block_clone = block.clone();
                        let hash_clone = hash.clone();

                        // Each block gets its own concurrent task with independent timeout
                        let fut = async move {
                            let block_start = Instant::now();

                            loop {
                                // Check timeout for THIS block independently
                                if block_start.elapsed() > parent_timeout {
                                    if log::log_enabled!(log::Level::Debug) {
                                        debug!(
                                            "P0-3: Block {} timeout after {}s, will retry",
                                            hash_clone,
                                            parent_timeout.as_secs()
                                        );
                                    }
                                    // Return block for retry
                                    return Ok::<_, BlockchainError>(Some((
                                        block_clone,
                                        hash_clone,
                                    )));
                                }

                                // Try to add the block
                                match blockchain
                                    .add_new_block(
                                        block_clone.clone(),
                                        Some(hash_clone.clone()),
                                        BroadcastOption::Miners,
                                        false,
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        if log::log_enabled!(log::Level::Debug) {
                                            debug!(
                                                "Deferred block {} successfully added",
                                                hash_clone
                                            );
                                        }
                                        return Ok(None); // Success, no retry needed
                                    }
                                    Err(BlockchainError::ParentNotFound(_)) => {
                                        // Still waiting for parent, sleep briefly then retry
                                        tos_common::tokio::time::sleep(Duration::from_millis(100))
                                            .await;
                                    }
                                    Err(BlockchainError::AlreadyInChain) => {
                                        // Block was added by another peer while we were waiting
                                        // This is success, not an error
                                        if log::log_enabled!(log::Level::Debug) {
                                            debug!("Deferred block {} already in chain (added via other path)", hash_clone);
                                        }
                                        return Ok(None); // Success, no retry needed
                                    }
                                    Err(e) => {
                                        // Other error, propagate it
                                        return Err(e);
                                    }
                                }
                            }
                        };

                        deferred_futures.push(fut);
                    }

                    // Process all deferred blocks concurrently
                    let mut still_deferred = Vec::new();
                    let mut added_count = 0u64;

                    while let Some(result) = deferred_futures.next().await {
                        match result {
                            Ok(None) => {
                                // Block was successfully added
                                added_count += 1;
                            }
                            Ok(Some((block, hash))) => {
                                // Block needs retry (timeout or parent still missing)
                                still_deferred.push((block, hash));
                            }
                            Err(e) => {
                                // Fatal error, fail the sync
                                if log::log_enabled!(log::Level::Warn) {
                                    warn!("Deferred block processing failed: {}", e);
                                }
                                self.object_tracker.mark_group_as_fail(group_id).await;
                                return Err(e.into());
                            }
                        }
                    }

                    total_requested += added_count;
                    deferred_blocks = still_deferred;

                    if !deferred_blocks.is_empty() {
                        retry_count += 1;
                        if log::log_enabled!(log::Level::Debug) {
                            debug!(
                                "{} blocks still waiting for parents after round {}/{} ({} added this round)",
                                deferred_blocks.len(), retry_count, max_retries, added_count
                            );
                        }
                    }
                }

                // If blocks still remain after max retries, emit P0-3 error
                if !deferred_blocks.is_empty() {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!(
                            "P0-3: {} blocks failed to sync after {} retries - max retries exceeded",
                            deferred_blocks.len(), max_retries
                        );
                    }
                    return Err(P2pError::SyncMaxRetriesExceeded(deferred_blocks.len()).into());
                }
            }

            let elapsed = start.elapsed().as_secs();
            let bps = if elapsed > 0 {
                total_requested / elapsed
            } else {
                total_requested
            };
            if log::log_enabled!(log::Level::Info) {
                info!(
                    "we've synced {} on {} blocks and {} top blocks in {}s ({} bps) from {}",
                    total_requested, blocks_len, top_len, elapsed, bps, peer
                );
            }

            // If we have synced a block and it was less than the max size
            // It may means we are up to date
            // Notify all peers about our new state
            if total_requested > 0 && blocks_len < requested_max_size {
                self.ping_peers().await;
            }
        }

        // ask inventory of this peer if we sync from too far
        // if we are not further than one sync, request the inventory
        if blocks_len > 0 && blocks_len < requested_max_size {
            let our_topoheight = self.blockchain.get_topo_height();

            stream::iter(self.peer_list.get_cloned_peers().await)
                .for_each_concurrent(None, |peer| async move {
                    let peer_topoheight = peer.get_topoheight();
                    // verify that we synced it partially well
                    if peer_topoheight >= our_topoheight && peer_topoheight - our_topoheight < STABLE_LIMIT {
                        if let Err(e) = self.request_inventory_of(&peer).await {
                            if log::log_enabled!(log::Level::Error) {
                                error!("Error while asking inventory to {}: {}", peer, e);
                            }
                        }
                    } else {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Skipping inventory request for {} because its topoheight {} is not in range of our topoheight {}", peer, peer_topoheight, our_topoheight);
                        }
                    }
                }).await;
        }

        Ok(())
    }

    // Try to re-execute the block requested if its not included in DAG order (it has no topoheight assigned)
    async fn try_re_execution_block(&self, hash: Immutable<Hash>) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("check re execution block {}", hash);
        }

        if self.disable_reexecute_blocks_on_sync {
            trace!("re execute blocks on sync is disabled");
            return Ok(());
        }

        {
            let storage = self.blockchain.get_storage().read().await;
            if storage.is_block_topological_ordered(&hash).await? {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("block {} is already ordered", hash);
                }
                return Ok(());
            }
        }

        if log::log_enabled!(log::Level::Warn) {
            warn!("Forcing block {} re-execution", hash);
        }
        let block = {
            let mut storage = self.blockchain.get_storage().write().await;
            debug!("storage write acquired for block forced re-execution");

            let block = storage.delete_block_with_hash(&hash).await?;
            let mut tips = storage.get_tips().await?;
            if tips.remove(&hash) {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Block {} was a tip, removing it from tips", hash);
                }
                storage.store_tips(&tips).await?;
            }

            block
        };

        // Replicate same behavior as above branch
        self.blockchain
            .add_new_block(block, Some(hash), BroadcastOption::Miners, false)
            .await
    }
}
