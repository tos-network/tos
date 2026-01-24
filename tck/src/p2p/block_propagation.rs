// Tests for header-only block propagation and object request flow.
// Validates the announce -> request -> receive lifecycle and edge cases.

#[cfg(test)]
mod tests {
    use super::super::mock::*;

    // =========================================================================
    // Test 1: Block announcement creates propagation entry
    // =========================================================================
    #[test]
    fn test_block_announcement_creates_entry() {
        let mut prop = MockBlockPropagation::new();
        let hash = make_hash(0x01);
        let peer_id = 1u64;

        let is_new = prop.announce_block(hash, peer_id);

        assert!(is_new);
        assert_eq!(prop.announced_blocks.len(), 1);
        assert_eq!(prop.announced_blocks[0], hash);
        assert!(prop.announcement_sources.contains_key(&hash));
        assert_eq!(prop.announcement_sources[&hash], vec![peer_id]);
    }

    // =========================================================================
    // Test 2: Object request follows block announcement
    // =========================================================================
    #[test]
    fn test_object_request_follows_announcement() {
        let mut prop = MockBlockPropagation::new();
        let hash = make_hash(0x02);
        let peer_id = 1u64;

        prop.announce_block(hash, peer_id);
        let result = prop.request_object(hash);

        assert!(result.is_ok());
        assert_eq!(prop.requested_objects.len(), 1);
        assert_eq!(prop.requested_objects[0], hash);
        assert_eq!(prop.pending_requests.get(&hash), Some(&false));
    }

    // =========================================================================
    // Test 3: Received block fulfills pending request
    // =========================================================================
    #[test]
    fn test_received_block_fulfills_request() {
        let mut prop = MockBlockPropagation::new();
        let hash = make_hash(0x03);
        let peer_id = 1u64;

        prop.announce_block(hash, peer_id);
        prop.request_object(hash).unwrap();

        let result = prop.receive_block(hash, hash);
        assert!(result.is_ok());
        assert_eq!(prop.received_blocks.len(), 1);
        assert_eq!(prop.received_blocks[0], hash);
        assert_eq!(prop.pending_requests.get(&hash), Some(&true));
    }

    // =========================================================================
    // Test 4: Duplicate announcement is ignored
    // =========================================================================
    #[test]
    fn test_duplicate_announcement_ignored() {
        let mut prop = MockBlockPropagation::new();
        let hash = make_hash(0x04);
        let peer_id_1 = 1u64;
        let peer_id_2 = 2u64;

        let first = prop.announce_block(hash, peer_id_1);
        let second = prop.announce_block(hash, peer_id_2);

        assert!(first);
        assert!(!second);
        assert_eq!(prop.announced_blocks.len(), 1);
        // Both peers are tracked as sources
        assert_eq!(prop.announcement_sources[&hash].len(), 2);
        assert_eq!(prop.announcement_sources[&hash][0], peer_id_1);
        assert_eq!(prop.announcement_sources[&hash][1], peer_id_2);
    }

    // =========================================================================
    // Test 5: Object request for unknown block fails
    // =========================================================================
    #[test]
    fn test_object_request_for_unknown_block_fails() {
        let mut prop = MockBlockPropagation::new();
        let hash = make_hash(0x05);

        let result = prop.request_object(hash);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Unknown block hash");
        assert!(prop.requested_objects.is_empty());
        assert!(prop.pending_requests.is_empty());
    }

    // =========================================================================
    // Test 6: Multiple announcements from different peers
    // =========================================================================
    #[test]
    fn test_multiple_announcements_from_different_peers() {
        let mut prop = MockBlockPropagation::new();
        let hash_a = make_hash(0x06);
        let hash_b = make_hash(0x07);
        let hash_c = make_hash(0x08);
        let peer_1 = 1u64;
        let peer_2 = 2u64;
        let peer_3 = 3u64;

        assert!(prop.announce_block(hash_a, peer_1));
        assert!(prop.announce_block(hash_b, peer_2));
        assert!(prop.announce_block(hash_c, peer_3));

        assert_eq!(prop.announced_blocks.len(), 3);
        assert_eq!(prop.announcement_sources[&hash_a], vec![peer_1]);
        assert_eq!(prop.announcement_sources[&hash_b], vec![peer_2]);
        assert_eq!(prop.announcement_sources[&hash_c], vec![peer_3]);
    }

    // =========================================================================
    // Test 7: Object request timeout (PEER_TIMEOUT_REQUEST_OBJECT = 15s)
    // =========================================================================
    #[test]
    fn test_object_request_timeout() {
        assert_eq!(PEER_TIMEOUT_REQUEST_OBJECT, 15_000);

        let mut limiter = MockRateLimiter::new();
        let request_id = 1u64;
        let start_time_ms = 1000u64;
        let deadline = start_time_ms + PEER_TIMEOUT_REQUEST_OBJECT;

        limiter.register_timeout(request_id, deadline);

        // Before timeout
        assert!(!limiter.is_timed_out(request_id, start_time_ms));
        assert!(!limiter.is_timed_out(request_id, start_time_ms + 14_999));

        // At exactly the timeout boundary
        assert!(limiter.is_timed_out(request_id, deadline));

        // After timeout
        assert!(limiter.is_timed_out(request_id, deadline + 1));
    }

    // =========================================================================
    // Test 8: Block response hash must match request hash
    // =========================================================================
    #[test]
    fn test_block_response_hash_must_match() {
        let mut prop = MockBlockPropagation::new();
        let request_hash = make_hash(0x09);
        let wrong_hash = make_hash(0x0A);
        let peer_id = 1u64;

        prop.announce_block(request_hash, peer_id);
        prop.request_object(request_hash).unwrap();

        // Mismatched hash should fail
        let result = prop.receive_block(request_hash, wrong_hash);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Response hash does not match request hash"
        );
        assert!(prop.received_blocks.is_empty());

        // Correct hash should succeed
        let result = prop.receive_block(request_hash, request_hash);
        assert!(result.is_ok());
        assert_eq!(prop.received_blocks.len(), 1);
    }

    // =========================================================================
    // Test 9: Announcement only contains header hash (not full block)
    // =========================================================================
    #[test]
    fn test_announcement_contains_header_hash_only() {
        let mut prop = MockBlockPropagation::new();
        let header_hash = make_hash(0x0B);
        let peer_id = 1u64;

        prop.announce_block(header_hash, peer_id);

        // The announced block is stored as a 32-byte hash only
        assert_eq!(prop.announced_blocks[0].len(), 32);
        // No full block data is stored at announcement time
        assert!(prop.received_blocks.is_empty());
        // The full block must be explicitly requested
        assert!(prop.requested_objects.is_empty());
    }

    // =========================================================================
    // Test 10: Sequential announcements maintain order
    // =========================================================================
    #[test]
    fn test_sequential_announcements_maintain_order() {
        let mut prop = MockBlockPropagation::new();
        let peer_id = 1u64;

        let hashes: Vec<Hash> = (1u8..=10).map(make_hash).collect();

        for hash in &hashes {
            prop.announce_block(*hash, peer_id);
        }

        assert_eq!(prop.announcement_order.len(), 10);
        for (i, hash) in hashes.iter().enumerate() {
            assert_eq!(prop.announcement_order[i], *hash);
        }
    }

    // =========================================================================
    // Test 11: Fulfilled requests are cleaned up
    // =========================================================================
    #[test]
    fn test_fulfilled_requests_cleanup() {
        let mut prop = MockBlockPropagation::new();
        let hash_a = make_hash(0x0C);
        let hash_b = make_hash(0x0D);
        let peer_id = 1u64;

        prop.announce_block(hash_a, peer_id);
        prop.announce_block(hash_b, peer_id);
        prop.request_object(hash_a).unwrap();
        prop.request_object(hash_b).unwrap();

        // Fulfill only hash_a
        prop.receive_block(hash_a, hash_a).unwrap();
        assert_eq!(prop.pending_count(), 1);

        // Cleanup fulfilled requests
        prop.cleanup_fulfilled();

        assert_eq!(prop.pending_requests.len(), 1);
        assert!(!prop.pending_requests.contains_key(&hash_a));
        assert!(prop.pending_requests.contains_key(&hash_b));
        assert_eq!(prop.pending_count(), 1);
    }

    // =========================================================================
    // Test 12: Multiple pending requests concurrently (up to 64)
    // =========================================================================
    #[test]
    fn test_multiple_pending_requests_up_to_concurrency_limit() {
        let mut prop = MockBlockPropagation::new();
        let peer_id = 1u64;

        // Create PEER_OBJECTS_CONCURRENCY announcements and requests
        for i in 0..PEER_OBJECTS_CONCURRENCY {
            let hash = make_hash(i as u8);
            prop.announce_block(hash, peer_id);
            prop.request_object(hash).unwrap();
        }

        assert_eq!(prop.pending_count(), PEER_OBJECTS_CONCURRENCY);
        assert_eq!(prop.pending_count(), 64);

        // All are unfulfilled
        for i in 0..PEER_OBJECTS_CONCURRENCY {
            let hash = make_hash(i as u8);
            assert_eq!(prop.pending_requests.get(&hash), Some(&false));
        }
    }

    // =========================================================================
    // Test 13: Request deduplication (same hash from multiple peers)
    // =========================================================================
    #[test]
    fn test_request_deduplication_same_hash_multiple_peers() {
        let mut prop = MockBlockPropagation::new();
        let hash = make_hash(0x0E);
        let peer_1 = 1u64;
        let peer_2 = 2u64;
        let peer_3 = 3u64;

        // First announcement is new
        assert!(prop.announce_block(hash, peer_1));
        // Subsequent for same hash are duplicates
        assert!(!prop.announce_block(hash, peer_2));
        assert!(!prop.announce_block(hash, peer_3));

        // Only one announcement entry
        assert_eq!(prop.announced_blocks.len(), 1);

        // Only one request needed
        prop.request_object(hash).unwrap();
        assert_eq!(prop.requested_objects.len(), 1);
        assert_eq!(prop.pending_count(), 1);

        // All three peers tracked as sources
        assert_eq!(prop.announcement_sources[&hash].len(), 3);
    }

    // =========================================================================
    // Test 14: Propagation to all connected peers
    // =========================================================================
    #[test]
    fn test_propagation_to_all_connected_peers() {
        let mut prop = MockBlockPropagation::new();
        let sender = 1u64;

        prop.connected_peers = vec![1, 2, 3, 4, 5];

        let targets = prop.propagation_targets(sender);

        // Sender is excluded
        assert_eq!(targets.len(), 4);
        assert!(!targets.contains(&sender));
        assert!(targets.contains(&2));
        assert!(targets.contains(&3));
        assert!(targets.contains(&4));
        assert!(targets.contains(&5));
    }

    // =========================================================================
    // Test 15: Priority peer blocks processed first
    // =========================================================================
    #[test]
    fn test_priority_peer_blocks_processed_first() {
        let mut prop = MockBlockPropagation::new();
        prop.priority_peers = vec![10, 20];

        let normal_peer = 1u64;
        let priority_peer = 10u64;

        // Announce from normal peer first, then from priority peer
        let hash_normal = make_hash(0x10);
        let hash_priority = make_hash(0x11);

        prop.announce_block(hash_normal, normal_peer);
        prop.announce_block(hash_priority, priority_peer);
        prop.request_object(hash_normal).unwrap();
        prop.request_object(hash_priority).unwrap();

        // Priority ordering should place priority peer blocks first
        let ordered = prop.get_priority_ordered_requests();
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0], hash_priority);
        assert_eq!(ordered[1], hash_normal);
    }

    // =========================================================================
    // Test 16: Block not propagated to sender
    // =========================================================================
    #[test]
    fn test_block_not_propagated_to_sender() {
        let mut prop = MockBlockPropagation::new();
        let sender = 5u64;

        prop.connected_peers = vec![1, 2, 3, 4, 5, 6, 7];

        let targets = prop.propagation_targets(sender);

        assert!(!targets.contains(&sender));
        assert_eq!(targets.len(), 6);
        for peer in &[1u64, 2, 3, 4, 6, 7] {
            assert!(targets.contains(peer));
        }
    }
}
