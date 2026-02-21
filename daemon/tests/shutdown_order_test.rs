#![allow(clippy::disallowed_methods)]

use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tempdir::TempDir;
use tos_common::network::Network;
use tos_daemon::core::{
    blockchain::Blockchain,
    config::{Config, RocksDBConfig},
    storage::RocksStorage,
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
    config.rocksdb = RocksDBConfig::for_tests();

    let storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy(),
        Network::Devnet,
        &config.rocksdb,
    );
    Blockchain::new(config, Network::Devnet, storage)
        .await
        .expect("create blockchain")
}

#[tokio::test]
async fn test_stop_waits_for_storage_semaphore() {
    let temp_dir = TempDir::new("shutdown_order_test").expect("temp dir");
    let blockchain = build_blockchain(&temp_dir).await;

    let permit = blockchain
        .storage_semaphore()
        .acquire()
        .await
        .expect("acquire semaphore");

    let mut stop_handle = tokio::spawn({
        let blockchain = Arc::clone(&blockchain);
        async move { blockchain.stop().await }
    });

    let early = tokio::time::timeout(Duration::from_millis(100), &mut stop_handle).await;
    assert!(early.is_err(), "stop() should wait for semaphore");

    drop(permit);

    let result = tokio::time::timeout(Duration::from_secs(5), stop_handle).await;
    assert!(result.is_ok(), "stop() should complete after release");
    assert!(
        result.expect("stop join").expect("stop result").is_ok(),
        "stop() returned error"
    );
}
