use rand::Rng;
use serde_json::json;
use std::str::FromStr;
use tempdir::TempDir;
use tokio::time::{sleep, Duration};

use tos_common::block::{Block, BlockVersion};
use tos_common::config::{COIN_VALUE, FEE_PER_KB, TOS_ASSET};
use tos_common::crypto::{Address, AddressType, KeyPair, PublicKey};
use tos_common::network::Network;
use tos_common::rpc::server::RPCServerHandler;
use tos_common::serializer::Serializer;
use tos_common::tns::REGISTRATION_FEE;
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::{
    CreateEscrowPayload, FeeType, Reference, RegisterNamePayload, TransactionType, TransferPayload,
    TxVersion,
};
use tos_daemon::core::blockchain::{estimate_required_tx_fees, Blockchain, BroadcastOption};
use tos_daemon::core::config::{Config, RocksDBConfig};
use tos_daemon::core::storage::{BalanceProvider, BlockDagProvider, NonceProvider, RocksStorage};
use tos_daemon::rpc::DaemonRpcServer;
use tos_daemon::vrf::WrappedMinerSecret;

struct TestRpcServer {
    base_url: String,
    server: std::sync::Arc<DaemonRpcServer<RocksStorage>>,
    miner_keypair: KeyPair,
    miner_pubkey: PublicKey,
    _temp_dir: TempDir,
}

const TEST_FUNDING_BALANCE: u64 = COIN_VALUE * 10;

fn pick_free_port() -> u16 {
    let mut rng = rand::thread_rng();
    rng.gen_range(10000..20000)
}

async fn ensure_account_ready(
    blockchain: &Blockchain<RocksStorage>,
    miner: &KeyPair,
    key: &PublicKey,
    min_balance: u64,
) {
    let miner_pubkey = miner.get_public_key().compress();
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let mut attempts = 0usize;
    loop {
        attempts += 1;
        let topoheight = blockchain.get_topo_height();
        let balance = {
            let storage = blockchain.get_storage().read().await;
            storage
                .get_balance_at_maximum_topoheight(key, &TOS_ASSET, topoheight)
                .await
                .expect("get balance")
                .map(|(_, v)| v.get_balance())
                .unwrap_or(0)
        };
        if balance >= min_balance {
            break;
        }
        let block = blockchain
            .mine_block(&miner_pubkey)
            .await
            .expect("mine block");
        blockchain
            .add_new_block(block, None, BroadcastOption::None, true)
            .await
            .expect("add block");
        if key == &miner_pubkey {
            continue;
        }
        let topoheight = blockchain.get_topo_height();
        let (reference_hash, nonce, miner_balance) = {
            let storage = blockchain.get_storage().read().await;
            let (reference_hash, _) = storage
                .get_block_header_at_topoheight(topoheight)
                .await
                .expect("get reference header");
            let nonce = storage
                .get_nonce_at_maximum_topoheight(&miner_pubkey, topoheight)
                .await
                .expect("get miner nonce")
                .map(|(_, v)| v.get_nonce())
                .unwrap_or(0);
            let miner_balance = storage
                .get_balance_at_maximum_topoheight(&miner_pubkey, &TOS_ASSET, topoheight)
                .await
                .expect("get miner balance")
                .map(|(_, v)| v.get_balance())
                .unwrap_or(0);
            (reference_hash, nonce, miner_balance)
        };
        let reference = Reference {
            topoheight,
            hash: reference_hash,
        };
        let amount_needed = min_balance.saturating_sub(balance);
        let payload = TransferPayload::new(TOS_ASSET, key.clone(), amount_needed, None);
        let draft = UnsignedTransaction::new_with_fee_type(
            TxVersion::T1,
            chain_id,
            miner_pubkey.clone(),
            TransactionType::Transfers(vec![payload.clone()]),
            0,
            FeeType::TOS,
            nonce,
            reference.clone(),
        );
        let draft_tx = draft.finalize(miner);
        let required_fee = {
            let storage = blockchain.get_storage().read().await;
            estimate_required_tx_fees(&*storage, topoheight, &draft_tx, BlockVersion::Nobunaga)
                .await
                .expect("estimate transfer fee")
        };
        let max_send = miner_balance.saturating_sub(required_fee);
        let amount = amount_needed.min(max_send);
        if amount == 0 {
            continue;
        }
        let send_payload = TransferPayload::new(TOS_ASSET, key.clone(), amount, None);
        let unsigned = UnsignedTransaction::new_with_fee_type(
            TxVersion::T1,
            chain_id,
            miner_pubkey.clone(),
            TransactionType::Transfers(vec![send_payload]),
            required_fee,
            FeeType::TOS,
            nonce,
            reference,
        );
        let tx = unsigned.finalize(miner);
        blockchain
            .add_tx_to_mempool(tx, true)
            .await
            .expect("add funding transfer");
        let block = blockchain
            .mine_block(&miner_pubkey)
            .await
            .expect("mine funding block");
        blockchain
            .add_new_block(block, None, BroadcastOption::None, true)
            .await
            .expect("add funding block");
        if attempts > 50 {
            panic!(
                "unable to fund account {} to min balance {}",
                key.as_address(false),
                min_balance
            );
        }
    }
}

async fn start_rpc_server() -> TestRpcServer {
    for _ in 0..50 {
        let temp_dir = TempDir::new("tck_multinode_rpc").unwrap();
        let port = pick_free_port();
        let miner_keypair = KeyPair::new();
        let miner_pubkey = miner_keypair.get_public_key().compress();
        let miner_secret_hex = miner_keypair.get_private_key().to_hex();
        let mut config: Config = serde_json::from_value(json!({
            "rpc": { "getwork": {}, "prometheus": {} },
            "p2p": { "proxy": {} },
            "rocksdb": {},
            "vrf": {}
        }))
        .expect("build daemon config");
        config.rpc.bind_address = format!("127.0.0.1:{}", port);
        config.rpc.disable = true;
        config.rpc.threads = 1;
        config.rpc.prometheus.enable = false;
        config.rpc.getwork.disable = true;
        config.p2p.disable = true;
        config.skip_pow_verification = true;
        config.dir_path = Some(format!("{}/", temp_dir.path().to_string_lossy()));
        config.rocksdb = RocksDBConfig::default();
        config.vrf.miner_private_key =
            Some(WrappedMinerSecret::from_str(&miner_secret_hex).expect("miner key"));

        let storage = RocksStorage::new(
            &temp_dir.path().to_string_lossy(),
            Network::Devnet,
            &config.rocksdb,
        );
        let blockchain = match Blockchain::new(config.clone(), Network::Devnet, storage).await {
            Ok(chain) => chain,
            Err(_) => continue,
        };
        ensure_account_ready(&blockchain, &miner_keypair, &miner_pubkey, 0).await;

        let mut rpc_config = config.rpc.clone();
        rpc_config.disable = false;
        match DaemonRpcServer::new(blockchain, rpc_config).await {
            Ok(server) => {
                sleep(Duration::from_millis(100)).await;
                return TestRpcServer {
                    base_url: format!("http://127.0.0.1:{}", port),
                    server,
                    miner_keypair,
                    miner_pubkey,
                    _temp_dir: temp_dir,
                };
            }
            Err(err) => {
                if matches!(
                    err,
                    tos_daemon::core::error::BlockchainError::ErrorStd(ref io_err)
                        if io_err.kind() == std::io::ErrorKind::AddrInUse
                ) {
                    continue;
                }
                panic!("start rpc server: {err:?}");
            }
        }
    }

    panic!("start rpc server: address already in use after retries");
}

async fn mine_and_apply(blockchain: &Blockchain<RocksStorage>, miner: &PublicKey) -> Block {
    let block = blockchain.mine_block(miner).await.expect("mine block");
    blockchain
        .add_new_block(block.clone(), None, BroadcastOption::None, true)
        .await
        .expect("add block");
    block
}

async fn submit_escrow_and_tns(
    blockchain: &Blockchain<RocksStorage>,
    miner_pubkey: &PublicKey,
    payer: &KeyPair,
    tns_owner: &KeyPair,
    provider_pubkey: &PublicKey,
) {
    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        hash: reference_hash,
        topoheight,
    };
    let chain_id = u8::try_from(blockchain.get_network().chain_id()).expect("chain id fits u8");
    let payer_pubkey = payer.get_public_key().compress();
    let tns_owner_pubkey = tns_owner.get_public_key().compress();

    let payload = CreateEscrowPayload {
        task_id: "task-mn-escrow".to_string(),
        provider: provider_pubkey.clone(),
        amount: 10_000,
        asset: TOS_ASSET,
        timeout_blocks: 100,
        challenge_window: 10,
        challenge_deposit_bps: 0,
        optimistic_release: false,
        arbitration_config: None,
        metadata: None,
    };

    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        payer_pubkey.clone(),
        TransactionType::CreateEscrow(payload),
        FEE_PER_KB,
        FeeType::TOS,
        0,
        reference.clone(),
    );
    let tx = unsigned.finalize(payer);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add create escrow tx");

    let payload = RegisterNamePayload::new("multinode".to_string());
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        chain_id,
        tns_owner_pubkey.clone(),
        TransactionType::RegisterName(payload),
        REGISTRATION_FEE,
        FeeType::TOS,
        0,
        reference,
    );
    let tx = unsigned.finalize(tns_owner);
    blockchain
        .add_tx_to_mempool(tx, true)
        .await
        .expect("add register name tx");

    mine_and_apply(blockchain, miner_pubkey).await;
}

#[test]
fn test_multinode_escrow_consensus_path() {
    let handle = std::thread::Builder::new()
        .name("tck-multinode-escrow-consensus".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(run_multinode_escrow_consensus_path());
        })
        .expect("spawn multinode escrow consensus thread");
    handle
        .join()
        .expect("join multinode escrow consensus thread");
}

async fn run_multinode_escrow_consensus_path() {
    let node0 = start_rpc_server().await;
    let node1 = start_rpc_server().await;
    let client = reqwest::Client::new();

    let blockchain0 = node0.server.get_rpc_handler().get_data().clone();
    let blockchain1 = node1.server.get_rpc_handler().get_data().clone();
    let miner_pubkey = node0.miner_pubkey.clone();

    let payer = KeyPair::new();
    let provider = KeyPair::new();
    let tns_owner = KeyPair::new();
    let payer_pubkey = payer.get_public_key().compress();
    let provider_pubkey = provider.get_public_key().compress();
    let tns_owner_pubkey = tns_owner.get_public_key().compress();
    let payer_address = Address::new(false, AddressType::Normal, payer_pubkey.clone());

    ensure_account_ready(
        &blockchain0,
        &node0.miner_keypair,
        &payer_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    ensure_account_ready(
        &blockchain0,
        &node0.miner_keypair,
        &tns_owner_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    ensure_account_ready(
        &blockchain1,
        &node1.miner_keypair,
        &payer_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;
    ensure_account_ready(
        &blockchain1,
        &node1.miner_keypair,
        &tns_owner_pubkey,
        TEST_FUNDING_BALANCE,
    )
    .await;

    submit_escrow_and_tns(
        &blockchain0,
        &miner_pubkey,
        &payer,
        &tns_owner,
        &provider_pubkey,
    )
    .await;
    submit_escrow_and_tns(
        &blockchain1,
        &node1.miner_pubkey,
        &payer,
        &tns_owner,
        &provider_pubkey,
    )
    .await;

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "get_escrows_by_client",
        "params": { "address": payer_address.to_string(), "skip": 0, "maximum": 10 }
    });

    let resp = client
        .post(format!("{}/json_rpc", node1.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_escrows_by_client request")
        .json::<serde_json::Value>()
        .await
        .expect("get_escrows_by_client response");

    let escrows = resp
        .get("result")
        .and_then(|v| v.get("escrows"))
        .and_then(|v| v.as_array())
        .expect("escrows array");
    assert!(escrows
        .iter()
        .any(|e| e.get("taskId") == Some(&json!("task-mn-escrow"))));

    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "get_account_name_hash",
        "params": { "address": Address::new(false, AddressType::Normal, tns_owner_pubkey).to_string() }
    });

    let resp = client
        .post(format!("{}/json_rpc", node1.base_url))
        .json(&req)
        .send()
        .await
        .expect("get_account_name_hash request")
        .json::<serde_json::Value>()
        .await
        .expect("get_account_name_hash response");

    let name_hash = resp
        .get("result")
        .and_then(|v| v.get("name_hash"))
        .and_then(|v| v.as_str())
        .expect("name_hash in response");
    assert_ne!(name_hash, "");

    node0.server.stop().await;
    node1.server.stop().await;
}
