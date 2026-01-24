use std::str::FromStr;

use serde_json::json;
use tempdir::TempDir;
use tokio::time::{timeout, Duration};

use tos_common::config::TOS_ASSET;
use tos_common::crypto::{Hashable, KeyPair};
use tos_common::network::Network;
use tos_common::serializer::Serializer;
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::{FeeType, Reference, TransactionType, TransferPayload, TxVersion};
use tos_daemon::config::DEV_PUBLIC_KEY;
use tos_daemon::core::blockchain::Blockchain;
use tos_daemon::core::blockchain::BroadcastOption;
use tos_daemon::core::config::{Config, RocksDBConfig};
use tos_daemon::core::simulator::Simulator;
use tos_daemon::core::storage::RocksStorage;
use tos_daemon::core::storage::{AccountProvider, BlockDagProvider, NonceProvider};
use tos_daemon::vrf::WrappedMinerSecret;

#[test]
fn test_simulator_string_round_trip() {
    let sims = [
        Simulator::Blockchain,
        Simulator::BlockDag,
        Simulator::Stress,
    ];
    for sim in sims {
        let as_str = sim.to_string();
        let parsed = Simulator::from_str(&as_str).expect("parse simulator");
        assert_eq!(parsed, sim);
    }
}

#[test]
fn test_simulator_json_round_trip() {
    let sim = Simulator::BlockDag;
    let json = serde_json::to_string(&sim).expect("serialize simulator");
    let parsed: Simulator = serde_json::from_str(&json).expect("deserialize simulator");
    assert_eq!(parsed, sim);
}

#[test]
fn test_simulator_invalid_string() {
    assert!(Simulator::from_str("not-a-simulator").is_err());
}

#[tokio::test]
async fn test_simulator_e2e_block_production() {
    let temp_dir = TempDir::new("tck_simulator_e2e").expect("tempdir");
    let miner_keypair = KeyPair::new();
    let miner_secret_hex = miner_keypair.get_private_key().to_hex();

    let mut config: Config = serde_json::from_value(json!({
        "rpc": { "getwork": {}, "prometheus": {} },
        "p2p": { "proxy": {} },
        "rocksdb": {},
        "vrf": {}
    }))
    .expect("build daemon config");
    config.rpc.disable = true;
    config.p2p.disable = true;
    config.skip_pow_verification = true;
    config.simulator = Some(Simulator::Blockchain);
    config.dir_path = Some(format!("{}/", temp_dir.path().to_string_lossy()));
    config.rocksdb = RocksDBConfig::default();
    config.vrf.miner_private_key =
        Some(WrappedMinerSecret::from_str(&miner_secret_hex).expect("miner key"));

    let storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy(),
        Network::Devnet,
        &config.rocksdb,
    );
    let blockchain = Blockchain::new(config, Network::Devnet, storage)
        .await
        .expect("start blockchain");

    assert!(blockchain.is_simulator_enabled());

    let miner_pubkey = miner_keypair.get_public_key().compress();
    let block = blockchain
        .mine_block(&miner_pubkey)
        .await
        .expect("mine block in simulator mode");
    blockchain
        .add_new_block(block, None, BroadcastOption::None, false)
        .await
        .expect("add block in simulator mode");

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(blockchain.get_topo_height() >= 1);
}

#[test]
#[ignore = "hangs in add_tx_to_mempool for transfer tx in simulator full chain path; tracked in BUGS.md"]
fn test_simulator_e2e_tx_inclusion_and_receipt() {
    const STACK_SIZE: usize = 16 * 1024 * 1024;
    std::thread::Builder::new()
        .name("tck_simulator_tx_e2e".to_string())
        .stack_size(STACK_SIZE)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            runtime.block_on(async {
                let temp_dir = TempDir::new("tck_simulator_tx_e2e").expect("tempdir");
                let miner_keypair = KeyPair::new();
                let miner_secret_hex = miner_keypair.get_private_key().to_hex();

                let mut config: Config = serde_json::from_value(json!({
                    "rpc": { "getwork": {}, "prometheus": {} },
                    "p2p": { "proxy": {} },
                    "rocksdb": {},
                    "vrf": {}
                }))
                .expect("build daemon config");
                config.rpc.disable = true;
                config.p2p.disable = true;
                config.skip_pow_verification = true;
                config.simulator = None;
                config.dir_path = Some(format!("{}/", temp_dir.path().to_string_lossy()));
                config.rocksdb = RocksDBConfig::default();
                config.vrf.miner_private_key =
                    Some(WrappedMinerSecret::from_str(&miner_secret_hex).expect("miner key"));

                let storage = RocksStorage::new(
                    &temp_dir.path().to_string_lossy(),
                    Network::Devnet,
                    &config.rocksdb,
                );
                let blockchain = Blockchain::new(config, Network::Devnet, storage)
                    .await
                    .expect("start blockchain");

                let sender = miner_keypair.clone();
                let recipient = KeyPair::new();
                let sender_pub = sender.get_public_key().compress();
                let sender_pub_for_tx = sender_pub.clone();
                let recipient_pub = recipient.get_public_key().compress();
                let miner_pub = miner_keypair.get_public_key().compress();
                eprintln!(
                    "simulator tx test accounts: sender={} recipient={} miner={} dev={}",
                    sender_pub.as_address(false),
                    recipient_pub.as_address(false),
                    miner_pub.as_address(false),
                    DEV_PUBLIC_KEY.as_address(false)
                );
                eprintln!(
                    "simulator tx test pubkeys: sender={} recipient={} miner={} dev={}",
                    hex::encode(sender_pub.as_bytes()),
                    hex::encode(recipient_pub.as_bytes()),
                    hex::encode(miner_pub.as_bytes()),
                    hex::encode(DEV_PUBLIC_KEY.as_bytes())
                );

                {
                    let mut storage = blockchain.get_storage().write().await;
                    storage
                        .set_account_registration_topoheight(&sender_pub, 0)
                        .await
                        .expect("register sender");
                    storage
                        .set_account_registration_topoheight(&recipient_pub, 0)
                        .await
                        .expect("register recipient");
                    storage
                        .set_account_registration_topoheight(&miner_pub, 0)
                        .await
                        .expect("register miner");
                    storage
                        .set_account_registration_topoheight(&DEV_PUBLIC_KEY, 0)
                        .await
                        .expect("register dev");
                }

                let miner_pubkey = sender.get_public_key().compress();
                let funding_block =
                    timeout(Duration::from_secs(5), blockchain.mine_block(&miner_pubkey))
                        .await
                        .expect("mine funding block timeout")
                        .expect("mine funding block");
                timeout(
                    Duration::from_secs(5),
                    blockchain.add_new_block(funding_block, None, BroadcastOption::None, false),
                )
                .await
                .expect("add funding block timeout")
                .expect("add funding block");

                let payload = TransferPayload::new(TOS_ASSET, recipient_pub, 10, None);
                let (reference_hash, _) = blockchain
                    .get_storage()
                    .read()
                    .await
                    .get_block_header_at_topoheight(blockchain.get_topo_height())
                    .await
                    .expect("get reference header");
                let reference = Reference {
                    topoheight: blockchain.get_topo_height(),
                    hash: reference_hash,
                };
                let unsigned = UnsignedTransaction::new_with_fee_type(
                    TxVersion::T1,
                    Network::Devnet.chain_id().try_into().unwrap(),
                    sender_pub_for_tx,
                    TransactionType::Transfers(vec![payload]),
                    20_000,
                    FeeType::TOS,
                    0,
                    reference,
                );
                let tx = unsigned.finalize(&sender);
                let tx_hash = tx.hash();

                timeout(
                    Duration::from_secs(5),
                    blockchain.add_tx_to_mempool(tx, false),
                )
                .await
                .expect("add tx to mempool timeout")
                .expect("add tx to mempool");
                assert!(blockchain.has_tx(&tx_hash).await.expect("has tx"));

                {
                    let storage = blockchain.get_storage().read().await;
                    let nonce = storage
                        .get_nonce_at_maximum_topoheight(&sender_pub, blockchain.get_topo_height())
                        .await
                        .expect("read sender nonce before add_new_block");
                    if let Some((topo, version)) = &nonce {
                        eprintln!(
                            "sender nonce before add_new_block: topo={} nonce={}",
                            topo,
                            version.get_nonce()
                        );
                    }
                    assert!(nonce.is_some(), "sender nonce missing before add_new_block");
                    let registered_at = storage
                        .get_account_registration_topoheight(&sender_pub)
                        .await
                        .expect("read sender registration");
                    eprintln!(
                        "sender registered_at before add_new_block: {}",
                        registered_at
                    );
                }

                let block = timeout(Duration::from_secs(5), blockchain.mine_block(&miner_pubkey))
                    .await
                    .expect("mine block timeout")
                    .expect("mine block");
                timeout(
                    Duration::from_secs(5),
                    blockchain.add_new_block(block, None, BroadcastOption::None, false),
                )
                .await
                .expect("add block timeout")
                .expect("add block");

                assert!(
                    timeout(Duration::from_secs(5), blockchain.is_tx_included(&tx_hash))
                        .await
                        .expect("is_tx_included timeout")
                        .expect("tx included")
                );
                let stored_tx = timeout(Duration::from_secs(5), blockchain.get_tx(&tx_hash))
                    .await
                    .expect("get tx timeout")
                    .expect("get tx");
                assert_eq!(stored_tx.as_ref().hash(), tx_hash);
            });
        })
        .expect("spawn simulator tx test thread")
        .join()
        .expect("simulator tx test thread panic");
}
