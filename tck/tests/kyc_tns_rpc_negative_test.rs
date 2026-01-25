use serde_json::json;
use tempdir::TempDir;
use tos_common::crypto::{Address, AddressType, KeyPair};
use tos_common::network::Network;
use tos_common::rpc::RPCHandler;
use tos_common::tns::MAX_NAME_LENGTH;
use tos_daemon::core::blockchain::Blockchain;
use tos_daemon::core::config::Config;
use tos_daemon::core::storage::RocksStorage;
use tos_daemon::rpc::rpc as daemon_rpc;

async fn make_blockchain() -> std::sync::Arc<Blockchain<RocksStorage>> {
    let temp_dir = TempDir::new("tck_rpc_kyc_tns").unwrap();
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
async fn test_kyc_missing_params() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    daemon_rpc::register_methods(&mut handler, true, true, true);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "has_kyc",
        "params": {}
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}

#[tokio::test]
async fn test_kyc_network_mismatch() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    daemon_rpc::register_methods(&mut handler, true, true, true);

    let keypair = KeyPair::new();
    let mainnet_address = Address::new(
        true,
        AddressType::Normal,
        keypair.get_public_key().compress(),
    );

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_kyc",
        "params": {
            "address": mainnet_address.to_string()
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}

#[tokio::test]
async fn test_tns_invalid_address_param() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    daemon_rpc::register_methods(&mut handler, true, true, true);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "has_registered_name",
        "params": {
            "address": "not-an-address"
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}

#[tokio::test]
async fn test_tns_name_too_long_returns_invalid_format() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    daemon_rpc::register_methods(&mut handler, true, true, true);

    let long_name = "a".repeat(MAX_NAME_LENGTH + 1);
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "is_name_available",
        "params": {
            "name": long_name
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap();
    let result = resp.get("result").expect("result field");
    assert_eq!(
        result.get("valid_format").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn test_tns_get_messages_invalid_limit_type() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    daemon_rpc::register_methods(&mut handler, true, true, true);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_messages",
        "params": {
            "recipient_name_hash": "0000000000000000000000000000000000000000000000000000000000000000",
            "offset": 0,
            "limit": "ten"
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}

#[tokio::test]
async fn test_tns_get_account_name_hash_invalid_address() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    daemon_rpc::register_methods(&mut handler, true, true, true);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_account_name_hash",
        "params": {
            "address": "invalid-address"
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}

#[tokio::test]
async fn test_tns_get_message_count_invalid_hash_type() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    daemon_rpc::register_methods(&mut handler, true, true, true);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_message_count",
        "params": {
            "recipient_name_hash": 12345
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}

#[tokio::test]
async fn test_tns_get_message_by_id_invalid_hash_type() {
    let blockchain = make_blockchain().await;
    let mut handler = RPCHandler::new(blockchain);
    daemon_rpc::register_methods(&mut handler, true, true, true);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_message_by_id",
        "params": {
            "message_id": 999
        }
    });

    let body = serde_json::to_vec(&req).unwrap();
    let resp = handler.handle_request(&body).await.unwrap_err().to_json();
    assert_eq!(extract_error_code(&resp), -32602);
}
