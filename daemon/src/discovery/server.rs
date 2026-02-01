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
use tos_common::tokio::sync::RwLock;
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

/// Pending ping information.
struct PendingPing {
    /// Target node ID.
    node_id: NodeId,
    /// Target address (for future validation).
    #[allow(dead_code)]
    address: SocketAddr,
    /// Time the ping was sent.
    sent_time: Instant,
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
                    let data = buf[..len].to_vec();
                    let server = Arc::clone(&self);
                    tos_common::tokio::spawn_task("discovery-handle", async move {
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

        // Send PONG
        let pong_hash = packet.hash();
        let source = NodeInfo::new(
            self.identity.node_id().clone(),
            from, // Use the address they see us as
            self.identity.public_key(),
        );
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

        // Validate PONG matches a pending PING
        let pending_info = {
            let mut pending = self.pending_pings.write().await;
            pending.remove(&pong.ping_hash)
        };

        let is_valid_response = match &pending_info {
            Some(info) => {
                // Verify the PONG is from the expected node
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

        // Only update external address for valid responses to our PINGs
        if is_valid_response {
            let mut external = self.external_address.write().await;
            if external.is_none() {
                *external = Some(from);
                if log::log_enabled!(log::Level::Info) {
                    info!("Discovered external address: {}", from);
                }
            }
        }

        // Touch the node in routing table
        self.routing_table.touch(&pong.source.node_id).await;

        Ok(())
    }

    /// Handle a FINDNODE message.
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

        // Send NEIGHBORS response
        let source = NodeInfo::new(
            self.identity.node_id().clone(),
            from, // Will be updated when we know our external address
            self.identity.public_key(),
        );
        let neighbors = Neighbors::new(source, nodes);
        self.send_message(Message::Neighbors(neighbors), from)
            .await?;

        Ok(())
    }

    /// Handle a NEIGHBORS message.
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

        // Update source in routing table
        let source_info = NodeInfo::new(
            neighbors.source.node_id.clone(),
            from,
            neighbors.source.public_key.clone(),
        );
        self.routing_table.insert(source_info).await;

        // Add all nodes to routing table
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

            let result = self.routing_table.insert(node.clone()).await;
            if matches!(result, InsertResult::Inserted) {
                // Ping new nodes to verify they're alive
                if let Err(e) = self.ping_node(&node.node_id, node.address).await {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Failed to ping new node {}: {}", node.address, e);
                    }
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

        let source = NodeInfo::new(
            self.identity.node_id().clone(),
            address, // This will be updated when we know our external address
            self.identity.public_key(),
        );

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
            // Clean up if too many pending
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
    pub async fn find_node(&self, target: &NodeId, address: SocketAddr) -> DiscoveryResult<()> {
        let source = NodeInfo::new(
            self.identity.node_id().clone(),
            address, // Will be updated when we know our external address
            self.identity.public_key(),
        );
        let find_node = FindNode::new(source, target.clone());
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
                if let Err(e) = self.find_node(target, entry.info.address).await {
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

    /// Clean up expired pending pings.
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
