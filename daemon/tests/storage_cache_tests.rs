//! Storage cache tests based on STORAGE-CACHE-IMPLEMENTATION.md (Section 20.3-20.4)

#![allow(clippy::disallowed_methods)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use indexmap::IndexSet;
use tokio::sync::Barrier;
use tokio::time::{timeout, Duration};
use tos_common::{
    block::{BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    crypto::{elgamal::KeyPair, Hash, Hashable},
    immutable::Immutable,
    network::Network,
    transaction::{FeeType, Reference, Transaction, TransactionType, TransferPayload, TxVersion},
    varuint::VarUint,
};
use tos_daemon::core::{
    error::BlockchainError,
    storage::{
        BlockDagProvider, BlockExecutionOrderProvider, BlockProvider, DagOrderProvider,
        RocksStorage, SnapshotProvider, StateProvider, Storage, Tips, TipsProvider,
        TransactionProvider,
    },
};
use tos_tck::utilities::create_test_rocksdb_storage;

fn create_transfer_transaction() -> Transaction {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let payload = TransferPayload::new(
        tos_common::config::TOS_ASSET,
        receiver.get_public_key().compress(),
        1,
        None,
    );
    let data = TransactionType::Transfers(vec![payload]);
    let signature = sender.sign(b"storage_cache_test");

    Transaction::new(
        TxVersion::T1,
        Network::Devnet.chain_id() as u8,
        sender.get_public_key().compress(),
        data,
        0,
        FeeType::TOS,
        0,
        Reference {
            hash: Hash::zero(),
            topoheight: 0,
        },
        None,
        signature,
    )
}

fn create_block_header(height: u64, txs: &[Arc<Transaction>]) -> (Arc<BlockHeader>, Hash) {
    let mut tx_hashes = IndexSet::new();
    for tx in txs {
        tx_hashes.insert(tx.hash());
    }

    let miner = KeyPair::new().get_public_key().compress();
    let header = BlockHeader::new(
        BlockVersion::Nobunaga,
        height,
        0,
        IndexSet::new(),
        [0u8; EXTRA_NONCE_SIZE],
        miner,
        tx_hashes,
    );
    let hash = header.hash();
    (Arc::new(header), hash)
}

async fn setup_single_block_chain(storage: &mut RocksStorage) -> Result<Hash, BlockchainError> {
    let (header, hash) = create_block_header(1, &[]);
    let difficulty = VarUint::from(1u64);
    let cumulative = VarUint::from(1u64);

    storage
        .save_block(
            header,
            &[],
            difficulty,
            cumulative,
            VarUint::from(0u64),
            Immutable::Owned(hash.clone()),
        )
        .await
        .expect("save_block should succeed for test setup");

    storage
        .add_block_execution_to_order(&hash)
        .await
        .expect("block execution order insert should succeed");

    storage
        .set_topo_height_for_block(&hash, 1)
        .await
        .expect("set_topo_height_for_block should succeed");

    storage
        .set_topoheight_metadata(1, 0, 0, 0)
        .expect("set_topoheight_metadata should succeed");

    storage
        .set_top_topoheight(1)
        .await
        .expect("set_top_topoheight should succeed");
    storage
        .set_top_height(1)
        .await
        .expect("set_top_height should succeed");

    let mut tips = Tips::new();
    tips.insert(hash.clone());
    storage
        .store_tips(&tips)
        .await
        .expect("store_tips should succeed");

    Ok(hash)
}

#[tokio::test]
async fn test_cache_write_read_consistency() {
    let storage = create_test_rocksdb_storage().await;
    let tx = create_transfer_transaction();
    let tx_hash = tx.hash();

    {
        let mut storage = storage.write().await;
        storage
            .add_transaction(&tx_hash, &tx)
            .await
            .expect("write transaction should succeed");
    }

    let storage = storage.read().await;
    let loaded = storage
        .get_transaction(&tx_hash)
        .await
        .expect("read transaction should succeed");
    assert_eq!(
        loaded.hash(),
        tx_hash,
        "read transaction hash should match written transaction"
    );
}

#[tokio::test]
async fn test_counter_increment_on_write() {
    let storage = create_test_rocksdb_storage().await;
    let tx = Arc::new(create_transfer_transaction());
    let (header, hash) = create_block_header(1, std::slice::from_ref(&tx));

    let mut storage = storage.write().await;
    storage
        .save_block(
            header,
            &[tx],
            VarUint::from(1u64),
            VarUint::from(1u64),
            VarUint::from(0u64),
            Immutable::Owned(hash),
        )
        .await
        .expect("save_block should succeed");

    let blocks = storage
        .count_blocks()
        .await
        .expect("count_blocks should succeed");
    let txs = storage
        .count_transactions()
        .await
        .expect("count_transactions should succeed");
    assert_eq!(blocks, 1, "blocks counter should increment on write");
    assert_eq!(txs, 1, "transactions counter should increment on write");
}

#[tokio::test]
async fn test_cache_delete_consistency() {
    let storage = create_test_rocksdb_storage().await;
    let tx = create_transfer_transaction();
    let tx_hash = tx.hash();

    let mut storage = storage.write().await;
    storage
        .add_transaction(&tx_hash, &tx)
        .await
        .expect("add_transaction should succeed");

    let objects = storage
        .cache()
        .objects
        .as_ref()
        .expect("objects cache should be enabled");
    assert!(
        objects
            .transactions_cache
            .lock()
            .await
            .get(&tx_hash)
            .is_some(),
        "transactions cache should contain entry after add"
    );

    storage
        .delete_transaction(&tx_hash)
        .await
        .expect("delete_transaction should succeed");

    let objects = storage
        .cache()
        .objects
        .as_ref()
        .expect("objects cache should be enabled");
    assert!(
        objects
            .transactions_cache
            .lock()
            .await
            .get(&tx_hash)
            .is_none(),
        "transactions cache should be cleared after delete"
    );
}

#[tokio::test]
async fn test_snapshot_rollback_cache_consistency() {
    let storage = create_test_rocksdb_storage().await;
    let tx = create_transfer_transaction();
    let tx_hash = tx.hash();

    {
        let mut storage = storage.write().await;
        storage
            .start_snapshot()
            .await
            .expect("start_snapshot should succeed");
        storage
            .add_transaction(&tx_hash, &tx)
            .await
            .expect("add_transaction should succeed in snapshot");
        assert!(
            storage
                .has_transaction(&tx_hash)
                .await
                .expect("has_transaction should succeed in snapshot"),
            "snapshot should expose newly written transaction"
        );
        storage
            .end_snapshot(false)
            .expect("end_snapshot(false) should succeed");
    }

    let storage = storage.read().await;
    assert!(
        !storage
            .has_transaction(&tx_hash)
            .await
            .expect("has_transaction should succeed after rollback"),
        "rollback should discard snapshot data"
    );
    let objects = storage
        .cache()
        .objects
        .as_ref()
        .expect("objects cache should be enabled");
    assert!(
        objects
            .transactions_cache
            .lock()
            .await
            .get(&tx_hash)
            .is_none(),
        "rollback should not pollute main cache"
    );
}

#[tokio::test]
async fn test_snapshot_apply_cache_consistency() {
    let storage = create_test_rocksdb_storage().await;
    let tx = create_transfer_transaction();
    let tx_hash = tx.hash();

    {
        let mut storage = storage.write().await;
        storage
            .start_snapshot()
            .await
            .expect("start_snapshot should succeed");
        storage
            .add_transaction(&tx_hash, &tx)
            .await
            .expect("add_transaction should succeed in snapshot");
        storage
            .end_snapshot(true)
            .expect("end_snapshot(true) should succeed");
    }

    let storage = storage.read().await;
    assert!(
        storage
            .has_transaction(&tx_hash)
            .await
            .expect("has_transaction should succeed after apply"),
        "apply should persist snapshot data"
    );
    let objects = storage
        .cache()
        .objects
        .as_ref()
        .expect("objects cache should be enabled");
    assert!(
        objects
            .transactions_cache
            .lock()
            .await
            .get(&tx_hash)
            .is_some(),
        "apply should keep cache in sync with persisted data"
    );
}

#[tokio::test]
async fn test_concurrent_reads() {
    let storage = create_test_rocksdb_storage().await;
    let tx = create_transfer_transaction();
    let tx_hash = tx.hash();

    {
        let mut storage = storage.write().await;
        storage
            .add_transaction(&tx_hash, &tx)
            .await
            .expect("add_transaction should succeed");
    }

    let mut handles = Vec::new();
    for _ in 0..8 {
        let storage = storage.clone();
        let tx_hash = tx_hash.clone();
        handles.push(tokio::spawn(async move {
            let storage = storage.read().await;
            let loaded = storage
                .get_transaction(&tx_hash)
                .await
                .expect("get_transaction should succeed concurrently");
            assert_eq!(
                loaded.hash(),
                tx_hash,
                "concurrent reads should return consistent transaction"
            );
        }));
    }

    for handle in handles {
        handle.await.expect("concurrent read task should succeed");
    }
}

#[tokio::test]
async fn test_concurrent_read_write() {
    let storage = create_test_rocksdb_storage().await;
    let tx = Arc::new(create_transfer_transaction());
    let tx_hash = tx.hash();
    let barrier = Arc::new(Barrier::new(3));

    let writer_storage = storage.clone();
    let writer_barrier = barrier.clone();
    let writer_tx = tx.clone();
    let writer_hash = tx_hash.clone();
    let writer = tokio::spawn(async move {
        writer_barrier.wait().await;
        let mut storage = writer_storage.write().await;
        storage
            .add_transaction(&writer_hash, &writer_tx)
            .await
            .expect("writer add_transaction should succeed");
    });

    let reader_storage = storage.clone();
    let reader_barrier = barrier.clone();
    let reader_hash = tx_hash.clone();
    let reader = tokio::spawn(async move {
        reader_barrier.wait().await;
        let storage = reader_storage.read().await;
        let _ = storage
            .has_transaction(&reader_hash)
            .await
            .expect("reader has_transaction should succeed");
    });

    barrier.wait().await;
    timeout(Duration::from_secs(3), async {
        writer.await.expect("writer task should complete");
        reader.await.expect("reader task should complete");
    })
    .await
    .expect("concurrent read/write should not deadlock");
}

#[tokio::test]
async fn test_snapshot_guard_drop_no_deadlock() {
    let storage = create_test_rocksdb_storage().await;
    let wrapper = tos_daemon::core::storage::snapshot::SnapshotWrapper::new(storage.as_ref());

    timeout(Duration::from_secs(3), async {
        let guard = wrapper.lock().await.expect("snapshot lock should succeed");
        drop(guard);
        let guard = wrapper
            .lock()
            .await
            .expect("snapshot relock should succeed");
        drop(guard);
    })
    .await
    .expect("SnapshotGuard drop should not deadlock");
}

#[tokio::test]
async fn test_lock_ordering_storage_then_cache() {
    let storage = create_test_rocksdb_storage().await;

    timeout(Duration::from_secs(3), async {
        let storage = storage.write().await;
        let objects = storage
            .cache()
            .objects
            .as_ref()
            .expect("objects cache should be enabled");
        let _cache_guard = objects.transactions_cache.lock().await;
    })
    .await
    .expect("storage then cache locking should not deadlock");
}

#[tokio::test]
async fn test_rewind_clears_dag_cache() {
    let storage = create_test_rocksdb_storage().await;
    let mut storage = storage.write().await;
    setup_single_block_chain(&mut storage)
        .await
        .expect("chain setup should succeed");

    {
        let chain = &mut storage.cache_mut().chain;
        chain
            .tip_base_cache
            .lock()
            .await
            .put((Hash::zero(), 1), (Hash::zero(), 0));
        chain
            .common_base_cache
            .lock()
            .await
            .put(Hash::zero(), (Hash::zero(), 0));
        chain.tip_work_score_cache.lock().await.put(
            (Hash::zero(), Hash::zero(), 0),
            (Tips::new(), VarUint::from(1u64)),
        );
        chain
            .full_order_cache
            .lock()
            .await
            .put((Hash::zero(), Hash::zero(), 0), IndexSet::new());
    }

    storage
        .pop_blocks(1, 1, 1, 0)
        .await
        .expect("pop_blocks should succeed");

    let chain = &storage.cache().chain;
    assert!(
        chain.tip_base_cache.lock().await.is_empty(),
        "tip_base_cache should be cleared after rewind"
    );
    assert!(
        chain.common_base_cache.lock().await.is_empty(),
        "common_base_cache should be cleared after rewind"
    );
    assert!(
        chain.tip_work_score_cache.lock().await.is_empty(),
        "tip_work_score_cache should be cleared after rewind"
    );
    assert!(
        chain.full_order_cache.lock().await.is_empty(),
        "full_order_cache should be cleared after rewind"
    );
}

#[tokio::test]
async fn test_delete_block_removes_from_cache() {
    let storage = create_test_rocksdb_storage().await;
    let (header, hash) = create_block_header(1, &[]);

    let mut storage = storage.write().await;
    storage
        .save_block(
            header,
            &[],
            VarUint::from(1u64),
            VarUint::from(1u64),
            VarUint::from(0u64),
            Immutable::Owned(hash.clone()),
        )
        .await
        .expect("save_block should succeed");

    let objects = storage
        .cache()
        .objects
        .as_ref()
        .expect("objects cache should be enabled");
    assert!(
        objects.blocks_cache.lock().await.get(&hash).is_some(),
        "blocks cache should contain block after save"
    );

    storage
        .delete_block_with_hash(&hash)
        .await
        .expect("delete_block_with_hash should succeed");

    let objects = storage
        .cache()
        .objects
        .as_ref()
        .expect("objects cache should be enabled");
    assert!(
        objects.blocks_cache.lock().await.get(&hash).is_none(),
        "blocks cache should be cleared after delete"
    );
}

#[tokio::test]
async fn test_snapshot_rollback_no_cache_pollution() {
    let storage = create_test_rocksdb_storage().await;
    let tx = create_transfer_transaction();
    let tx_hash = tx.hash();

    {
        let mut storage = storage.write().await;
        storage
            .start_snapshot()
            .await
            .expect("start_snapshot should succeed");
        let objects = storage
            .cache()
            .objects
            .as_ref()
            .expect("objects cache should be enabled");
        objects
            .transactions_cache
            .lock()
            .await
            .put(tx_hash.clone(), Arc::new(tx));
        storage
            .end_snapshot(false)
            .expect("end_snapshot(false) should succeed");
    }

    let storage = storage.read().await;
    let objects = storage
        .cache()
        .objects
        .as_ref()
        .expect("objects cache should be enabled");
    assert!(
        objects
            .transactions_cache
            .lock()
            .await
            .get(&tx_hash)
            .is_none(),
        "rollback should discard snapshot cache entries"
    );
}

#[tokio::test]
async fn test_counter_decremented_on_delete() {
    let storage = create_test_rocksdb_storage().await;
    let mut storage = storage.write().await;
    setup_single_block_chain(&mut storage)
        .await
        .expect("chain setup should succeed");

    let before = storage
        .count_blocks()
        .await
        .expect("count_blocks before rewind should succeed");
    assert_eq!(before, 1, "blocks counter should start at 1");

    storage
        .pop_blocks(1, 1, 1, 0)
        .await
        .expect("pop_blocks should succeed");

    let after = storage
        .count_blocks()
        .await
        .expect("count_blocks after rewind should succeed");
    assert_eq!(after, 0, "blocks counter should decrement after delete");
}
