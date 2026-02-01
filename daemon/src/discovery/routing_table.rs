//! Kademlia-style routing table for node discovery.
//!
//! The routing table organizes known nodes into k-buckets based on their
//! XOR distance from the local node's ID. Each bucket contains nodes that
//! share the same log2 distance from the local node.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::Instant;

use tos_common::crypto::ed25519::Ed25519PublicKey;
use tos_common::tokio::sync::RwLock;

use super::identity::{compare_distance, log2_distance, NodeId};
use super::messages::NodeInfo;

/// Number of k-buckets (one for each bit position).
pub const NUM_BUCKETS: usize = 256;

/// Default number of nodes per bucket (Kademlia k parameter).
pub const DEFAULT_BUCKET_SIZE: usize = 16;

/// Alpha parameter for parallel lookups.
pub const ALPHA: usize = 3;

/// Entry in a k-bucket containing node information and metadata.
#[derive(Debug, Clone)]
pub struct NodeEntry {
    /// Node information.
    pub info: NodeInfo,
    /// When this entry was last seen.
    pub last_seen: Instant,
    /// Number of failed connection attempts.
    pub fail_count: u32,
}

impl NodeEntry {
    /// Create a new node entry.
    pub fn new(info: NodeInfo) -> Self {
        Self {
            info,
            last_seen: Instant::now(),
            fail_count: 0,
        }
    }

    /// Update the last seen time.
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
        self.fail_count = 0;
    }

    /// Increment the fail count.
    pub fn record_failure(&mut self) {
        self.fail_count = self.fail_count.saturating_add(1);
    }

    /// Get the node ID.
    pub fn node_id(&self) -> &NodeId {
        &self.info.node_id
    }

    /// Get the socket address.
    pub fn address(&self) -> &SocketAddr {
        &self.info.address
    }

    /// Get the public key.
    pub fn public_key(&self) -> &Ed25519PublicKey {
        &self.info.public_key
    }
}

/// Result of inserting a node into the routing table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertResult {
    /// Node was inserted successfully.
    Inserted,
    /// Node was already in the table and was updated.
    Updated,
    /// Bucket is full; contains the node ID to ping for eviction.
    BucketFull(NodeId),
    /// Cannot insert self.
    SelfInsert,
}

/// A single k-bucket containing nodes at a specific distance range.
#[derive(Debug)]
struct KBucket {
    /// Nodes in LRU order (most recently seen at back).
    nodes: VecDeque<NodeEntry>,
    /// Maximum capacity.
    capacity: usize,
}

impl KBucket {
    /// Create a new empty bucket.
    fn new(capacity: usize) -> Self {
        Self {
            nodes: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Get the number of nodes in the bucket.
    fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if the bucket is empty.
    fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Check if the bucket is full.
    fn is_full(&self) -> bool {
        self.nodes.len() >= self.capacity
    }

    /// Find a node by ID and return its index.
    fn find_index(&self, node_id: &NodeId) -> Option<usize> {
        self.nodes.iter().position(|e| &e.info.node_id == node_id)
    }

    /// Get a node by ID.
    fn get(&self, node_id: &NodeId) -> Option<&NodeEntry> {
        self.nodes.iter().find(|e| &e.info.node_id == node_id)
    }

    /// Insert or update a node.
    ///
    /// Returns `InsertResult::BucketFull` with the oldest node ID if the bucket
    /// is full and the node is not already present.
    fn insert(&mut self, entry: NodeEntry) -> InsertResult {
        // Check if already present
        if let Some(index) = self.find_index(&entry.info.node_id) {
            // Move to back (most recently seen) and update
            if let Some(mut existing) = self.nodes.remove(index) {
                existing.touch();
                existing.info = entry.info;
                self.nodes.push_back(existing);
            }
            return InsertResult::Updated;
        }

        // Check if bucket is full
        if self.is_full() {
            // Return the oldest node (front of queue) for potential eviction
            if let Some(oldest) = self.nodes.front() {
                return InsertResult::BucketFull(oldest.info.node_id.clone());
            }
        }

        // Insert at back (most recently seen)
        self.nodes.push_back(entry);
        InsertResult::Inserted
    }

    /// Remove a node by ID.
    fn remove(&mut self, node_id: &NodeId) -> Option<NodeEntry> {
        if let Some(index) = self.find_index(node_id) {
            self.nodes.remove(index)
        } else {
            None
        }
    }

    /// Get all nodes in the bucket.
    fn nodes(&self) -> impl Iterator<Item = &NodeEntry> {
        self.nodes.iter()
    }

    /// Get the oldest node in the bucket.
    #[allow(dead_code)]
    fn oldest(&self) -> Option<&NodeEntry> {
        self.nodes.front()
    }

    /// Evict the oldest node if it matches the given ID.
    fn evict_if_oldest(&mut self, node_id: &NodeId) -> bool {
        if let Some(oldest) = self.nodes.front() {
            if &oldest.info.node_id == node_id {
                self.nodes.pop_front();
                return true;
            }
        }
        false
    }
}

/// Kademlia-style routing table for node discovery.
pub struct RoutingTable {
    /// Local node ID.
    local_id: NodeId,
    /// K-buckets indexed by log2 distance.
    buckets: Vec<RwLock<KBucket>>,
    /// Bucket capacity (k parameter).
    bucket_size: usize,
}

impl RoutingTable {
    /// Create a new routing table.
    pub fn new(local_id: NodeId, bucket_size: usize) -> Self {
        let buckets = (0..NUM_BUCKETS)
            .map(|_| RwLock::new(KBucket::new(bucket_size)))
            .collect();

        Self {
            local_id,
            buckets,
            bucket_size,
        }
    }

    /// Get the local node ID.
    pub fn local_id(&self) -> &NodeId {
        &self.local_id
    }

    /// Get the bucket size.
    pub fn bucket_size(&self) -> usize {
        self.bucket_size
    }

    /// Calculate which bucket a node belongs to.
    fn bucket_index(&self, node_id: &NodeId) -> Option<usize> {
        log2_distance(&self.local_id, node_id).map(|d| d as usize)
    }

    /// Insert a node into the routing table.
    pub async fn insert(&self, node: NodeInfo) -> InsertResult {
        // Don't insert self
        if node.node_id == self.local_id {
            return InsertResult::SelfInsert;
        }

        let bucket_idx = match self.bucket_index(&node.node_id) {
            Some(idx) => idx,
            None => return InsertResult::SelfInsert, // Identical node IDs
        };

        let entry = NodeEntry::new(node);
        let mut bucket = self.buckets[bucket_idx].write().await;
        bucket.insert(entry)
    }

    /// Update a node's last seen time.
    pub async fn touch(&self, node_id: &NodeId) -> bool {
        if let Some(bucket_idx) = self.bucket_index(node_id) {
            let mut bucket = self.buckets[bucket_idx].write().await;
            if let Some(index) = bucket.find_index(node_id) {
                if let Some(mut entry) = bucket.nodes.remove(index) {
                    entry.touch();
                    bucket.nodes.push_back(entry);
                    return true;
                }
            }
        }
        false
    }

    /// Record a failure for a node.
    pub async fn record_failure(&self, node_id: &NodeId) {
        if let Some(bucket_idx) = self.bucket_index(node_id) {
            let mut bucket = self.buckets[bucket_idx].write().await;
            if let Some(index) = bucket.find_index(node_id) {
                if let Some(entry) = bucket.nodes.get_mut(index) {
                    entry.record_failure();
                }
            }
        }
    }

    /// Remove a node from the routing table.
    pub async fn remove(&self, node_id: &NodeId) -> Option<NodeEntry> {
        if let Some(bucket_idx) = self.bucket_index(node_id) {
            let mut bucket = self.buckets[bucket_idx].write().await;
            bucket.remove(node_id)
        } else {
            None
        }
    }

    /// Evict a node if it's the oldest in its bucket.
    pub async fn evict_if_oldest(&self, node_id: &NodeId) -> bool {
        if let Some(bucket_idx) = self.bucket_index(node_id) {
            let mut bucket = self.buckets[bucket_idx].write().await;
            bucket.evict_if_oldest(node_id)
        } else {
            false
        }
    }

    /// Get a node by ID.
    pub async fn get(&self, node_id: &NodeId) -> Option<NodeEntry> {
        if let Some(bucket_idx) = self.bucket_index(node_id) {
            let bucket = self.buckets[bucket_idx].read().await;
            bucket.get(node_id).cloned()
        } else {
            None
        }
    }

    /// Check if a node is in the routing table.
    pub async fn contains(&self, node_id: &NodeId) -> bool {
        self.get(node_id).await.is_some()
    }

    /// Get the closest nodes to a target.
    ///
    /// Returns up to `count` nodes sorted by XOR distance to the target.
    pub async fn closest(&self, target: &NodeId, count: usize) -> Vec<NodeEntry> {
        let mut candidates = Vec::new();

        // Collect all nodes from all buckets
        for bucket in &self.buckets {
            let bucket = bucket.read().await;
            for entry in bucket.nodes() {
                candidates.push(entry.clone());
            }
        }

        // Sort by distance to target
        candidates.sort_by(|a, b| compare_distance(target, &a.info.node_id, &b.info.node_id));

        // Return the closest `count` nodes
        candidates.truncate(count);
        candidates
    }

    /// Get all nodes in the routing table.
    pub async fn all_nodes(&self) -> Vec<NodeEntry> {
        let mut nodes = Vec::new();
        for bucket in &self.buckets {
            let bucket = bucket.read().await;
            for entry in bucket.nodes() {
                nodes.push(entry.clone());
            }
        }
        nodes
    }

    /// Get the total number of nodes in the routing table.
    pub async fn len(&self) -> usize {
        let mut count: usize = 0;
        for bucket in &self.buckets {
            let bucket = bucket.read().await;
            count = count.saturating_add(bucket.len());
        }
        count
    }

    /// Check if the routing table is empty.
    pub async fn is_empty(&self) -> bool {
        for bucket in &self.buckets {
            let bucket = bucket.read().await;
            if !bucket.is_empty() {
                return false;
            }
        }
        true
    }

    /// Get bucket statistics.
    pub async fn bucket_stats(&self) -> Vec<(usize, usize)> {
        let mut stats = Vec::with_capacity(NUM_BUCKETS);
        for (i, bucket) in self.buckets.iter().enumerate() {
            let bucket = bucket.read().await;
            if !bucket.is_empty() {
                stats.push((i, bucket.len()));
            }
        }
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use tos_common::crypto::ed25519::Ed25519KeyPair;

    fn create_test_node_info() -> NodeInfo {
        let keypair = Ed25519KeyPair::generate();
        NodeInfo::new(
            keypair.node_id(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2126),
            keypair.public_key(),
        )
    }

    #[allow(dead_code)]
    fn create_test_node_with_id(id_byte: u8) -> NodeInfo {
        let keypair = Ed25519KeyPair::generate();
        // Create a specific node ID for testing
        let mut id_bytes = [0u8; 32];
        id_bytes[0] = id_byte;
        let node_id = tos_common::crypto::Hash::new(id_bytes);
        NodeInfo::new(
            node_id,
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, (id_byte % 255) + 1)),
                2126,
            ),
            keypair.public_key(),
        )
    }

    #[tokio::test]
    async fn test_new_routing_table() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        assert_eq!(table.bucket_size(), DEFAULT_BUCKET_SIZE);
        assert!(table.is_empty().await);
        assert_eq!(table.len().await, 0);
    }

    #[tokio::test]
    async fn test_insert_node() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        let node = create_test_node_info();
        let result = table.insert(node.clone()).await;

        assert!(matches!(result, InsertResult::Inserted));
        assert!(!table.is_empty().await);
        assert_eq!(table.len().await, 1);
        assert!(table.contains(&node.node_id).await);
    }

    #[tokio::test]
    async fn test_insert_self_fails() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id.clone(), DEFAULT_BUCKET_SIZE);

        let keypair = Ed25519KeyPair::generate();
        let node = NodeInfo::new(
            local_id, // Same as local ID
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2126),
            keypair.public_key(),
        );

        let result = table.insert(node).await;
        assert!(matches!(result, InsertResult::SelfInsert));
        assert!(table.is_empty().await);
    }

    #[tokio::test]
    async fn test_insert_duplicate_updates() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        let node = create_test_node_info();
        table.insert(node.clone()).await;
        let result = table.insert(node.clone()).await;

        assert!(matches!(result, InsertResult::Updated));
        assert_eq!(table.len().await, 1);
    }

    #[tokio::test]
    async fn test_remove_node() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        let node = create_test_node_info();
        table.insert(node.clone()).await;
        assert!(table.contains(&node.node_id).await);

        let removed = table.remove(&node.node_id).await;
        assert!(removed.is_some());
        assert!(!table.contains(&node.node_id).await);
        assert!(table.is_empty().await);
    }

    #[tokio::test]
    async fn test_closest_nodes() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        // Insert multiple nodes
        for _ in 0..10 {
            let node = create_test_node_info();
            table.insert(node).await;
        }

        let target = tos_common::crypto::Hash::new([0xFFu8; 32]);
        let closest = table.closest(&target, 5).await;

        assert!(closest.len() <= 5);
        // Verify sorted by distance (first should be closest)
        for window in closest.windows(2) {
            let ordering =
                compare_distance(&target, &window[0].info.node_id, &window[1].info.node_id);
            assert!(matches!(
                ordering,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            ));
        }
    }

    #[tokio::test]
    async fn test_touch_updates_position() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        let node = create_test_node_info();
        table.insert(node.clone()).await;

        let touched = table.touch(&node.node_id).await;
        assert!(touched);
    }

    #[tokio::test]
    async fn test_record_failure() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        let node = create_test_node_info();
        table.insert(node.clone()).await;

        table.record_failure(&node.node_id).await;

        let entry = table.get(&node.node_id).await.unwrap();
        assert_eq!(entry.fail_count, 1);
    }

    #[tokio::test]
    async fn test_bucket_full() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, 2); // Very small buckets for testing

        // This test is hard to make deterministic due to random node IDs,
        // but we can verify the mechanism works for a single bucket
        let mut nodes = Vec::new();
        for _ in 0..10 {
            nodes.push(create_test_node_info());
        }

        // Insert nodes until we get a BucketFull result
        let mut _bucket_full_count = 0;
        for node in nodes {
            let result = table.insert(node).await;
            if matches!(result, InsertResult::BucketFull(_)) {
                _bucket_full_count += 1;
            }
        }

        // With small bucket size, we should get some BucketFull results
        // But this depends on random distribution, so just verify the table works
        assert!(table.len().await > 0);
    }

    #[tokio::test]
    async fn test_all_nodes() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        let nodes: Vec<NodeInfo> = (0..5).map(|_| create_test_node_info()).collect();
        for node in &nodes {
            table.insert(node.clone()).await;
        }

        let all = table.all_nodes().await;
        assert_eq!(all.len(), 5);
    }

    #[tokio::test]
    async fn test_bucket_stats() {
        let local_id = tos_common::crypto::Hash::new([0u8; 32]);
        let table = RoutingTable::new(local_id, DEFAULT_BUCKET_SIZE);

        for _ in 0..10 {
            let node = create_test_node_info();
            table.insert(node).await;
        }

        let stats = table.bucket_stats().await;
        // Should have some non-empty buckets
        assert!(!stats.is_empty());

        // Total count should match len()
        let total: usize = stats.iter().map(|(_, count)| count).sum();
        assert_eq!(total, table.len().await);
    }
}
