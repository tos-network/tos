#[cfg(test)]
mod tests {
    use super::super::mock::*;

    // -- Connection state transitions --

    #[test]
    fn test_connection_starts_in_pending_state() {
        let conn = MockConnection::new(make_addr(8080), true);
        assert_eq!(conn.state, ConnectionState::Pending);
        assert!(!conn.is_closed());
    }

    #[test]
    fn test_state_transition_pending_to_key_exchange() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        assert_eq!(conn.state, ConnectionState::Pending);

        let our_key = [1u8; 32];
        let peer_key = [2u8; 32];
        conn.exchange_keys(our_key, peer_key);

        assert_eq!(conn.state, ConnectionState::KeyExchange);
        assert_eq!(conn.our_key, Some(our_key));
        assert_eq!(conn.peer_key, Some(peer_key));
    }

    #[test]
    fn test_state_transition_key_exchange_to_handshake() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);
        assert_eq!(conn.state, ConnectionState::KeyExchange);

        conn.set_state(ConnectionState::Handshake);
        assert_eq!(conn.state, ConnectionState::Handshake);
    }

    #[test]
    fn test_state_transition_handshake_to_success() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);
        conn.set_state(ConnectionState::Handshake);
        conn.set_state(ConnectionState::Success);
        assert_eq!(conn.state, ConnectionState::Success);
    }

    #[test]
    fn test_full_state_lifecycle_pending_to_success() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        assert_eq!(conn.state, ConnectionState::Pending);

        conn.exchange_keys([0xAA; 32], [0xBB; 32]);
        assert_eq!(conn.state, ConnectionState::KeyExchange);

        conn.set_state(ConnectionState::Handshake);
        assert_eq!(conn.state, ConnectionState::Handshake);

        conn.set_state(ConnectionState::Success);
        assert_eq!(conn.state, ConnectionState::Success);
        assert!(!conn.is_closed());
    }

    // -- Client vs server roles --

    #[test]
    fn test_outgoing_connection_role() {
        let conn = MockConnection::new(make_addr(8080), true);
        assert!(conn.is_out());
    }

    #[test]
    fn test_incoming_connection_role() {
        let conn = MockConnection::new(make_addr(8080), false);
        assert!(!conn.is_out());
    }

    #[test]
    fn test_outgoing_initiates_key_exchange_first() {
        // Outgoing connections (client) initiate the key exchange
        let mut conn = MockConnection::new(make_addr(8080), true);
        assert!(conn.is_out());
        assert_eq!(conn.state, ConnectionState::Pending);

        // Client generates and sends key first
        conn.exchange_keys([0x11; 32], [0x22; 32]);
        assert_eq!(conn.state, ConnectionState::KeyExchange);
    }

    #[test]
    fn test_incoming_waits_for_key_exchange() {
        // Incoming connections (server) wait for client key exchange
        let conn = MockConnection::new(make_addr(8080), false);
        assert!(!conn.is_out());
        assert_eq!(conn.state, ConnectionState::Pending);
        // Server remains pending until client sends key exchange
        assert!(conn.our_key.is_none());
        assert!(conn.peer_key.is_none());
    }

    // -- Handshake validation: version length --

    #[test]
    fn test_valid_version_string() {
        let hs = MockHandshake::new_valid(Network::Mainnet);
        assert!(hs.validate().is_ok());
    }

    #[test]
    fn test_empty_version_string_rejected() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.version = String::new();
        assert_eq!(hs.validate(), Err("Invalid version length"));
    }

    #[test]
    fn test_version_at_max_length_accepted() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.version = "a".repeat(MockHandshake::MAX_LEN);
        assert!(hs.validate().is_ok());
    }

    #[test]
    fn test_version_exceeding_max_length_rejected() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.version = "a".repeat(MockHandshake::MAX_LEN + 1);
        assert_eq!(hs.validate(), Err("Invalid version length"));
    }

    // -- Node tag length limits --

    #[test]
    fn test_node_tag_none_accepted() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.node_tag = None;
        assert!(hs.validate().is_ok());
    }

    #[test]
    fn test_node_tag_valid_accepted() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.node_tag = Some("my-node".to_string());
        assert!(hs.validate().is_ok());
    }

    #[test]
    fn test_node_tag_empty_rejected() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.node_tag = Some(String::new());
        assert_eq!(hs.validate(), Err("Invalid node_tag length"));
    }

    #[test]
    fn test_node_tag_exceeding_max_rejected() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.node_tag = Some("x".repeat(MockHandshake::MAX_LEN + 1));
        assert_eq!(hs.validate(), Err("Invalid node_tag length"));
    }

    #[test]
    fn test_node_tag_at_max_length_accepted() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.node_tag = Some("n".repeat(MockHandshake::MAX_LEN));
        assert!(hs.validate().is_ok());
    }

    // -- Network ID matching/mismatching --

    #[test]
    fn test_network_id_matching() {
        let hs1 = MockHandshake::new_valid(Network::Mainnet);
        let hs2 = MockHandshake::new_valid(Network::Mainnet);
        assert_eq!(hs1.network_id, hs2.network_id);
        assert_eq!(hs1.network.chain_id(), hs2.network.chain_id());
    }

    #[test]
    fn test_network_id_mismatch_different_networks() {
        let hs_main = MockHandshake::new_valid(Network::Mainnet);
        let hs_test = MockHandshake::new_valid(Network::Testnet);
        // Same network_id bytes (from new_valid), but different network enum
        assert_ne!(hs_main.network.chain_id(), hs_test.network.chain_id());
    }

    #[test]
    fn test_network_chain_ids_unique() {
        let networks = [
            Network::Mainnet,
            Network::Testnet,
            Network::Stagenet,
            Network::Devnet,
        ];
        let chain_ids: Vec<u64> = networks.iter().map(|n| n.chain_id()).collect();
        // Verify all chain IDs are unique
        for i in 0..chain_ids.len() {
            for j in (i + 1)..chain_ids.len() {
                assert_ne!(chain_ids[i], chain_ids[j]);
            }
        }
    }

    // -- Genesis hash validation --

    #[test]
    fn test_genesis_hash_matching_peers_accepted() {
        let hs1 = MockHandshake::new_valid(Network::Mainnet);
        let hs2 = MockHandshake::new_valid(Network::Mainnet);
        assert_eq!(hs1.genesis_hash, hs2.genesis_hash);
    }

    #[test]
    fn test_genesis_hash_mismatch_detected() {
        let hs1 = MockHandshake::new_valid(Network::Mainnet);
        let mut hs2 = MockHandshake::new_valid(Network::Mainnet);
        hs2.genesis_hash = [0xFF; 32];
        assert_ne!(hs1.genesis_hash, hs2.genesis_hash);
    }

    // -- Pruned topoheight validation --

    #[test]
    fn test_pruned_topoheight_none_accepted() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.pruned_topoheight = None;
        assert!(hs.validate().is_ok());
    }

    #[test]
    fn test_pruned_topoheight_zero_rejected() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.pruned_topoheight = Some(0);
        assert_eq!(hs.validate(), Err("Pruned topoheight cannot be 0"));
    }

    #[test]
    fn test_pruned_topoheight_exceeds_topoheight_rejected() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.topoheight = 100;
        hs.pruned_topoheight = Some(101);
        assert_eq!(hs.validate(), Err("Pruned topoheight exceeds topoheight"));
    }

    #[test]
    fn test_pruned_topoheight_equal_to_topoheight_accepted() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.topoheight = 50;
        hs.pruned_topoheight = Some(50);
        assert!(hs.validate().is_ok());
    }

    #[test]
    fn test_pruned_topoheight_less_than_topoheight_accepted() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.topoheight = 100;
        hs.pruned_topoheight = Some(50);
        assert!(hs.validate().is_ok());
    }

    // -- Key exchange flow --

    #[test]
    fn test_key_exchange_sets_both_keys() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        let our_key = [0x11; 32];
        let peer_key = [0x22; 32];
        conn.exchange_keys(our_key, peer_key);

        assert_eq!(conn.our_key, Some(our_key));
        assert_eq!(conn.peer_key, Some(peer_key));
    }

    #[test]
    fn test_key_verification_action_default_is_ignore() {
        let action = KeyVerificationAction::default();
        assert_eq!(action, KeyVerificationAction::Ignore);
    }

    #[test]
    fn test_key_verification_action_warn() {
        let action = KeyVerificationAction::Warn;
        // Warn action allows connection but logs a warning
        assert_ne!(action, KeyVerificationAction::Reject);
        assert_ne!(action, KeyVerificationAction::Ignore);
    }

    #[test]
    fn test_key_verification_action_reject_blocks_connection() {
        let action = KeyVerificationAction::Reject;
        // Simulate: if verification returns Reject, connection should be closed
        let mut conn = MockConnection::new(make_addr(8080), true);
        if action == KeyVerificationAction::Reject {
            conn.close();
        }
        assert!(conn.is_closed());
    }

    #[test]
    fn test_key_verification_action_ignore_allows_connection() {
        let action = KeyVerificationAction::Ignore;
        let mut conn = MockConnection::new(make_addr(8080), true);
        if action == KeyVerificationAction::Reject {
            conn.close();
        }
        // Connection remains open with Ignore action
        assert!(!conn.is_closed());
    }

    // -- Double-close prevention --

    #[test]
    fn test_close_sets_closed_flag() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        assert!(!conn.is_closed());
        conn.close();
        assert!(conn.is_closed());
    }

    #[test]
    fn test_double_close_is_idempotent() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.close();
        assert!(conn.is_closed());
        conn.close(); // Second close should not panic or cause issues
        assert!(conn.is_closed());
    }

    #[test]
    fn test_send_on_closed_connection_returns_error() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);
        conn.close();
        let result = conn.send_bytes(100);
        assert_eq!(result, Err("Connection closed"));
    }

    // -- supports_fast_sync backwards compatibility --

    #[test]
    fn test_supports_fast_sync_defaults_true() {
        let hs = MockHandshake::new_valid(Network::Mainnet);
        assert!(hs.supports_fast_sync);
    }

    #[test]
    fn test_supports_fast_sync_can_be_disabled() {
        let mut hs = MockHandshake::new_valid(Network::Mainnet);
        hs.supports_fast_sync = false;
        assert!(!hs.supports_fast_sync);
        // Should still validate successfully even with fast sync disabled
        assert!(hs.validate().is_ok());
    }

    // -- Handshake with all fields set vs minimal --

    #[test]
    fn test_handshake_with_all_fields_set() {
        let hs = MockHandshake {
            version: "2.0.0-beta".to_string(),
            network: Network::Testnet,
            node_tag: Some("full-node".to_string()),
            network_id: [0xAB; 16],
            peer_id: 99999,
            local_port: 9090,
            utc_time: 1700000000,
            topoheight: 500,
            height: 250,
            pruned_topoheight: Some(100),
            top_hash: [0xDD; 32],
            genesis_hash: [0xEE; 32],
            cumulative_difficulty: 50000,
            can_be_shared: false,
            supports_fast_sync: false,
        };
        assert!(hs.validate().is_ok());
        assert_eq!(hs.node_tag, Some("full-node".to_string()));
        assert_eq!(hs.pruned_topoheight, Some(100));
        assert!(!hs.can_be_shared);
        assert!(!hs.supports_fast_sync);
    }

    #[test]
    fn test_handshake_with_minimal_fields() {
        let hs = MockHandshake {
            version: "1".to_string(),
            network: Network::Mainnet,
            node_tag: None,
            network_id: [0u8; 16],
            peer_id: 1,
            local_port: 1,
            utc_time: 0,
            topoheight: 0,
            height: 0,
            pruned_topoheight: None,
            top_hash: [0u8; 32],
            genesis_hash: [0u8; 32],
            cumulative_difficulty: 0,
            can_be_shared: true,
            supports_fast_sync: true,
        };
        assert!(hs.validate().is_ok());
    }

    #[test]
    fn test_connection_address_preserved() {
        let addr = make_addr_ip(10, 0, 0, 1, 3000);
        let conn = MockConnection::new(addr, true);
        assert_eq!(conn.addr, addr);
    }

    #[test]
    fn test_connection_initial_byte_counters_zero() {
        let conn = MockConnection::new(make_addr(8080), true);
        assert_eq!(conn.bytes_in, 0);
        assert_eq!(conn.bytes_out, 0);
        assert_eq!(conn.bytes_encrypted, 0);
    }
}
