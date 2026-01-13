// TNS Name Hash Function

use crate::crypto::Hash;

/// Compute TNS name hash using blake3
/// Input name should be the username part without @tos.network suffix
/// The name is normalized to lowercase before hashing
pub fn tns_name_hash(name: &str) -> Hash {
    let normalized = name.to_ascii_lowercase();
    let hash_bytes = blake3::hash(normalized.as_bytes());
    Hash::new(hash_bytes.into())
}

/// Compute message ID for replay protection
/// msg_id = blake3(sender_hash || recipient_hash || nonce)
pub fn compute_message_id(sender_hash: &Hash, recipient_hash: &Hash, nonce: u64) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(sender_hash.as_bytes());
    hasher.update(recipient_hash.as_bytes());
    hasher.update(&nonce.to_le_bytes());
    let hash_bytes = hasher.finalize();
    Hash::new(hash_bytes.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_hash_case_insensitive() {
        let hash1 = tns_name_hash("alice");
        let hash2 = tns_name_hash("Alice");
        let hash3 = tns_name_hash("ALICE");

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }

    #[test]
    fn test_name_hash_different_names() {
        let hash1 = tns_name_hash("alice");
        let hash2 = tns_name_hash("bob");

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_message_id_uniqueness() {
        let sender = tns_name_hash("alice");
        let recipient = tns_name_hash("bob");

        let id1 = compute_message_id(&sender, &recipient, 1);
        let id2 = compute_message_id(&sender, &recipient, 2);
        let id3 = compute_message_id(&recipient, &sender, 1);

        // Different nonces → different IDs
        assert_ne!(id1, id2);
        // Different sender/recipient order → different IDs
        assert_ne!(id1, id3);
    }
}
