#![allow(clippy::disallowed_methods)]

//! TNS (TOS Name Service) E2E Tests
//!
//! Tests for the TNS format validation, name hashing, and message computation.
//! These tests verify the core logic without requiring full blockchain state.

use tos_common::{
    crypto::Hash,
    tns::{
        calculate_message_fee, is_confusing_name, is_reserved_name, normalize_name, tns_name_hash,
        BASE_MESSAGE_FEE, MAX_NAME_LENGTH, MAX_TTL, MIN_TTL,
    },
    transaction::{
        verify::{
            compute_message_id, get_register_name_hash, verify_ephemeral_message_format,
            verify_register_name_format, VerificationError,
        },
        EphemeralMessagePayload, RegisterNamePayload,
    },
};

// ============================================================================
// RegisterName Format Tests
// ============================================================================

#[test]
fn test_register_name_valid_simple() {
    let payload = RegisterNamePayload::new("alice".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_valid_uppercase_normalized() {
    let payload = RegisterNamePayload::new("Alice".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_valid_with_digits() {
    let payload = RegisterNamePayload::new("bob123".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_valid_with_dot() {
    let payload = RegisterNamePayload::new("john.doe".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_valid_with_hyphen() {
    let payload = RegisterNamePayload::new("alice-wang".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_valid_with_underscore() {
    let payload = RegisterNamePayload::new("user_name".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_valid_mixed_separators() {
    let payload = RegisterNamePayload::new("a.b-c_d".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_valid_min_length() {
    let payload = RegisterNamePayload::new("abc".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_valid_max_length() {
    let name = "a".repeat(MAX_NAME_LENGTH);
    let payload = RegisterNamePayload::new(name);
    let result = verify_register_name_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_register_name_invalid_too_short() {
    let payload = RegisterNamePayload::new("ab".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(
        result,
        Err(VerificationError::InvalidNameLength(2))
    ));
}

#[test]
fn test_register_name_invalid_too_long() {
    let name = "a".repeat(MAX_NAME_LENGTH + 1);
    let payload = RegisterNamePayload::new(name);
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(
        result,
        Err(VerificationError::InvalidNameLength(65))
    ));
}

#[test]
fn test_register_name_invalid_starts_with_digit() {
    let payload = RegisterNamePayload::new("123abc".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::InvalidNameStart)));
}

#[test]
fn test_register_name_invalid_starts_with_dot() {
    let payload = RegisterNamePayload::new(".alice".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::InvalidNameStart)));
}

#[test]
fn test_register_name_invalid_ends_with_dot() {
    let payload = RegisterNamePayload::new("alice.".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::InvalidNameEnd)));
}

#[test]
fn test_register_name_invalid_ends_with_hyphen() {
    let payload = RegisterNamePayload::new("alice-".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::InvalidNameEnd)));
}

#[test]
fn test_register_name_invalid_ends_with_underscore() {
    let payload = RegisterNamePayload::new("alice_".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::InvalidNameEnd)));
}

#[test]
fn test_register_name_invalid_consecutive_dots() {
    let payload = RegisterNamePayload::new("alice..bob".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(
        result,
        Err(VerificationError::ConsecutiveSeparators)
    ));
}

#[test]
fn test_register_name_invalid_consecutive_mixed_separators() {
    let payload = RegisterNamePayload::new("alice.-bob".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(
        result,
        Err(VerificationError::ConsecutiveSeparators)
    ));
}

#[test]
fn test_register_name_invalid_at_symbol() {
    let payload = RegisterNamePayload::new("alice@bob".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(
        result,
        Err(VerificationError::InvalidNameCharacter('@'))
    ));
}

#[test]
fn test_register_name_invalid_space() {
    let payload = RegisterNamePayload::new("alice bob".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(
        result,
        Err(VerificationError::InvalidNameCharacter(' '))
    ));
}

#[test]
fn test_register_name_invalid_reserved_admin() {
    let payload = RegisterNamePayload::new("admin".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::ReservedName(_))));
}

#[test]
fn test_register_name_invalid_reserved_system() {
    let payload = RegisterNamePayload::new("system".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::ReservedName(_))));
}

#[test]
fn test_register_name_invalid_confusing_address_prefix() {
    let payload = RegisterNamePayload::new("tos1alice".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::ConfusingName(_))));
}

#[test]
fn test_register_name_invalid_confusing_phishing_keyword() {
    let payload = RegisterNamePayload::new("alice_official".to_string());
    let result = verify_register_name_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::ConfusingName(_))));
}

// ============================================================================
// EphemeralMessage Format Tests
// ============================================================================

#[test]
fn test_ephemeral_message_valid() {
    let alice_hash = tns_name_hash("alice");
    let bob_hash = tns_name_hash("bob");

    let payload =
        EphemeralMessagePayload::new(alice_hash, bob_hash, 1, 1000, vec![1, 2, 3, 4], [0u8; 32]);

    let result = verify_ephemeral_message_format::<()>(&payload);
    assert!(result.is_ok());
}

#[test]
fn test_ephemeral_message_invalid_self_send() {
    let alice_hash = tns_name_hash("alice");

    let payload = EphemeralMessagePayload::new(
        alice_hash.clone(),
        alice_hash, // Same as sender
        1,
        1000,
        vec![1, 2, 3, 4],
        [0u8; 32],
    );

    let result = verify_ephemeral_message_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::SelfMessage)));
}

#[test]
fn test_ephemeral_message_invalid_ttl_too_low() {
    let alice_hash = tns_name_hash("alice");
    let bob_hash = tns_name_hash("bob");

    let payload = EphemeralMessagePayload::new(
        alice_hash,
        bob_hash,
        1,
        MIN_TTL - 1, // Below minimum
        vec![1, 2, 3, 4],
        [0u8; 32],
    );

    let result = verify_ephemeral_message_format::<()>(&payload);
    assert!(matches!(
        result,
        Err(VerificationError::InvalidMessageTTL(_))
    ));
}

#[test]
fn test_ephemeral_message_invalid_ttl_too_high() {
    let alice_hash = tns_name_hash("alice");
    let bob_hash = tns_name_hash("bob");

    let payload = EphemeralMessagePayload::new(
        alice_hash,
        bob_hash,
        1,
        MAX_TTL + 1, // Above maximum
        vec![1, 2, 3, 4],
        [0u8; 32],
    );

    let result = verify_ephemeral_message_format::<()>(&payload);
    assert!(matches!(
        result,
        Err(VerificationError::InvalidMessageTTL(_))
    ));
}

#[test]
fn test_ephemeral_message_invalid_empty() {
    let alice_hash = tns_name_hash("alice");
    let bob_hash = tns_name_hash("bob");

    let payload = EphemeralMessagePayload::new(
        alice_hash,
        bob_hash,
        1,
        1000,
        vec![], // Empty content
        [0u8; 32],
    );

    let result = verify_ephemeral_message_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::EmptyMessage)));
}

#[test]
fn test_ephemeral_message_invalid_too_large() {
    let alice_hash = tns_name_hash("alice");
    let bob_hash = tns_name_hash("bob");

    // MAX_ENCRYPTED_SIZE = 188 bytes (140 + 48 overhead)
    let large_content = vec![0u8; 200];

    let payload =
        EphemeralMessagePayload::new(alice_hash, bob_hash, 1, 1000, large_content, [0u8; 32]);

    let result = verify_ephemeral_message_format::<()>(&payload);
    assert!(matches!(result, Err(VerificationError::MessageTooLarge(_))));
}

// ============================================================================
// Name Hashing Tests
// ============================================================================

#[test]
fn test_name_hash_consistency() {
    let name = "alice";
    let hash1 = tns_name_hash(name);
    let hash2 = tns_name_hash(name);

    // Same name should produce same hash
    assert_eq!(hash1, hash2);
}

#[test]
fn test_name_hash_different_names() {
    let hash_alice = tns_name_hash("alice");
    let hash_bob = tns_name_hash("bob");

    // Different names should produce different hashes
    assert_ne!(hash_alice, hash_bob);
}

#[test]
fn test_name_hash_case_insensitive() {
    // After normalization, case should not matter
    let normalized_upper = normalize_name("ALICE").unwrap();
    let normalized_lower = normalize_name("alice").unwrap();

    let hash_upper = tns_name_hash(&normalized_upper);
    let hash_lower = tns_name_hash(&normalized_lower);

    assert_eq!(hash_upper, hash_lower);
}

#[test]
fn test_name_hash_non_zero() {
    let hash = tns_name_hash("alice");
    assert_ne!(hash, Hash::zero());
}

// ============================================================================
// Message ID Computation Tests
// ============================================================================

#[test]
fn test_message_id_consistency() {
    let alice_hash = tns_name_hash("alice");
    let bob_hash = tns_name_hash("bob");

    let payload1 = EphemeralMessagePayload::new(
        alice_hash.clone(),
        bob_hash.clone(),
        1,
        1000,
        vec![1, 2, 3],
        [0u8; 32],
    );
    let payload2 =
        EphemeralMessagePayload::new(alice_hash, bob_hash, 1, 1000, vec![1, 2, 3], [0u8; 32]);

    let id1 = compute_message_id(&payload1);
    let id2 = compute_message_id(&payload2);

    // Same parameters should produce same message ID
    assert_eq!(id1, id2);
}

#[test]
fn test_message_id_different_nonce() {
    let alice_hash = tns_name_hash("alice");
    let bob_hash = tns_name_hash("bob");

    let payload1 = EphemeralMessagePayload::new(
        alice_hash.clone(),
        bob_hash.clone(),
        1, // nonce 1
        1000,
        vec![1, 2, 3],
        [0u8; 32],
    );
    let payload2 = EphemeralMessagePayload::new(
        alice_hash,
        bob_hash,
        2, // nonce 2
        1000,
        vec![1, 2, 3],
        [0u8; 32],
    );

    let id1 = compute_message_id(&payload1);
    let id2 = compute_message_id(&payload2);

    // Different nonce should produce different message ID
    assert_ne!(id1, id2);
}

#[test]
fn test_message_id_different_sender() {
    let alice_hash = tns_name_hash("alice");
    let bob_hash = tns_name_hash("bob");
    let charlie_hash = tns_name_hash("charlie");

    let payload1 = EphemeralMessagePayload::new(
        alice_hash, // from alice
        charlie_hash.clone(),
        1,
        1000,
        vec![1, 2, 3],
        [0u8; 32],
    );
    let payload2 = EphemeralMessagePayload::new(
        bob_hash, // from bob
        charlie_hash,
        1,
        1000,
        vec![1, 2, 3],
        [0u8; 32],
    );

    let id1 = compute_message_id(&payload1);
    let id2 = compute_message_id(&payload2);

    // Different sender should produce different message ID
    assert_ne!(id1, id2);
}

// ============================================================================
// Message Fee Calculation Tests
// ============================================================================

#[test]
fn test_message_fee_min_ttl() {
    let fee = calculate_message_fee(MIN_TTL);
    assert_eq!(fee, BASE_MESSAGE_FEE);
}

#[test]
fn test_message_fee_tier_1() {
    // TTL <= 100 blocks = 1x base fee
    let fee = calculate_message_fee(100);
    assert_eq!(fee, BASE_MESSAGE_FEE);
}

#[test]
fn test_message_fee_tier_2() {
    // TTL > 100 and <= 28800 blocks = 2x base fee
    let fee = calculate_message_fee(1000);
    assert_eq!(fee, BASE_MESSAGE_FEE * 2);
}

#[test]
fn test_message_fee_tier_3() {
    // TTL > 28800 blocks = 3x base fee
    let fee = calculate_message_fee(50000);
    assert_eq!(fee, BASE_MESSAGE_FEE * 3);
}

#[test]
fn test_message_fee_max_ttl() {
    let fee = calculate_message_fee(MAX_TTL);
    assert_eq!(fee, BASE_MESSAGE_FEE * 3);
}

// ============================================================================
// Normalize Name Tests
// ============================================================================

#[test]
fn test_normalize_name_lowercase() {
    let result = normalize_name("ALICE");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "alice");
}

#[test]
fn test_normalize_name_mixed_case() {
    let result = normalize_name("AlIcE");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "alice");
}

#[test]
fn test_normalize_name_leading_space() {
    let result = normalize_name(" alice");
    assert!(result.is_err());
}

#[test]
fn test_normalize_name_trailing_space() {
    let result = normalize_name("alice ");
    assert!(result.is_err());
}

#[test]
fn test_normalize_name_non_ascii() {
    // Cyrillic 'a' which looks like Latin 'a'
    let result = normalize_name("аlice");
    assert!(result.is_err());
}

// ============================================================================
// Reserved Name Tests
// ============================================================================

#[test]
fn test_reserved_name_admin() {
    assert!(is_reserved_name("admin"));
}

#[test]
fn test_reserved_name_system() {
    assert!(is_reserved_name("system"));
}

#[test]
fn test_reserved_name_validator() {
    assert!(is_reserved_name("validator"));
}

#[test]
fn test_not_reserved_name() {
    assert!(!is_reserved_name("alice"));
    assert!(!is_reserved_name("bob"));
    assert!(!is_reserved_name("johndoe"));
}

// ============================================================================
// Confusing Name Tests
// ============================================================================

#[test]
fn test_confusing_name_address_prefix_tos1() {
    assert!(is_confusing_name("tos1alice"));
}

#[test]
fn test_confusing_name_address_prefix_tst1() {
    assert!(is_confusing_name("tst1bob"));
}

#[test]
fn test_confusing_name_address_prefix_0x() {
    assert!(is_confusing_name("0xdeadbeef"));
}

#[test]
fn test_confusing_name_phishing_official() {
    assert!(is_confusing_name("alice_official"));
}

#[test]
fn test_confusing_name_phishing_verified() {
    assert!(is_confusing_name("verified_alice"));
}

#[test]
fn test_confusing_name_phishing_support() {
    assert!(is_confusing_name("alice_support"));
}

#[test]
fn test_not_confusing_name() {
    assert!(!is_confusing_name("alice"));
    assert!(!is_confusing_name("bob.doe"));
    assert!(!is_confusing_name("user123"));
}

// ============================================================================
// Payload Getters Tests
// ============================================================================

#[test]
fn test_register_name_payload_getter() {
    let payload = RegisterNamePayload::new("alice".to_string());
    assert_eq!(payload.get_name(), "alice");
}

#[test]
fn test_ephemeral_message_payload_getters() {
    let sender_hash = Hash::new([1u8; 32]);
    let recipient_hash = Hash::new([2u8; 32]);
    let content = vec![1, 2, 3, 4];
    let handle = [5u8; 32];

    let payload = EphemeralMessagePayload::new(
        sender_hash.clone(),
        recipient_hash.clone(),
        42,
        1000,
        content.clone(),
        handle,
    );

    assert_eq!(payload.get_sender_name_hash(), &sender_hash);
    assert_eq!(payload.get_recipient_name_hash(), &recipient_hash);
    assert_eq!(payload.get_message_nonce(), 42);
    assert_eq!(payload.get_ttl_blocks(), 1000);
    assert_eq!(payload.get_encrypted_content(), &content[..]);
    assert_eq!(payload.get_receiver_handle(), &handle);
}

// ============================================================================
// Get Register Name Hash Tests
// ============================================================================

#[test]
fn test_get_register_name_hash_valid() {
    let payload = RegisterNamePayload::new("alice".to_string());
    let hash = get_register_name_hash(&payload);
    assert!(hash.is_some());
    assert_eq!(hash.unwrap(), tns_name_hash("alice"));
}

#[test]
fn test_get_register_name_hash_normalized() {
    let payload = RegisterNamePayload::new("ALICE".to_string());
    let hash = get_register_name_hash(&payload);
    assert!(hash.is_some());
    // Should be normalized to lowercase before hashing
    assert_eq!(hash.unwrap(), tns_name_hash("alice"));
}

#[test]
fn test_get_register_name_hash_invalid() {
    // Invalid name (has leading space) should return None
    let payload = RegisterNamePayload::new(" alice".to_string());
    let hash = get_register_name_hash(&payload);
    assert!(hash.is_none());
}
