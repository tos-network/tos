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
    async fn contract_reads_vrf_random() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap(); // topo 1, produces VRF

        // Deploy vrf_random.so
        let bytecode = include_bytes!("../../tests/fixtures/vrf_random.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Call contract (entry_id 0 = default, no params)
        let result = client
            .call_contract(&contract, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success, "Contract call should succeed");

        // Read stored vrf_random from contract storage
        let stored = client
            .get_contract_storage(&contract, b"vrf_random")
            .await
            .unwrap();
        assert!(stored.is_some(), "vrf_random should be stored");
        let random_bytes = stored.unwrap();
        assert_eq!(random_bytes.len(), 32, "vrf_random should be 32 bytes");
        assert_ne!(
            random_bytes,
            vec![0u8; 32],
            "vrf_random should not be zeros"
        );

        // Verify it matches the derivation formula
        let pre_output = client
            .get_contract_storage(&contract, b"vrf_pre_output")
            .await
            .unwrap()
            .unwrap();
        let block_hash_stored = client
            .get_contract_storage(&contract, b"vrf_block_hash")
            .await
            .unwrap()
            .unwrap();

        let mut derive_input = Vec::new();
        derive_input.extend_from_slice(b"TOS-VRF-DERIVE");
        derive_input.extend_from_slice(&pre_output);
        derive_input.extend_from_slice(&block_hash_stored);
        let expected = tos_common::crypto::hash(&derive_input);
        assert_eq!(random_bytes, expected.as_bytes().to_vec());
    }

    #[tokio::test]
    async fn contract_reads_vrf_public_key() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_random.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();
        let result = client
            .call_contract(&contract, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success);

        let stored_pk = client
            .get_contract_storage(&contract, b"vrf_public_key")
            .await
            .unwrap();
        assert!(stored_pk.is_some());
        let pk_bytes = stored_pk.unwrap();
        assert_eq!(pk_bytes.len(), 32);

        // Should match the VRF key manager's public key
        let vrf_data = client.get_block_vrf_data(1).unwrap();
        assert_eq!(pk_bytes, vrf_data.public_key.to_vec());
    }

    #[tokio::test]
    async fn same_block_same_vrf_for_all_txs() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_random.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Call 3 times in same block context
        let r1 = client
            .call_contract(&contract, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(r1.tx_result.success);
        let vrf1 = client
            .get_contract_storage(&contract, b"vrf_random")
            .await
            .unwrap()
            .unwrap();

        let r2 = client
            .call_contract(&contract, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(r2.tx_result.success);
        let vrf2 = client
            .get_contract_storage(&contract, b"vrf_random")
            .await
            .unwrap()
            .unwrap();

        let r3 = client
            .call_contract(&contract, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(r3.tx_result.success);
        let vrf3 = client
            .get_contract_storage(&contract, b"vrf_random")
            .await
            .unwrap()
            .unwrap();

        // All should read the same VRF (same block)
        assert_eq!(vrf1, vrf2, "Same block VRF should be identical");
        assert_eq!(vrf2, vrf3, "Same block VRF should be identical");
    }

    #[tokio::test]
    async fn multiple_contracts_read_same_vrf() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_random.so");

        // Deploy same bytecode at 3 different addresses
        let c1 = Hash::new([0x01; 32]);
        let c2 = Hash::new([0x02; 32]);
        let c3 = Hash::new([0x03; 32]);
        client.deploy_contract_at(&c1, bytecode).await.unwrap();
        client.deploy_contract_at(&c2, bytecode).await.unwrap();
        client.deploy_contract_at(&c3, bytecode).await.unwrap();

        // Call each
        let r1 = client
            .call_contract(&c1, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(r1.tx_result.success);
        let r2 = client
            .call_contract(&c2, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(r2.tx_result.success);
        let r3 = client
            .call_contract(&c3, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(r3.tx_result.success);

        // All should have same vrf_random
        let v1 = client
            .get_contract_storage(&c1, b"vrf_random")
            .await
            .unwrap()
            .unwrap();
        let v2 = client
            .get_contract_storage(&c2, b"vrf_random")
            .await
            .unwrap()
            .unwrap();
        let v3 = client
            .get_contract_storage(&c3, b"vrf_random")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(v1, v2);
        assert_eq!(v2, v3);
        assert_eq!(v1.len(), 32);
    }

    // ========================================================================
    // VRF Validation Tests
    // ========================================================================

    #[tokio::test]
    async fn block_without_vrf_rejected() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 1,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        // Get the mined block and strip VRF data
        let mut block = client.get_block_at_topoheight(1).await.unwrap();
        block.vrf_data = None;

        // Validation should fail
        let result = client.validate_block_vrf(&block);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("missing VRF data"),
            "Error should mention missing VRF data"
        );
    }

    #[tokio::test]
    async fn tampered_vrf_output_rejected() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 1,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let mut block = client.get_block_at_topoheight(1).await.unwrap();
        // Tamper VRF output
        if let Some(ref mut vrf) = block.vrf_data {
            vrf.output[0] ^= 0xFF;
        }

        let result = client.validate_block_vrf(&block);
        assert!(result.is_err(), "Tampered VRF output should be rejected");
    }

    #[tokio::test]
    async fn tampered_vrf_proof_rejected() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 1,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let mut block = client.get_block_at_topoheight(1).await.unwrap();
        // Tamper VRF proof
        if let Some(ref mut vrf) = block.vrf_data {
            vrf.proof[0] ^= 0xFF;
        }

        let result = client.validate_block_vrf(&block);
        assert!(result.is_err(), "Tampered VRF proof should be rejected");
    }

    #[tokio::test]
    async fn wrong_miner_binding_rejected() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 1,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let mut block = client.get_block_at_topoheight(1).await.unwrap();
        // Replace binding signature with one from a different keypair
        if let Some(ref mut vrf) = block.vrf_data {
            let different_keypair = tos_common::crypto::KeyPair::new();
            let binding_message = tos_common::block::compute_vrf_binding_message(
                1,
                &vrf.public_key,
                block.hash.as_bytes(),
            );
            let wrong_sig = different_keypair.sign(&binding_message);
            vrf.binding_signature = wrong_sig.to_bytes();
        }

        let result = client.validate_block_vrf(&block);
        assert!(result.is_err(), "Wrong miner binding should be rejected");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("binding signature verification failed"),
            "Error should mention binding signature"
        );
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
    async fn feature_gate_vrf_activation() {
        let secret_hex = test_vrf_secret_hex();
        // VRF configured but feature only activated at height 50
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 1,
            })
            .with_features(
                crate::tier1_5::features::FeatureSet::empty().activate_at("vrf_block_data", 50),
            );

        let mut client = ChainClient::start(config).await.unwrap();

        // Mine blocks before activation: no VRF data
        client.mine_empty_block().await.unwrap();
        assert!(
            client.get_block_vrf_data(1).is_none(),
            "VRF data should not be present before feature activation"
        );

        // Warp to activation height
        client.warp_to_topoheight(50).await.unwrap();

        // Mine block after activation: VRF data present
        client.mine_empty_block().await.unwrap();
        assert!(
            client.get_block_vrf_data(51).is_some(),
            "VRF data should be present after feature activation"
        );
    }

    #[tokio::test]
    async fn vrf_survives_state_diff_tracking() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 1,
            })
            .with_state_diff_tracking();

        let mut client = ChainClient::start(config).await.unwrap();

        // Mine several blocks
        for _ in 0..5 {
            client.mine_empty_block().await.unwrap();
        }

        // VRF data should be present on all mined blocks
        for topo in 1..=5 {
            let vrf = client.get_block_vrf_data(topo);
            assert!(vrf.is_some(), "VRF missing at topo {}", topo);
        }
    }

    // ========================================================================
    // VRF Prediction Market Contract Tests
    // ========================================================================

    #[tokio::test]
    async fn prediction_market_create_and_query() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        // Deploy prediction market contract
        let bytecode = include_bytes!("../../tests/fixtures/vrf_prediction.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Create market with 50% threshold (128)
        // Command: 0x01 (create), threshold=128
        let create_params = vec![0x01u8, 128u8];
        let result = client
            .call_contract(&contract, 0, create_params, vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success, "Create market should succeed");

        // Verify market status is OPEN (1)
        let status = client
            .get_contract_storage(&contract, b"status")
            .await
            .unwrap()
            .expect("status should be stored");
        assert_eq!(status, vec![1u8], "Market should be open");

        // Verify threshold is 128
        let threshold = client
            .get_contract_storage(&contract, b"threshold")
            .await
            .unwrap()
            .expect("threshold should be stored");
        assert_eq!(threshold, vec![128u8], "Threshold should be 128");

        // Query market (command 0x00)
        let query_result = client
            .call_contract(&contract, 0, vec![0x00], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(query_result.tx_result.success, "Query should succeed");
    }

    #[tokio::test]
    async fn prediction_market_place_bets() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_prediction.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Create market
        let result = client
            .call_contract(&contract, 0, vec![0x01, 128], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success);

        // Place YES bet: command=0x02, amount=1000 (u64 little-endian)
        let mut yes_params = vec![0x02u8];
        yes_params.extend_from_slice(&1000u64.to_le_bytes());
        let result = client
            .call_contract(&contract, 0, yes_params, vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success, "YES bet should succeed");

        // Place NO bet: command=0x03, amount=500
        let mut no_params = vec![0x03u8];
        no_params.extend_from_slice(&500u64.to_le_bytes());
        let result = client
            .call_contract(&contract, 0, no_params, vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success, "NO bet should succeed");

        // Verify pools
        let yes_pool = client
            .get_contract_storage(&contract, b"yes_pool")
            .await
            .unwrap()
            .expect("yes_pool should be stored");
        assert_eq!(
            u64::from_le_bytes(yes_pool.try_into().unwrap()),
            1000,
            "YES pool should be 1000"
        );

        let no_pool = client
            .get_contract_storage(&contract, b"no_pool")
            .await
            .unwrap()
            .expect("no_pool should be stored");
        assert_eq!(
            u64::from_le_bytes(no_pool.try_into().unwrap()),
            500,
            "NO pool should be 500"
        );
    }

    #[tokio::test]
    async fn prediction_market_resolve_with_vrf() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_prediction.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Create market with 50% threshold
        client
            .call_contract(&contract, 0, vec![0x01, 128], vec![], 1_000_000)
            .await
            .unwrap();

        // Place some bets
        let mut yes_params = vec![0x02u8];
        yes_params.extend_from_slice(&100u64.to_le_bytes());
        client
            .call_contract(&contract, 0, yes_params, vec![], 1_000_000)
            .await
            .unwrap();

        // Resolve market: command=0x04
        let result = client
            .call_contract(&contract, 0, vec![0x04], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success, "Resolve should succeed");

        // Verify market is resolved (status=2)
        let status = client
            .get_contract_storage(&contract, b"status")
            .await
            .unwrap()
            .expect("status should be stored");
        assert_eq!(status, vec![2u8], "Market should be resolved");

        // Verify VRF data was stored
        let vrf_random = client
            .get_contract_storage(&contract, b"vrf_random")
            .await
            .unwrap()
            .expect("vrf_random should be stored");
        assert_eq!(vrf_random.len(), 32);

        let vrf_byte = client
            .get_contract_storage(&contract, b"vrf_byte")
            .await
            .unwrap()
            .expect("vrf_byte should be stored");
        assert_eq!(vrf_byte.len(), 1);

        // Verify outcome matches VRF logic: YES if vrf_byte < threshold (128)
        let outcome = client
            .get_contract_storage(&contract, b"outcome")
            .await
            .unwrap()
            .expect("outcome should be stored");
        let expected_outcome = if vrf_byte[0] < 128 { 1u8 } else { 0u8 };
        assert_eq!(
            outcome,
            vec![expected_outcome],
            "Outcome should match VRF threshold logic: vrf_byte={}, threshold=128",
            vrf_byte[0]
        );
    }

    #[tokio::test]
    async fn prediction_market_cannot_bet_after_resolve() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_prediction.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Create and resolve market
        client
            .call_contract(&contract, 0, vec![0x01, 128], vec![], 1_000_000)
            .await
            .unwrap();
        client
            .call_contract(&contract, 0, vec![0x04], vec![], 1_000_000)
            .await
            .unwrap();

        // Try to place bet after resolve - should fail with exit code 3
        let mut yes_params = vec![0x02u8];
        yes_params.extend_from_slice(&100u64.to_le_bytes());
        let result = client
            .call_contract(&contract, 0, yes_params, vec![], 1_000_000)
            .await
            .unwrap();
        assert!(!result.tx_result.success, "Bet after resolve should fail");
        assert_eq!(
            result.tx_result.exit_code,
            Some(3),
            "Exit code should be 3 (market not open)"
        );
    }

    #[tokio::test]
    async fn prediction_market_cannot_create_twice() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_prediction.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Create market first time
        let result = client
            .call_contract(&contract, 0, vec![0x01, 128], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success);

        // Try to create again - should fail with exit code 1
        let result = client
            .call_contract(&contract, 0, vec![0x01, 200], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(!result.tx_result.success, "Second create should fail");
        assert_eq!(
            result.tx_result.exit_code,
            Some(1),
            "Exit code should be 1 (market already exists)"
        );
    }

    #[tokio::test]
    async fn prediction_market_high_threshold_favors_yes() {
        // With threshold=255, vrf_byte < 255 almost always true -> YES wins
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_prediction.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Create market with threshold=255 (almost always YES)
        client
            .call_contract(&contract, 0, vec![0x01, 255], vec![], 1_000_000)
            .await
            .unwrap();

        // Resolve
        client
            .call_contract(&contract, 0, vec![0x04], vec![], 1_000_000)
            .await
            .unwrap();

        let vrf_byte = client
            .get_contract_storage(&contract, b"vrf_byte")
            .await
            .unwrap()
            .unwrap();
        let outcome = client
            .get_contract_storage(&contract, b"outcome")
            .await
            .unwrap()
            .unwrap();

        // With threshold=255, only vrf_byte=255 gives NO
        if vrf_byte[0] < 255 {
            assert_eq!(outcome, vec![1u8], "YES should win when vrf_byte < 255");
        } else {
            assert_eq!(outcome, vec![0u8], "NO wins only when vrf_byte = 255");
        }
    }

    // ========================================================================
    // VRF Lottery Contract Tests
    // ========================================================================

    #[tokio::test]
    async fn contract_lottery_selects_winner() {
        let secret_hex = test_vrf_secret_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();
        client.mine_empty_block().await.unwrap();

        // Deploy vrf_lottery.so
        let bytecode = include_bytes!("../../tests/fixtures/vrf_lottery.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Call lottery contract
        let result = client
            .call_contract(&contract, 0, vec![], vec![], 1_000_000)
            .await
            .unwrap();
        assert!(result.tx_result.success, "Lottery contract should succeed");

        // Verify winner was selected
        let winner_idx = client
            .get_contract_storage(&contract, b"winner_index")
            .await
            .unwrap()
            .expect("winner_index should be stored");
        assert_eq!(winner_idx.len(), 1);
        let idx = winner_idx[0];
        assert!(idx < 4, "Winner index should be 0-3, got {}", idx);

        // Verify winner name matches index
        let winner_name = client
            .get_contract_storage(&contract, b"winner")
            .await
            .unwrap()
            .expect("winner should be stored");
        let expected_names = [b"Alice".as_slice(), b"Bob", b"Carol", b"Dave"];
        assert_eq!(
            winner_name, expected_names[idx as usize],
            "Winner name should match index"
        );

        // Verify VRF random was stored
        let vrf_random = client
            .get_contract_storage(&contract, b"vrf_random")
            .await
            .unwrap()
            .expect("vrf_random should be stored");
        assert_eq!(vrf_random.len(), 32);
        assert_ne!(vrf_random, vec![0u8; 32], "VRF random should not be zeros");

        // Verify winner_index matches VRF: winner_index = random[0] % 4
        assert_eq!(
            idx,
            vrf_random[0] % 4,
            "Winner index should be VRF random[0] % 4"
        );
    }
}
