// Layer 1: Real Handshake Serialization + Validation Tests
//
// Tests the actual tos_daemon::p2p::packet::Handshake:
// - Serializer::write / Serializer::read roundtrip
// - Field validation rules (version length, node_tag length, pruned_topoheight != 0)
// - Boundary conditions and edge cases
// - Getter method correctness

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use tos_common::{
        crypto::Hash,
        difficulty::CumulativeDifficulty,
        network::Network,
        serializer::{Reader, Serializer, Writer},
    };
    use tos_daemon::p2p::packet::Handshake;

    fn make_handshake(
        version: &str,
        node_tag: Option<&str>,
        pruned_topoheight: Option<u64>,
    ) -> Handshake<'static> {
        let version_string = version.to_string();
        let tag = node_tag.map(|t| t.to_string());
        let network_id: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let top_hash = Hash::zero();
        let genesis_hash = Hash::new([0xAB; 32]);
        let cum_diff = CumulativeDifficulty::from(1000u64);

        Handshake::new(
            Cow::Owned(version_string),
            Network::Mainnet,
            Cow::Owned(tag),
            Cow::Owned(network_id),
            12345u64,
            8080u16,
            1700000000u64,
            500u64,
            100u64,
            pruned_topoheight,
            Cow::Owned(top_hash),
            Cow::Owned(genesis_hash),
            Cow::Owned(cum_diff),
            true,
            true,
        )
    }

    fn serialize_handshake(h: &Handshake<'_>) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        h.write(&mut writer);
        bytes
    }

    fn deserialize_handshake(
        data: &[u8],
    ) -> Result<Handshake<'static>, tos_common::serializer::ReaderError> {
        let mut reader = Reader::new(data);
        Handshake::read(&mut reader)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Roundtrip tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_roundtrip_basic() {
        let h = make_handshake("1.0.0", Some("TestNode"), None);
        let bytes = serialize_handshake(&h);
        let decoded = deserialize_handshake(&bytes).unwrap();

        assert_eq!(decoded.get_version(), "1.0.0");
        assert_eq!(decoded.get_network(), &Network::Mainnet);
        assert_eq!(decoded.get_node_tag(), &Some("TestNode".to_string()));
        assert_eq!(decoded.get_peer_id(), 12345);
        assert_eq!(decoded.get_local_port(), 8080);
        assert_eq!(decoded.get_utc_time(), 1700000000);
        assert_eq!(decoded.get_topoheight(), 500);
        assert_eq!(decoded.get_block_height(), 100);
        assert_eq!(decoded.get_pruned_topoheight(), &None);
        assert_eq!(decoded.get_block_top_hash(), &Hash::zero());
        assert_eq!(decoded.get_block_genesis_hash(), &Hash::new([0xAB; 32]));
        assert!(decoded.supports_fast_sync());
    }

    #[test]
    fn test_handshake_roundtrip_no_node_tag() {
        let h = make_handshake("2.0.0", None, None);
        let bytes = serialize_handshake(&h);
        let decoded = deserialize_handshake(&bytes).unwrap();

        assert_eq!(decoded.get_version(), "2.0.0");
        assert_eq!(decoded.get_node_tag(), &None);
    }

    #[test]
    fn test_handshake_roundtrip_with_pruned_topoheight() {
        let h = make_handshake("1.0.0", None, Some(42));
        let bytes = serialize_handshake(&h);
        let decoded = deserialize_handshake(&bytes).unwrap();

        assert_eq!(decoded.get_pruned_topoheight(), &Some(42));
    }

    #[test]
    fn test_handshake_roundtrip_max_version_length() {
        let version = "1234567890123456"; // exactly 16 chars
        let h = make_handshake(version, None, None);
        let bytes = serialize_handshake(&h);
        let decoded = deserialize_handshake(&bytes).unwrap();

        assert_eq!(decoded.get_version(), version);
    }

    #[test]
    fn test_handshake_roundtrip_max_node_tag_length() {
        let tag = "1234567890123456"; // exactly 16 chars
        let h = make_handshake("1.0.0", Some(tag), None);
        let bytes = serialize_handshake(&h);
        let decoded = deserialize_handshake(&bytes).unwrap();

        assert_eq!(decoded.get_node_tag(), &Some(tag.to_string()));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Validation: version length
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_reject_empty_version() {
        let h = make_handshake("x", None, None);
        let mut bytes = serialize_handshake(&h);

        // Overwrite the first byte (version length) to 0
        bytes[0] = 0;

        let result = deserialize_handshake(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_handshake_reject_version_too_long() {
        let h = make_handshake("1.0.0", None, None);
        let mut bytes = serialize_handshake(&h);

        // Set version length to 17 (exceeds MAX_LEN=16)
        bytes[0] = 17;
        bytes.extend_from_slice(&[b'A'; 20]);

        let result = deserialize_handshake(&bytes);
        assert!(result.is_err());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Validation: node_tag length
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_reject_node_tag_too_long() {
        let h = make_handshake("1.0.0", Some("short"), None);
        let bytes = serialize_handshake(&h);

        // Format: version_len(1) + "1.0.0"(5) + network(1) = 7 bytes
        // node_tag uses write_optional_string: len_byte acts as both presence flag and length
        // So the tag length byte is at offset 7
        let tag_len_offset = 1 + 5 + 1; // = 7
        let mut corrupted = bytes.clone();
        corrupted[tag_len_offset] = 17; // exceed MAX_LEN
        corrupted.extend_from_slice(&[b'B'; 20]);

        let result = deserialize_handshake(&corrupted);
        assert!(result.is_err());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Validation: pruned_topoheight must not be 0
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_reject_pruned_topoheight_zero() {
        let h = make_handshake("1.0.0", None, Some(1));
        let bytes = serialize_handshake(&h);

        // Offset: version_len(1) + "1.0.0"(5) + network(1) + tag_none(1) + network_id(16)
        //       + peer_id(8) + port(2) + time(8) + topo(8) + height(8) = 58
        let pruned_offset = 1 + 5 + 1 + 1 + 16 + 8 + 2 + 8 + 8 + 8; // = 58

        // Verify the option flag is 1 (Some)
        assert_eq!(bytes[pruned_offset], 1);

        // Zero out the u64 value (bytes after the flag)
        let mut corrupted = bytes.clone();
        for i in 0..8 {
            corrupted[pruned_offset + 1 + i] = 0;
        }

        let result = deserialize_handshake(&corrupted);
        assert!(result.is_err());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Size calculation
    //
    // BUG FOUND: Handshake::size() is off by 1 when node_tag is Some.
    // The write() uses write_optional_string (no bool flag, uses len=0 for None)
    // but size() uses Option<String>::Serializer::size() which adds a bool flag.
    // This means size() over-reports by 1 byte when node_tag is present.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_size_no_tag_matches_serialized_length() {
        // Without node_tag, size() is correct
        let h = make_handshake("v1", None, None);
        let bytes = serialize_handshake(&h);
        assert_eq!(h.size(), bytes.len());
    }

    #[test]
    fn test_handshake_size_with_tag_overreports_by_one() {
        // BUG: size() over-reports by 1 when node_tag is Some
        // because write_optional_string doesn't use a bool flag
        let h = make_handshake("1.0.0", Some("Node1"), Some(50));
        let bytes = serialize_handshake(&h);
        assert_eq!(
            h.size(),
            bytes.len() + 1,
            "size() over-reports by 1 due to optional_string format mismatch"
        );
    }

    #[test]
    fn test_handshake_size_max_fields_overreports() {
        let h = make_handshake("1234567890123456", Some("1234567890123456"), Some(u64::MAX));
        let bytes = serialize_handshake(&h);
        assert_eq!(
            h.size(),
            bytes.len() + 1,
            "size() over-reports by 1 due to optional_string format mismatch"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Network variants
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_testnet_roundtrip() {
        let network_id: [u8; 16] = [0xFF; 16];
        let h = Handshake::new(
            Cow::Owned("1.0.0".to_string()),
            Network::Testnet,
            Cow::Owned(None),
            Cow::Owned(network_id),
            999u64,
            9090u16,
            1700000000u64,
            0u64,
            0u64,
            None,
            Cow::Owned(Hash::zero()),
            Cow::Owned(Hash::zero()),
            Cow::Owned(CumulativeDifficulty::from(0u64)),
            false,
            false,
        );

        let bytes = serialize_handshake(&h);
        let decoded = deserialize_handshake(&bytes).unwrap();

        assert_eq!(decoded.get_network(), &Network::Testnet);
        assert_eq!(decoded.get_network_id(), &[0xFF; 16]);
        assert!(!decoded.supports_fast_sync());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Truncated data
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_reject_truncated_data() {
        let h = make_handshake("1.0.0", None, None);
        let bytes = serialize_handshake(&h);

        // Note: bytes.len()-1 is NOT tested because the last field (supports_fast_sync)
        // uses unwrap_or(true) on read, so missing it still succeeds.
        for truncate_at in [1, 5, 10, 20, 40, bytes.len() - 2] {
            if truncate_at < bytes.len() {
                let result = deserialize_handshake(&bytes[..truncate_at]);
                assert!(
                    result.is_err(),
                    "Should fail at truncate_at={}",
                    truncate_at
                );
            }
        }
    }

    #[test]
    fn test_handshake_reject_empty_data() {
        let result = deserialize_handshake(&[]);
        assert!(result.is_err());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Extreme values
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_max_u64_values() {
        let h = Handshake::new(
            Cow::Owned("v".to_string()),
            Network::Mainnet,
            Cow::Owned(None),
            Cow::Owned([0u8; 16]),
            u64::MAX,
            u16::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            Some(u64::MAX),
            Cow::Owned(Hash::max()),
            Cow::Owned(Hash::max()),
            Cow::Owned(CumulativeDifficulty::from(u64::MAX)),
            true,
            true,
        );

        let bytes = serialize_handshake(&h);
        let decoded = deserialize_handshake(&bytes).unwrap();

        assert_eq!(decoded.get_peer_id(), u64::MAX);
        assert_eq!(decoded.get_local_port(), u16::MAX);
        assert_eq!(decoded.get_utc_time(), u64::MAX);
        assert_eq!(decoded.get_topoheight(), u64::MAX);
        assert_eq!(decoded.get_block_height(), u64::MAX);
        assert_eq!(decoded.get_pruned_topoheight(), &Some(u64::MAX));
        assert_eq!(decoded.get_block_top_hash(), &Hash::max());
    }

    #[test]
    fn test_handshake_zero_values() {
        let h = Handshake::new(
            Cow::Owned("v".to_string()),
            Network::Mainnet,
            Cow::Owned(None),
            Cow::Owned([0u8; 16]),
            0u64,
            0u16,
            0u64,
            0u64,
            0u64,
            None,
            Cow::Owned(Hash::zero()),
            Cow::Owned(Hash::zero()),
            Cow::Owned(CumulativeDifficulty::from(0u64)),
            false,
            false,
        );

        let bytes = serialize_handshake(&h);
        let decoded = deserialize_handshake(&bytes).unwrap();

        assert_eq!(decoded.get_peer_id(), 0);
        assert_eq!(decoded.get_topoheight(), 0);
        assert_eq!(decoded.get_block_height(), 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Deterministic serialization
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_handshake_deterministic_serialization() {
        let h1 = make_handshake("1.0.0", Some("Node"), Some(10));
        let h2 = make_handshake("1.0.0", Some("Node"), Some(10));

        let bytes1 = serialize_handshake(&h1);
        let bytes2 = serialize_handshake(&h2);

        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_handshake_different_inputs_different_bytes() {
        let h1 = make_handshake("1.0.0", Some("Node1"), None);
        let h2 = make_handshake("1.0.0", Some("Node2"), None);

        let bytes1 = serialize_handshake(&h1);
        let bytes2 = serialize_handshake(&h2);

        assert_ne!(bytes1, bytes2);
    }
}
