// Phase 16: VRF Proof Generation & Verification (Layer 1)
//
// Pure unit tests for VRF proof correctness using direct daemon imports.
// No framework dependencies - tests VrfKeyManager sign/verify and
// compute_vrf_input/compute_vrf_binding_message determinism.

#[cfg(test)]
mod tests {
    use tos_common::block::{compute_vrf_binding_message, compute_vrf_input};
    use tos_common::crypto::KeyPair;
    use tos_daemon::vrf::{
        VrfKeyManager, VrfProof, VRF_OUTPUT_SIZE, VRF_PROOF_SIZE, VRF_PUBLIC_KEY_SIZE,
    };

    /// Devnet chain_id for testing
    const DEVNET_CHAIN_ID: u64 = 3;
    const MAINNET_CHAIN_ID: u64 = 0;
    const TESTNET_CHAIN_ID: u64 = 1;

    /// Helper: create a miner keypair and compressed public key
    fn new_miner() -> (KeyPair, tos_common::crypto::elgamal::CompressedPublicKey) {
        let keypair = KeyPair::new();
        let compressed = keypair.get_public_key().compress();
        (keypair, compressed)
    }

    // ========================================================================
    // Sign/Verify Roundtrip Tests
    // ========================================================================

    #[test]
    fn sign_verify_roundtrip() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0xABu8; 32];

        let data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("VRF sign should succeed");

        // VRF proof verifies
        assert!(
            manager.verify_vrf_proof(&block_hash, &miner, &data).is_ok(),
            "VRF proof should verify for same block_hash and miner"
        );
    }

    #[test]
    fn different_block_hash_different_output() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();

        let hash_a = [0x01u8; 32];
        let hash_b = [0x02u8; 32];

        let data_a = manager
            .sign(DEVNET_CHAIN_ID, &hash_a, &miner, &miner_kp)
            .expect("sign a");
        let data_b = manager
            .sign(DEVNET_CHAIN_ID, &hash_b, &miner, &miner_kp)
            .expect("sign b");

        assert_ne!(
            data_a.output.as_bytes(),
            data_b.output.as_bytes(),
            "Different block hashes must produce different VRF outputs"
        );
    }

    #[test]
    fn different_miner_different_output() {
        let manager = VrfKeyManager::new();
        let block_hash = [0x42u8; 32];

        let (miner_a_kp, miner_a) = new_miner();
        let (miner_b_kp, miner_b) = new_miner();

        let data_a = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner_a, &miner_a_kp)
            .expect("sign a");
        let data_b = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner_b, &miner_b_kp)
            .expect("sign b");

        assert_ne!(
            data_a.output.as_bytes(),
            data_b.output.as_bytes(),
            "Same block_hash with different miners must produce different VRF outputs"
        );
    }

    #[test]
    fn same_inputs_deterministic_output() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x99u8; 32];

        let data_1 = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign 1");
        let data_2 = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign 2");

        assert_eq!(
            data_1.output.as_bytes(),
            data_2.output.as_bytes(),
            "Same inputs must produce identical VRF outputs"
        );
        // Note: VRF proofs may differ across calls due to random nonce
        // used for zero-knowledge property. Only the output is deterministic.
        // Both proofs should still verify correctly.
        assert!(
            manager
                .verify_vrf_proof(&block_hash, &miner, &data_1)
                .is_ok(),
            "First proof should verify"
        );
        assert!(
            manager
                .verify_vrf_proof(&block_hash, &miner, &data_2)
                .is_ok(),
            "Second proof should verify"
        );
    }

    // ========================================================================
    // Chain ID Separation Tests
    // ========================================================================

    #[test]
    fn chain_id_separation() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x42u8; 32];

        let data_mainnet = manager
            .sign(MAINNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("mainnet sign");
        let data_devnet = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("devnet sign");

        // VRF outputs are same (VRF input does not include chain_id)
        assert_eq!(
            data_mainnet.output.as_bytes(),
            data_devnet.output.as_bytes(),
            "VRF output should be chain-independent"
        );

        // Binding signatures are different (chain_id is in binding message)
        assert_ne!(
            data_mainnet.binding_signature.to_bytes(),
            data_devnet.binding_signature.to_bytes(),
            "Binding signatures must differ across chains"
        );

        // Mainnet binding verifies only against mainnet message
        let vrf_pk_bytes = data_mainnet.public_key.to_bytes();
        let mainnet_msg = compute_vrf_binding_message(MAINNET_CHAIN_ID, &vrf_pk_bytes, &block_hash);
        assert!(data_mainnet
            .binding_signature
            .verify(&mainnet_msg, miner_kp.get_public_key()));

        // Mainnet binding FAILS against devnet message
        let devnet_msg = compute_vrf_binding_message(DEVNET_CHAIN_ID, &vrf_pk_bytes, &block_hash);
        assert!(
            !data_mainnet
                .binding_signature
                .verify(&devnet_msg, miner_kp.get_public_key()),
            "Cross-chain binding verification must fail"
        );
    }

    // ========================================================================
    // Invalid Data Rejection Tests
    // ========================================================================

    #[test]
    fn invalid_proof_rejected() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x42u8; 32];

        let mut data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign");

        // Tamper with proof bytes
        let mut proof_bytes = data.proof.to_bytes();
        proof_bytes[0] ^= 0xFF;
        proof_bytes[1] ^= 0xFF;
        data.proof = VrfProof::from_bytes(&proof_bytes).expect("proof bytes");

        assert!(
            manager
                .verify_vrf_proof(&block_hash, &miner, &data)
                .is_err(),
            "Tampered proof must be rejected"
        );
    }

    #[test]
    fn invalid_binding_signature_rejected() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x42u8; 32];

        let data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign");

        // Create a different miner and verify binding against them
        let (other_kp, _) = new_miner();
        let vrf_pk_bytes = data.public_key.to_bytes();
        let binding_msg = compute_vrf_binding_message(DEVNET_CHAIN_ID, &vrf_pk_bytes, &block_hash);

        // The binding signature was created by miner_kp, not other_kp
        assert!(
            !data
                .binding_signature
                .verify(&binding_msg, other_kp.get_public_key()),
            "Binding signature must not verify for wrong miner"
        );
    }

    #[test]
    fn wrong_block_hash_proof_rejected() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();

        let block_hash = [0x42u8; 32];
        let wrong_hash = [0x43u8; 32];

        let data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign");

        // Verify against wrong block hash
        assert!(
            manager
                .verify_vrf_proof(&wrong_hash, &miner, &data)
                .is_err(),
            "VRF proof must not verify for wrong block hash"
        );
    }

    #[test]
    fn wrong_miner_proof_rejected() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner_a) = new_miner();
        let (_, miner_b) = new_miner();
        let block_hash = [0x42u8; 32];

        let data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner_a, &miner_kp)
            .expect("sign for miner_a");

        // Verify against wrong miner identity
        assert!(
            manager
                .verify_vrf_proof(&block_hash, &miner_b, &data)
                .is_err(),
            "VRF proof must not verify for different miner"
        );
    }

    // ========================================================================
    // Key Management Tests
    // ========================================================================

    #[test]
    fn from_hex_roundtrip() {
        let manager1 = VrfKeyManager::new();
        let hex_secret = manager1.secret_key_hex();

        let manager2 =
            VrfKeyManager::from_hex(&hex_secret).expect("from_hex should succeed for valid key");

        assert_eq!(
            manager1.public_key().as_bytes(),
            manager2.public_key().as_bytes(),
            "Reconstructed manager should have same public key"
        );

        // Sign with both and compare output
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x55u8; 32];

        let data1 = manager1
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign 1");
        let data2 = manager2
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign 2");

        assert_eq!(data1.output.as_bytes(), data2.output.as_bytes());
    }

    #[test]
    fn from_hex_invalid_rejected() {
        // Too short
        assert!(VrfKeyManager::from_hex("deadbeef").is_err());

        // Invalid hex characters
        assert!(VrfKeyManager::from_hex(
            "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
        )
        .is_err());

        // Empty string
        assert!(VrfKeyManager::from_hex("").is_err());
    }

    // ========================================================================
    // VRF Input / Binding Message Determinism Tests
    // ========================================================================

    #[test]
    fn vrf_input_binding_deterministic() {
        let (_, miner) = new_miner();
        let block_hash = [0x42u8; 32];

        let input_a = compute_vrf_input(&block_hash, &miner);
        let input_b = compute_vrf_input(&block_hash, &miner);

        assert_eq!(
            input_a, input_b,
            "compute_vrf_input must be deterministic for same params"
        );
    }

    #[test]
    fn vrf_binding_message_includes_chain_id() {
        let vrf_pk = [0x11u8; VRF_PUBLIC_KEY_SIZE];
        let block_hash = [0x42u8; 32];

        let msg_mainnet = compute_vrf_binding_message(MAINNET_CHAIN_ID, &vrf_pk, &block_hash);
        let msg_testnet = compute_vrf_binding_message(TESTNET_CHAIN_ID, &vrf_pk, &block_hash);
        let msg_devnet = compute_vrf_binding_message(DEVNET_CHAIN_ID, &vrf_pk, &block_hash);

        assert_ne!(msg_mainnet, msg_testnet);
        assert_ne!(msg_mainnet, msg_devnet);
        assert_ne!(msg_testnet, msg_devnet);
    }

    #[test]
    fn vrf_input_differs_by_miner() {
        let block_hash = [0x42u8; 32];
        let (_, miner_a) = new_miner();
        let (_, miner_b) = new_miner();

        let input_a = compute_vrf_input(&block_hash, &miner_a);
        let input_b = compute_vrf_input(&block_hash, &miner_b);

        assert_ne!(
            input_a, input_b,
            "Different miners must produce different VRF inputs"
        );
    }

    // ========================================================================
    // Size Correctness Tests
    // ========================================================================

    #[test]
    fn output_is_32_bytes() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x42u8; 32];

        let data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign");

        assert_eq!(
            data.output.as_bytes().len(),
            VRF_OUTPUT_SIZE,
            "VRF output must be {} bytes",
            VRF_OUTPUT_SIZE
        );
    }

    #[test]
    fn proof_is_64_bytes() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x42u8; 32];

        let data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign");

        assert_eq!(
            data.proof.to_bytes().len(),
            VRF_PROOF_SIZE,
            "VRF proof must be {} bytes",
            VRF_PROOF_SIZE
        );
    }

    #[test]
    fn public_key_is_32_bytes() {
        let manager = VrfKeyManager::new();
        assert_eq!(
            manager.public_key().as_bytes().len(),
            VRF_PUBLIC_KEY_SIZE,
            "VRF public key must be {} bytes",
            VRF_PUBLIC_KEY_SIZE
        );
    }

    // ========================================================================
    // Boundary Value Tests
    // ========================================================================

    #[test]
    fn zero_block_hash_valid() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x00u8; 32];

        let data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign with zero hash");

        assert!(manager.verify_vrf_proof(&block_hash, &miner, &data).is_ok());
    }

    #[test]
    fn max_value_block_hash_valid() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0xFFu8; 32];

        let data = manager
            .sign(DEVNET_CHAIN_ID, &block_hash, &miner, &miner_kp)
            .expect("sign with max hash");

        assert!(manager.verify_vrf_proof(&block_hash, &miner, &data).is_ok());
    }

    #[test]
    fn max_chain_id_valid() {
        let manager = VrfKeyManager::new();
        let (miner_kp, miner) = new_miner();
        let block_hash = [0x42u8; 32];

        // Maximum u64 chain_id should still produce a valid signature
        let data = manager
            .sign(u64::MAX, &block_hash, &miner, &miner_kp)
            .expect("sign with max chain_id");

        assert!(manager.verify_vrf_proof(&block_hash, &miner, &data).is_ok());

        // Binding signature verifies for the correct chain_id
        let vrf_pk_bytes = data.public_key.to_bytes();
        let binding_msg = compute_vrf_binding_message(u64::MAX, &vrf_pk_bytes, &block_hash);
        assert!(data
            .binding_signature
            .verify(&binding_msg, miner_kp.get_public_key()));
    }
}
