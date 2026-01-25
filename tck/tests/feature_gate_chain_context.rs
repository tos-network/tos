use tos_common::crypto::Hash;
use tos_tck::tier1_5::{ChainClient, ChainClientConfig, FeatureSet, GenesisAccount};

#[tokio::test]
async fn test_feature_gate_activation_by_topoheight() {
    let features = FeatureSet::empty().activate_at("rpc_v2_responses", 2);
    let config = ChainClientConfig::default()
        .with_account(GenesisAccount::new(Hash::new([7u8; 32]), 1_000_000))
        .with_features(features);

    let mut client = ChainClient::start(config).await.expect("start ChainClient");

    assert_eq!(client.topoheight(), 0);
    assert!(!client.is_feature_active("rpc_v2_responses"));

    client.mine_empty_block().await.expect("mine block 1");
    assert_eq!(client.topoheight(), 1);
    assert!(!client.is_feature_active("rpc_v2_responses"));

    client.mine_empty_block().await.expect("mine block 2");
    assert_eq!(client.topoheight(), 2);
    assert!(client.is_feature_active("rpc_v2_responses"));
}

#[tokio::test]
async fn test_feature_gate_deactivation_override() {
    let features = FeatureSet::mainnet().deactivate("rpc_v2_responses");
    let config = ChainClientConfig::default()
        .with_account(GenesisAccount::new(Hash::new([9u8; 32]), 1_000_000))
        .with_features(features);

    let client = ChainClient::start(config).await.expect("start ChainClient");
    assert!(!client.is_feature_active("rpc_v2_responses"));
}
