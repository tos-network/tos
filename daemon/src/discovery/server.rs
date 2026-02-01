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
    /// Pending PING requests (seq -> (target_node_id, sent_time)).
    pending_pings: RwLock<HashMap<u64, (NodeId, Instant)>>,
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
                // Get the source public key from our routing table or pending pings
                if let Some(entry) = self.routing_table.get(&pong.source.node_id).await {
                    packet.verify(&entry.info.public_key)?;
                }
                self.handle_pong(pong, from).await
            }
            Message::FindNode(find_node) => {
                // Find the sender in our routing table
                if let Some(entry) = self.routing_table.get(&find_node.target).await {
                    packet.verify(&entry.info.public_key)?;
                }
                self.handle_find_node(find_node, from).await
            }
            Message::Neighbors(neighbors) => self.handle_neighbors(neighbors, from).await,
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

        // Add/update sender in routing table
        let node_info = NodeInfo::new(ping.source.node_id.clone(), from, ping.source.public_key);
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

        // Update routing table
        let node_info = NodeInfo::new(pong.source.node_id.clone(), from, pong.source.public_key);
        self.routing_table.insert(node_info).await;

        // Update our external address if provided
        // The PONG contains the address they see us as
        let mut external = self.external_address.write().await;
        if external.is_none() {
            *external = Some(from);
        }

        // Remove from pending pings
        // Note: We'd need to match by ping_hash, but for simplicity we just touch the node
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

        // Find closest nodes to target
        let closest = self
            .routing_table
            .closest(&find_node.target, MAX_NEIGHBORS)
            .await;
        let nodes: Vec<NodeInfo> = closest.into_iter().map(|e| e.info).collect();

        // Send NEIGHBORS response
        let neighbors = Neighbors::new(nodes);
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

        // Add all nodes to routing table
        for node in &neighbors.nodes {
            // Don't add ourselves
            if node.node_id == *self.identity.node_id() {
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

        // Track pending ping
        {
            let mut pending = self.pending_pings.write().await;
            // Clean up if too many pending
            if pending.len() >= MAX_PENDING_PINGS {
                let old_keys: Vec<u64> = pending
                    .iter()
                    .filter(|(_, (_, time))| time.elapsed() > RESPONSE_TIMEOUT)
                    .map(|(k, _)| *k)
                    .collect();
                for key in old_keys {
                    pending.remove(&key);
                }
            }
            pending.insert(seq, (node_id.clone(), Instant::now()));
        }

        self.send_message(Message::Ping(ping), address).await
    }

    /// Send a FINDNODE request.
    pub async fn find_node(&self, target: &NodeId, address: SocketAddr) -> DiscoveryResult<()> {
        let find_node = FindNode::new(target.clone());
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
        let mut pending = self.pending_pings.write().await;
        let expired: Vec<u64> = pending
            .iter()
            .filter(|(_, (_, time))| time.elapsed() > RESPONSE_TIMEOUT)
            .map(|(k, _)| *k)
            .collect();

        for key in expired {
            if let Some((node_id, _)) = pending.remove(&key) {
                self.routing_table.record_failure(&node_id).await;
            }
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
