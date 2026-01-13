// TNS (TOS Name Service) transaction verification

use crate::{
    crypto::elgamal::CompressedHandle,
    tns::{
        calculate_message_fee, is_confusing_name, is_reserved_name, normalize_name, tns_name_hash,
        MAX_ENCRYPTED_SIZE, MAX_NAME_LENGTH, MAX_TTL, MIN_NAME_LENGTH, MIN_TTL, REGISTRATION_FEE,
    },
    transaction::{
        payload::{EphemeralMessagePayload, RegisterNamePayload},
        verify::VerificationError,
    },
};

/// Verify RegisterName payload format (stateless)
///
/// This function validates the name format according to RFC 5321 dot-atom aligned rules:
/// - Length: 3-64 characters
/// - Must start with a letter
/// - Cannot end with separator (. - _)
/// - Only lowercase letters, digits, and separators allowed
/// - No consecutive separators
/// - Not a reserved name
/// - Not a confusing name (phishing protection)
pub fn verify_register_name_format<E>(
    payload: &RegisterNamePayload,
) -> Result<(), VerificationError<E>> {
    // 0. Normalize first (reject spaces, non-ASCII, convert to lowercase)
    let name = normalize_name(payload.get_name()).map_err(|e| match e {
        crate::tns::NormalizeError::HasWhitespace => VerificationError::InvalidNameCharacter(' '),
        crate::tns::NormalizeError::NonAsciiCharacter(c) => {
            VerificationError::InvalidNameCharacter(c)
        }
    })?;

    // 1. Length check: 3-64 characters
    if !(MIN_NAME_LENGTH..=MAX_NAME_LENGTH).contains(&name.len()) {
        return Err(VerificationError::InvalidNameLength(name.len()));
    }

    // 2. Must start with letter
    let first_char = name
        .chars()
        .next()
        .ok_or(VerificationError::InvalidNameLength(0))?;
    if !first_char.is_ascii_lowercase() {
        return Err(VerificationError::InvalidNameStart);
    }

    // 3. Cannot end with separator
    if let Some(last) = name.chars().last() {
        if matches!(last, '.' | '-' | '_') {
            return Err(VerificationError::InvalidNameEnd);
        }
    }

    // 4. Character set & consecutive separator check
    let mut prev_is_separator = false;
    for c in name.chars() {
        let is_separator = matches!(c, '.' | '-' | '_');

        // Only allow a-z, 0-9, ., -, _
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && !is_separator {
            return Err(VerificationError::InvalidNameCharacter(c));
        }

        // No consecutive separators
        if is_separator && prev_is_separator {
            return Err(VerificationError::ConsecutiveSeparators);
        }
        prev_is_separator = is_separator;
    }

    // 5. Reserved name check
    if is_reserved_name(&name) {
        return Err(VerificationError::ReservedName(name));
    }

    // 6. Confusing name check (phishing protection)
    if is_confusing_name(&name) {
        return Err(VerificationError::ConfusingName(name));
    }

    Ok(())
}

/// Get the normalized name hash for a RegisterName payload
pub fn get_register_name_hash(payload: &RegisterNamePayload) -> Option<crate::crypto::Hash> {
    let name = normalize_name(payload.get_name()).ok()?;
    Some(tns_name_hash(&name))
}

/// Verify EphemeralMessage payload format (stateless)
///
/// This function validates the message format:
/// - TTL in range 100-86400 blocks
/// - Encrypted content not empty
/// - Encrypted content not too large (max 188 bytes)
/// - Cannot send to self
pub fn verify_ephemeral_message_format<E>(
    payload: &EphemeralMessagePayload,
) -> Result<(), VerificationError<E>> {
    // 1. TTL range check
    let ttl = payload.get_ttl_blocks();
    if !(MIN_TTL..=MAX_TTL).contains(&ttl) {
        return Err(VerificationError::InvalidMessageTTL(ttl));
    }

    // 2. Message cannot be empty
    let content = payload.get_encrypted_content();
    if content.is_empty() {
        return Err(VerificationError::EmptyMessage);
    }

    // 3. Message size check (max 188 bytes = 140 plaintext + 48 crypto overhead)
    if content.len() > MAX_ENCRYPTED_SIZE {
        return Err(VerificationError::MessageTooLarge(content.len()));
    }

    // 4. Self-message check (no state needed)
    if payload.get_sender_name_hash() == payload.get_recipient_name_hash() {
        return Err(VerificationError::SelfMessage);
    }

    // 5. Validate receiver_handle is a valid curve point
    // This prevents spam with undecryptable messages
    // We construct a CompressedHandle and verify it can be decompressed to a valid curve point
    use tos_crypto::curve25519_dalek::ristretto::CompressedRistretto;
    let compressed = match CompressedRistretto::from_slice(payload.get_receiver_handle()) {
        Ok(c) => c,
        Err(_) => return Err(VerificationError::InvalidReceiverHandle),
    };
    let handle = CompressedHandle::new(compressed);
    if handle.decompress().is_err() {
        return Err(VerificationError::InvalidReceiverHandle);
    }

    Ok(())
}

/// Compute message ID for replay protection
///
/// Message ID = blake3(sender_name_hash || recipient_name_hash || message_nonce)
pub fn compute_message_id(payload: &EphemeralMessagePayload) -> crate::crypto::Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(payload.get_sender_name_hash().as_bytes());
    hasher.update(payload.get_recipient_name_hash().as_bytes());
    hasher.update(&payload.get_message_nonce().to_le_bytes());
    let hash_bytes: [u8; 32] = hasher.finalize().into();
    crate::crypto::Hash::new(hash_bytes)
}

/// Verify that the transaction fee is sufficient for name registration
///
/// Name registration requires a fixed fee of REGISTRATION_FEE (0.1 TOS)
pub fn verify_register_name_fee<E>(tx_fee: u64) -> Result<(), VerificationError<E>> {
    if tx_fee < REGISTRATION_FEE {
        return Err(VerificationError::InsufficientTnsFee {
            required: REGISTRATION_FEE,
            provided: tx_fee,
        });
    }
    Ok(())
}

/// Verify that the transaction fee is sufficient for ephemeral message
///
/// Message fee depends on TTL:
/// - TTL <= 100 blocks (~30 min): BASE_MESSAGE_FEE (0.00005 TOS)
/// - TTL <= 28,800 blocks (~1 day): BASE_MESSAGE_FEE * 2 (0.00010 TOS)
/// - TTL > 28,800 blocks: BASE_MESSAGE_FEE * 3 (0.00015 TOS)
pub fn verify_ephemeral_message_fee<E>(
    payload: &EphemeralMessagePayload,
    tx_fee: u64,
) -> Result<(), VerificationError<E>> {
    let required_fee = calculate_message_fee(payload.get_ttl_blocks());
    if tx_fee < required_fee {
        return Err(VerificationError::InsufficientTnsFee {
            required: required_fee,
            provided: tx_fee,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Hash;

    fn make_register_name_payload(name: &str) -> RegisterNamePayload {
        RegisterNamePayload::new(name.to_string())
    }

    // ===== Valid Name Tests =====

    #[test]
    fn test_valid_name_simple() {
        let payload = make_register_name_payload("alice");
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_name_uppercase_normalized() {
        // Uppercase should be normalized to lowercase
        let payload = make_register_name_payload("Alice");
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_name_with_digits() {
        let payload = make_register_name_payload("bob123");
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_name_with_dot() {
        let payload = make_register_name_payload("john.doe");
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_name_with_hyphen() {
        let payload = make_register_name_payload("alice-wang");
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_name_with_underscore() {
        let payload = make_register_name_payload("user_name");
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_name_mixed_separators() {
        let payload = make_register_name_payload("a.b-c_d");
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_name_min_length() {
        let payload = make_register_name_payload("abc");
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_name_max_length() {
        let name = "a".repeat(64);
        let payload = make_register_name_payload(&name);
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    // ===== Invalid Name Tests =====

    #[test]
    fn test_invalid_name_too_short() {
        let payload = make_register_name_payload("ab");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameLength(2))
        ));
    }

    #[test]
    fn test_invalid_name_too_long() {
        let name = "a".repeat(65);
        let payload = make_register_name_payload(&name);
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameLength(65))
        ));
    }

    #[test]
    fn test_invalid_name_starts_with_digit() {
        let payload = make_register_name_payload("123abc");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameStart)
        ));
    }

    #[test]
    fn test_invalid_name_starts_with_dot() {
        let payload = make_register_name_payload(".alice");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameStart)
        ));
    }

    #[test]
    fn test_invalid_name_ends_with_dot() {
        let payload = make_register_name_payload("alice.");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameEnd)
        ));
    }

    #[test]
    fn test_invalid_name_ends_with_hyphen() {
        let payload = make_register_name_payload("alice-");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameEnd)
        ));
    }

    #[test]
    fn test_invalid_name_ends_with_underscore() {
        let payload = make_register_name_payload("alice_");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameEnd)
        ));
    }

    #[test]
    fn test_invalid_name_consecutive_dots() {
        let payload = make_register_name_payload("alice..bob");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::ConsecutiveSeparators)
        ));
    }

    #[test]
    fn test_invalid_name_consecutive_mixed_separators() {
        let payload = make_register_name_payload("alice.-bob");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::ConsecutiveSeparators)
        ));
    }

    #[test]
    fn test_invalid_name_at_symbol() {
        let payload = make_register_name_payload("alice@bob");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameCharacter('@'))
        ));
    }

    #[test]
    fn test_invalid_name_space() {
        let payload = make_register_name_payload("alice bob");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::InvalidNameCharacter(' '))
        ));
    }

    #[test]
    fn test_invalid_name_reserved_admin() {
        let payload = make_register_name_payload("admin");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::ReservedName(_))
        ));
    }

    #[test]
    fn test_invalid_name_reserved_system() {
        let payload = make_register_name_payload("system");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::ReservedName(_))
        ));
    }

    #[test]
    fn test_invalid_name_confusing_address_prefix() {
        let payload = make_register_name_payload("tos1alice");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::ConfusingName(_))
        ));
    }

    #[test]
    fn test_invalid_name_confusing_pure_digits() {
        let payload = make_register_name_payload("a123456");
        // This should pass - it starts with letter and isn't pure digits
        assert!(verify_register_name_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_invalid_name_confusing_phishing_keyword() {
        let payload = make_register_name_payload("alice_official");
        assert!(matches!(
            verify_register_name_format::<()>(&payload),
            Err(VerificationError::ConfusingName(_))
        ));
    }

    // ===== EphemeralMessage Format Tests =====

    fn make_ephemeral_message_payload(
        ttl: u32,
        content: Vec<u8>,
        sender: Hash,
        recipient: Hash,
    ) -> EphemeralMessagePayload {
        EphemeralMessagePayload::new(sender, recipient, 1, ttl, content, [0u8; 32])
    }

    #[test]
    fn test_valid_ephemeral_message() {
        let payload = make_ephemeral_message_payload(
            1000,
            vec![1, 2, 3, 4],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        assert!(verify_ephemeral_message_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_invalid_ephemeral_message_ttl_too_low() {
        let payload =
            make_ephemeral_message_payload(99, vec![1, 2, 3], Hash::zero(), Hash::new([1u8; 32]));
        assert!(matches!(
            verify_ephemeral_message_format::<()>(&payload),
            Err(VerificationError::InvalidMessageTTL(99))
        ));
    }

    #[test]
    fn test_invalid_ephemeral_message_ttl_too_high() {
        let payload = make_ephemeral_message_payload(
            86401,
            vec![1, 2, 3],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        assert!(matches!(
            verify_ephemeral_message_format::<()>(&payload),
            Err(VerificationError::InvalidMessageTTL(86401))
        ));
    }

    #[test]
    fn test_invalid_ephemeral_message_empty() {
        let payload =
            make_ephemeral_message_payload(1000, vec![], Hash::zero(), Hash::new([1u8; 32]));
        assert!(matches!(
            verify_ephemeral_message_format::<()>(&payload),
            Err(VerificationError::EmptyMessage)
        ));
    }

    #[test]
    fn test_invalid_ephemeral_message_too_large() {
        let content = vec![0u8; 200]; // > 188 bytes
        let payload = make_ephemeral_message_payload(
            1000,
            content.clone(),
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        assert!(matches!(
            verify_ephemeral_message_format::<()>(&payload),
            Err(VerificationError::MessageTooLarge(_))
        ));
    }

    #[test]
    fn test_invalid_ephemeral_message_self_send() {
        let payload =
            make_ephemeral_message_payload(1000, vec![1, 2, 3], Hash::zero(), Hash::zero());
        assert!(matches!(
            verify_ephemeral_message_format::<()>(&payload),
            Err(VerificationError::SelfMessage)
        ));
    }

    #[test]
    fn test_message_id_computation() {
        let payload = make_ephemeral_message_payload(
            1000,
            vec![1, 2, 3, 4],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        let msg_id = compute_message_id(&payload);

        // Should be deterministic
        let msg_id2 = compute_message_id(&payload);
        assert_eq!(msg_id, msg_id2);
    }

    // ===== Fee Verification Tests =====

    #[test]
    fn test_register_name_fee_sufficient() {
        use crate::tns::REGISTRATION_FEE;
        assert!(verify_register_name_fee::<()>(REGISTRATION_FEE).is_ok());
        assert!(verify_register_name_fee::<()>(REGISTRATION_FEE + 1000).is_ok());
    }

    #[test]
    fn test_register_name_fee_insufficient() {
        use crate::tns::REGISTRATION_FEE;
        let result = verify_register_name_fee::<()>(REGISTRATION_FEE - 1);
        assert!(matches!(
            result,
            Err(VerificationError::InsufficientTnsFee { .. })
        ));
    }

    #[test]
    fn test_ephemeral_message_fee_min_ttl() {
        use crate::tns::{BASE_MESSAGE_FEE, MIN_TTL};
        let payload = make_ephemeral_message_payload(
            MIN_TTL,
            vec![1, 2, 3],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        // Sufficient fee
        assert!(verify_ephemeral_message_fee::<()>(&payload, BASE_MESSAGE_FEE).is_ok());
        // Insufficient fee
        let result = verify_ephemeral_message_fee::<()>(&payload, BASE_MESSAGE_FEE - 1);
        assert!(matches!(
            result,
            Err(VerificationError::InsufficientTnsFee { .. })
        ));
    }

    #[test]
    fn test_ephemeral_message_fee_medium_ttl() {
        use crate::tns::BASE_MESSAGE_FEE;
        let payload = make_ephemeral_message_payload(
            5000, // Medium TTL (> 100, <= 17280)
            vec![1, 2, 3],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        let required = BASE_MESSAGE_FEE.saturating_mul(2);
        // Sufficient fee
        assert!(verify_ephemeral_message_fee::<()>(&payload, required).is_ok());
        // Insufficient fee
        let result = verify_ephemeral_message_fee::<()>(&payload, required - 1);
        assert!(matches!(
            result,
            Err(VerificationError::InsufficientTnsFee { .. })
        ));
    }

    #[test]
    fn test_ephemeral_message_fee_high_ttl() {
        use crate::tns::BASE_MESSAGE_FEE;
        let payload = make_ephemeral_message_payload(
            50000, // High TTL (> 17280)
            vec![1, 2, 3],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        let required = BASE_MESSAGE_FEE.saturating_mul(3);
        // Sufficient fee
        assert!(verify_ephemeral_message_fee::<()>(&payload, required).is_ok());
        // Insufficient fee
        let result = verify_ephemeral_message_fee::<()>(&payload, required - 1);
        assert!(matches!(
            result,
            Err(VerificationError::InsufficientTnsFee { .. })
        ));
    }

    // ===== Edge Case Tests =====

    #[test]
    fn test_valid_ephemeral_message_min_ttl_boundary() {
        use crate::tns::MIN_TTL;
        let payload = make_ephemeral_message_payload(
            MIN_TTL, // Exactly 100 blocks
            vec![1, 2, 3],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        assert!(verify_ephemeral_message_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_ephemeral_message_max_ttl_boundary() {
        use crate::tns::MAX_TTL;
        let payload = make_ephemeral_message_payload(
            MAX_TTL, // Exactly 86400 blocks
            vec![1, 2, 3],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        assert!(verify_ephemeral_message_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_valid_ephemeral_message_max_content_boundary() {
        use crate::tns::MAX_ENCRYPTED_SIZE;
        let content = vec![0u8; MAX_ENCRYPTED_SIZE]; // Exactly 188 bytes
        let payload =
            make_ephemeral_message_payload(1000, content, Hash::zero(), Hash::new([1u8; 32]));
        assert!(verify_ephemeral_message_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_invalid_ephemeral_message_content_one_over_max() {
        use crate::tns::MAX_ENCRYPTED_SIZE;
        let content = vec![0u8; MAX_ENCRYPTED_SIZE + 1]; // 189 bytes
        let payload =
            make_ephemeral_message_payload(1000, content, Hash::zero(), Hash::new([1u8; 32]));
        assert!(matches!(
            verify_ephemeral_message_format::<()>(&payload),
            Err(VerificationError::MessageTooLarge(189))
        ));
    }

    #[test]
    fn test_calculate_message_fee_tier_boundaries() {
        use crate::tns::{calculate_message_fee, BASE_MESSAGE_FEE, MIN_TTL};

        // Tier 1: TTL <= 100 (MIN_TTL)
        assert_eq!(calculate_message_fee(MIN_TTL), BASE_MESSAGE_FEE);
        assert_eq!(calculate_message_fee(1), BASE_MESSAGE_FEE); // Below MIN_TTL

        // Tier 2: 100 < TTL <= 28800 (TTL_ONE_DAY)
        assert_eq!(calculate_message_fee(101), BASE_MESSAGE_FEE * 2);
        assert_eq!(calculate_message_fee(28800), BASE_MESSAGE_FEE * 2);

        // Tier 3: TTL > 28800
        assert_eq!(calculate_message_fee(28801), BASE_MESSAGE_FEE * 3);
        assert_eq!(calculate_message_fee(86400), BASE_MESSAGE_FEE * 3);
    }

    #[test]
    fn test_ephemeral_message_fee_tier_boundaries() {
        use crate::tns::BASE_MESSAGE_FEE;

        // Tier 1 boundary: TTL = 100
        let payload_tier1 =
            make_ephemeral_message_payload(100, vec![1, 2, 3], Hash::zero(), Hash::new([1u8; 32]));
        assert!(verify_ephemeral_message_fee::<()>(&payload_tier1, BASE_MESSAGE_FEE).is_ok());

        // Tier 2 boundary start: TTL = 101
        let payload_tier2_start =
            make_ephemeral_message_payload(101, vec![1, 2, 3], Hash::zero(), Hash::new([1u8; 32]));
        assert!(
            verify_ephemeral_message_fee::<()>(&payload_tier2_start, BASE_MESSAGE_FEE * 2).is_ok()
        );
        // Tier 1 fee should be insufficient for tier 2
        assert!(
            verify_ephemeral_message_fee::<()>(&payload_tier2_start, BASE_MESSAGE_FEE).is_err()
        );

        // Tier 2 boundary end: TTL = 28800
        let payload_tier2_end = make_ephemeral_message_payload(
            28800,
            vec![1, 2, 3],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        assert!(
            verify_ephemeral_message_fee::<()>(&payload_tier2_end, BASE_MESSAGE_FEE * 2).is_ok()
        );

        // Tier 3 boundary start: TTL = 28801
        let payload_tier3 = make_ephemeral_message_payload(
            28801,
            vec![1, 2, 3],
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        assert!(verify_ephemeral_message_fee::<()>(&payload_tier3, BASE_MESSAGE_FEE * 3).is_ok());
        // Tier 2 fee should be insufficient for tier 3
        assert!(verify_ephemeral_message_fee::<()>(&payload_tier3, BASE_MESSAGE_FEE * 2).is_err());
    }

    #[test]
    fn test_valid_ephemeral_message_single_byte_content() {
        let payload = make_ephemeral_message_payload(
            1000,
            vec![1], // Minimum content: 1 byte
            Hash::zero(),
            Hash::new([1u8; 32]),
        );
        assert!(verify_ephemeral_message_format::<()>(&payload).is_ok());
    }

    #[test]
    fn test_message_id_deterministic() {
        let payload1 =
            make_ephemeral_message_payload(1000, vec![1, 2, 3], Hash::zero(), Hash::new([1u8; 32]));
        let payload2 =
            make_ephemeral_message_payload(1000, vec![1, 2, 3], Hash::zero(), Hash::new([1u8; 32]));

        // Same parameters should produce same message ID
        assert_eq!(compute_message_id(&payload1), compute_message_id(&payload2));
    }

    #[test]
    fn test_message_id_changes_with_nonce() {
        let sender = Hash::zero();
        let recipient = Hash::new([1u8; 32]);

        let payload1 = EphemeralMessagePayload::new(
            sender.clone(),
            recipient.clone(),
            1, // nonce 1
            1000,
            vec![1, 2, 3],
            [0u8; 32],
        );
        let payload2 = EphemeralMessagePayload::new(
            sender,
            recipient,
            2, // nonce 2
            1000,
            vec![1, 2, 3],
            [0u8; 32],
        );

        // Different nonces should produce different message IDs
        assert_ne!(compute_message_id(&payload1), compute_message_id(&payload2));
    }
}
