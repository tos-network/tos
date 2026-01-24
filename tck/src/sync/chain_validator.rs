// Layer 1: Blockchain Storage & Genesis State Tests
//
// Tests the actual blockchain storage providers:
// - Genesis block state (height, topoheight)
// - Storage trait method access (DagOrderProvider, DifficultyProvider)
// - Block execution order tracking
// - Blocks at height retrieval
// - Cumulative difficulty for genesis

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tempdir::TempDir;
    use tos_common::difficulty::CumulativeDifficulty;
    use tos_common::network::Network;
    use tos_daemon::core::blockchain::Blockchain;
    use tos_daemon::core::config::Config;
    use tos_daemon::core::storage::{
        BlockExecutionOrderProvider, BlocksAtHeightProvider, DagOrderProvider, DifficultyProvider,
        RocksStorage,
    };

    async fn make_blockchain() -> (Arc<Blockchain<RocksStorage>>, TempDir) {
        let temp_dir = TempDir::new("tck_chain_validator").unwrap();
        let config: Config = serde_json::from_value(serde_json::json!({
            "rpc": { "getwork": {}, "prometheus": {} },
            "p2p": { "proxy": {} },
            "rocksdb": {}
        }))
        .unwrap();
        let storage = RocksStorage::new(
            &temp_dir.path().to_string_lossy(),
            Network::Devnet,
            &config.rocksdb,
        );
        let blockchain = Blockchain::new(config, Network::Devnet, storage)
            .await
            .expect("create blockchain");
        (blockchain, temp_dir)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Blockchain genesis state verification
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_blockchain_has_genesis_block() {
        let (blockchain, _dir) = make_blockchain().await;

        // Verify blockchain was initialized with genesis
        let height = blockchain.get_height();
        assert_eq!(height, 0, "Genesis block should be at height 0");

        let topo = blockchain.get_topo_height();
        assert_eq!(topo, 0, "Genesis topoheight should be 0");
    }

    #[tokio::test]
    async fn test_blockchain_genesis_hash_retrievable() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let genesis_hash = storage.get_hash_at_topo_height(0).await;
        assert!(
            genesis_hash.is_ok(),
            "Should be able to retrieve genesis hash"
        );

        let header = storage
            .get_block_header_by_hash(&genesis_hash.unwrap())
            .await;
        assert!(header.is_ok(), "Should be able to retrieve genesis header");
    }

    #[tokio::test]
    async fn test_blockchain_genesis_cumulative_difficulty() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let genesis_hash = storage
            .get_hash_at_topo_height(0)
            .await
            .expect("genesis hash");
        let cum_diff = storage
            .get_cumulative_difficulty_for_block_hash(&genesis_hash)
            .await;
        assert!(
            cum_diff.is_ok(),
            "Genesis should have cumulative difficulty"
        );

        let diff = cum_diff.unwrap();
        // Genesis cumulative difficulty should be > 0
        assert!(
            diff > CumulativeDifficulty::from(0u64),
            "Genesis cumulative difficulty should be positive"
        );
    }

    #[tokio::test]
    async fn test_blockchain_genesis_header_has_height_zero() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let genesis_hash = storage
            .get_hash_at_topo_height(0)
            .await
            .expect("genesis hash");
        let header = storage
            .get_block_header_by_hash(&genesis_hash)
            .await
            .expect("genesis header");

        assert_eq!(header.get_height(), 0, "Genesis header height should be 0");
    }

    #[tokio::test]
    async fn test_blockchain_genesis_height_lookup() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let genesis_hash = storage
            .get_hash_at_topo_height(0)
            .await
            .expect("genesis hash");

        let height = storage
            .get_height_for_block_hash(&genesis_hash)
            .await
            .expect("genesis height");
        assert_eq!(height, 0, "Genesis height lookup should return 0");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Storage provider traits
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_storage_block_execution_order() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let genesis_hash = storage
            .get_hash_at_topo_height(0)
            .await
            .expect("genesis hash");

        // Genesis should have a position in the block execution order
        let position = storage.get_block_position_in_order(&genesis_hash).await;
        assert!(
            position.is_ok(),
            "Genesis should have execution order position"
        );
    }

    #[tokio::test]
    async fn test_storage_has_block_in_execution_order() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let genesis_hash = storage
            .get_hash_at_topo_height(0)
            .await
            .expect("genesis hash");

        let has_order = storage
            .has_block_position_in_order(&genesis_hash)
            .await
            .expect("check execution order");
        assert!(has_order, "Genesis should be in execution order");
    }

    #[tokio::test]
    async fn test_storage_blocks_at_height() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let blocks = storage.get_blocks_at_height(0).await;
        assert!(blocks.is_ok());

        let block_set = blocks.unwrap();
        assert_eq!(
            block_set.len(),
            1,
            "Should have exactly one block at height 0 (genesis)"
        );
    }

    #[tokio::test]
    async fn test_storage_no_blocks_at_height_one() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let has_blocks = storage.has_blocks_at_height(1).await;
        assert!(has_blocks.is_ok());
        assert!(
            !has_blocks.unwrap(),
            "Should have no blocks at height 1 on fresh blockchain"
        );
    }

    #[tokio::test]
    async fn test_storage_genesis_is_topologically_ordered() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let genesis_hash = storage
            .get_hash_at_topo_height(0)
            .await
            .expect("genesis hash");

        let is_ordered = storage
            .is_block_topological_ordered(&genesis_hash)
            .await
            .expect("check topo order");
        assert!(is_ordered, "Genesis block should be topologically ordered");
    }

    #[tokio::test]
    async fn test_storage_nonexistent_hash_not_ordered() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let fake_hash = tos_common::crypto::Hash::new([0xAB; 32]);

        let is_ordered = storage
            .is_block_topological_ordered(&fake_hash)
            .await
            .expect("check topo order");
        assert!(!is_ordered, "Nonexistent hash should not be ordered");
    }

    #[tokio::test]
    async fn test_storage_genesis_past_blocks() {
        let (blockchain, _dir) = make_blockchain().await;

        let storage = blockchain.get_storage().read().await;
        let genesis_hash = storage
            .get_hash_at_topo_height(0)
            .await
            .expect("genesis hash");

        let past_blocks = storage
            .get_past_blocks_for_block_hash(&genesis_hash)
            .await
            .expect("genesis past blocks");

        // Genesis block should have no past blocks (it's the first block)
        assert!(
            past_blocks.is_empty(),
            "Genesis should have no past blocks, got {}",
            past_blocks.len()
        );
    }
}
