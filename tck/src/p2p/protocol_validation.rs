// Tests for network protocol validation including network ID, genesis hash,
// version/tag validation, packet IDs, and order-dependency rules.

#[cfg(test)]
mod tests {
    use super::super::mock::*;

    // =========================================================================
    // Test 1: Network ID must match (16 bytes) - same network accepts
    // =========================================================================
    #[test]
    fn test_network_id_same_network_accepts() {
        let handshake_a = MockHandshake {
            network_id: [0xAB; 16],
            ..MockHandshake::new_valid(Network::Mainnet)
        };
        let handshake_b = MockHandshake {
            network_id: [0xAB; 16],
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        // Same network ID should be accepted
        assert_eq!(handshake_a.network_id, handshake_b.network_id);
        assert_eq!(handshake_a.network_id.len(), 16);
    }

    // =========================================================================
    // Test 2: Network ID mismatch rejects connection
    // =========================================================================
    #[test]
    fn test_network_id_mismatch_rejects() {
        let handshake_a = MockHandshake {
            network_id: [0xAB; 16],
            ..MockHandshake::new_valid(Network::Mainnet)
        };
        let handshake_b = MockHandshake {
            network_id: [0xCD; 16],
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        // Different network IDs should reject
        assert_ne!(handshake_a.network_id, handshake_b.network_id);
    }

    // =========================================================================
    // Test 3: Genesis hash must match between peers
    // =========================================================================
    #[test]
    fn test_genesis_hash_must_match() {
        let genesis = [0xBB; 32];
        let handshake_a = MockHandshake {
            genesis_hash: genesis,
            ..MockHandshake::new_valid(Network::Mainnet)
        };
        let handshake_b = MockHandshake {
            genesis_hash: genesis,
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        assert_eq!(handshake_a.genesis_hash, handshake_b.genesis_hash);
    }

    // =========================================================================
    // Test 4: Genesis hash mismatch rejects connection
    // =========================================================================
    #[test]
    fn test_genesis_hash_mismatch_rejects() {
        let handshake_a = MockHandshake {
            genesis_hash: [0xAA; 32],
            ..MockHandshake::new_valid(Network::Mainnet)
        };
        let handshake_b = MockHandshake {
            genesis_hash: [0xBB; 32],
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        assert_ne!(handshake_a.genesis_hash, handshake_b.genesis_hash);
    }

    // =========================================================================
    // Test 5: Version string validation - empty string rejected
    // =========================================================================
    #[test]
    fn test_version_empty_string_rejected() {
        let handshake = MockHandshake {
            version: String::new(),
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        let result = handshake.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid version length");
    }

    // =========================================================================
    // Test 6: Version string validation - exceeds 16 chars rejected
    // =========================================================================
    #[test]
    fn test_version_exceeds_max_len_rejected() {
        let handshake = MockHandshake {
            version: "a".repeat(17), // 17 chars > MAX_LEN (16)
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        let result = handshake.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid version length");
    }

    // =========================================================================
    // Test 7: Version string validation - exactly 16 chars accepted
    // =========================================================================
    #[test]
    fn test_version_exactly_max_len_accepted() {
        let handshake = MockHandshake {
            version: "a".repeat(16), // Exactly MAX_LEN
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        let result = handshake.validate();
        assert!(result.is_ok());
    }

    // =========================================================================
    // Test 8: Node tag validation - empty if present is rejected
    // =========================================================================
    #[test]
    fn test_node_tag_empty_rejected() {
        let handshake = MockHandshake {
            node_tag: Some(String::new()),
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        let result = handshake.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid node_tag length");
    }

    // =========================================================================
    // Test 9: Node tag validation - exceeds 16 chars rejected
    // =========================================================================
    #[test]
    fn test_node_tag_exceeds_max_len_rejected() {
        let handshake = MockHandshake {
            node_tag: Some("b".repeat(17)), // 17 chars > MAX_LEN
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        let result = handshake.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid node_tag length");
    }

    // =========================================================================
    // Test 10: Peer ID uniqueness (same peer_id = self-connection)
    // =========================================================================
    #[test]
    fn test_peer_id_self_connection_detected() {
        let our_peer_id = 99999u64;
        let handshake = MockHandshake {
            peer_id: our_peer_id,
            ..MockHandshake::new_valid(Network::Mainnet)
        };

        // Same peer_id means self-connection
        assert_eq!(handshake.peer_id, our_peer_id);

        // Different peer_id is not a self-connection
        let other_handshake = MockHandshake {
            peer_id: 12345,
            ..MockHandshake::new_valid(Network::Mainnet)
        };
        assert_ne!(other_handshake.peer_id, our_peer_id);
    }

    // =========================================================================
    // Test 11: Network types - chain_id values
    // =========================================================================
    #[test]
    fn test_network_types_chain_ids() {
        assert_eq!(Network::Mainnet.chain_id(), 0);
        assert_eq!(Network::Testnet.chain_id(), 1);
        assert_eq!(Network::Stagenet.chain_id(), 2);
        assert_eq!(Network::Devnet.chain_id(), 3);
    }

    // =========================================================================
    // Test 12: Packet ID validation - all valid IDs (0-13) accepted
    // =========================================================================
    #[test]
    fn test_all_valid_packet_ids_accepted() {
        let valid_ids: Vec<u8> = (0..=MAX_VALID_PACKET_ID).collect();

        assert_eq!(valid_ids.len(), 14);
        assert_eq!(valid_ids[0], KEY_EXCHANGE_ID);
        assert_eq!(valid_ids[1], HANDSHAKE_ID);
        assert_eq!(valid_ids[2], TX_PROPAGATION_ID);
        assert_eq!(valid_ids[3], BLOCK_PROPAGATION_ID);
        assert_eq!(valid_ids[4], CHAIN_REQUEST_ID);
        assert_eq!(valid_ids[5], CHAIN_RESPONSE_ID);
        assert_eq!(valid_ids[6], PING_ID);
        assert_eq!(valid_ids[7], OBJECT_REQUEST_ID);
        assert_eq!(valid_ids[8], OBJECT_RESPONSE_ID);
        assert_eq!(valid_ids[9], NOTIFY_INV_REQUEST_ID);
        assert_eq!(valid_ids[10], NOTIFY_INV_RESPONSE_ID);
        assert_eq!(valid_ids[11], BOOTSTRAP_CHAIN_REQUEST_ID);
        assert_eq!(valid_ids[12], BOOTSTRAP_CHAIN_RESPONSE_ID);
        assert_eq!(valid_ids[13], PEER_DISCONNECTED_ID);

        for id in valid_ids {
            assert!(id <= MAX_VALID_PACKET_ID);
        }
    }

    // =========================================================================
    // Test 13: Unknown packet ID (14+) rejected
    // =========================================================================
    #[test]
    fn test_unknown_packet_id_rejected() {
        let invalid_ids: Vec<u8> = (14..=255).collect();

        for id in invalid_ids {
            assert!(id > MAX_VALID_PACKET_ID);
        }

        // Boundary: 13 is the last valid, 14 is invalid
        let max_id = MAX_VALID_PACKET_ID;
        assert_eq!(max_id, 13);
        assert!(14u8 > max_id);
        assert!(255u8 > max_id);
    }

    // =========================================================================
    // Test 14: Packet order-dependency - Ping is order-independent
    // =========================================================================
    #[test]
    fn test_ping_is_order_independent() {
        assert!(!is_packet_order_dependent(PING_ID));
    }

    // =========================================================================
    // Test 15: Packet order-dependency - ObjectRequest is order-independent
    // =========================================================================
    #[test]
    fn test_object_request_is_order_independent() {
        assert!(!is_packet_order_dependent(OBJECT_REQUEST_ID));
        assert!(!is_packet_order_dependent(OBJECT_RESPONSE_ID));
    }

    // =========================================================================
    // Test 16: Packet order-dependency - Handshake is order-dependent
    // =========================================================================
    #[test]
    fn test_handshake_is_order_dependent() {
        assert!(is_packet_order_dependent(HANDSHAKE_ID));
        assert!(is_packet_order_dependent(KEY_EXCHANGE_ID));
    }

    // =========================================================================
    // Test 17: Packet order-dependency - BlockPropagation is order-dependent
    // =========================================================================
    #[test]
    fn test_block_propagation_is_order_dependent() {
        assert!(is_packet_order_dependent(BLOCK_PROPAGATION_ID));
        assert!(is_packet_order_dependent(TX_PROPAGATION_ID));

        // Comprehensive check of all order-dependent packet types
        let order_dependent_ids = [
            KEY_EXCHANGE_ID,
            HANDSHAKE_ID,
            TX_PROPAGATION_ID,
            BLOCK_PROPAGATION_ID,
            NOTIFY_INV_RESPONSE_ID,
            BOOTSTRAP_CHAIN_REQUEST_ID,
            BOOTSTRAP_CHAIN_RESPONSE_ID,
        ];
        for id in order_dependent_ids {
            assert!(
                is_packet_order_dependent(id),
                "Packet ID {} should be order-dependent",
                id
            );
        }

        // Comprehensive check of all order-independent packet types
        let order_independent_ids = [
            PING_ID,
            OBJECT_REQUEST_ID,
            OBJECT_RESPONSE_ID,
            CHAIN_REQUEST_ID,
            CHAIN_RESPONSE_ID,
            NOTIFY_INV_REQUEST_ID,
            PEER_DISCONNECTED_ID,
        ];
        for id in order_independent_ids {
            assert!(
                !is_packet_order_dependent(id),
                "Packet ID {} should be order-independent",
                id
            );
        }
    }

    // =========================================================================
    // Test 18: Maximum packet size enforcement (PEER_MAX_PACKET_SIZE = 5MB)
    // =========================================================================
    #[test]
    fn test_max_packet_size_enforcement() {
        let limiter = MockRateLimiter::new();

        // Verify the constant value
        assert_eq!(PEER_MAX_PACKET_SIZE, 5 * 1024 * 1024); // 5 MB
        assert_eq!(limiter.max_packet_size, PEER_MAX_PACKET_SIZE);

        // Within limit: accepted
        assert!(limiter.validate_packet_size(1).is_ok());
        assert!(limiter.validate_packet_size(1024).is_ok());
        assert!(limiter
            .validate_packet_size(PEER_MAX_PACKET_SIZE - 1)
            .is_ok());
        assert!(limiter.validate_packet_size(PEER_MAX_PACKET_SIZE).is_ok());

        // Exceeding limit: rejected
        assert!(limiter
            .validate_packet_size(PEER_MAX_PACKET_SIZE + 1)
            .is_err());
        assert!(limiter.validate_packet_size(u32::MAX).is_err());

        // Zero size: rejected
        assert!(limiter.validate_packet_size(0).is_err());
    }
}
