use super::{
    super::{error::P2pError, packet::*, Connection},
    SharedPeerList,
};
use crate::{
    config::{
        CHAIN_SYNC_TIMEOUT_SECS, PEER_BLOCK_CACHE_SIZE, PEER_FAIL_TIME_RESET,
        PEER_OBJECTS_CONCURRENCY, PEER_PACKET_CHANNEL_SIZE, PEER_PEERS_CACHE_SIZE,
        PEER_TIMEOUT_BOOTSTRAP_STEP, PEER_TIMEOUT_REQUEST_OBJECT, PEER_TX_CACHE_SIZE,
    },
    core::ghostdag::BlueWorkType,
    p2p::packet::PacketWrapper,
};
use anyhow::Context;
use bytes::Bytes;
use log::{debug, trace, warn};
use lru::LruCache;
use metrics::counter;
use std::{
    borrow::Cow,
    collections::VecDeque,
    fmt::{Display, Error, Formatter},
    hash::{Hash as StdHash, Hasher},
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};
use tos_common::{
    api::daemon::{Direction, TimedDirection},
    block::TopoHeight,
    crypto::Hash,
    serializer::Serializer,
    time::{get_current_time_in_seconds, TimestampSeconds},
    tokio::{
        select,
        sync::{broadcast, mpsc, oneshot, Mutex, Semaphore},
        time::timeout,
    },
};

// Compile-time validation that P2P configuration constants are non-zero
// These assertions ensure that NonZeroUsize::new_unchecked is safe to use
const _: () = assert!(
    PEER_OBJECTS_CONCURRENCY > 0,
    "PEER_OBJECTS_CONCURRENCY must be non-zero"
);
const _: () = assert!(
    PEER_PEERS_CACHE_SIZE > 0,
    "PEER_PEERS_CACHE_SIZE must be non-zero"
);
const _: () = assert!(
    PEER_TX_CACHE_SIZE > 0,
    "PEER_TX_CACHE_SIZE must be non-zero"
);
const _: () = assert!(
    PEER_BLOCK_CACHE_SIZE > 0,
    "PEER_BLOCK_CACHE_SIZE must be non-zero"
);

// A RequestedObjects is a map of all objects requested from a peer
// This is done to be awaitable with a timeout
pub type RequestedObjects = LruCache<ObjectRequest, broadcast::Sender<OwnedObjectResponse>>;

pub type Tx = mpsc::Sender<Bytes>;
pub type Rx = mpsc::Receiver<Bytes>;

// Enum used to track the state of a task
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TaskState {
    // not started yet
    Inactive,
    // running
    Active,
    // task has been cancelled
    Exiting,
    // Task has exited
    Finished,
    Unknown,
}

// A Peer represents a connection to another node in the network
// It is used to propagate and receive blocks / transactions and do chain sync
// It contains all the necessary information to manage the connection and the communication
pub struct Peer {
    // Connection of the peer to manage read/write to TCP Stream
    connection: Connection,
    // unique ID of the peer to recognize him
    id: u64,
    // Node tag if provided
    node_tag: Option<String>,
    // port on which the node is listening on its side
    local_port: u16,
    // daemon version
    version: String,
    // if this node can be trusted (seed node or added manually by user)
    priority: bool,
    // current block top hash for this peer
    top_hash: Mutex<Hash>,
    // current highest topo height for this peer
    topoheight: AtomicU64,
    // current highest block height for this peer
    height: AtomicU64,
    // last time we got a chain request
    last_chain_sync: AtomicU64,
    // last time we got a fail
    last_fail_count: AtomicU64,
    // fail count: if greater than 20, we should close this connection
    fail_count: AtomicU8,
    // ========================================================================
    // UNSOLICITED BLOCK RATE LIMITING (P1-1)
    // Tracks blocks received without explicit request to detect flood attacks
    // ========================================================================
    // Count of unsolicited blocks in current 1-second window
    unsolicited_block_count: AtomicU32,
    // Start of current rate window (seconds since epoch)
    unsolicited_block_window_start: AtomicU64,
    // shared pointer to the peer list in case of disconnection
    peer_list: SharedPeerList,
    // map of requested objects from this peer
    objects_requested: Mutex<RequestedObjects>,
    // all peers sent/received
    peers: Mutex<LruCache<SocketAddr, TimedDirection>>,
    // last time we received a peerlist from this peer
    last_peer_list: AtomicU64,
    // last time we got a ping packet from this peer
    last_ping: AtomicU64,
    // last time we sent a ping packet to this peer
    last_ping_sent: AtomicU64,
    // GHOSTDAG blue_work of peer chain (replaces cumulative_difficulty)
    blue_work: Mutex<BlueWorkType>,
    // All transactions propagated from/to this peer
    txs_cache: Mutex<LruCache<Arc<Hash>, (Direction, bool)>>,
    // last blocks propagated to/from this peer
    blocks_propagation: Mutex<LruCache<Arc<Hash>, (TimedDirection, bool)>>,
    // last time we got an inventory packet from this peer
    last_inventory: AtomicU64,
    // if we requested this peer to send us an inventory notification
    requested_inventory: AtomicBool,
    // pruned topoheight if its a pruned node
    pruned_topoheight: AtomicU64,
    // Store the pruned state of the peer
    // cannot be set to false if its already to true (protocol rules)
    is_pruned: AtomicBool,
    // used for await on bootstrap chain packets
    // Because we are in a TCP stream, we know that all our
    // requests will be answered in the order we sent them
    // So we can use a queue to store the senders and pop them
    bootstrap_requests: Mutex<VecDeque<oneshot::Sender<StepResponse>>>,
    // used to wait on chain response when syncing chain
    sync_chain: Mutex<Option<oneshot::Sender<ChainResponse>>>,
    // IP address with local port
    outgoing_address: SocketAddr,
    // Determine if this peer allows to be shared to others and/or through API
    sharable: bool,
    // Channel to send bytes to the writer task
    tx: Tx,
    // Channel to notify the tasks to exit
    exit_channel: broadcast::Sender<()>,
    // Tracking dedicated tasks
    read_task: Mutex<TaskState>,
    write_task: Mutex<TaskState>,
    // Semaphore to prevent requesting too many
    // objects at once
    objects_semaphore: Semaphore,
    // Should we broadcast transactions to this peer
    // Due to needed order of TXs to be accepted
    // We must wait that the peer received our inventory
    propagate_txs: AtomicBool,
}

impl Peer {
    /// Create a new Peer instance with GHOSTDAG blue_work for chain comparison
    pub fn new(
        connection: Connection,
        id: u64,
        node_tag: Option<String>,
        local_port: u16,
        version: String,
        top_hash: Hash,
        topoheight: TopoHeight,
        height: u64,
        pruned_topoheight: Option<TopoHeight>,
        priority: bool,
        blue_work: BlueWorkType,
        peer_list: SharedPeerList,
        sharable: bool,
        propagate_txs: bool,
    ) -> (Self, Rx) {
        let mut outgoing_address = *connection.get_address();
        outgoing_address.set_port(local_port);

        let (exit_channel, _) = broadcast::channel(1);
        let (tx, rx) = mpsc::channel(PEER_PACKET_CHANNEL_SIZE);

        (
            Self {
                connection,
                id,
                node_tag,
                local_port,
                version,
                top_hash: Mutex::new(top_hash),
                topoheight: AtomicU64::new(topoheight),
                height: AtomicU64::new(height),
                priority,
                last_fail_count: AtomicU64::new(0),
                fail_count: AtomicU8::new(0),
                // P1-1: Initialize unsolicited block rate tracking
                unsolicited_block_count: AtomicU32::new(0),
                unsolicited_block_window_start: AtomicU64::new(0),
                last_chain_sync: AtomicU64::new(0),
                peer_list,
                objects_requested: Mutex::new(LruCache::new(
                    // SAFETY: Compile-time assertion above guarantees PEER_OBJECTS_CONCURRENCY > 0
                    unsafe { NonZeroUsize::new_unchecked(PEER_OBJECTS_CONCURRENCY) },
                )),
                peers: Mutex::new(LruCache::new(
                    // SAFETY: Compile-time assertion above guarantees PEER_PEERS_CACHE_SIZE > 0
                    unsafe { NonZeroUsize::new_unchecked(PEER_PEERS_CACHE_SIZE) },
                )),
                last_peer_list: AtomicU64::new(0),
                last_ping: AtomicU64::new(0),
                last_ping_sent: AtomicU64::new(0),
                blue_work: Mutex::new(blue_work),
                txs_cache: Mutex::new(LruCache::new(
                    // SAFETY: Compile-time assertion above guarantees PEER_TX_CACHE_SIZE > 0
                    unsafe { NonZeroUsize::new_unchecked(PEER_TX_CACHE_SIZE) },
                )),
                blocks_propagation: Mutex::new(LruCache::new(
                    // SAFETY: Compile-time assertion above guarantees PEER_BLOCK_CACHE_SIZE > 0
                    unsafe { NonZeroUsize::new_unchecked(PEER_BLOCK_CACHE_SIZE) },
                )),
                last_inventory: AtomicU64::new(0),
                requested_inventory: AtomicBool::new(false),
                pruned_topoheight: AtomicU64::new(pruned_topoheight.unwrap_or(0)),
                is_pruned: AtomicBool::new(pruned_topoheight.is_some()),
                bootstrap_requests: Mutex::new(VecDeque::new()),
                sync_chain: Mutex::new(None),
                outgoing_address,
                sharable,
                exit_channel,
                tx,
                read_task: Mutex::new(TaskState::Inactive),
                write_task: Mutex::new(TaskState::Inactive),
                objects_semaphore: Semaphore::new(PEER_OBJECTS_CONCURRENCY),
                propagate_txs: AtomicBool::new(propagate_txs),
            },
            rx,
        )
    }

    // This is used to mark that peer is ready to get our propagated transactions
    pub fn set_ready_to_propagate_txs(&self, value: bool) {
        self.propagate_txs.store(value, Ordering::SeqCst);
    }

    // Is this peer ready to receive our propagated transactions
    pub fn is_ready_for_txs_propagation(&self) -> bool {
        self.propagate_txs.load(Ordering::SeqCst)
    }

    // Subscribe to the exit channel to be notified when peer disconnects
    pub fn get_exit_receiver(&self) -> broadcast::Receiver<()> {
        self.exit_channel.subscribe()
    }

    // Get the IP address of the peer
    pub fn get_ip(&self) -> IpAddr {
        self.connection.get_address().ip()
    }

    // Get all transactions propagated from/to this peer
    pub fn get_txs_cache(&self) -> &Mutex<LruCache<Arc<Hash>, (Direction, bool)>> {
        &self.txs_cache
    }

    // Get all blocks propagated from/to this peer
    pub fn get_blocks_propagation(&self) -> &Mutex<LruCache<Arc<Hash>, (TimedDirection, bool)>> {
        &self.blocks_propagation
    }

    // Get its connection object to manage p2p communication
    pub fn get_connection(&self) -> &Connection {
        &self.connection
    }

    // Get the unique ID of the peer
    pub fn get_id(&self) -> u64 {
        self.id
    }

    // Get the node tag of the peer
    pub fn get_node_tag(&self) -> &Option<String> {
        &self.node_tag
    }

    // Get the local port of the peer
    pub fn get_local_port(&self) -> u16 {
        self.local_port
    }

    // Get the running version reported during handshake
    pub fn get_version(&self) -> &String {
        &self.version
    }

    // Get the topoheight of the peer
    pub fn get_topoheight(&self) -> TopoHeight {
        self.topoheight.load(Ordering::SeqCst)
    }

    // Set the topoheight of the peer
    pub fn set_topoheight(&self, topoheight: TopoHeight) {
        self.topoheight.store(topoheight, Ordering::SeqCst);
    }

    // Get the height of the peer
    pub fn get_height(&self) -> u64 {
        self.height.load(Ordering::SeqCst)
    }

    // Set the height of the peer
    pub fn set_height(&self, height: u64) {
        self.height.store(height, Ordering::SeqCst);
    }

    // Is the peer running a pruned chain
    pub fn is_pruned(&self) -> bool {
        self.is_pruned.load(Ordering::SeqCst)
    }

    // Get the pruned topoheight
    pub fn get_pruned_topoheight(&self) -> Option<TopoHeight> {
        if self.is_pruned() {
            Some(self.pruned_topoheight.load(Ordering::SeqCst))
        } else {
            None
        }
    }

    // Update the pruned topoheight state
    pub fn set_pruned_topoheight(&self, pruned_topoheight: Option<TopoHeight>) {
        if let Some(pruned_topoheight) = pruned_topoheight {
            self.is_pruned.store(true, Ordering::SeqCst);
            self.pruned_topoheight
                .store(pruned_topoheight, Ordering::SeqCst);
        } else {
            self.is_pruned.store(false, Ordering::SeqCst);
        }
    }

    // Store the top block hash
    pub async fn set_top_block_hash(&self, hash: Hash) {
        *self.top_hash.lock().await = hash
    }

    // Get the top block hash of peer chain
    pub fn get_top_block_hash(&self) -> &Mutex<Hash> {
        &self.top_hash
    }

    /// Get the GHOSTDAG blue_work for chain comparison
    /// blue_work is the cumulative work of all blue blocks in the GHOSTDAG consensus
    pub fn get_blue_work(&self) -> &Mutex<BlueWorkType> {
        &self.blue_work
    }

    /// Store the GHOSTDAG blue_work
    /// This is updated by ping packet and used for chain selection
    pub async fn set_blue_work(&self, blue_work: BlueWorkType) {
        *self.blue_work.lock().await = blue_work;
    }

    // Verify if its a outgoing connection
    pub fn is_out(&self) -> bool {
        self.connection.is_out()
    }

    /// Get the priority flag of the peer.
    ///
    /// # Security Note
    ///
    /// Priority peers are trusted by **local configuration only** (seed nodes or manually
    /// added by the operator). Only priority peers can trigger deep rewinds below the
    /// stable height during chain sync.
    ///
    /// **WARNING**: Do NOT promote arbitrary remote peers to priority based on their
    /// sync behavior or any other dynamic metric. This could be exploited by attackers
    /// to gain elevated privileges and trigger unwanted chain reorganizations.
    ///
    /// The priority flag should only be set:
    /// - For seed nodes defined in the network configuration
    /// - For peers explicitly added by the node operator via CLI/config
    ///
    /// TODO: Consider adding metrics `deep_rewind_triggered_total{reason="priority_peer"}`
    /// to monitor when priority peers trigger deep rewinds.
    pub fn is_priority(&self) -> bool {
        self.priority
    }

    // Get the sharable flag of the peer
    pub fn sharable(&self) -> bool {
        self.sharable
    }

    // Get the last time we got a fail from the peer
    pub fn get_last_fail_count(&self) -> u64 {
        self.last_fail_count.load(Ordering::SeqCst)
    }

    // Set the last fail count of the peer
    pub fn set_last_fail_count(&self, value: u64) {
        self.last_fail_count.store(value, Ordering::SeqCst);
    }

    // Get the fail count of the peer
    pub fn get_fail_count(&self) -> u8 {
        self.fail_count.load(Ordering::SeqCst)
    }

    // Update the fail count of the peer
    // This is used by display to have up-to-date data
    // We don't add anything, just reset the counter if its long time we didn't get a fail
    fn update_fail_count_default(&self) -> bool {
        self.update_fail_count(get_current_time_in_seconds(), 0)
    }

    // Update the fail count of the peer
    fn update_fail_count(&self, current_time: u64, to_store: u8) -> bool {
        let last_fail = self.get_last_fail_count();
        let reset = last_fail + PEER_FAIL_TIME_RESET < current_time;
        if reset {
            // reset counter
            self.fail_count.store(to_store, Ordering::SeqCst);
        }
        reset
    }

    // Increment the fail count of the peer
    // This is used to track the number of times we failed to communicate with the peer
    // If the fail count is greater than 20, we should close the connection
    pub fn increment_fail_count(&self) {
        let current_time = get_current_time_in_seconds();
        // if its long time we didn't get a fail, reset the fail count to 1 (because of current fail)
        // otherwise, add 1
        if !self.update_fail_count(current_time, 1) {
            self.fail_count.fetch_add(1, Ordering::SeqCst);
        }
        self.set_last_fail_count(current_time);
    }

    // Get the last time we got a chain sync request
    // This is used to prevent spamming the chain sync packet
    pub fn get_last_chain_sync(&self) -> TimestampSeconds {
        self.last_chain_sync.load(Ordering::SeqCst)
    }

    // Store the last time we got a chain sync request
    pub fn set_last_chain_sync(&self, time: TimestampSeconds) {
        self.last_chain_sync.store(time, Ordering::SeqCst);
    }

    // Get all objects requested from this peer
    pub async fn clear_objects_requested(&self) {
        let mut objects = self.objects_requested.lock().await;
        objects.clear();
    }

    // Remove a requested object from the requested list
    pub async fn remove_object_request(
        &self,
        request: &ObjectRequest,
    ) -> Option<broadcast::Sender<OwnedObjectResponse>> {
        let mut objects = self.objects_requested.lock().await;
        objects.pop(request)
    }

    // Request a object from this peer and wait on it until we receive it or until timeout
    pub async fn request_blocking_object(
        &self,
        request: ObjectRequest,
    ) -> Result<OwnedObjectResponse, P2pError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("waiting for permit {}", request);
        }
        let _permit = self.objects_semaphore.acquire().await?;
        if log::log_enabled!(log::Level::Debug) {
            debug!("requesting {}", request);
        }
        counter!("tos_p2p_objects_requests", "peer" => self.get_id().to_string()).increment(1u64);

        let mut receiver = {
            let mut objects = self.objects_requested.lock().await;
            if let Some(sender) = objects.get(&request) {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "{} was already sent to {}, subscribing to the same channel",
                        request, self
                    );
                }
                sender.subscribe()
            } else {
                self.send_packet(Packet::ObjectRequest(Cow::Borrowed(&request)))
                    .await?;
                let (sender, receiver) = broadcast::channel(1);
                // clone is necessary in case timeout has occured
                if objects.put(request.clone(), sender).is_some() {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!("{} was already pending for {}", request, self);
                    }
                };
                receiver
            }
        };

        let mut exit_channel = self.get_exit_receiver();
        let object = select! {
            _ = exit_channel.recv() => return Err(P2pError::Disconnected),
            res = timeout(Duration::from_millis(PEER_TIMEOUT_REQUEST_OBJECT), receiver.recv()) => match res {
                Ok(res) => res.context("Error on blocking object response")?,
                Err(_) => {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!("Requested data {} from {} has timed out", request, self);
                    }
                    let mut objects = self.objects_requested.lock().await;
                    // remove it from request list
                    objects.pop(&request);
                    return Err(P2pError::ObjectRequestTimedOut(request));
                }
            }
        };
        if log::log_enabled!(log::Level::Debug) {
            debug!("received response for request {}", request);
        }

        // Verify that the object is the one we requested
        let object_hash = object.get_hash();
        if *object_hash != *request.get_hash() {
            return Err(P2pError::InvalidObjectResponse(object_hash.clone()));
        }

        // Returns error if the object is not found
        if let OwnedObjectResponse::NotFound(request) = &object {
            return Err(P2pError::ObjectNotFound(request.clone()));
        }

        Ok(object)
    }

    // Request a bootstrap chain from this peer and wait on it until we receive it or until timeout
    pub async fn request_boostrap_chain(
        &self,
        step: StepRequest<'_>,
    ) -> Result<StepResponse, P2pError> {
        let step_kind = step.kind();
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "waiting for permit for bootstrap chain step: {:?}",
                step_kind
            );
        }

        let _permit = self.objects_semaphore.acquire().await?;
        if log::log_enabled!(log::Level::Debug) {
            debug!("Requesting bootstrap chain step: {:?}", step_kind);
        }
        counter!("tos_p2p_bootstrap_requests", "peer" => self.get_id().to_string()).increment(1u64);

        let (sender, receiver) = tokio::sync::oneshot::channel();
        {
            let mut senders = self.bootstrap_requests.lock().await;

            // send the packet while holding the lock so we ensure the correct order
            self.send_packet(Packet::BootstrapChainRequest(BootstrapChainRequest::new(
                step,
            )))
            .await?;

            senders.push_back(sender);
        }

        let mut exit_channel = self.get_exit_receiver();
        let response = select! {
            _ = exit_channel.recv() => return Err(P2pError::Disconnected),
            res = timeout(Duration::from_millis(PEER_TIMEOUT_BOOTSTRAP_STEP), receiver) => match res {
                Ok(res) => res?,
                Err(e) => {
                    // Clear the bootstrap chain channel to preserve the order
                    {
                        let mut senders = self.bootstrap_requests.lock().await;
                        senders.pop_front();
                    }

                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Requested bootstrap chain step {:?} has timed out", step_kind);
                    }
                    return Err(P2pError::AsyncTimeOut(e));
                }
            }
        };

        // check that the response is what we asked for
        let response_kind = response.kind();
        if response_kind != step_kind {
            return Err(P2pError::InvalidBootstrapStep(step_kind, response_kind));
        }

        Ok(response)
    }

    // Request a sync chain from this peer and wait on it until we receive it or until timeout
    pub async fn request_sync_chain(
        &self,
        request: PacketWrapper<'_, ChainRequest>,
    ) -> Result<ChainResponse, P2pError> {
        debug!("Requesting sync chain");
        let (sender, receiver) = tokio::sync::oneshot::channel();
        {
            let mut sender_lock = self.sync_chain.lock().await;
            *sender_lock = Some(sender);
        }

        trace!("sending chain request packet");
        self.send_packet(Packet::ChainRequest(request)).await?;

        trace!("waiting for chain response");
        let mut exit_channel = self.get_exit_receiver();
        let response = select! {
            _ = exit_channel.recv() => return Err(P2pError::Disconnected),
            res = timeout(Duration::from_secs(CHAIN_SYNC_TIMEOUT_SECS), receiver) => match res {
                Ok(res) => res?,
                Err(e) => {
                    // Clear the sync chain channel
                    let contains = self.sync_chain.lock().await.take().is_some();
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Requested sync chain has timed out, contains: {}", contains);
                    }
                    return Err(P2pError::AsyncTimeOut(e));
                }
            }
        };

        Ok(response)
    }

    // Get the bootstrap chain channel
    // Like the sync chain channel, but for bootstrap (fast sync) syncing
    pub async fn get_next_bootstrap_request(&self) -> Option<oneshot::Sender<StepResponse>> {
        let mut requests = self.bootstrap_requests.lock().await;
        requests.pop_front()
    }

    // Clear all pending requests in case something went wrong
    pub async fn clear_bootstrap_requests(&self) {
        let mut requests = self.bootstrap_requests.lock().await;
        requests.clear();
    }

    // Get the sync chain channel
    // This is used for chain sync requests to be fully awaited
    pub fn get_sync_chain_channel(&self) -> &Mutex<Option<oneshot::Sender<ChainResponse>>> {
        &self.sync_chain
    }

    // Get all shared peers between this peer and us
    pub fn get_peers(&self) -> &Mutex<LruCache<SocketAddr, TimedDirection>> {
        &self.peers
    }

    // Get the last time we got a peer list
    pub fn get_last_peer_list(&self) -> TimestampSeconds {
        self.last_peer_list.load(Ordering::SeqCst)
    }

    // Track the last time we got a peer list
    // This is used to prevent spamming the peer list
    pub fn set_last_peer_list(&self, value: TimestampSeconds) {
        self.last_peer_list.store(value, Ordering::SeqCst)
    }

    // Get the last time we got a ping packet from this peer
    pub fn get_last_ping(&self) -> TimestampSeconds {
        self.last_ping.load(Ordering::SeqCst)
    }

    // Track the last time we got a ping packet from this peer
    pub fn set_last_ping(&self, value: TimestampSeconds) {
        self.last_ping.store(value, Ordering::SeqCst)
    }

    // Get the last time we sent a ping packet to this peer
    pub fn get_last_ping_sent(&self) -> TimestampSeconds {
        self.last_ping_sent.load(Ordering::SeqCst)
    }

    // Track the last time we sent a ping packet to this peer
    pub fn set_last_ping_sent(&self, value: TimestampSeconds) {
        self.last_ping.store(value, Ordering::SeqCst)
    }

    // ========================================================================
    // UNSOLICITED BLOCK RATE LIMITING (P1-1)
    //
    // These methods track blocks received without explicit request to detect
    // flood attacks. Uses a sliding window of 1 second.
    //
    // Reference: TOS_FORK_PREVENTION_IMPLEMENTATION_V2.md
    // ========================================================================

    /// Record an unsolicited block and check if rate limit is exceeded.
    ///
    /// # Arguments
    /// * `max_rate` - Maximum allowed unsolicited blocks per second
    ///
    /// # Returns
    /// * `true` if rate limit exceeded (caller should take action)
    /// * `false` if within acceptable rate
    ///
    /// # Algorithm
    /// Uses a 1-second sliding window. If the current second differs from
    /// the stored window start, the counter resets. Otherwise, it increments.
    /// Compare-exchange ensures thread-safety without locks.
    pub fn record_unsolicited_block_and_check(&self, max_rate: u32) -> bool {
        let current_time = get_current_time_in_seconds();
        let window_start = self.unsolicited_block_window_start.load(Ordering::SeqCst);

        // Check if we're in a new time window
        if current_time != window_start {
            // Try to reset the window atomically
            // If another thread already reset it, that's fine - we'll just increment
            match self.unsolicited_block_window_start.compare_exchange(
                window_start,
                current_time,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    // Successfully reset window, start count at 1
                    self.unsolicited_block_count.store(1, Ordering::SeqCst);
                    return false; // First block in new window, always OK
                }
                Err(actual) => {
                    // Another thread reset the window, check if it's current
                    if actual == current_time {
                        // Window is current, just increment
                        let count = self.unsolicited_block_count.fetch_add(1, Ordering::SeqCst) + 1;
                        return count > max_rate;
                    } else {
                        // Window is stale, try again (recursive would be cleaner but avoid for simplicity)
                        self.unsolicited_block_window_start
                            .store(current_time, Ordering::SeqCst);
                        self.unsolicited_block_count.store(1, Ordering::SeqCst);
                        return false;
                    }
                }
            }
        }

        // Same window, increment and check
        let count = self.unsolicited_block_count.fetch_add(1, Ordering::SeqCst) + 1;
        count > max_rate
    }

    /// Get the current unsolicited block count for monitoring/debugging
    pub fn get_unsolicited_block_count(&self) -> u32 {
        self.unsolicited_block_count.load(Ordering::SeqCst)
    }

    /// Reset the unsolicited block counter (e.g., after successful sync)
    pub fn reset_unsolicited_block_count(&self) {
        self.unsolicited_block_count.store(0, Ordering::SeqCst);
        self.unsolicited_block_window_start
            .store(0, Ordering::SeqCst);
    }

    // Get the last time a inventory has been requested
    pub fn get_last_inventory(&self) -> TimestampSeconds {
        self.last_inventory.load(Ordering::SeqCst)
    }

    // Set the last inventory time
    pub fn set_last_inventory(&self, value: TimestampSeconds) {
        self.last_inventory.store(value, Ordering::SeqCst)
    }

    // Get the requested inventory flag
    pub fn has_requested_inventory(&self) -> bool {
        self.requested_inventory.load(Ordering::SeqCst)
    }

    // Set the requested inventory flag
    pub fn set_requested_inventory(&self, value: bool) {
        self.requested_inventory.store(value, Ordering::SeqCst)
    }

    // Get the outgoing address of the peer
    // This represents the IP address of the peer and the port on which it is listening
    pub fn get_outgoing_address(&self) -> &SocketAddr {
        &self.outgoing_address
    }

    // Close the peer connection and remove it from the peer list
    pub async fn close_and_temp_ban(&self, seconds: u64) -> Result<(), P2pError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("temp ban {}", self);
        }
        if !self.is_priority() {
            self.peer_list
                .temp_ban_address(&self.get_connection().get_address().ip(), seconds, false)
                .await?;
        } else {
            if log::log_enabled!(log::Level::Debug) {
                debug!("{} is a priority peer, closing only", self);
            }
        }

        self.peer_list.remove_peer(self.get_id(), true).await?;

        Ok(())
    }

    // Signal the exit of the peer to the tasks
    // This is listened by write task to close the connection
    pub async fn signal_exit(&self) -> Result<(), P2pError> {
        self.exit_channel
            .send(())
            .map_err(|e| P2pError::SendError(e.to_string()))?;

        Ok(())
    }

    // Close the peer connection and remove it from the peer list
    pub async fn close(&self) -> Result<(), P2pError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Deleting peer {} from peerlist", self);
        }
        let res = self.peer_list.remove_peer(self.get_id(), true).await;

        if log::log_enabled!(log::Level::Trace) {
            trace!("Closing connection internal with {}", self);
        }
        self.get_connection()
            .close()
            .await
            .context("Error while closing internal connection")?;

        res
    }

    // Send a packet to the peer
    // This will transform the packet into bytes and send it to the peer
    pub async fn send_packet(&self, packet: Packet<'_>) -> Result<(), P2pError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Sending {:?}", packet);
        }
        self.send_bytes(Bytes::from(packet.to_bytes())).await
    }

    // Send packet bytes to the peer
    // This will send the bytes to the writer task through its channel
    pub async fn send_bytes(&self, bytes: Bytes) -> Result<(), P2pError> {
        self.tx
            .send(bytes)
            .await
            .map_err(|e| P2pError::SendError(e.to_string()))
    }

    pub async fn set_read_task_state(&self, state: TaskState) {
        *self.read_task.lock().await = state;
    }

    pub async fn set_write_task_state(&self, state: TaskState) {
        *self.write_task.lock().await = state;
    }

    pub async fn get_read_task_state(&self) -> TaskState {
        *self.read_task.lock().await
    }

    pub async fn get_write_task_state(&self) -> TaskState {
        *self.write_task.lock().await
    }
}

impl Display for Peer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), Error> {
        // update fail counter to have up-to-date data to display
        self.update_fail_count_default();
        let peers = if let Ok(peers) = self.get_peers().try_lock() {
            if log::log_enabled!(log::Level::Trace) {
                format!(
                    "{}",
                    peers
                        .iter()
                        .map(|(p, d)| format!("{} = {:?}", p, d))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("{}", peers.len())
            }
        } else {
            "Couldn't retrieve data".to_string()
        };

        let top_hash = if let Ok(hash) = self.get_top_block_hash().try_lock() {
            hash.to_string()
        } else {
            "Couldn't retrieve data".to_string()
        };

        let pruned_state = if let Some(value) = self.get_pruned_topoheight() {
            format!("Yes ({})", value)
        } else {
            "No".to_string()
        };

        let read_task = self
            .read_task
            .try_lock()
            .map(|v| *v)
            .unwrap_or(TaskState::Unknown);
        let write_task = self
            .write_task
            .try_lock()
            .map(|v| *v)
            .unwrap_or(TaskState::Unknown);

        write!(f, "Peer[connection: {}, id: {}, topoheight: {}, top hash: {}, height: {}, pruned: {}, priority: {}, tag: {}, version: {}, fail count: {}, out: {}, peers: {}, tasks: {:?}/{:?}, txs: {}]",
            self.get_connection(),
            self.get_id(),
            self.get_topoheight(),
            top_hash,
            self.get_height(),
            pruned_state,
            self.is_priority(),
            self.get_node_tag().as_ref().unwrap_or(&"None".to_owned()),
            self.get_version(),
            self.get_fail_count(),
            self.is_out(),
            peers,
            read_task,
            write_task,
            self.is_ready_for_txs_propagation()
        )
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        // This shouldn't happen, but in case we have a lurking bug somewhere
        if !self.get_connection().is_closed() {
            if log::log_enabled!(log::Level::Warn) {
                warn!("{} was not closed correctly /!\\", self)
            }
        }
    }
}

impl PartialEq for Peer {
    fn eq(&self, other: &Self) -> bool {
        self.get_id() == other.get_id()
    }
}

impl Eq for Peer {}

impl StdHash for Peer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.get_id().hash(state);
    }
}
