//! UDP server for the discovery protocol.
//!
//! The discovery server handles:
//! - PING/PONG for liveness checks
//! - FINDNODE/NEIGHBORS for peer discovery
//! - Bootstrap node connections

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{debug, error, info, trace, warn};
use tos_common::crypto::Hash;
use tos_common::serializer::Serializer;
use tos_common::tokio::net::UdpSocket;
use tos_common::tokio::sync::{RwLock, Semaphore};
use tos_common::tokio::time::interval;

use super::config::DiscoveryConfig;
use super::error::{DiscoveryError, DiscoveryResult};
use super::identity::{NodeId, NodeIdentity};
use super::messages::{
    FindNode, Message, Neighbors, NodeInfo, Ping, Pong, SignedPacket, MAX_NEIGHBORS,
    MAX_PACKET_SIZE,
};
use super::routing_table::{InsertResult, RoutingTable, ALPHA};
use super::url::TosNodeUrl;

/// Interval for sending refresh requests to bootstrap nodes.
const BOOTSTRAP_INTERVAL: Duration = Duration::from_secs(60);

/// Interval for refreshing random buckets.
const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// Timeout for waiting for responses.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum pending PING requests.
const MAX_PENDING_PINGS: usize = 256;

/// Maximum pending FINDNODE requests.
const MAX_PENDING_FINDNODES: usize = 256;

/// Maximum concurrent packet handlers (DoS prevention).
const MAX_CONCURRENT_HANDLERS: usize = 64;

/// How long an endpoint validation remains valid.
const ENDPOINT_VALIDATION_DURATION: Duration = Duration::from_secs(300);

/// Maximum validated endpoints to track.
const MAX_VALIDATED_ENDPOINTS: usize = 1024;

/// Maximum processed PONGs to track (replay prevention).
const MAX_PROCESSED_PONGS: usize = 512;

/// Pending ping information.
struct PendingPing {
    /// Target node ID.
    node_id: NodeId,
    /// Target address (for validation).
    address: SocketAddr,
    /// Time the ping was sent.
    sent_time: Instant,
}

/// Pending FINDNODE request information.
struct PendingFindNode {
    /// Target node ID we're looking for (stored for future validation).
    #[allow(dead_code)]
    target: NodeId,
    /// Address we sent the request to.
    address: SocketAddr,
    /// Time the request was sent.
    sent_time: Instant,
}

/// Validated endpoint information.
struct ValidatedEndpoint {
    /// Node ID at this endpoint.
    node_id: NodeId,
    /// When the endpoint was validated.
    validated_at: Instant,
}

/// Discovery server handling UDP communication.
pub struct DiscoveryServer {
    /// UDP socket for sending/receiving packets.
    socket: Arc<UdpSocket>,
    /// Node identity (key pair and node ID).
    identity: Arc<NodeIdentity>,
    /// Routing table for known nodes.
    routing_table: Arc<RoutingTable>,
    /// Configuration.
    config: DiscoveryConfig,
    /// Running flag.
    running: AtomicBool,
    /// Sequence counter for PING messages.
    seq_counter: AtomicU64,
    /// Pending PING requests (ping_hash -> PendingPing).
    pending_pings: RwLock<HashMap<Hash, PendingPing>>,
    /// Our external address (as seen by other nodes).
    external_address: RwLock<Option<SocketAddr>>,
    /// Semaphore to limit concurrent packet handlers (DoS prevention).
    handler_semaphore: Arc<Semaphore>,
    /// Validated endpoints (SocketAddr -> ValidatedEndpoint).
    /// Only respond to FINDNODE from validated endpoints (anti-amplification).
    validated_endpoints: RwLock<HashMap<SocketAddr, ValidatedEndpoint>>,
    /// Pending FINDNODE requests (node_id of sender -> PendingFindNode).
    /// Only accept NEIGHBORS from senders with matching pending requests.
    pending_findnodes: RwLock<HashMap<NodeId, PendingFindNode>>,
    /// Recently processed PONG hashes (replay prevention).
    processed_pongs: RwLock<HashMap<Hash, Instant>>,
}

impl DiscoveryServer {
    /// Create a new discovery server.
    pub async fn new(
        config: DiscoveryConfig,
        identity: NodeIdentity,
    ) -> DiscoveryResult<Arc<Self>> {
        let bind_address = config.get_bind_address();
        let socket = UdpSocket::bind(&bind_address)
            .await
            .map_err(|e| DiscoveryError::BindFailed(bind_address.clone(), e))?;

        if log::log_enabled!(log::Level::Info) {
            info!(
                "Discovery server listening on {} (node_id: {})",
                bind_address,
                hex::encode(identity.node_id().as_bytes())
            );
        }

        let routing_table = Arc::new(RoutingTable::new(
            identity.node_id().clone(),
            config.bucket_size,
        ));

        Ok(Arc::new(Self {
            socket: Arc::new(socket),
            identity: Arc::new(identity),
            routing_table,
            config,
            running: AtomicBool::new(false),
            seq_counter: AtomicU64::new(0),
            pending_pings: RwLock::new(HashMap::new()),
            external_address: RwLock::new(None),
            handler_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_HANDLERS)),
            validated_endpoints: RwLock::new(HashMap::new()),
            pending_findnodes: RwLock::new(HashMap::new()),
            processed_pongs: RwLock::new(HashMap::new()),
        }))
    }

    /// Get the node identity.
    pub fn identity(&self) -> &NodeIdentity {
        &self.identity
    }

    /// Get the routing table.
    pub fn routing_table(&self) -> &Arc<RoutingTable> {
        &self.routing_table
    }

    /// Get the node URL for this server.
    pub async fn node_url(&self) -> Option<TosNodeUrl> {
        let external_addr = self.external_address.read().await;
        external_addr.map(|addr| TosNodeUrl::new(self.identity.node_id().clone(), addr))
    }

    /// Get our local NodeInfo with the correct address.
    ///
    /// Uses external_address if known, otherwise falls back to the socket's local address.
    async fn local_node_info(&self) -> NodeInfo {
        // Default address when local_addr fails (should not happen in practice)
        const DEFAULT_ADDR: std::net::SocketAddr =
            std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)), 0);

        let address = {
            let external = self.external_address.read().await;
            match *external {
                Some(addr) => addr,
                None => self.socket.local_addr().unwrap_or(DEFAULT_ADDR),
            }
        };
        NodeInfo::new(
            self.identity.node_id().clone(),
            address,
            self.identity.public_key(),
        )
    }

    /// Check if the server is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Stop the server.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Start the discovery server.
    pub async fn start(self: Arc<Self>) {
        if self.running.swap(true, Ordering::SeqCst) {
            if log::log_enabled!(log::Level::Warn) {
                warn!("Discovery server already running");
            }
            return;
        }

        if log::log_enabled!(log::Level::Info) {
            info!("Starting discovery server");
        }

        if self.config.is_bootnode() {
            if log::log_enabled!(log::Level::Info) {
                info!("Running in discovery-only (bootnode) mode");
            }
        }

        // Parse and connect to bootstrap nodes
        self.connect_bootstrap_nodes().await;

        // Spawn the receive loop
        let server = Arc::clone(&self);
        tos_common::tokio::spawn_task("discovery-receive", async move {
            server.receive_loop().await;
        });

        // Spawn the maintenance loop
        let server = Arc::clone(&self);
        tos_common::tokio::spawn_task("discovery-maintenance", async move {
            server.maintenance_loop().await;
        });
    }

    /// Connect to bootstrap nodes from configuration.
    async fn connect_bootstrap_nodes(&self) {
        for url_str in &self.config.bootstrap_nodes {
            match TosNodeUrl::parse(url_str) {
                Ok(url) => {
                    if log::log_enabled!(log::Level::Info) {
                        info!("Connecting to bootstrap node: {}", url);
                    }
                    if let Err(e) = self.ping_node(&url.node_id, url.address).await {
                        if log::log_enabled!(log::Level::Warn) {
                            warn!("Failed to ping bootstrap node {}: {}", url, e);
                        }
                    }
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Error) {
                        error!("Invalid bootstrap node URL '{}': {}", url_str, e);
                    }
                }
            }
        }
    }

    /// Main receive loop for handling incoming packets.
    async fn receive_loop(self: Arc<Self>) {
        let mut buf = vec![0u8; MAX_PACKET_SIZE];

        while self.running.load(Ordering::SeqCst) {
            match self.socket.recv_from(&mut buf).await {
                Ok((len, from)) => {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!("Received {} bytes from {}", len, from);
                    }

                    // Rate limiting: try to acquire a permit (non-blocking)
                    let permit = match self.handler_semaphore.clone().try_acquire_owned() {
                        Ok(permit) => permit,
                        Err(_) => {
                            // Too many concurrent handlers, drop this packet
                            if log::log_enabled!(log::Level::Debug) {
                                debug!(
                                    "Dropping packet from {} (at handler capacity {})",
                                    from, MAX_CONCURRENT_HANDLERS
                                );
                            }
                            continue;
                        }
                    };

                    let data = buf[..len].to_vec();
                    let server = Arc::clone(&self);
                    tos_common::tokio::spawn_task("discovery-handle", async move {
                        // Permit is automatically released when dropped at end of task
                        let _permit = permit;
                        if let Err(e) = server.handle_packet(&data, from).await {
                            if log::log_enabled!(log::Level::Debug) {
                                debug!("Error handling packet from {}: {}", from, e);
                            }
                        }
                    });
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Error) {
                        error!("Error receiving packet: {}", e);
                    }
                }
            }
        }
    }

    /// Maintenance loop for periodic tasks.
    async fn maintenance_loop(self: Arc<Self>) {
        let mut refresh_timer = interval(REFRESH_INTERVAL);
        let mut bootstrap_timer = interval(BOOTSTRAP_INTERVAL);
        let mut cleanup_timer = interval(Duration::from_secs(30));

        while self.running.load(Ordering::SeqCst) {
            tos_common::tokio::select! {
                _ = refresh_timer.tick() => {
                    self.refresh_random_bucket().await;
                }
                _ = bootstrap_timer.tick() => {
                    self.connect_bootstrap_nodes().await;
                }
                _ = cleanup_timer.tick() => {
                    self.cleanup_pending_pings().await;
                }
            }
        }
    }

    /// Handle an incoming packet.
    async fn handle_packet(&self, data: &[u8], from: SocketAddr) -> DiscoveryResult<()> {
        let packet = SignedPacket::decode(data)?;

        // Check expiration
        if packet.message.is_expired() {
            return Err(DiscoveryError::MessageExpired(0, 0));
        }

        match &packet.message {
            Message::Ping(ping) => {
                // Verify signature
                packet.verify(&ping.source.public_key)?;
                self.handle_ping(&packet, ping, from).await
            }
            Message::Pong(pong) => {
                // Always verify PONG signature using the included public key
                packet.verify(&pong.source.public_key)?;
                self.handle_pong(pong, from).await
            }
            Message::FindNode(find_node) => {
                // Verify signature using the source's public key
                packet.verify(&find_node.source.public_key)?;
                self.handle_find_node(find_node, from).await
            }
            Message::Neighbors(neighbors) => {
                // Verify signature using the source's public key
                packet.verify(&neighbors.source.public_key)?;
                self.handle_neighbors(neighbors, from).await
            }
        }
    }

    /// Handle a PING message.
    async fn handle_ping(
        &self,
        packet: &SignedPacket,
        ping: &Ping,
        from: SocketAddr,
    ) -> DiscoveryResult<()> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Received PING from {} (node_id: {})",
                from,
                hex::encode(ping.source.node_id.as_bytes())
            );
        }

        // Validate node ID matches public key
        if !ping.source.verify_node_id() {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "PING from {} has invalid node_id (doesn't match public key)",
                    from
                );
            }
            return Err(DiscoveryError::InvalidNodeId(
                "expected".to_string(),
                "mismatch".to_string(),
            ));
        }

        // Add/update sender in routing table
        let node_info = NodeInfo::new(
            ping.source.node_id.clone(),
            from,
            ping.source.public_key.clone(),
        );
        self.routing_table.insert(node_info.clone()).await;

        // Send PONG with our correct address
        let pong_hash = packet.hash();
        let source = self.local_node_info().await;
        let pong = Pong::new(pong_hash, source);
        self.send_message(Message::Pong(pong), from).await?;

        Ok(())
    }

    /// Handle a PONG message.
    async fn handle_pong(&self, pong: &Pong, from: SocketAddr) -> DiscoveryResult<()> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Received PONG from {} (node_id: {})",
                from,
                hex::encode(pong.source.node_id.as_bytes())
            );
        }

        // Validate node ID matches public key
        if !pong.source.verify_node_id() {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "PONG from {} has invalid node_id (doesn't match public key)",
                    from
                );
            }
            return Err(DiscoveryError::InvalidNodeId(
                "expected".to_string(),
                "mismatch".to_string(),
            ));
        }

        // Fix 4: Check for PONG replay attack
        {
            let mut processed = self.processed_pongs.write().await;

            // Check if this ping_hash was already processed
            if processed.contains_key(&pong.ping_hash) {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "Duplicate PONG from {} (ping_hash already processed, possible replay)",
                        from
                    );
                }
                return Ok(()); // Silently ignore replayed PONGs
            }

            // Clean up old entries if at capacity
            if processed.len() >= MAX_PROCESSED_PONGS {
                let cutoff = Instant::now() - RESPONSE_TIMEOUT;
                processed.retain(|_, time| *time > cutoff);
            }

            // Track this ping_hash as processed
            if processed.len() < MAX_PROCESSED_PONGS {
                processed.insert(pong.ping_hash.clone(), Instant::now());
            }
        }

        // Validate PONG matches a pending PING
        let pending_info = {
            let mut pending = self.pending_pings.write().await;
            pending.remove(&pong.ping_hash)
        };

        let is_valid_response = match &pending_info {
            Some(info) => {
                // Verify the PONG is from the expected node AND address
                if info.node_id != pong.source.node_id {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!(
                            "PONG from {} has unexpected node_id (expected: {}, got: {})",
                            from,
                            hex::encode(info.node_id.as_bytes()),
                            hex::encode(pong.source.node_id.as_bytes())
                        );
                    }
                    false
                } else if info.address != from {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!(
                            "PONG has unexpected source address (expected: {}, got: {})",
                            info.address, from
                        );
                    }
                    false
                } else {
                    true
                }
            }
            None => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Received unsolicited PONG from {} (no matching pending PING)",
                        from
                    );
                }
                false
            }
        };

        // Update routing table (even for unsolicited PONGs, if signature was valid)
        let node_info = NodeInfo::new(
            pong.source.node_id.clone(),
            from,
            pong.source.public_key.clone(),
        );
        self.routing_table.insert(node_info).await;

        // Only update external address and validate endpoint for valid responses
        if is_valid_response {
            // Update external address discovery
            let mut external = self.external_address.write().await;
            if external.is_none() {
                *external = Some(from);
                if log::log_enabled!(log::Level::Info) {
                    info!("Discovered external address: {}", from);
                }
            }
            drop(external);

            // Fix 1: Mark this endpoint as validated (anti-amplification)
            {
                let mut validated = self.validated_endpoints.write().await;

                // Clean up old entries if at capacity
                if validated.len() >= MAX_VALIDATED_ENDPOINTS {
                    let cutoff = Instant::now() - ENDPOINT_VALIDATION_DURATION;
                    validated.retain(|_, v| v.validated_at > cutoff);
                }

                // Mark endpoint as validated
                if validated.len() < MAX_VALIDATED_ENDPOINTS {
                    validated.insert(
                        from,
                        ValidatedEndpoint {
                            node_id: pong.source.node_id.clone(),
                            validated_at: Instant::now(),
                        },
                    );
                    if log::log_enabled!(log::Level::Trace) {
                        trace!(
                            "Validated endpoint {} (node_id: {})",
                            from,
                            hex::encode(pong.source.node_id.as_bytes())
                        );
                    }
                }
            }
        }

        // Touch the node in routing table
        self.routing_table.touch(&pong.source.node_id).await;

        Ok(())
    }

    /// Handle a FINDNODE message.
    ///
    /// Fix 1: Only respond to FINDNODE from validated endpoints to prevent
    /// amplification attacks. NEIGHBORS response can be much larger than
    /// FINDNODE request, so we require prior PING/PONG exchange.
    async fn handle_find_node(
        &self,
        find_node: &FindNode,
        from: SocketAddr,
    ) -> DiscoveryResult<()> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Received FINDNODE from {} (target: {})",
                from,
                hex::encode(find_node.target.as_bytes())
            );
        }

        // Validate node ID matches public key
        if !find_node.source.verify_node_id() {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "FINDNODE from {} has invalid node_id (doesn't match public key)",
                    from
                );
            }
            return Err(DiscoveryError::InvalidNodeId(
                "expected".to_string(),
                "mismatch".to_string(),
            ));
        }

        // Fix 1: Check if endpoint is validated (anti-amplification)
        let is_validated = {
            let validated = self.validated_endpoints.read().await;
            if let Some(endpoint) = validated.get(&from) {
                // Check if validation is still fresh and node_id matches
                endpoint.validated_at.elapsed() < ENDPOINT_VALIDATION_DURATION
                    && endpoint.node_id == find_node.source.node_id
            } else {
                false
            }
        };

        if !is_validated {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Ignoring FINDNODE from unvalidated endpoint {} (send PING first)",
                    from
                );
            }
            // Send a PING to initiate validation, don't respond with NEIGHBORS yet
            // The sender should complete PING/PONG exchange before sending FINDNODE
            if let Err(e) = self.ping_node(&find_node.source.node_id, from).await {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Failed to ping unvalidated endpoint {}: {}", from, e);
                }
            }
            return Err(DiscoveryError::EndpointNotValidated(from.to_string()));
        }

        // Add/update sender in routing table
        let node_info = NodeInfo::new(
            find_node.source.node_id.clone(),
            from,
            find_node.source.public_key.clone(),
        );
        self.routing_table.insert(node_info).await;

        // Find closest nodes to target
        let closest = self
            .routing_table
            .closest(&find_node.target, MAX_NEIGHBORS)
            .await;
        let nodes: Vec<NodeInfo> = closest.into_iter().map(|e| e.info).collect();

        // Send NEIGHBORS response with our correct address
        let source = self.local_node_info().await;
        let neighbors = Neighbors::new(source, nodes);
        self.send_message(Message::Neighbors(neighbors), from)
            .await?;

        Ok(())
    }

    /// Handle a NEIGHBORS message.
    ///
    /// Fix 2: Only accept NEIGHBORS responses that correspond to outstanding
    /// FINDNODE requests to prevent routing table poisoning and third-party scanning.
    async fn handle_neighbors(
        &self,
        neighbors: &Neighbors,
        from: SocketAddr,
    ) -> DiscoveryResult<()> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Received NEIGHBORS from {} ({} nodes)",
                from,
                neighbors.nodes.len()
            );
        }

        // Validate source node ID matches public key
        if !neighbors.source.verify_node_id() {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "NEIGHBORS from {} has invalid source node_id (doesn't match public key)",
                    from
                );
            }
            return Err(DiscoveryError::InvalidNodeId(
                "expected".to_string(),
                "mismatch".to_string(),
            ));
        }

        // Fix 2: Verify this NEIGHBORS corresponds to a pending FINDNODE request
        let pending_request = {
            let mut pending = self.pending_findnodes.write().await;
            pending.remove(&neighbors.source.node_id)
        };

        if pending_request.is_none() {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "Ignoring unsolicited NEIGHBORS from {} (no matching FINDNODE request)",
                    from
                );
            }
            return Err(DiscoveryError::UnsolicitedResponse(
                "NEIGHBORS".to_string(),
                from.to_string(),
            ));
        }

        // Verify the response is from the expected address
        if let Some(ref req) = pending_request {
            if req.address != from {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "NEIGHBORS from unexpected address (expected: {}, got: {})",
                        req.address, from
                    );
                }
                // Still process it since node_id matched, but log the discrepancy
            }
        }

        // Update source in routing table
        let source_info = NodeInfo::new(
            neighbors.source.node_id.clone(),
            from,
            neighbors.source.public_key.clone(),
        );
        self.routing_table.insert(source_info).await;

        // Process nodes from NEIGHBORS - ping to verify before adding
        for node in &neighbors.nodes {
            // Don't add ourselves
            if node.node_id == *self.identity.node_id() {
                continue;
            }

            // Validate node ID matches public key
            if !node.verify_node_id() {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "NEIGHBORS contains node with invalid node_id: {}",
                        hex::encode(node.node_id.as_bytes())
                    );
                }
                continue;
            }

            // Check if already in routing table
            if self.routing_table.contains(&node.node_id).await {
                // Already known, skip
                continue;
            }

            // Check if bucket has space
            let result = self.routing_table.insert(node.clone()).await;
            match result {
                InsertResult::Inserted => {
                    // Ping new nodes to verify they're alive
                    // If they don't respond, they'll be evicted via record_failure
                    if let Err(e) = self.ping_node(&node.node_id, node.address).await {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Failed to ping new node {}: {}", node.address, e);
                        }
                        // Remove immediately if we couldn't even send the ping
                        self.routing_table.remove(&node.node_id).await;
                    }
                }
                InsertResult::Updated => {
                    // Already in table, touch to update
                }
                InsertResult::BucketFull(oldest_id) => {
                    // Bucket is full, ping oldest node to check if it's still alive
                    if let Some(oldest) = self.routing_table.get(&oldest_id).await {
                        if self
                            .ping_node(&oldest_id, oldest.info.address)
                            .await
                            .is_err()
                        {
                            // Couldn't send ping, try to evict and insert new node
                            if self.routing_table.evict_if_oldest(&oldest_id).await {
                                self.routing_table.insert(node.clone()).await;
                            }
                        }
                        // If ping sent successfully, the oldest node will be evicted
                        // via record_failure if it doesn't respond
                    }
                }
                InsertResult::SelfInsert => {
                    // Should not happen since we check for self above
                }
            }
        }

        Ok(())
    }

    /// Send a message to an address.
    async fn send_message(&self, message: Message, to: SocketAddr) -> DiscoveryResult<()> {
        // Serialize the message
        let msg_bytes = message.to_bytes();

        // Sign the message
        let signature = self.identity.sign(&msg_bytes);

        // Create signed packet
        let packet = SignedPacket::new(message, signature);
        let data = packet.encode();

        if data.len() > MAX_PACKET_SIZE {
            return Err(DiscoveryError::PacketTooLarge(data.len(), MAX_PACKET_SIZE));
        }

        self.socket.send_to(&data, to).await?;

        if log::log_enabled!(log::Level::Trace) {
            trace!("Sent {} bytes to {}", data.len(), to);
        }

        Ok(())
    }

    /// Send a PING to a node.
    pub async fn ping_node(&self, node_id: &NodeId, address: SocketAddr) -> DiscoveryResult<()> {
        let seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);

        // Use our correct local address
        let source = self.local_node_info().await;
        let ping = Ping::new(source, seq);
        let message = Message::Ping(ping);

        // Create signed packet and compute hash
        let msg_bytes = message.to_bytes();
        let signature = self.identity.sign(&msg_bytes);
        let packet = SignedPacket::new(message, signature);
        let ping_hash = packet.hash();
        let data = packet.encode();

        if data.len() > MAX_PACKET_SIZE {
            return Err(DiscoveryError::PacketTooLarge(data.len(), MAX_PACKET_SIZE));
        }

        // Track pending ping with the hash
        {
            let mut pending = self.pending_pings.write().await;
            // Clean up expired entries if too many pending
            if pending.len() >= MAX_PENDING_PINGS {
                let old_keys: Vec<Hash> = pending
                    .iter()
                    .filter(|(_, info)| info.sent_time.elapsed() > RESPONSE_TIMEOUT)
                    .map(|(k, _)| k.clone())
                    .collect();
                for key in old_keys {
                    pending.remove(&key);
                }
            }
            // Enforce hard cap - don't insert if still at max after cleanup
            if pending.len() >= MAX_PENDING_PINGS {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "Pending pings at capacity ({}), dropping ping to {}",
                        MAX_PENDING_PINGS, address
                    );
                }
                return Ok(()); // Drop the ping silently
            }
            pending.insert(
                ping_hash,
                PendingPing {
                    node_id: node_id.clone(),
                    address,
                    sent_time: Instant::now(),
                },
            );
        }

        self.socket.send_to(&data, address).await?;

        if log::log_enabled!(log::Level::Trace) {
            trace!("Sent PING ({} bytes) to {}", data.len(), address);
        }

        Ok(())
    }

    /// Send a FINDNODE request.
    ///
    /// Fix 2: Track pending FINDNODE to only accept solicited NEIGHBORS responses.
    pub async fn find_node(
        &self,
        target: &NodeId,
        address: SocketAddr,
        sender_node_id: &NodeId,
    ) -> DiscoveryResult<()> {
        // Use our correct local address
        let source = self.local_node_info().await;
        let find_node = FindNode::new(source, target.clone());

        // Track pending FINDNODE request
        {
            let mut pending = self.pending_findnodes.write().await;

            // Clean up old entries if at capacity
            if pending.len() >= MAX_PENDING_FINDNODES {
                let cutoff = Instant::now() - RESPONSE_TIMEOUT;
                pending.retain(|_, v| v.sent_time > cutoff);
            }

            // Track this request
            if pending.len() < MAX_PENDING_FINDNODES {
                pending.insert(
                    sender_node_id.clone(),
                    PendingFindNode {
                        target: target.clone(),
                        address,
                        sent_time: Instant::now(),
                    },
                );
            }
        }

        self.send_message(Message::FindNode(find_node), address)
            .await
    }

    /// Perform a lookup for nodes close to a target.
    pub async fn lookup(&self, target: &NodeId) -> Vec<NodeInfo> {
        let mut seen = std::collections::HashSet::new();
        let mut closest = self.routing_table.closest(target, ALPHA).await;

        // Query the closest nodes iteratively
        for _ in 0..3 {
            for entry in &closest {
                if seen.contains(&entry.info.node_id) {
                    continue;
                }
                seen.insert(entry.info.node_id.clone());

                // Send FINDNODE
                if let Err(e) = self
                    .find_node(target, entry.info.address, &entry.info.node_id)
                    .await
                {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("FINDNODE failed to {}: {}", entry.info.address, e);
                    }
                    continue;
                }
            }

            // Wait a bit for responses
            tos_common::tokio::time::sleep(Duration::from_millis(500)).await;

            // Get updated closest nodes
            let new_closest = self.routing_table.closest(target, MAX_NEIGHBORS).await;
            if new_closest.is_empty() {
                break;
            }
            closest = new_closest;
        }

        closest.into_iter().map(|e| e.info).collect()
    }

    /// Refresh a random bucket by doing a lookup for a random ID in that bucket's range.
    async fn refresh_random_bucket(&self) {
        // Generate a random target using OsRng which is Send-safe
        let target_bytes: [u8; 32] = rand::random();
        let target = Hash::new(target_bytes);

        if log::log_enabled!(log::Level::Debug) {
            debug!("Refreshing routing table with lookup for random target");
        }

        self.lookup(&target).await;
    }

    /// Clean up expired pending pings and other tracking structures.
    async fn cleanup_pending_pings(&self) {
        // Collect expired entries and release lock before calling record_failure
        let expired_nodes: Vec<NodeId> = {
            let mut pending = self.pending_pings.write().await;
            let expired_keys: Vec<Hash> = pending
                .iter()
                .filter(|(_, info)| info.sent_time.elapsed() > RESPONSE_TIMEOUT)
                .map(|(k, _)| k.clone())
                .collect();

            expired_keys
                .into_iter()
                .filter_map(|key| pending.remove(&key).map(|info| info.node_id))
                .collect()
        }; // Lock is released here

        // Now record failures without holding the lock
        for node_id in expired_nodes {
            self.routing_table.record_failure(&node_id).await;
        }

        // Clean up expired pending FINDNODE requests
        {
            let mut pending = self.pending_findnodes.write().await;
            pending.retain(|_, v| v.sent_time.elapsed() <= RESPONSE_TIMEOUT);
        }

        // Clean up expired processed PONGs
        {
            let mut processed = self.processed_pongs.write().await;
            let cutoff = Instant::now() - RESPONSE_TIMEOUT;
            processed.retain(|_, time| *time > cutoff);
        }

        // Clean up expired validated endpoints
        {
            let mut validated = self.validated_endpoints.write().await;
            validated.retain(|_, v| v.validated_at.elapsed() < ENDPOINT_VALIDATION_DURATION);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_creation() {
        let config = DiscoveryConfig {
            port: 0, // Let OS assign port
            ..Default::default()
        };
        let identity = NodeIdentity::generate();

        let server = DiscoveryServer::new(config, identity).await;
        assert!(server.is_ok());
    }

    #[tokio::test]
    async fn test_server_identity() {
        let config = DiscoveryConfig {
            port: 0,
            ..Default::default()
        };
        let identity = NodeIdentity::generate();
        let node_id = identity.node_id().clone();

        let server = DiscoveryServer::new(config, identity).await.unwrap();
        assert_eq!(server.identity().node_id(), &node_id);
    }

    #[tokio::test]
    async fn test_server_start_stop() {
        let config = DiscoveryConfig {
            port: 0,
            ..Default::default()
        };
        let identity = NodeIdentity::generate();

        let server = DiscoveryServer::new(config, identity).await.unwrap();
        assert!(!server.is_running());

        // Start would spawn tasks, just test the flag
        server.running.store(true, Ordering::SeqCst);
        assert!(server.is_running());

        server.stop();
        assert!(!server.is_running());
    }
}
