//! P2P Protocol Testing - Tests connection lifecycle, encryption, peer management,
//! and message propagation without requiring real network connections.

/// Block propagation and object request tests
pub mod block_propagation;
/// ChaCha20-Poly1305 encryption and key rotation tests
pub mod encryption;
/// Connection handshake protocol tests
pub mod handshake;
/// Ping-based peer discovery tests
pub mod peer_discovery;
/// Whitelist/Graylist/Blacklist management tests
pub mod peer_list;
/// Fail-count and ban logic tests
pub mod peer_reputation;
/// Network ID, version, genesis hash validation tests
pub mod protocol_validation;
/// Concurrency and packet size limit tests
pub mod rate_limiting;

#[cfg(test)]
#[allow(missing_docs)]
pub mod mock {
    use std::collections::{HashMap, HashSet};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    // Key constants from daemon/src/config.rs
    pub const PEER_FAIL_TIME_RESET: u64 = 30 * 60; // 1800 seconds
    pub const PEER_FAIL_LIMIT: u8 = 50;
    pub const PEER_FAIL_TO_CONNECT_LIMIT: u8 = 3;
    pub const PEER_TEMP_BAN_TIME_ON_CONNECT: u64 = 60;
    pub const PEER_TEMP_BAN_TIME: u64 = 15 * 60; // 900 seconds
    pub const P2P_PING_PEER_LIST_LIMIT: usize = 16;
    pub const P2P_DEFAULT_MAX_PEERS: usize = 32;
    pub const P2P_DEFAULT_MAX_OUTGOING_PEERS: usize = 8;
    pub const P2P_PEERLIST_RETRY_AFTER: u64 = 15 * 60; // 900 seconds
    pub const PEER_TIMEOUT_REQUEST_OBJECT: u64 = 15_000;
    pub const PEER_TIMEOUT_BOOTSTRAP_STEP: u64 = 60_000;
    pub const PEER_TIMEOUT_INIT_CONNECTION: u64 = 5_000;
    pub const PEER_MAX_PACKET_SIZE: u32 = 5 * 1024 * 1024; // 5MB
    pub const ROTATE_EVERY_N_BYTES: usize = 1_073_741_824; // 1GB
    pub const TIPS_LIMIT: usize = 3;
    pub const STABLE_LIMIT: u64 = 24;
    pub const CHAIN_SYNC_REQUEST_MAX_BLOCKS: usize = 64;

    // Connection state machine
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConnectionState {
        Pending,
        KeyExchange,
        Handshake,
        Success,
    }

    // Cipher side for key rotation
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CipherSide {
        Our,
        Peer,
        Both,
    }

    impl CipherSide {
        pub fn is_our(&self) -> bool {
            matches!(self, CipherSide::Our | CipherSide::Both)
        }
        pub fn is_peer(&self) -> bool {
            matches!(self, CipherSide::Peer | CipherSide::Both)
        }
    }

    // Key verification action for DH exchange
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum KeyVerificationAction {
        Warn,
        Reject,
        #[default]
        Ignore,
    }

    // Peer list entry state
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PeerListEntryState {
        Whitelist,
        Graylist,
        Blacklist,
    }

    // Network type
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Network {
        Mainnet,
        Testnet,
        Stagenet,
        Devnet,
    }

    impl Network {
        pub fn chain_id(&self) -> u64 {
            match self {
                Network::Mainnet => 0,
                Network::Testnet => 1,
                Network::Stagenet => 2,
                Network::Devnet => 3,
            }
        }
    }

    // Packet types (IDs)
    pub const KEY_EXCHANGE_ID: u8 = 0;
    pub const HANDSHAKE_ID: u8 = 1;
    pub const TX_PROPAGATION_ID: u8 = 2;
    pub const BLOCK_PROPAGATION_ID: u8 = 3;
    pub const CHAIN_REQUEST_ID: u8 = 4;
    pub const CHAIN_RESPONSE_ID: u8 = 5;
    pub const PING_ID: u8 = 6;
    pub const OBJECT_REQUEST_ID: u8 = 7;
    pub const OBJECT_RESPONSE_ID: u8 = 8;
    pub const NOTIFY_INV_REQUEST_ID: u8 = 9;
    pub const NOTIFY_INV_RESPONSE_ID: u8 = 10;
    pub const BOOTSTRAP_CHAIN_REQUEST_ID: u8 = 11;
    pub const BOOTSTRAP_CHAIN_RESPONSE_ID: u8 = 12;
    pub const PEER_DISCONNECTED_ID: u8 = 13;

    // Mock encryption key type
    pub type EncryptionKey = [u8; 32];

    // Mock hash type
    pub type Hash = [u8; 32];

    // Mock MockConnection for testing state machine
    #[derive(Debug)]
    pub struct MockConnection {
        pub state: ConnectionState,
        pub out: bool,
        pub addr: SocketAddr,
        pub bytes_in: u64,
        pub bytes_out: u64,
        pub bytes_encrypted: u64,
        pub connected_on: u64,
        pub closed: bool,
        pub rotate_key_in: usize,
        pub rotate_key_out: usize,
        pub our_key: Option<EncryptionKey>,
        pub peer_key: Option<EncryptionKey>,
        pub our_nonce: u64,
        pub peer_nonce: u64,
    }

    impl MockConnection {
        pub fn new(addr: SocketAddr, out: bool) -> Self {
            Self {
                state: ConnectionState::Pending,
                out,
                addr,
                bytes_in: 0,
                bytes_out: 0,
                bytes_encrypted: 0,
                connected_on: 0,
                closed: false,
                rotate_key_in: 0,
                rotate_key_out: 0,
                our_key: None,
                peer_key: None,
                our_nonce: 0,
                peer_nonce: 0,
            }
        }

        pub fn set_state(&mut self, state: ConnectionState) {
            self.state = state;
        }

        pub fn is_out(&self) -> bool {
            self.out
        }

        pub fn is_closed(&self) -> bool {
            self.closed
        }

        pub fn close(&mut self) {
            self.closed = true;
        }

        // Simulate key exchange: set both keys and move to KeyExchange state
        pub fn exchange_keys(&mut self, our_key: EncryptionKey, peer_key: EncryptionKey) {
            self.our_key = Some(our_key);
            self.peer_key = Some(peer_key);
            self.state = ConnectionState::KeyExchange;
        }

        // Simulate key rotation
        pub fn rotate_key(&mut self, new_key: EncryptionKey, side: CipherSide) {
            if side.is_our() {
                self.our_key = Some(new_key);
                self.rotate_key_out += 1;
                self.bytes_encrypted = 0;
            }
            if side.is_peer() {
                self.peer_key = Some(new_key);
                self.rotate_key_in += 1;
            }
        }

        // Simulate sending bytes
        pub fn send_bytes(&mut self, count: u64) -> Result<(), &'static str> {
            if self.closed {
                return Err("Connection closed");
            }
            if self.our_key.is_none() {
                return Err("Encryption not ready");
            }
            self.bytes_out += count;
            self.bytes_encrypted += count;
            Ok(())
        }

        // Check if key rotation is needed
        pub fn needs_key_rotation(&self) -> bool {
            self.bytes_encrypted as usize >= ROTATE_EVERY_N_BYTES
        }
    }

    // Mock handshake for testing protocol negotiation
    #[derive(Debug, Clone)]
    pub struct MockHandshake {
        pub version: String,
        pub network: Network,
        pub node_tag: Option<String>,
        pub network_id: [u8; 16],
        pub peer_id: u64,
        pub local_port: u16,
        pub utc_time: u64,
        pub topoheight: u64,
        pub height: u64,
        pub pruned_topoheight: Option<u64>,
        pub top_hash: Hash,
        pub genesis_hash: Hash,
        pub cumulative_difficulty: u64,
        pub can_be_shared: bool,
        pub supports_fast_sync: bool,
    }

    impl MockHandshake {
        pub const MAX_LEN: usize = 16;

        pub fn new_valid(network: Network) -> Self {
            Self {
                version: "1.0.0".to_string(),
                network,
                node_tag: None,
                network_id: [1u8; 16],
                peer_id: 12345,
                local_port: 8080,
                utc_time: 1700000000,
                topoheight: 100,
                height: 50,
                pruned_topoheight: None,
                top_hash: [0xAA; 32],
                genesis_hash: [0xBB; 32],
                cumulative_difficulty: 1000,
                can_be_shared: true,
                supports_fast_sync: true,
            }
        }

        pub fn validate(&self) -> Result<(), &'static str> {
            if self.version.is_empty() || self.version.len() > Self::MAX_LEN {
                return Err("Invalid version length");
            }
            if let Some(ref tag) = self.node_tag {
                if tag.is_empty() || tag.len() > Self::MAX_LEN {
                    return Err("Invalid node_tag length");
                }
            }
            if let Some(pruned) = self.pruned_topoheight {
                if pruned == 0 {
                    return Err("Pruned topoheight cannot be 0");
                }
                if pruned > self.topoheight {
                    return Err("Pruned topoheight exceeds topoheight");
                }
            }
            Ok(())
        }
    }

    // MockPing for testing peer discovery
    #[derive(Debug, Clone)]
    pub struct MockPing {
        pub top_hash: Hash,
        pub topoheight: u64,
        pub height: u64,
        pub pruned_topoheight: Option<u64>,
        pub cumulative_difficulty: u64,
        pub peer_list: Vec<SocketAddr>,
    }

    impl MockPing {
        pub fn new(height: u64, topoheight: u64) -> Self {
            Self {
                top_hash: [0xCC; 32],
                topoheight,
                height,
                pruned_topoheight: None,
                cumulative_difficulty: height * 100,
                peer_list: Vec::new(),
            }
        }

        pub fn add_peer(&mut self, addr: SocketAddr) -> bool {
            if self.peer_list.len() >= P2P_PING_PEER_LIST_LIMIT {
                return false;
            }
            if self.peer_list.contains(&addr) {
                return false;
            }
            self.peer_list.push(addr);
            true
        }

        pub fn validate(&self) -> Result<(), &'static str> {
            if self.peer_list.len() > P2P_PING_PEER_LIST_LIMIT {
                return Err("Peer list exceeds limit");
            }
            if let Some(pruned) = self.pruned_topoheight {
                if pruned == 0 {
                    return Err("Pruned topoheight cannot be 0");
                }
                if pruned > self.topoheight {
                    return Err("Pruned topoheight exceeds topoheight");
                }
            }
            // Check for duplicates
            let unique: HashSet<_> = self.peer_list.iter().collect();
            if unique.len() != self.peer_list.len() {
                return Err("Duplicate peers in list");
            }
            Ok(())
        }
    }

    // MockPeerListEntry for testing peer reputation
    #[derive(Debug, Clone)]
    pub struct MockPeerListEntry {
        pub addr: SocketAddr,
        pub first_seen: Option<u64>,
        pub last_seen: Option<u64>,
        pub last_connection_try: Option<u64>,
        pub out_success: bool,
        pub fail_count: u8,
        pub local_port: Option<u16>,
        pub temp_ban_until: Option<u64>,
        pub state: PeerListEntryState,
    }

    impl MockPeerListEntry {
        pub fn new_graylist(addr: SocketAddr) -> Self {
            Self {
                addr,
                first_seen: None,
                last_seen: None,
                last_connection_try: None,
                out_success: false,
                fail_count: 0,
                local_port: None,
                temp_ban_until: None,
                state: PeerListEntryState::Graylist,
            }
        }

        pub fn is_temp_banned(&self, now: u64) -> bool {
            self.temp_ban_until.is_some_and(|until| until > now)
        }

        pub fn increment_fail_count(&mut self, now: u64, temp_ban: bool) {
            if self.state == PeerListEntryState::Whitelist {
                return; // Whitelisted peers don't get fail count incremented
            }
            self.fail_count = self.fail_count.saturating_add(1);
            if temp_ban && self.fail_count.is_multiple_of(PEER_FAIL_TO_CONNECT_LIMIT) {
                self.temp_ban_until = Some(now + PEER_TEMP_BAN_TIME_ON_CONNECT);
            }
        }

        pub fn should_disconnect(&self) -> bool {
            self.fail_count >= PEER_FAIL_LIMIT
        }

        pub fn reset_fail_count(&mut self, now: u64) {
            if let Some(last_seen) = self.last_seen {
                if last_seen + PEER_FAIL_TIME_RESET < now {
                    self.fail_count = 0;
                }
            }
        }

        pub fn can_retry(&self, now: u64) -> bool {
            if self.is_temp_banned(now) {
                return false;
            }
            match self.last_connection_try {
                None => true,
                Some(last_try) => {
                    let delay = self.fail_count as u64 * P2P_PEERLIST_RETRY_AFTER;
                    now >= last_try + delay
                }
            }
        }
    }

    // MockPeerList for testing list management
    #[derive(Debug)]
    pub struct MockPeerList {
        pub peers: HashMap<IpAddr, MockPeerListEntry>,
        pub max_peers: usize,
    }

    impl Default for MockPeerList {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockPeerList {
        pub fn new() -> Self {
            Self {
                peers: HashMap::new(),
                max_peers: P2P_DEFAULT_MAX_PEERS,
            }
        }

        pub fn add_peer(&mut self, entry: MockPeerListEntry) -> Result<(), &'static str> {
            if self.peers.len() >= self.max_peers {
                return Err("Peer list full");
            }
            let ip = entry.addr.ip();
            if self.peers.contains_key(&ip) {
                return Err("Peer already exists");
            }
            self.peers.insert(ip, entry);
            Ok(())
        }

        pub fn remove_peer(&mut self, ip: &IpAddr) -> Option<MockPeerListEntry> {
            self.peers.remove(ip)
        }

        pub fn whitelist(&mut self, ip: &IpAddr) -> Result<(), &'static str> {
            match self.peers.get_mut(ip) {
                Some(entry) => {
                    entry.state = PeerListEntryState::Whitelist;
                    Ok(())
                }
                None => Err("Peer not found"),
            }
        }

        pub fn blacklist(&mut self, ip: &IpAddr) -> Result<(), &'static str> {
            match self.peers.get_mut(ip) {
                Some(entry) => {
                    entry.state = PeerListEntryState::Blacklist;
                    Ok(())
                }
                None => Err("Peer not found"),
            }
        }

        pub fn is_allowed(&self, ip: &IpAddr, now: u64) -> bool {
            match self.peers.get(ip) {
                None => true, // Unknown peers are allowed
                Some(entry) => {
                    if entry.state == PeerListEntryState::Blacklist {
                        return false;
                    }
                    !entry.is_temp_banned(now)
                }
            }
        }

        pub fn find_peer_to_connect(&self, now: u64) -> Option<SocketAddr> {
            // First: try whitelist peers with out_success
            for entry in self.peers.values() {
                if entry.state == PeerListEntryState::Whitelist
                    && entry.out_success
                    && entry.can_retry(now)
                {
                    return Some(entry.addr);
                }
            }
            // Then: try graylist peers
            for entry in self.peers.values() {
                if entry.state == PeerListEntryState::Graylist && entry.can_retry(now) {
                    return Some(entry.addr);
                }
            }
            None
        }

        pub fn get_whitelist(&self) -> Vec<&MockPeerListEntry> {
            self.peers
                .values()
                .filter(|e| e.state == PeerListEntryState::Whitelist)
                .collect()
        }

        pub fn get_graylist(&self) -> Vec<&MockPeerListEntry> {
            self.peers
                .values()
                .filter(|e| e.state == PeerListEntryState::Graylist)
                .collect()
        }

        pub fn get_blacklist(&self) -> Vec<&MockPeerListEntry> {
            self.peers
                .values()
                .filter(|e| e.state == PeerListEntryState::Blacklist)
                .collect()
        }
    }

    // Additional constants for protocol tests
    pub const PEER_OBJECTS_CONCURRENCY: usize = 64;
    pub const PEER_TIMEOUT_INIT_OUTGOING_CONNECTION: u64 = 30_000;
    pub const PEER_SEND_BYTES_TIMEOUT: u64 = 3_000;
    pub const MAX_VALID_PACKET_ID: u8 = PEER_DISCONNECTED_ID;

    /// Determine if a packet ID is order-dependent.
    /// Order-independent: Ping, ObjectRequest, ObjectResponse,
    ///     ChainRequest, ChainResponse, NotifyInventoryRequest, PeerDisconnected
    /// Order-dependent: everything else
    pub fn is_packet_order_dependent(id: u8) -> bool {
        !matches!(
            id,
            PING_ID
                | OBJECT_REQUEST_ID
                | OBJECT_RESPONSE_ID
                | CHAIN_REQUEST_ID
                | CHAIN_RESPONSE_ID
                | NOTIFY_INV_REQUEST_ID
                | PEER_DISCONNECTED_ID
        )
    }

    /// Mock block propagation tracker for testing header-only block
    /// announcements and object request flows.
    #[derive(Debug, Clone)]
    pub struct MockBlockPropagation {
        pub announced_blocks: Vec<Hash>,
        pub requested_objects: Vec<Hash>,
        pub received_blocks: Vec<Hash>,
        pub pending_requests: HashMap<Hash, bool>, // hash -> fulfilled
        pub announcement_sources: HashMap<Hash, Vec<u64>>, // hash -> list of peer_ids
        pub announcement_order: Vec<Hash>,
        pub connected_peers: Vec<u64>,
        pub priority_peers: Vec<u64>,
    }

    impl Default for MockBlockPropagation {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockBlockPropagation {
        pub fn new() -> Self {
            Self {
                announced_blocks: Vec::new(),
                requested_objects: Vec::new(),
                received_blocks: Vec::new(),
                pending_requests: HashMap::new(),
                announcement_sources: HashMap::new(),
                announcement_order: Vec::new(),
                connected_peers: Vec::new(),
                priority_peers: Vec::new(),
            }
        }

        /// Announce a block hash from a given peer.
        /// Returns true if this is a new announcement, false if duplicate.
        pub fn announce_block(&mut self, hash: Hash, from_peer: u64) -> bool {
            if self.announced_blocks.contains(&hash) {
                self.announcement_sources
                    .entry(hash)
                    .or_default()
                    .push(from_peer);
                return false;
            }
            self.announced_blocks.push(hash);
            self.announcement_order.push(hash);
            self.announcement_sources
                .entry(hash)
                .or_default()
                .push(from_peer);
            true
        }

        /// Request an object by hash. Fails if hash was never announced.
        pub fn request_object(&mut self, hash: Hash) -> Result<(), &'static str> {
            if !self.announced_blocks.contains(&hash) {
                return Err("Unknown block hash");
            }
            self.requested_objects.push(hash);
            self.pending_requests.insert(hash, false);
            Ok(())
        }

        /// Receive a block response. Validates response hash matches request hash.
        pub fn receive_block(
            &mut self,
            request_hash: Hash,
            response_hash: Hash,
        ) -> Result<(), &'static str> {
            if request_hash != response_hash {
                return Err("Response hash does not match request hash");
            }
            if let Some(fulfilled) = self.pending_requests.get_mut(&request_hash) {
                *fulfilled = true;
                self.received_blocks.push(request_hash);
                Ok(())
            } else {
                Err("No pending request for this hash")
            }
        }

        /// Clean up fulfilled requests from pending map.
        pub fn cleanup_fulfilled(&mut self) {
            self.pending_requests.retain(|_, fulfilled| !*fulfilled);
        }

        /// Get the number of currently pending (unfulfilled) requests.
        pub fn pending_count(&self) -> usize {
            self.pending_requests
                .values()
                .filter(|fulfilled| !**fulfilled)
                .count()
        }

        /// Get peers that should receive propagation for a block.
        /// Excludes the sender peer from the list.
        pub fn propagation_targets(&self, sender: u64) -> Vec<u64> {
            self.connected_peers
                .iter()
                .filter(|p| **p != sender)
                .copied()
                .collect()
        }

        /// Check if a peer is a priority peer.
        pub fn is_priority_peer(&self, peer: u64) -> bool {
            self.priority_peers.contains(&peer)
        }

        /// Get pending requests sorted with priority peer blocks first.
        pub fn get_priority_ordered_requests(&self) -> Vec<Hash> {
            let mut priority_hashes: Vec<Hash> = Vec::new();
            let mut normal_hashes: Vec<Hash> = Vec::new();

            for hash in &self.announcement_order {
                if self.pending_requests.get(hash).copied() == Some(false) {
                    if let Some(sources) = self.announcement_sources.get(hash) {
                        if sources.iter().any(|p| self.priority_peers.contains(p)) {
                            priority_hashes.push(*hash);
                        } else {
                            normal_hashes.push(*hash);
                        }
                    }
                }
            }

            priority_hashes.extend(normal_hashes);
            priority_hashes
        }
    }

    /// Mock rate limiter for concurrency and packet size tests.
    #[derive(Debug)]
    pub struct MockRateLimiter {
        pub concurrent_requests: usize,
        pub max_concurrent: usize,
        pub bytes_sent: u64,
        pub max_packet_size: u32,
        pub request_timeouts: HashMap<u64, u64>, // request_id -> deadline_ms
    }

    impl Default for MockRateLimiter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockRateLimiter {
        pub fn new() -> Self {
            Self {
                concurrent_requests: 0,
                max_concurrent: PEER_OBJECTS_CONCURRENCY,
                bytes_sent: 0,
                max_packet_size: PEER_MAX_PACKET_SIZE,
                request_timeouts: HashMap::new(),
            }
        }

        /// Try to acquire a concurrency slot. Returns true if acquired.
        pub fn try_acquire(&mut self) -> bool {
            if self.concurrent_requests < self.max_concurrent {
                self.concurrent_requests = self.concurrent_requests.saturating_add(1);
                true
            } else {
                false
            }
        }

        /// Release a concurrency slot.
        pub fn release(&mut self) {
            self.concurrent_requests = self.concurrent_requests.saturating_sub(1);
        }

        /// Validate packet size (must be > 0 and <= max).
        pub fn validate_packet_size(&self, size: u32) -> Result<(), &'static str> {
            if size == 0 {
                return Err("Packet size is zero");
            }
            if size > self.max_packet_size {
                return Err("Packet size exceeds maximum");
            }
            Ok(())
        }

        /// Register a timeout for a request.
        pub fn register_timeout(&mut self, request_id: u64, deadline_ms: u64) {
            self.request_timeouts.insert(request_id, deadline_ms);
        }

        /// Check if a request has timed out.
        pub fn is_timed_out(&self, request_id: u64, current_time_ms: u64) -> bool {
            self.request_timeouts
                .get(&request_id)
                .map(|deadline| current_time_ms >= *deadline)
                .unwrap_or(false)
        }

        /// Remove timeout tracking for a completed request.
        pub fn clear_timeout(&mut self, request_id: u64) {
            self.request_timeouts.remove(&request_id);
        }

        /// Add bytes to the sent counter. Returns true if rotation threshold reached.
        pub fn add_bytes_sent(&mut self, bytes: u64) -> bool {
            self.bytes_sent = self.bytes_sent.saturating_add(bytes);
            self.bytes_sent >= ROTATE_EVERY_N_BYTES as u64
        }

        /// Reset the bytes counter after key rotation.
        pub fn reset_bytes_counter(&mut self) {
            self.bytes_sent = 0;
        }
    }

    // Helper to create test socket addresses
    pub fn make_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, port as u8)), port)
    }

    pub fn make_addr_ip(a: u8, b: u8, c: u8, d: u8, port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(a, b, c, d)), port)
    }

    /// Helper to create a hash filled with a single byte value.
    pub fn make_hash(b: u8) -> Hash {
        [b; 32]
    }
}
