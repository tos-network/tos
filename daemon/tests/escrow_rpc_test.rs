#![allow(clippy::disallowed_methods)]

use serde_json::json;
use std::sync::Arc;
use tempdir::TempDir;
use tos_common::{
    asset::{AssetData, VersionedAssetData},
    config::{COIN_DECIMALS, TOS_ASSET},
    crypto::{Hash, KeyPair},
    escrow::{
        AppealInfo, ArbitrationConfig, ArbitrationMode, DisputeInfo, EscrowAccount, EscrowState,
        ResolutionRecord,
    },
    network::Network,
    rpc::RPCHandler,
    versioned_type::Versioned,
};
use tos_daemon::core::{
    blockchain::Blockchain,
    config::{Config, RocksDBConfig},
    storage::{AssetProvider, EscrowProvider, RocksStorage},
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
async fn test_escrow_rpc_endpoints() {
    let temp_dir = TempDir::new("escrow_rpc_test").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;

    let payer = KeyPair::new();
    let payee = KeyPair::new();
    let payer_pub = payer.get_public_key().compress();
    let payee_pub = payee.get_public_key().compress();

    let escrow_id = Hash::new([7u8; 32]);
    let dispute_id = Hash::new([9u8; 32]);

    let escrow = EscrowAccount {
        id: escrow_id.clone(),
        task_id: "task-1".to_string(),
        payer: payer_pub.clone(),
        payee: payee_pub.clone(),
        amount: 100,
        total_amount: 100,
        released_amount: 0,
        refunded_amount: 0,
        pending_release_amount: None,
        challenge_deposit: 0,
        asset: TOS_ASSET,
        state: EscrowState::Resolved,
        dispute_id: Some(dispute_id.clone()),
        dispute_round: Some(0),
        challenge_window: 10,
        challenge_deposit_bps: 500,
        optimistic_release: true,
        release_requested_at: None,
        created_at: 1,
        updated_at: 2,
        timeout_at: 100,
        timeout_blocks: 99,
        arbitration_config: Some(ArbitrationConfig {
            mode: ArbitrationMode::Single,
            arbiters: vec![KeyPair::new().get_public_key().compress()],
            threshold: None,
            fee_amount: 5,
            allow_appeal: true,
        }),
        dispute: Some(DisputeInfo {
            initiator: payer_pub.clone(),
            reason: "dispute".to_string(),
            evidence_hash: None,
            disputed_at: 2,
            deadline: 100,
        }),
        appeal: Some(AppealInfo {
            appellant: payee_pub.clone(),
            reason: "appeal".to_string(),
            new_evidence_hash: None,
            deposit: 10,
            appealed_at: 3,
            deadline: 100,
            votes: Vec::new(),
            committee: Vec::new(),
            threshold: 1,
        }),
        resolutions: vec![ResolutionRecord {
            tier: 1,
            resolver: vec![KeyPair::new().get_public_key().compress()],
            client_amount: 50,
            provider_amount: 50,
            resolution_hash: Hash::new([3u8; 32]),
            resolved_at: 2,
            appealed: true,
        }],
    };

    {
        let mut storage = blockchain.get_storage().write().await;
        storage.set_escrow(&escrow).await.expect("set escrow");
        storage
            .add_escrow_history(&escrow_id, 2, &dispute_id)
            .await
            .expect("add escrow history");
        storage
            .add_escrow_history(&escrow_id, 5, &Hash::new([5u8; 32]))
            .await
            .expect("add escrow history");
    }

    let mut handler = RPCHandler::new(blockchain);
    tos_daemon::rpc::rpc::register_methods(&mut handler, false, false, false);

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_escrow",
        "params": { "escrow_id": escrow_id }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let escrow_result = response
        .get("result")
        .and_then(|val| val.get("id"))
        .expect("escrow id");
    let escrow_hex = escrow_id.to_hex();
    assert_eq!(escrow_result.as_str(), Some(escrow_hex.as_str()));

    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "get_escrows_by_client",
        "params": { "address": payer_pub.to_address(false).to_string(), "maximum": 10 }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let escrows = response
        .get("result")
        .and_then(|val| val.get("escrows"))
        .and_then(|val| val.as_array())
        .expect("escrows array");
    assert_eq!(escrows.len(), 1);

    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "get_escrows_by_provider",
        "params": { "address": payee_pub.to_address(false).to_string(), "maximum": 10 }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let escrows = response
        .get("result")
        .and_then(|val| val.get("escrows"))
        .and_then(|val| val.as_array())
        .expect("escrows array");
    assert_eq!(escrows.len(), 1);

    let request = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "get_escrows_by_task",
        "params": { "task_id": "task-1", "maximum": 10 }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let escrows = response
        .get("result")
        .and_then(|val| val.get("escrows"))
        .and_then(|val| val.as_array())
        .expect("escrows array");
    assert_eq!(escrows.len(), 1);

    let request = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "get_dispute_details",
        "params": { "escrow_id": escrow_id }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let dispute = response
        .get("result")
        .and_then(|val| val.get("dispute"))
        .expect("dispute");
    assert_eq!(
        dispute.get("reason").and_then(|v| v.as_str()),
        Some("dispute")
    );

    let request = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "get_appeal_status",
        "params": { "escrow_id": escrow_id }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let appeal = response
        .get("result")
        .and_then(|val| val.get("appeal"))
        .expect("appeal");
    assert_eq!(
        appeal.get("reason").and_then(|v| v.as_str()),
        Some("appeal")
    );

    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "get_escrow_history",
        "params": { "escrow_id": escrow_id, "maximum": 10 }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let entries = response
        .get("result")
        .and_then(|val| val.get("entries"))
        .and_then(|val| val.as_array())
        .expect("entries array");
    assert_eq!(entries.len(), 2);

    let request = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "get_escrow_history",
        "params": { "escrow_id": escrow_id, "maximum": 1, "descending": true }
    });
    let response = handler
        .handle_request(&serde_json::to_vec(&request).expect("encode request"))
        .await
        .expect("handle request");
    let entries = response
        .get("result")
        .and_then(|val| val.get("entries"))
        .and_then(|val| val.as_array())
        .expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].get("topoheight").and_then(|v| v.as_u64()),
        Some(5)
    );
}
