use serde_json::json;
use tempdir::TempDir;
use tos_common::crypto::{Address, AddressType, KeyPair};
use tos_common::network::Network;
use tos_common::rpc::RPCHandler;
use tos_daemon::core::blockchain::Blockchain;
use tos_daemon::core::config::Config;
use tos_daemon::core::storage::RocksStorage;
use tos_daemon::rpc::{arbitration as arbitration_rpc, escrow as escrow_rpc};

async fn make_blockchain() -> std::sync::Arc<Blockchain<RocksStorage>> {
    let temp_dir = TempDir::new("tck_rpc_negative").unwrap();
    let mut config: Config = serde_json::from_value(serde_json::json!({
        "rpc": { "getwork": {}, "prometheus": {} },
        "p2p": { "proxy": {} },
        "rocksdb": {},
        "vrf": {}
    }))
    .expect("build daemon config");
    config.rpc.disable = true;
    config.p2p.disable = true;
    config.skip_pow_verification = true;
    config.dir_path = Some(format!("{}/", temp_dir.path().to_string_lossy()));
    let storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy(),
        Network::Devnet,
        &config.rocksdb,
    );
    Blockchain::new(config, Network::Devnet, storage)
        .await
        .expect("create blockchain")
}

fn extract_error_code(resp: &serde_json::Value) -> i64 {
    resp.get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_i64())
        .expect("error code")
}

#[tokio::test]
async fn test_rpc_invalid_params_error() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    escrow_rpc::register_methods(&mut handler);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_escrow",
        "params": {}
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}

#[tokio::test]
async fn test_rpc_invalid_version_error() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    escrow_rpc::register_methods(&mut handler);

    let req = json!({
        "jsonrpc": "1.0",
        "id": 1,
        "method": "get_escrow",
        "params": {}
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32600);
}

#[tokio::test]
async fn test_rpc_method_not_found() {
    let blockchain = make_blockchain().await;
    let handler = RPCHandler::new(blockchain);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "nonexistent_method",
        "params": {}
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32601);
}

#[tokio::test]
async fn test_escrow_network_mismatch() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    escrow_rpc::register_methods(&mut handler);

    let keypair = KeyPair::new();
    let mainnet_address = Address::new(
        true,
        AddressType::Normal,
        keypair.get_public_key().compress(),
    );

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_escrows_by_client",
        "params": {
            "address": mainnet_address.to_string(),
            "maximum": 1,
            "skip": 0
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}

#[tokio::test]
async fn test_arbitration_network_mismatch() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    arbitration_rpc::register_methods(&mut handler);

    let keypair = KeyPair::new();
    let mainnet_address = Address::new(
        true,
        AddressType::Normal,
        keypair.get_public_key().compress(),
    );

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_arbiter_withdraw_status",
        "params": {
            "address": mainnet_address.to_string()
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}
