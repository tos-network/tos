use tempdir::TempDir;

use tos_common::config::TOS_ASSET;
use tos_common::crypto::{Hash, Hashable, KeyPair, Signature};
use tos_common::network::Network;
use tos_common::transaction::verify::ZKPCache;
use tos_common::transaction::{
    FeeType, Reference, Transaction, TransactionType, TransferPayload, TxVersion,
};
use tos_daemon::core::config::RocksDBConfig;
use tos_daemon::core::mempool::Mempool;
use tos_daemon::core::storage::{RocksStorage, TransactionProvider};
use tos_daemon::core::TxCache;

fn make_dummy_transfer() -> (Transaction, Hash) {
    let sender = KeyPair::new();
    let recipient = KeyPair::new();
    let payload = TransferPayload::new(TOS_ASSET, recipient.get_public_key().compress(), 1, None);
    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };
    let signature = Signature::from_bytes(&[0u8; 64]).expect("signature");
    let tx = Transaction::new(
        TxVersion::T1,
        0,
        sender.get_public_key().compress(),
        TransactionType::Transfers(vec![payload]),
        1000,
        FeeType::TOS,
        0,
        reference,
        None,
        signature,
    );
    let tx_hash = tx.hash();
    (tx, tx_hash)
}

#[tokio::test]
async fn test_tx_cache_storage_presence_and_eviction() {
    let temp_dir = TempDir::new("tck_tx_cache").expect("tempdir");
    let mut storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy(),
        Network::Devnet,
        &RocksDBConfig::for_tests(),
    );

    let mempool = Mempool::new(Network::Devnet, false);
    let (tx, tx_hash) = make_dummy_transfer();

    storage
        .add_transaction(&tx_hash, &tx)
        .await
        .expect("store tx");

    let cache = TxCache::new(&storage, &mempool, false);
    assert!(cache
        .is_already_verified(&tx_hash)
        .await
        .expect("cache lookup"));

    storage
        .delete_transaction(&tx_hash)
        .await
        .expect("delete tx");

    let cache_after = TxCache::new(&storage, &mempool, false);
    assert!(!cache_after
        .is_already_verified(&tx_hash)
        .await
        .expect("cache miss after delete"));

    let disabled_cache = TxCache::new(&storage, &mempool, true);
    assert!(!disabled_cache
        .is_already_verified(&tx_hash)
        .await
        .expect("disabled cache always false"));
}
