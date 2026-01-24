#[cfg(test)]
mod tests {
    use super::super::mock::*;

    // Mock cipher state for testing nonce/key rotation logic without actual crypto
    #[derive(Debug)]
    struct MockCipherState {
        key: EncryptionKey,
        nonce: u64,
        ready: bool,
    }

    impl MockCipherState {
        fn new() -> Self {
            Self {
                key: [0u8; 32],
                nonce: 0,
                ready: false,
            }
        }

        fn with_key(key: EncryptionKey) -> Self {
            Self {
                key,
                nonce: 0,
                ready: true,
            }
        }

        fn set_key(&mut self, key: EncryptionKey) {
            self.key = key;
            self.nonce = 0;
            self.ready = true;
        }

        fn encrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, &'static str> {
            if !self.ready {
                return Err("WriteNotReady");
            }
            if self.nonce == u64::MAX {
                return Err("Nonce overflow");
            }
            // Simulate encryption: XOR data with key bytes (not real crypto)
            let encrypted: Vec<u8> = data
                .iter()
                .enumerate()
                .map(|(i, &b)| b ^ self.key[i % 32])
                .collect();
            self.nonce += 1;
            Ok(encrypted)
        }

        fn decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, &'static str> {
            if !self.ready {
                return Err("ReadNotReady");
            }
            if self.nonce == u64::MAX {
                return Err("Nonce overflow");
            }
            // Simulate decryption: XOR reverses XOR encryption
            let decrypted: Vec<u8> = data
                .iter()
                .enumerate()
                .map(|(i, &b)| b ^ self.key[i % 32])
                .collect();
            self.nonce += 1;
            Ok(decrypted)
        }

        fn rotate_key(&mut self, new_key: EncryptionKey) {
            self.key = new_key;
            self.nonce = 0;
        }

        fn mark_ready(&mut self) {
            self.ready = true;
        }
    }

    // Composite encryption state with separate our/peer ciphers
    #[derive(Debug)]
    struct MockEncryption {
        our_cipher: MockCipherState,
        peer_cipher: MockCipherState,
        bytes_encrypted: u64,
        rotation_count_out: usize,
        rotation_count_in: usize,
    }

    impl MockEncryption {
        fn new() -> Self {
            Self {
                our_cipher: MockCipherState::new(),
                peer_cipher: MockCipherState::new(),
                bytes_encrypted: 0,
                rotation_count_out: 0,
                rotation_count_in: 0,
            }
        }

        fn setup_keys(&mut self, our_key: EncryptionKey, peer_key: EncryptionKey) {
            self.our_cipher.set_key(our_key);
            self.peer_cipher.set_key(peer_key);
        }

        fn encrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, &'static str> {
            let result = self.our_cipher.encrypt(data)?;
            self.bytes_encrypted += data.len() as u64;
            Ok(result)
        }

        fn decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, &'static str> {
            self.peer_cipher.decrypt(data)
        }

        fn needs_key_rotation(&self) -> bool {
            self.bytes_encrypted as usize >= ROTATE_EVERY_N_BYTES
        }

        fn rotate_our_key(&mut self, new_key: EncryptionKey) {
            self.our_cipher.rotate_key(new_key);
            self.bytes_encrypted = 0;
            self.rotation_count_out += 1;
        }

        fn rotate_peer_key(&mut self, new_key: EncryptionKey) {
            self.peer_cipher.rotate_key(new_key);
            self.rotation_count_in += 1;
        }

        fn rotate_both(&mut self, our_key: EncryptionKey, peer_key: EncryptionKey) {
            self.rotate_our_key(our_key);
            self.rotate_peer_key(peer_key);
        }
    }

    fn generate_mock_key(seed: u8) -> EncryptionKey {
        [seed; 32]
    }

    // -- Nonce starts at 0 --

    #[test]
    fn test_nonce_starts_at_zero() {
        let cipher = MockCipherState::with_key([1u8; 32]);
        assert_eq!(cipher.nonce, 0);
    }

    #[test]
    fn test_nonce_starts_at_zero_in_connection() {
        let conn = MockConnection::new(make_addr(8080), true);
        assert_eq!(conn.our_nonce, 0);
        assert_eq!(conn.peer_nonce, 0);
    }

    // -- Nonce increments by 1 per operation --

    #[test]
    fn test_nonce_increments_on_encrypt() {
        let mut cipher = MockCipherState::with_key([1u8; 32]);
        let data = b"hello world";

        cipher.encrypt(data).unwrap();
        assert_eq!(cipher.nonce, 1);

        cipher.encrypt(data).unwrap();
        assert_eq!(cipher.nonce, 2);

        cipher.encrypt(data).unwrap();
        assert_eq!(cipher.nonce, 3);
    }

    #[test]
    fn test_nonce_increments_on_decrypt() {
        let mut cipher = MockCipherState::with_key([1u8; 32]);
        let data = b"encrypted data";

        cipher.decrypt(data).unwrap();
        assert_eq!(cipher.nonce, 1);

        cipher.decrypt(data).unwrap();
        assert_eq!(cipher.nonce, 2);
    }

    // -- Nonce overflow check --

    #[test]
    fn test_nonce_overflow_returns_error_on_encrypt() {
        let mut cipher = MockCipherState::with_key([1u8; 32]);
        cipher.nonce = u64::MAX;

        let result = cipher.encrypt(b"test");
        assert_eq!(result, Err("Nonce overflow"));
    }

    #[test]
    fn test_nonce_overflow_returns_error_on_decrypt() {
        let mut cipher = MockCipherState::with_key([1u8; 32]);
        cipher.nonce = u64::MAX;

        let result = cipher.decrypt(b"test");
        assert_eq!(result, Err("Nonce overflow"));
    }

    #[test]
    fn test_nonce_max_minus_one_succeeds() {
        let mut cipher = MockCipherState::with_key([1u8; 32]);
        cipher.nonce = u64::MAX - 1;

        let result = cipher.encrypt(b"test");
        assert!(result.is_ok());
        assert_eq!(cipher.nonce, u64::MAX);
    }

    // -- Key rotation resets nonce to 0 --

    #[test]
    fn test_key_rotation_resets_nonce() {
        let mut cipher = MockCipherState::with_key([1u8; 32]);
        cipher.encrypt(b"data1").unwrap();
        cipher.encrypt(b"data2").unwrap();
        assert_eq!(cipher.nonce, 2);

        cipher.rotate_key([2u8; 32]);
        assert_eq!(cipher.nonce, 0);
        assert_eq!(cipher.key, [2u8; 32]);
    }

    #[test]
    fn test_key_rotation_on_connection_resets_bytes_encrypted() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);
        conn.send_bytes(1000).unwrap();
        assert_eq!(conn.bytes_encrypted, 1000);

        conn.rotate_key([3u8; 32], CipherSide::Our);
        assert_eq!(conn.bytes_encrypted, 0);
    }

    // -- CipherSide: Our, Peer, Both --

    #[test]
    fn test_cipher_side_our_affects_only_our_key() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);

        conn.rotate_key([3u8; 32], CipherSide::Our);
        assert_eq!(conn.our_key, Some([3u8; 32]));
        assert_eq!(conn.peer_key, Some([2u8; 32])); // Unchanged
    }

    #[test]
    fn test_cipher_side_peer_affects_only_peer_key() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);

        conn.rotate_key([3u8; 32], CipherSide::Peer);
        assert_eq!(conn.our_key, Some([1u8; 32])); // Unchanged
        assert_eq!(conn.peer_key, Some([3u8; 32]));
    }

    #[test]
    fn test_cipher_side_both_affects_both_keys() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);

        conn.rotate_key([5u8; 32], CipherSide::Both);
        assert_eq!(conn.our_key, Some([5u8; 32]));
        assert_eq!(conn.peer_key, Some([5u8; 32]));
    }

    #[test]
    fn test_cipher_side_is_our_predicate() {
        assert!(CipherSide::Our.is_our());
        assert!(!CipherSide::Peer.is_our());
        assert!(CipherSide::Both.is_our());
    }

    #[test]
    fn test_cipher_side_is_peer_predicate() {
        assert!(!CipherSide::Our.is_peer());
        assert!(CipherSide::Peer.is_peer());
        assert!(CipherSide::Both.is_peer());
    }

    // -- Encryption not ready before key set --

    #[test]
    fn test_encrypt_before_key_set_returns_write_not_ready() {
        let mut cipher = MockCipherState::new();
        let result = cipher.encrypt(b"hello");
        assert_eq!(result, Err("WriteNotReady"));
    }

    #[test]
    fn test_decrypt_before_key_set_returns_read_not_ready() {
        let mut cipher = MockCipherState::new();
        let result = cipher.decrypt(b"hello");
        assert_eq!(result, Err("ReadNotReady"));
    }

    #[test]
    fn test_send_bytes_before_key_exchange_returns_error() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        let result = conn.send_bytes(100);
        assert_eq!(result, Err("Encryption not ready"));
    }

    // -- Key rotation at 1GB threshold --

    #[test]
    fn test_needs_rotation_below_threshold() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);
        conn.send_bytes(1000).unwrap();
        assert!(!conn.needs_key_rotation());
    }

    #[test]
    fn test_needs_rotation_at_threshold() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);
        conn.bytes_encrypted = ROTATE_EVERY_N_BYTES as u64;
        assert!(conn.needs_key_rotation());
    }

    #[test]
    fn test_needs_rotation_above_threshold() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);
        conn.bytes_encrypted = ROTATE_EVERY_N_BYTES as u64 + 1;
        assert!(conn.needs_key_rotation());
    }

    #[test]
    fn test_rotation_threshold_is_1gb() {
        // Verify the constant is 1GB (1,073,741,824 bytes)
        assert_eq!(ROTATE_EVERY_N_BYTES, 1_073_741_824);
    }

    // -- bytes_encrypted counter resets on rotation --

    #[test]
    fn test_bytes_encrypted_resets_on_our_key_rotation() {
        let mut enc = MockEncryption::new();
        enc.setup_keys([1u8; 32], [2u8; 32]);

        enc.encrypt(b"some data here").unwrap();
        assert!(enc.bytes_encrypted > 0);

        enc.rotate_our_key([3u8; 32]);
        assert_eq!(enc.bytes_encrypted, 0);
    }

    #[test]
    fn test_bytes_encrypted_accumulates_correctly() {
        let mut enc = MockEncryption::new();
        enc.setup_keys([1u8; 32], [2u8; 32]);

        enc.encrypt(b"hello").unwrap(); // 5 bytes
        assert_eq!(enc.bytes_encrypted, 5);

        enc.encrypt(b"world!").unwrap(); // 6 bytes
        assert_eq!(enc.bytes_encrypted, 11);
    }

    // -- Multiple key rotations --

    #[test]
    fn test_multiple_our_key_rotations_tracked() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);

        assert_eq!(conn.rotate_key_out, 0);

        conn.rotate_key([3u8; 32], CipherSide::Our);
        assert_eq!(conn.rotate_key_out, 1);

        conn.rotate_key([4u8; 32], CipherSide::Our);
        assert_eq!(conn.rotate_key_out, 2);

        conn.rotate_key([5u8; 32], CipherSide::Our);
        assert_eq!(conn.rotate_key_out, 3);
    }

    #[test]
    fn test_multiple_peer_key_rotations_tracked() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);

        assert_eq!(conn.rotate_key_in, 0);

        conn.rotate_key([3u8; 32], CipherSide::Peer);
        assert_eq!(conn.rotate_key_in, 1);

        conn.rotate_key([4u8; 32], CipherSide::Peer);
        assert_eq!(conn.rotate_key_in, 2);
    }

    #[test]
    fn test_both_rotation_increments_both_counters() {
        let mut conn = MockConnection::new(make_addr(8080), true);
        conn.exchange_keys([1u8; 32], [2u8; 32]);

        conn.rotate_key([9u8; 32], CipherSide::Both);
        assert_eq!(conn.rotate_key_out, 1);
        assert_eq!(conn.rotate_key_in, 1);
    }

    #[test]
    fn test_rotation_count_in_mock_encryption() {
        let mut enc = MockEncryption::new();
        enc.setup_keys([1u8; 32], [2u8; 32]);

        enc.rotate_our_key([3u8; 32]);
        enc.rotate_our_key([4u8; 32]);
        enc.rotate_peer_key([5u8; 32]);

        assert_eq!(enc.rotation_count_out, 2);
        assert_eq!(enc.rotation_count_in, 1);
    }

    // -- Encrypt/decrypt round-trip --

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [0x42u8; 32];
        let mut enc_cipher = MockCipherState::with_key(key);
        let mut dec_cipher = MockCipherState::with_key(key);

        let plaintext = b"Hello, P2P protocol!";
        let ciphertext = enc_cipher.encrypt(plaintext).unwrap();
        let recovered = dec_cipher.decrypt(&ciphertext).unwrap();

        assert_eq!(recovered, plaintext.to_vec());
    }

    #[test]
    fn test_encrypt_produces_different_output_than_input() {
        let mut cipher = MockCipherState::with_key([0xFF; 32]);
        let plaintext = b"test data";
        let ciphertext = cipher.encrypt(plaintext).unwrap();

        // With a non-zero key, XOR should produce different bytes
        assert_ne!(ciphertext, plaintext.to_vec());
    }

    #[test]
    fn test_different_keys_produce_different_ciphertext() {
        let plaintext = b"same plaintext";

        let mut cipher1 = MockCipherState::with_key([0x11; 32]);
        let mut cipher2 = MockCipherState::with_key([0x22; 32]);

        let ct1 = cipher1.encrypt(plaintext).unwrap();
        let ct2 = cipher2.encrypt(plaintext).unwrap();

        assert_ne!(ct1, ct2);
    }

    // -- Generate key produces 32-byte keys --

    #[test]
    fn test_generate_mock_key_produces_32_bytes() {
        let key = generate_mock_key(0xAB);
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_generate_mock_key_different_seeds_different_keys() {
        let key1 = generate_mock_key(1);
        let key2 = generate_mock_key(2);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_encryption_key_type_is_32_bytes() {
        let key: EncryptionKey = [0u8; 32];
        assert_eq!(key.len(), 32);
    }

    // -- Mark ready state transitions --

    #[test]
    fn test_cipher_not_ready_initially() {
        let cipher = MockCipherState::new();
        assert!(!cipher.ready);
    }

    #[test]
    fn test_cipher_ready_after_set_key() {
        let mut cipher = MockCipherState::new();
        cipher.set_key([1u8; 32]);
        assert!(cipher.ready);
    }

    #[test]
    fn test_cipher_ready_after_mark_ready() {
        let mut cipher = MockCipherState::new();
        cipher.key = [1u8; 32]; // Set key manually without set_key
        assert!(!cipher.ready);
        cipher.mark_ready();
        assert!(cipher.ready);
    }

    #[test]
    fn test_cipher_with_key_constructor_is_ready() {
        let cipher = MockCipherState::with_key([0xAA; 32]);
        assert!(cipher.ready);
        assert_eq!(cipher.nonce, 0);
    }

    // -- Independent our/peer cipher states --

    #[test]
    fn test_our_and_peer_ciphers_independent_nonces() {
        let mut enc = MockEncryption::new();
        enc.setup_keys([1u8; 32], [2u8; 32]);

        enc.encrypt(b"msg1").unwrap();
        enc.encrypt(b"msg2").unwrap();
        assert_eq!(enc.our_cipher.nonce, 2);
        assert_eq!(enc.peer_cipher.nonce, 0);

        enc.decrypt(b"data").unwrap();
        assert_eq!(enc.our_cipher.nonce, 2);
        assert_eq!(enc.peer_cipher.nonce, 1);
    }

    #[test]
    fn test_our_and_peer_ciphers_independent_keys() {
        let mut enc = MockEncryption::new();
        enc.setup_keys([0x11; 32], [0x22; 32]);

        assert_eq!(enc.our_cipher.key, [0x11; 32]);
        assert_eq!(enc.peer_cipher.key, [0x22; 32]);

        enc.rotate_our_key([0x33; 32]);
        assert_eq!(enc.our_cipher.key, [0x33; 32]);
        assert_eq!(enc.peer_cipher.key, [0x22; 32]); // Unchanged
    }

    #[test]
    fn test_encrypt_not_ready_when_only_peer_key_set() {
        let mut enc = MockEncryption::new();
        // Only set peer key, our cipher remains not ready
        enc.peer_cipher.set_key([2u8; 32]);

        let result = enc.encrypt(b"test");
        assert_eq!(result, Err("WriteNotReady"));

        // But decrypt should work
        let result = enc.decrypt(b"test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_decrypt_not_ready_when_only_our_key_set() {
        let mut enc = MockEncryption::new();
        // Only set our key, peer cipher remains not ready
        enc.our_cipher.set_key([1u8; 32]);

        let result = enc.decrypt(b"test");
        assert_eq!(result, Err("ReadNotReady"));

        // But encrypt should work
        let result = enc.encrypt(b"test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_rotate_both_keys_resets_both_nonces() {
        let mut enc = MockEncryption::new();
        enc.setup_keys([1u8; 32], [2u8; 32]);

        enc.encrypt(b"a").unwrap();
        enc.encrypt(b"b").unwrap();
        enc.decrypt(b"c").unwrap();

        assert_eq!(enc.our_cipher.nonce, 2);
        assert_eq!(enc.peer_cipher.nonce, 1);

        enc.rotate_both([3u8; 32], [4u8; 32]);
        assert_eq!(enc.our_cipher.nonce, 0);
        assert_eq!(enc.peer_cipher.nonce, 0);
        assert_eq!(enc.bytes_encrypted, 0);
    }

    #[test]
    fn test_mock_encryption_needs_rotation_check() {
        let mut enc = MockEncryption::new();
        enc.setup_keys([1u8; 32], [2u8; 32]);

        assert!(!enc.needs_key_rotation());

        enc.bytes_encrypted = ROTATE_EVERY_N_BYTES as u64;
        assert!(enc.needs_key_rotation());
    }
}
