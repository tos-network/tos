use std::str::FromStr;

use tempdir::TempDir;
use tokio::time::timeout;

use tos_common::api::daemon::{ForkCondition, HardFork};
use tos_common::block::BlockVersion;
use tos_common::config::TOS_ASSET;
use tos_common::crypto::{Hashable, KeyPair};
use tos_common::network::Network;
use tos_common::serializer::Serializer;
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::{FeeType, Reference, TransactionType, TransferPayload, TxVersion};
use tos_daemon::config::{
    clear_hard_forks_override_for_tests, set_hard_forks_override_for_tests, DEV_PUBLIC_KEY,
};
use tos_daemon::core::blockchain::BroadcastOption;
use tos_daemon::core::blockchain::{estimate_required_tx_fees, Blockchain};
use tos_daemon::core::config::{Config, RocksDBConfig};
use tos_daemon::core::hard_fork::get_activated_hard_fork;
use tos_daemon::core::storage::{BalanceProvider, BlockDagProvider, NonceProvider, RocksStorage};
use tos_daemon::vrf::WrappedMinerSecret;

struct HardForkOverrideGuard;

impl HardForkOverrideGuard {
    fn new(forks: Vec<HardFork>) -> Self {
        set_hard_forks_override_for_tests(forks);
        Self
    }
}

impl Drop for HardForkOverrideGuard {
    fn drop(&mut self) {
        clear_hard_forks_override_for_tests();
    }
}

async fn ensure_account_ready(
    blockchain: &Blockchain<RocksStorage>,
    miner: &KeyPair,
    key: &tos_common::crypto::PublicKey,
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
            panic!("unable to fund account to min balance {}", min_balance);
        }
    }
}

async fn submit_transfer_and_mine(
    blockchain: &Blockchain<RocksStorage>,
    sender: &KeyPair,
    recipient: &tos_common::crypto::PublicKey,
    amount: u64,
) -> tos_common::crypto::Hash {
    let sender_pub = sender.get_public_key().compress();
    let topoheight = blockchain.get_topo_height();
    let (reference_hash, _) = blockchain
        .get_storage()
        .read()
        .await
        .get_block_header_at_topoheight(topoheight)
        .await
        .expect("get reference header");
    let reference = Reference {
        topoheight,
        hash: reference_hash,
    };
    let nonce = blockchain
        .get_storage()
        .read()
        .await
        .get_nonce_at_maximum_topoheight(&sender_pub, topoheight)
        .await
        .expect("get sender nonce")
        .map(|(_, v)| v.get_nonce())
        .unwrap_or(0);
    let payload = TransferPayload::new(TOS_ASSET, recipient.clone(), amount, None);
    let draft = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        Network::Devnet.chain_id().try_into().unwrap(),
        sender_pub.clone(),
        TransactionType::Transfers(vec![payload.clone()]),
        0,
        FeeType::TOS,
        nonce,
        reference.clone(),
    );
    let draft_tx = draft.finalize(sender);
    let required_fee = {
        let storage = blockchain.get_storage().read().await;
        estimate_required_tx_fees(&*storage, topoheight, &draft_tx, BlockVersion::Nobunaga)
            .await
            .expect("estimate transfer fee")
    };
    let unsigned = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        Network::Devnet.chain_id().try_into().unwrap(),
        sender_pub,
        TransactionType::Transfers(vec![payload]),
        required_fee,
        FeeType::TOS,
        nonce,
        reference,
    );
    let tx = unsigned.finalize(sender);
    let tx_hash = tx.hash();

    timeout(
        std::time::Duration::from_secs(5),
        blockchain.add_tx_to_mempool(tx, false),
    )
    .await
    .expect("add tx timeout")
    .expect("add tx");

    let miner_pub = sender.get_public_key().compress();
    let block = timeout(
        std::time::Duration::from_secs(5),
        blockchain.mine_block(&miner_pub),
    )
    .await
    .expect("mine block timeout")
    .expect("mine block");
    timeout(
        std::time::Duration::from_secs(5),
        blockchain.add_new_block(block, None, BroadcastOption::None, false),
    )
    .await
    .expect("add block timeout")
    .expect("add block");

    assert!(timeout(
        std::time::Duration::from_secs(5),
        blockchain.is_tx_included(&tx_hash)
    )
    .await
    .expect("is_tx_included timeout")
    .expect("tx included"));

    tx_hash
}

#[test]
fn test_hard_fork_e2e_parity_block_activation() {
    const STACK_SIZE: usize = 16 * 1024 * 1024;
    std::thread::Builder::new()
        .name("tck-hard-fork-e2e".to_string())
        .stack_size(STACK_SIZE)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            runtime.block_on(async { run_hard_fork_e2e_parity().await });
        })
        .expect("spawn hard fork e2e thread")
        .join()
        .expect("hard fork e2e thread panic");
}

async fn run_hard_fork_e2e_parity() {
    let _guard = HardForkOverrideGuard::new(vec![
        HardFork {
            condition: ForkCondition::Block(0),
            version: BlockVersion::Nobunaga,
            changelog: "Nobunaga (genesis)",
            version_requirement: None,
        },
        HardFork {
            condition: ForkCondition::Block(2),
            version: BlockVersion::Nobunaga,
            changelog: "Future fork parity test",
            version_requirement: Some(">=0.0.0"),
        },
    ]);

    let temp_dir = TempDir::new("tck_hard_fork_e2e").expect("tempdir");
    let miner_keypair = KeyPair::new();
    let miner_secret_hex = miner_keypair.get_private_key().to_hex();

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
    let recipient_pub = recipient.get_public_key().compress();
    let miner_pub = miner_keypair.get_public_key().compress();

    // Pre-fork (height 0)
    let initial_height = blockchain.get_topo_height();
    let pre_fork =
        get_activated_hard_fork(&Network::Devnet, initial_height, 0, 0).expect("activated fork");
    assert_eq!(pre_fork.changelog, "Nobunaga (genesis)");

    ensure_account_ready(&blockchain, &miner_keypair, &miner_pub, 0).await;
    ensure_account_ready(&blockchain, &miner_keypair, &recipient_pub, 1).await;
    ensure_account_ready(&blockchain, &miner_keypair, &DEV_PUBLIC_KEY, 1).await;

    let _tx1 = submit_transfer_and_mine(&blockchain, &sender, &recipient_pub, 10).await;

    // Ensure we reach height 2 (fork activation)
    while blockchain.get_topo_height() < 2 {
        let miner_pubkey = miner_keypair.get_public_key().compress();
        let block = timeout(
            std::time::Duration::from_secs(5),
            blockchain.mine_block(&miner_pubkey),
        )
        .await
        .expect("mine block timeout")
        .expect("mine block");
        timeout(
            std::time::Duration::from_secs(5),
            blockchain.add_new_block(block, None, BroadcastOption::None, false),
        )
        .await
        .expect("add block timeout")
        .expect("add block");
    }

    let post_fork = get_activated_hard_fork(&Network::Devnet, blockchain.get_topo_height(), 0, 0)
        .expect("activated fork");
    assert_eq!(post_fork.changelog, "Future fork parity test");

    let _tx2 = submit_transfer_and_mine(&blockchain, &sender, &recipient_pub, 11).await;
}
