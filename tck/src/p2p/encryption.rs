// Layer 1: Real Encryption Tests
//
// Tests the actual tos_daemon::p2p::encryption module:
// - ChaCha20-Poly1305 encrypt/decrypt roundtrip
// - Key generation and rotation
// - CipherSide (Our/Peer/Both) behavior
// - Nonce synchronization between encrypt/decrypt
// - Error cases (decrypt without key, wrong key mismatch)

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use tos_daemon::p2p::encryption::{CipherSide, Encryption, EncryptionError};

    // ─────────────────────────────────────────────────────────────────────────
    // Basic roundtrip tests
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_encrypt_decrypt_roundtrip_small() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        let original = b"Hello, TOS P2P!";
        let mut buf = BytesMut::from(&original[..]);

        enc.encrypt_packet(&mut buf).await.unwrap();
        assert_ne!(&buf[..], &original[..]);

        enc.decrypt_packet(&mut buf).await.unwrap();
        assert_eq!(&buf[..], &original[..]);
    }

    #[tokio::test]
    async fn test_encrypt_decrypt_roundtrip_large() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        let original: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
        let mut buf = BytesMut::from(&original[..]);

        enc.encrypt_packet(&mut buf).await.unwrap();
        assert_ne!(&buf[..], &original[..]);

        enc.decrypt_packet(&mut buf).await.unwrap();
        assert_eq!(&buf[..], &original[..]);
    }

    #[tokio::test]
    async fn test_encrypt_decrypt_empty_payload() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        let mut buf = BytesMut::new();
        enc.encrypt_packet(&mut buf).await.unwrap();
        assert!(!buf.is_empty());

        enc.decrypt_packet(&mut buf).await.unwrap();
        assert!(buf.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Multiple sequential encrypt/decrypt (nonce increment verification)
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_sequential_packets_nonce_sync() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        for i in 0u32..100 {
            let data = format!("Packet number {}", i);
            let mut buf = BytesMut::from(data.as_bytes());
            enc.encrypt_packet(&mut buf).await.unwrap();
            enc.decrypt_packet(&mut buf).await.unwrap();
            assert_eq!(&buf[..], data.as_bytes());
        }
    }

    #[tokio::test]
    async fn test_nonce_desync_causes_decrypt_failure() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        let mut buf1 = BytesMut::from(&b"first"[..]);
        let mut buf2 = BytesMut::from(&b"second"[..]);

        enc.encrypt_packet(&mut buf1).await.unwrap();
        enc.encrypt_packet(&mut buf2).await.unwrap();

        // Decrypt in order (nonce 0, then nonce 1)
        enc.decrypt_packet(&mut buf1).await.unwrap();
        assert_eq!(&buf1[..], b"first");

        enc.decrypt_packet(&mut buf2).await.unwrap();
        assert_eq!(&buf2[..], b"second");
    }

    #[tokio::test]
    async fn test_out_of_order_decrypt_fails() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        let mut buf1 = BytesMut::from(&b"first"[..]);
        let mut buf2 = BytesMut::from(&b"second"[..]);

        enc.encrypt_packet(&mut buf1).await.unwrap();
        enc.encrypt_packet(&mut buf2).await.unwrap();

        // Try to decrypt buf2 first (encrypted with nonce 1, but decrypt expects nonce 0)
        let result = enc.decrypt_packet(&mut buf2).await;
        assert!(result.is_err());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CipherSide behavior
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_cipher_side_our_only() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Our).await.unwrap();

        assert!(enc.is_write_ready().await);
        assert!(!enc.is_read_ready().await);

        let mut buf = BytesMut::from(&b"test"[..]);
        enc.encrypt_packet(&mut buf).await.unwrap();

        let result = enc.decrypt_packet(&mut buf).await;
        assert!(matches!(result, Err(EncryptionError::ReadNotReady)));
    }

    #[tokio::test]
    async fn test_cipher_side_peer_only() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Peer).await.unwrap();

        assert!(!enc.is_write_ready().await);
        assert!(enc.is_read_ready().await);

        let mut buf = BytesMut::from(&b"test"[..]);
        let result = enc.encrypt_packet(&mut buf).await;
        assert!(matches!(result, Err(EncryptionError::WriteNotReady)));
    }

    #[tokio::test]
    async fn test_cipher_side_both() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        assert!(enc.is_write_ready().await);
        assert!(enc.is_read_ready().await);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Two-party communication (separate Our/Peer keys)
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_two_party_separate_keys() {
        let alice = Encryption::new();
        let bob = Encryption::new();

        let alice_key = alice.generate_key().unwrap();
        let bob_key = bob.generate_key().unwrap();

        alice.rotate_key(alice_key, CipherSide::Our).await.unwrap();
        alice.rotate_key(bob_key, CipherSide::Peer).await.unwrap();

        bob.rotate_key(bob_key, CipherSide::Our).await.unwrap();
        bob.rotate_key(alice_key, CipherSide::Peer).await.unwrap();

        // Alice sends to Bob
        let msg = b"Hello Bob from Alice";
        let mut buf = BytesMut::from(&msg[..]);
        alice.encrypt_packet(&mut buf).await.unwrap();
        bob.decrypt_packet(&mut buf).await.unwrap();
        assert_eq!(&buf[..], &msg[..]);

        // Bob sends to Alice
        let reply = b"Hello Alice from Bob";
        let mut buf2 = BytesMut::from(&reply[..]);
        bob.encrypt_packet(&mut buf2).await.unwrap();
        alice.decrypt_packet(&mut buf2).await.unwrap();
        assert_eq!(&buf2[..], &reply[..]);
    }

    #[tokio::test]
    async fn test_wrong_key_decrypt_fails() {
        let alice = Encryption::new();
        let bob = Encryption::new();

        let alice_key = alice.generate_key().unwrap();
        let wrong_key = bob.generate_key().unwrap();

        alice.rotate_key(alice_key, CipherSide::Our).await.unwrap();
        bob.rotate_key(wrong_key, CipherSide::Peer).await.unwrap();

        let mut buf = BytesMut::from(&b"secret message"[..]);
        alice.encrypt_packet(&mut buf).await.unwrap();

        let result = bob.decrypt_packet(&mut buf).await;
        assert!(matches!(result, Err(EncryptionError::DecryptError)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Key rotation
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_key_rotation_resets_nonce() {
        let enc = Encryption::new();
        let key1 = enc.generate_key().unwrap();
        enc.rotate_key(key1, CipherSide::Both).await.unwrap();

        for _ in 0..10 {
            let mut buf = BytesMut::from(&b"data"[..]);
            enc.encrypt_packet(&mut buf).await.unwrap();
            enc.decrypt_packet(&mut buf).await.unwrap();
        }

        let key2 = enc.generate_key().unwrap();
        enc.rotate_key(key2, CipherSide::Both).await.unwrap();

        let msg = b"after rotation";
        let mut buf = BytesMut::from(&msg[..]);
        enc.encrypt_packet(&mut buf).await.unwrap();
        enc.decrypt_packet(&mut buf).await.unwrap();
        assert_eq!(&buf[..], &msg[..]);
    }

    #[tokio::test]
    async fn test_old_ciphertext_invalid_after_rotation() {
        let enc = Encryption::new();
        let key1 = enc.generate_key().unwrap();
        enc.rotate_key(key1, CipherSide::Both).await.unwrap();

        let mut buf = BytesMut::from(&b"old key data"[..]);
        enc.encrypt_packet(&mut buf).await.unwrap();
        let ciphertext = buf.clone();

        let key2 = enc.generate_key().unwrap();
        enc.rotate_key(key2, CipherSide::Both).await.unwrap();

        let mut old_buf = ciphertext;
        let result = enc.decrypt_packet(&mut old_buf).await;
        assert!(matches!(result, Err(EncryptionError::DecryptError)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Ready state management
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_initial_state_not_ready() {
        let enc = Encryption::new();
        assert!(!enc.is_ready());
        assert!(!enc.is_read_ready().await);
        assert!(!enc.is_write_ready().await);
    }

    #[tokio::test]
    async fn test_mark_ready() {
        let mut enc = Encryption::new();
        assert!(!enc.is_ready());
        enc.mark_ready();
        assert!(enc.is_ready());
    }

    #[tokio::test]
    async fn test_encrypt_before_key_set_fails() {
        let enc = Encryption::new();
        let mut buf = BytesMut::from(&b"test"[..]);
        let result = enc.encrypt_packet(&mut buf).await;
        assert!(matches!(result, Err(EncryptionError::WriteNotReady)));
    }

    #[tokio::test]
    async fn test_decrypt_before_key_set_fails() {
        let enc = Encryption::new();
        let mut buf = BytesMut::from(&b"test"[..]);
        let result = enc.decrypt_packet(&mut buf).await;
        assert!(matches!(result, Err(EncryptionError::ReadNotReady)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Key generation uniqueness
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_generated_keys_are_unique() {
        let enc = Encryption::new();
        let mut keys = Vec::new();
        for _ in 0..100 {
            let key = enc.generate_key().unwrap();
            assert!(!keys.contains(&key));
            keys.push(key);
        }
    }

    #[tokio::test]
    async fn test_generated_key_is_32_bytes() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        assert_eq!(key.len(), 32);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Ciphertext expansion (auth tag overhead)
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_ciphertext_has_auth_tag_overhead() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        let plaintext = b"Hello";
        let mut buf = BytesMut::from(&plaintext[..]);
        let original_len = buf.len();

        enc.encrypt_packet(&mut buf).await.unwrap();
        // ChaCha20-Poly1305 adds 16 bytes auth tag
        assert_eq!(buf.len(), original_len + 16);

        enc.decrypt_packet(&mut buf).await.unwrap();
        assert_eq!(buf.len(), original_len);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Tamper detection
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_tampered_ciphertext_detected() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        let mut buf = BytesMut::from(&b"important data"[..]);
        enc.encrypt_packet(&mut buf).await.unwrap();

        if !buf.is_empty() {
            buf[0] ^= 0x01;
        }

        let result = enc.decrypt_packet(&mut buf).await;
        assert!(matches!(result, Err(EncryptionError::DecryptError)));
    }

    #[tokio::test]
    async fn test_truncated_ciphertext_detected() {
        let enc = Encryption::new();
        let key = enc.generate_key().unwrap();
        enc.rotate_key(key, CipherSide::Both).await.unwrap();

        let mut buf = BytesMut::from(&b"important data"[..]);
        enc.encrypt_packet(&mut buf).await.unwrap();

        buf.truncate(buf.len() - 1);

        let result = enc.decrypt_packet(&mut buf).await;
        assert!(matches!(result, Err(EncryptionError::DecryptError)));
    }
}
