//! Security tests for Codex Audit findings (November 2024)
//!
//! These tests verify the fixes for security issues identified in the Codex audit:
//! - Issue 2: DAA window BFS traversal limit (DoS prevention)
//! - Issue 3: Chain validator header field validation (daa_score, timestamp)
//! - Issue 4: Header serialization panic prevention (validate_parent_levels)
//!
//! Reference: docs/CONSENSUS_SECURITY_AUDIT.md

#![allow(dead_code)]
#![allow(clippy::disallowed_methods)]

use tos_common::{
    block::{BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    config::MAX_PARENT_LEVELS,
    crypto::{BlueWorkType, Hash, Hashable, KeyPair},
    serializer::Serializer,
};

/// ============================================================================
/// Issue 4: Header Serialization Panic Prevention Tests
/// ============================================================================
mod header_validation_tests {
    use super::*;

    /// Test that validate_parent_levels() accepts valid headers
    #[test]
    fn test_valid_header_passes_validation() {
        let miner = KeyPair::new().get_public_key().compress();
        let header = BlockHeader::new(
            BlockVersion::Baseline,
            vec![vec![Hash::zero()]],
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        assert!(
            header.validate_parent_levels().is_ok(),
            "Valid header should pass validation"
        );
    }

    /// Test that validate_parent_levels() rejects headers with too many levels
    #[test]
    fn test_too_many_parent_levels_rejected() {
        let miner = KeyPair::new().get_public_key().compress();

        // Create MAX_PARENT_LEVELS + 1 levels (exceeds limit)
        let too_many_levels: Vec<Vec<Hash>> = (0..=MAX_PARENT_LEVELS)
            .map(|_| vec![Hash::zero()])
            .collect();

        let header = BlockHeader::new(
            BlockVersion::Baseline,
            too_many_levels,
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        assert!(
            header.validate_parent_levels().is_err(),
            "Header with {} levels should fail (MAX_PARENT_LEVELS={})",
            MAX_PARENT_LEVELS + 1,
            MAX_PARENT_LEVELS
        );
    }

    /// Test that validate_parent_levels() rejects levels with too many parents
    #[test]
    fn test_too_many_parents_in_level_rejected() {
        let miner = KeyPair::new().get_public_key().compress();

        // Create a level with 256 parents (exceeds u8 max of 255)
        let too_many_parents: Vec<Hash> = (0..256).map(|_| Hash::zero()).collect();

        let header = BlockHeader::new(
            BlockVersion::Baseline,
            vec![too_many_parents],
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        assert!(
            header.validate_parent_levels().is_err(),
            "Header with 256 parents in a level should fail (max 255)"
        );
    }

    /// Test that deserialization rejects headers with too many parent levels
    #[test]
    fn test_deserialize_rejects_too_many_levels() {
        let miner = KeyPair::new().get_public_key().compress();

        // Create a valid header first
        let valid_header = BlockHeader::new(
            BlockVersion::Baseline,
            vec![vec![Hash::zero()]],
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        // Serialize it
        let mut bytes = valid_header.to_bytes();

        // Corrupt the levels count byte (byte 1 after version) to exceed MAX_PARENT_LEVELS
        // Version is 1 byte, then comes levels_count
        if bytes.len() > 1 {
            bytes[1] = (MAX_PARENT_LEVELS + 1) as u8;
        }

        // Deserialization should fail
        let result = BlockHeader::from_bytes(&bytes);
        assert!(
            result.is_err(),
            "Deserialization should reject corrupted level count"
        );
    }

    /// Test boundary: exactly MAX_PARENT_LEVELS should be valid
    #[test]
    fn test_max_parent_levels_boundary() {
        let miner = KeyPair::new().get_public_key().compress();

        // Exactly MAX_PARENT_LEVELS levels (should be valid)
        let max_levels: Vec<Vec<Hash>> =
            (0..MAX_PARENT_LEVELS).map(|_| vec![Hash::zero()]).collect();

        let header = BlockHeader::new(
            BlockVersion::Baseline,
            max_levels,
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        assert!(
            header.validate_parent_levels().is_ok(),
            "Header with exactly MAX_PARENT_LEVELS={} should be valid",
            MAX_PARENT_LEVELS
        );
    }

    /// Test boundary: exactly 255 parents in a level should be valid
    #[test]
    fn test_max_parents_per_level_boundary() {
        let miner = KeyPair::new().get_public_key().compress();

        // Exactly 255 parents (should be valid)
        let max_parents: Vec<Hash> = (0..255).map(|_| Hash::zero()).collect();

        let header = BlockHeader::new(
            BlockVersion::Baseline,
            vec![max_parents],
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        assert!(
            header.validate_parent_levels().is_ok(),
            "Header with exactly 255 parents in a level should be valid"
        );
    }

    /// Test that empty parents_by_level is valid
    #[test]
    fn test_empty_parents_valid() {
        let miner = KeyPair::new().get_public_key().compress();

        let header = BlockHeader::new(
            BlockVersion::Baseline,
            vec![], // Empty parents (genesis-like)
            0,
            0,
            BlueWorkType::zero(),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        assert!(
            header.validate_parent_levels().is_ok(),
            "Header with empty parents should be valid"
        );
    }

    /// Test that try_to_bytes() succeeds for valid headers
    #[test]
    fn test_try_to_bytes_valid_header() {
        let miner = KeyPair::new().get_public_key().compress();
        let header = BlockHeader::new(
            BlockVersion::Baseline,
            vec![vec![Hash::zero()]],
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        // SECURITY FIX (Codex Audit): try_to_bytes() should succeed for valid headers
        let result = header.try_to_bytes();
        assert!(
            result.is_ok(),
            "try_to_bytes() should succeed for valid header"
        );

        // Verify the bytes match the infallible version
        assert_eq!(
            result.unwrap(),
            header.to_bytes(),
            "try_to_bytes() should produce same bytes as to_bytes()"
        );
    }

    /// Test that try_to_bytes() fails for headers with too many levels
    #[test]
    fn test_try_to_bytes_too_many_levels() {
        let miner = KeyPair::new().get_public_key().compress();

        // Create MAX_PARENT_LEVELS + 1 levels (exceeds limit)
        let too_many_levels: Vec<Vec<Hash>> = (0..=MAX_PARENT_LEVELS)
            .map(|_| vec![Hash::zero()])
            .collect();

        let header = BlockHeader::new(
            BlockVersion::Baseline,
            too_many_levels,
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        // SECURITY FIX (Codex Audit): try_to_bytes() should fail for invalid headers
        let result = header.try_to_bytes();
        assert!(
            result.is_err(),
            "try_to_bytes() should fail for header with too many levels"
        );
    }

    /// Test that try_to_bytes() fails for headers with too many parents in a level
    #[test]
    fn test_try_to_bytes_too_many_parents() {
        let miner = KeyPair::new().get_public_key().compress();

        // Create a level with 256 parents (exceeds u8 max of 255)
        let too_many_parents: Vec<Hash> = (0..256).map(|_| Hash::zero()).collect();

        let header = BlockHeader::new(
            BlockVersion::Baseline,
            vec![too_many_parents],
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        // SECURITY FIX (Codex Audit): try_to_bytes() should fail for invalid headers
        let result = header.try_to_bytes();
        assert!(
            result.is_err(),
            "try_to_bytes() should fail for header with 256 parents"
        );
    }
}

/// ============================================================================
/// Issue 2: DAA Window BFS Traversal Limit Tests
/// ============================================================================
mod daa_window_limit_tests {
    /// Test that DAA window constants are properly defined
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_daa_window_constants_exist() {
        use tos_daemon::core::ghostdag::daa::{DAA_WINDOW_SIZE, MAX_DAA_WINDOW_BLOCKS};

        // Verify the constant is reasonable (4x window size)
        // These assertions on constants are intentional - they document and verify
        // the expected values of security-critical constants at test time.
        assert!(
            MAX_DAA_WINDOW_BLOCKS >= DAA_WINDOW_SIZE as usize,
            "MAX_DAA_WINDOW_BLOCKS should be at least DAA_WINDOW_SIZE"
        );

        // Verify it's not unbounded
        assert!(
            MAX_DAA_WINDOW_BLOCKS <= 100_000,
            "MAX_DAA_WINDOW_BLOCKS should have a reasonable upper bound"
        );
    }

    /// Test that the error type for DAA window overflow exists
    #[test]
    fn test_daa_window_error_variant_exists() {
        use tos_daemon::core::error::BlockchainError;

        // Create the error to verify it exists
        let error = BlockchainError::DaaWindowTooLarge(10000, 8064);

        // Verify error message format
        let error_msg = format!("{}", error);
        assert!(
            error_msg.contains("DAA window"),
            "Error message should mention DAA window: {}",
            error_msg
        );
    }
}

/// ============================================================================
/// Issue 3: Chain Validator Header Field Validation Tests
/// ============================================================================
mod chain_validator_tests {
    use super::*;

    /// Test that InvalidDaaScore error variant exists
    #[test]
    fn test_invalid_daa_score_error_exists() {
        use tos_daemon::core::error::BlockchainError;

        let block_hash = Hash::zero();
        let expected = 100u64;
        let actual = 50u64;

        let error = BlockchainError::InvalidDaaScore(block_hash, expected, actual);

        let error_msg = format!("{}", error);
        assert!(
            error_msg.contains("DAA score") || error_msg.contains("daa_score"),
            "Error message should mention DAA score: {}",
            error_msg
        );
    }

    /// Test that TimestampIsLessThanParent error variant exists
    #[test]
    fn test_timestamp_less_than_parent_error_exists() {
        use tos_daemon::core::error::BlockchainError;

        let timestamp = 1234567890u64;
        let error = BlockchainError::TimestampIsLessThanParent(timestamp);

        let error_msg = format!("{}", error);
        assert!(
            error_msg.to_lowercase().contains("timestamp"),
            "Error message should mention timestamp: {}",
            error_msg
        );
    }
}

/// ============================================================================
/// Regression Tests: Ensure fixes don't break normal operation
/// ============================================================================
mod regression_tests {
    use super::*;

    /// Test that normal header serialization/deserialization still works
    #[test]
    fn test_normal_header_roundtrip() {
        let miner = KeyPair::new().get_public_key().compress();

        let original = BlockHeader::new(
            BlockVersion::Baseline,
            vec![vec![Hash::zero(), Hash::new([1u8; 32])]],
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::new([2u8; 32]),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::new([3u8; 32]),
            Hash::new([4u8; 32]),
            Hash::new([5u8; 32]),
        );

        // Validate before serialization
        assert!(
            original.validate_parent_levels().is_ok(),
            "Normal header should pass validation"
        );

        // Serialize
        let bytes = original.to_bytes();

        // Deserialize
        let restored = BlockHeader::from_bytes(&bytes).expect("Deserialization should succeed");

        // Verify fields match
        assert_eq!(original.blue_score, restored.blue_score);
        assert_eq!(original.daa_score, restored.daa_score);
        assert_eq!(original.blue_work, restored.blue_work);
        assert_eq!(original.timestamp, restored.timestamp);
        assert_eq!(original.nonce, restored.nonce);
        assert_eq!(original.bits, restored.bits);
        assert_eq!(original.hash(), restored.hash());
    }

    /// Test that multi-level parent structures work correctly
    #[test]
    fn test_multi_level_parents_roundtrip() {
        let miner = KeyPair::new().get_public_key().compress();

        let parents_by_level = vec![
            vec![Hash::zero(), Hash::new([1u8; 32])], // Level 0: 2 parents
            vec![Hash::new([2u8; 32])],               // Level 1: 1 parent
            vec![Hash::new([3u8; 32]), Hash::new([4u8; 32])], // Level 2: 2 parents
        ];

        let original = BlockHeader::new(
            BlockVersion::Baseline,
            parents_by_level.clone(),
            100,
            100,
            BlueWorkType::from(1000u64),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        // Validate
        assert!(original.validate_parent_levels().is_ok());

        // Roundtrip
        let bytes = original.to_bytes();
        let restored = BlockHeader::from_bytes(&bytes).unwrap();

        // Verify parent structure is preserved
        assert_eq!(
            restored.get_parents_by_level().len(),
            parents_by_level.len()
        );
        for (i, level) in parents_by_level.iter().enumerate() {
            assert_eq!(
                restored.get_parents_by_level()[i].len(),
                level.len(),
                "Level {} parent count mismatch",
                i
            );
        }
    }
}
