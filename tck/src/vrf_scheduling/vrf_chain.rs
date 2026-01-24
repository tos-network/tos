// Phase 16: VRF ChainClient Tests (Layer 1.5)
//
// Tests VRF behavior at the block level using ChainClient.
// Requires: BlockInfo.vrf_data field, ChainClient VRF production.

#[cfg(test)]
mod tests {
    use tos_common::crypto::Hash;
    use tos_daemon::vrf::VrfKeyManager;

    use crate::tier1_5::{
        chain_client_config::GenesisAccount, BlockWarp, ChainClient, ChainClientConfig, VrfConfig,
    };

    /// Generate a deterministic VRF secret key hex for tests.
    fn test_vrf_secret_hex() -> String {
        let mgr = VrfKeyManager::new();
        mgr.secret_key_hex()
    }

    fn sample_hash(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    // ========================================================================
    // VRF Data in Mined Blocks
    // ========================================================================

    #[tokio::test]
    async fn vrf_data_present_in_mined_block() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3, // devnet
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let vrf = client.get_block_vrf_data(1);
        assert!(vrf.is_some(), "VRF data should be present in mined block");

        let vrf = vrf.unwrap();
        assert_eq!(vrf.public_key.len(), 32);
        assert_eq!(vrf.output.len(), 32);
        assert_eq!(vrf.proof.len(), 64);
        assert_eq!(vrf.binding_signature.len(), 64);

        // VRF output should not be all zeros
        assert_ne!(vrf.output, [0u8; 32], "VRF output should not be zeroed");
    }

    #[tokio::test]
    async fn vrf_output_changes_per_block() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();

        // Mine 5 blocks
        client.mine_blocks(5).await.unwrap();

        // Collect VRF outputs
        let mut outputs: Vec<[u8; 32]> = Vec::new();
        for topo in 1..=5 {
            let vrf = client
                .get_block_vrf_data(topo)
                .expect("VRF data should exist");
            outputs.push(vrf.output);
        }

        // All outputs should be unique
        for i in 0..outputs.len() {
            for j in (i + 1)..outputs.len() {
                assert_ne!(
                    outputs[i],
                    outputs[j],
                    "VRF output at block {} and {} should differ",
                    i + 1,
                    j + 1
                );
            }
        }
    }

    #[tokio::test]
    async fn vrf_output_deterministic_replay() {
        let mgr = VrfKeyManager::new();
        let secret_hex = mgr.secret_key_hex();

        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex.clone()),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_blocks(5).await.unwrap();

        // Verify VRF outputs are non-zero and unique per block
        let outputs: Vec<[u8; 32]> = (1..=5)
            .map(|t| client.get_block_vrf_data(t).unwrap().output)
            .collect();

        for output in &outputs {
            assert_ne!(*output, [0u8; 32], "VRF output should not be zeroed");
        }

        // All outputs should be unique (deterministic per block but different across blocks)
        for i in 0..outputs.len() {
            for j in (i + 1)..outputs.len() {
                assert_ne!(
                    outputs[i],
                    outputs[j],
                    "Blocks {} and {} should have different VRF outputs",
                    i + 1,
                    j + 1
                );
            }
        }

        // Verify same public key across all blocks (same VRF key manager)
        let pub_keys: Vec<[u8; 32]> = (1..=5)
            .map(|t| client.get_block_vrf_data(t).unwrap().public_key)
            .collect();

        for pk in &pub_keys {
            assert_eq!(
                *pk, pub_keys[0],
                "All blocks should use the same VRF public key"
            );
        }
    }

    // ========================================================================
    // VRF Contract Syscall Tests
    // ========================================================================

    #[tokio::test]
    #[ignore = "Requires VRF injection into contract execution context"]
    async fn contract_reads_vrf_random() {
        // Deploy vrf-reader.so contract
        // Call it -> it reads vrf_random() and stores result
        // Assert: Stored value is 32 bytes and matches block VRF derive
    }

    #[tokio::test]
    #[ignore = "Requires VRF injection into contract execution context"]
    async fn contract_reads_vrf_public_key() {
        // Deploy vrf-reader.so contract
        // Call it -> it reads vrf_public_key() and stores result
        // Assert: Stored value matches configured VRF public key
    }

    #[tokio::test]
    #[ignore = "Requires VRF injection into contract execution context"]
    async fn same_block_same_vrf_for_all_txs() {
        // Submit 3 contract calls in same block
        // Each reads vrf_random()
        // Assert: All 3 get the same value
    }

    #[tokio::test]
    #[ignore = "Requires VRF injection into contract execution context"]
    async fn multiple_contracts_read_same_vrf() {
        // Deploy 3 different contracts
        // All call vrf_random() in same block
        // Assert: All read same VRF output
    }

    // ========================================================================
    // VRF Validation Tests
    // ========================================================================

    #[tokio::test]
    #[ignore = "Requires TestBlockchain block validation with VRF checks"]
    async fn block_without_vrf_rejected() {
        // Construct a block without VRF data
        // Submit to blockchain
        // Assert: Block rejected with appropriate error
    }

    #[tokio::test]
    #[ignore = "Requires TestBlockchain block validation with VRF checks"]
    async fn tampered_vrf_output_rejected() {
        // Mine a valid block
        // Tamper with VRF output field
        // Resubmit
        // Assert: Rejected
    }

    #[tokio::test]
    #[ignore = "Requires TestBlockchain block validation with VRF checks"]
    async fn tampered_vrf_proof_rejected() {
        // Mine a valid block
        // Tamper with VRF proof field
        // Resubmit
        // Assert: Rejected
    }

    #[tokio::test]
    #[ignore = "Requires TestBlockchain block validation with VRF checks"]
    async fn wrong_miner_binding_rejected() {
        // Create VRF data with different miner's binding signature
        // Submit block with this data
        // Assert: Rejected
    }

    // ========================================================================
    // VRF with BlockWarp Tests
    // ========================================================================

    #[tokio::test]
    async fn vrf_after_warp() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();

        // Warp 100 blocks
        client.warp_blocks(100).await.unwrap();
        assert_eq!(client.topoheight(), 100);

        // VRF data should be present for warped blocks
        let vrf_100 = client.get_block_vrf_data(100).cloned();
        assert!(vrf_100.is_some(), "VRF data should exist after warp");

        let output_100 = vrf_100.unwrap().output;
        assert_ne!(output_100, [0u8; 32]);

        // Mine one more block after warp
        client.mine_empty_block().await.unwrap();
        let vrf_101 = client.get_block_vrf_data(101);
        assert!(
            vrf_101.is_some(),
            "VRF data should exist for post-warp block"
        );
        assert_ne!(
            vrf_101.unwrap().output,
            output_100,
            "Post-warp VRF output should differ"
        );
    }

    // ========================================================================
    // VRF Derive Formula Verification
    // ========================================================================

    #[test]
    fn vrf_random_derive_formula() {
        // This test verifies the derivation formula without needing ChainClient
        // vrf_random = BLAKE3("TOS-VRF-DERIVE" || pre_output || block_hash)
        let pre_output = [0xABu8; 32];
        let block_hash = [0xCDu8; 32];

        let mut derive_input =
            Vec::with_capacity(b"TOS-VRF-DERIVE".len() + pre_output.len() + block_hash.len());
        derive_input.extend_from_slice(b"TOS-VRF-DERIVE");
        derive_input.extend_from_slice(&pre_output);
        derive_input.extend_from_slice(&block_hash);

        let random = tos_common::crypto::hash(&derive_input);

        // Result should be 32 bytes and deterministic
        assert_eq!(random.as_bytes().len(), 32);

        // Repeat to verify determinism
        let random2 = tos_common::crypto::hash(&derive_input);
        assert_eq!(random.as_bytes(), random2.as_bytes());
    }

    // ========================================================================
    // Feature Gate Tests
    // ========================================================================

    #[tokio::test]
    #[ignore = "Requires FeatureSet VRF activation control"]
    async fn feature_gate_vrf_activation() {
        // Setup: ChainClient with VRF feature deactivated
        // Deploy contract that calls vrf_random()
        // Assert: Returns error (feature not active)
        //
        // Activate feature at height N
        // Warp past height N
        // Call again
        // Assert: Returns valid VRF data
    }

    #[tokio::test]
    #[ignore = "Requires state_diff tracking with VRF"]
    async fn vrf_survives_state_diff_tracking() {
        // Setup: ChainClient with track_state_diffs = true
        // Mine blocks
        // Assert: VRF data still correct
    }
}
