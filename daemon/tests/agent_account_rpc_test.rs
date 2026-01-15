#![allow(clippy::disallowed_methods)]

use serde_json::json;
use std::sync::Arc;
use tempdir::TempDir;
use tos_common::{
    account::{AgentAccountMeta, SessionKey, VersionedBalance, VersionedNonce},
    asset::{AssetData, VersionedAssetData},
    config::{COIN_DECIMALS, TOS_ASSET},
    crypto::{Hash, KeyPair},
    network::Network,
    rpc::RPCHandler,
    versioned_type::Versioned,
};
use tos_daemon::core::{
    blockchain::Blockchain,
    config::{Config, RocksDBConfig},
    storage::{AgentAccountProvider, AssetProvider, BalanceProvider, NonceProvider, RocksStorage},
};

async fn build_blockchain(temp_dir: &TempDir) -> Arc<Blockchain<RocksStorage>> {
    let mut config: Config = serde_json::from_value(json!({
        "rpc": { "getwork": {}, "prometheus": {} },
        "p2p": { "proxy": {} },
        "rocksdb": {},
        "vrf": {}
    }))
    .expect("build daemon config");
    config.rpc.disable = true;
    config.rpc.getwork.disable = true;
    config.p2p.disable = true;
    config.skip_pow_verification = true;
    config.dir_path = Some(format!("{}/", temp_dir.path().to_string_lossy()));
    config.rocksdb = RocksDBConfig::default();

    let storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy(),
        Network::Devnet,
        &config.rocksdb,
    );
    let blockchain = Blockchain::new(config, Network::Devnet, storage)
        .await
        .expect("create blockchain");

    // Register TOS asset for completeness
    {
        let mut storage_write = blockchain.get_storage().write().await;
        let asset_data = AssetData::new(
            COIN_DECIMALS,
            "TOS".to_string(),
            "TOS".to_string(),
            None,
            None,
        );
        let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
        storage_write
            .add_asset(&TOS_ASSET, 0, versioned)
            .await
            .expect("register TOS asset");
    }

    blockchain
}

#[tokio::test]
async fn test_agent_account_rpc_getters() {
    let temp_dir = TempDir::new("agent_account_rpc_test").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;

    let owner = KeyPair::new();
    let controller = KeyPair::new();
    let owner_pub = owner.get_public_key().compress();
    let controller_pub = controller.get_public_key().compress();

    {
        let mut storage = blockchain.get_storage().write().await;
        storage
            .set_last_nonce_to(&owner_pub, 0, &VersionedNonce::new(0, Some(0)))
            .await
            .expect("set nonce");
        storage
            .set_last_balance_to(
                &owner_pub,
                &TOS_ASSET,
                0,
                &VersionedBalance::new(1000, Some(0)),
            )
            .await
            .expect("set balance");

        let meta = AgentAccountMeta {
            owner: owner_pub.clone(),
            controller: controller_pub,
            policy_hash: Hash::new([1u8; 32]),
            status: 0,
            energy_pool: None,
            session_key_root: None,
        };
        storage
            .set_agent_account_meta(&owner_pub, &meta)
            .await
            .expect("set agent meta");

        let session_key = SessionKey {
            key_id: 1,
            public_key: KeyPair::new().get_public_key().compress(),
            expiry_topoheight: 9999,
            max_value_per_window: 1000,
            allowed_targets: vec![],
            allowed_assets: vec![TOS_ASSET],
        };
        storage
            .set_session_key(&owner_pub, &session_key)
            .await
            .expect("set session key");
    }

    let mut handler = RPCHandler::new(blockchain);
    tos_daemon::rpc::rpc::register_methods(&mut handler, false, false);

    let address = owner_pub.clone().to_address(false).to_string();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_agent_account",
        "params": { "address": address.clone() }
    });

    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let meta = response
        .get("result")
        .and_then(|res| res.get("meta"))
        .expect("meta result");
    assert_eq!(meta.get("status").and_then(|v| v.as_u64()), Some(0));

    let address = owner_pub.to_address(false).to_string();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "get_agent_session_keys",
        "params": { "address": address }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let keys = response
        .get("result")
        .and_then(|res| res.get("keys"))
        .and_then(|val| val.as_array())
        .expect("keys result");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].get("key_id").and_then(|v| v.as_u64()), Some(1));
}
