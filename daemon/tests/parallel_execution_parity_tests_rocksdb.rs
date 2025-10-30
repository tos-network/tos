//! Integration tests for parallel execution parity using RocksDB
//!
//! These tests are migrated from parallel_execution_parity_tests.rs to use RocksDB
//! instead of SledStorage, eliminating deadlock issues.
//!
//! Tests verify that parallel execution produces identical results to sequential
//! execution for common transfer scenarios.

use std::collections::HashMap;
use std::sync::Arc;

use tempdir::TempDir;
use tokio::sync::RwLock;

use tos_common::{
    account::{VersionedBalance, VersionedNonce},
    asset::{AssetData, VersionedAssetData},
    block::{Block, BlockHeader, BlockVersion, TopoHeight, EXTRA_NONCE_SIZE},
    config::{COIN_DECIMALS, COIN_VALUE, TOS_ASSET},
    crypto::{elgamal::CompressedPublicKey, Hash, Hashable, KeyPair, PublicKey},
    immutable::Immutable,
    network::Network,
    serializer::{Reader, Serializer, Writer},
    transaction::TxVersion,
    transaction::{
        builder::{
            AccountState as AccountStateTrait, FeeBuilder, FeeHelper, TransactionBuilder,
            TransactionTypeBuilder, TransferBuilder,
        },
        FeeType, Reference, Transaction,
    },
    versioned_type::Versioned,
};

use tos_daemon::core::{
    config::RocksDBConfig,
    executor::{get_optimal_parallelism, ParallelExecutor},
    state::{parallel_chain_state::ParallelChainState, ApplicableChainState},
    storage::{
        rocksdb::RocksStorage,
        AccountProvider, AssetProvider, BalanceProvider, NonceProvider,
    },
};

use tos_environment::Environment;

/// Mock account state for transaction building
struct MockAccountState {
    balances: HashMap<Hash, u64>,
    nonce: u64,
}

impl MockAccountState {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
            nonce: 0,
        }
    }

    fn with_balance(mut self, asset: Hash, amount: u64) -> Self {
        self.balances.insert(asset, amount);
        self
    }

    fn with_nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }
}

impl AccountStateTrait for MockAccountState {
    fn is_mainnet(&self) -> bool {
        false
    }

    fn get_account_balance(&self, asset: &Hash) -> Result<u64, Self::Error> {
        Ok(*self.balances.get(asset).unwrap_or(&(1000 * COIN_VALUE)))
    }

    fn get_reference(&self) -> Reference {
        Reference {
            topoheight: 0,
            hash: Hash::zero(),
        }
    }

    fn update_account_balance(
        &mut self,
        asset: &Hash,
        new_balance: u64,
    ) -> Result<(), Self::Error> {
        self.balances.insert(asset.clone(), new_balance);
        Ok(())
    }

    fn get_nonce(&self) -> Result<u64, Self::Error> {
        Ok(self.nonce)
    }

    fn update_nonce(&mut self, new_nonce: u64) -> Result<(), Self::Error> {
        self.nonce = new_nonce;
        Ok(())
    }

    fn is_account_registered(&self, _key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

impl FeeHelper for MockAccountState {
    type Error = Box<dyn std::error::Error>;

    fn account_exists(&self, _key: &CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Register TOS asset in storage
async fn register_tos_asset(storage: &mut RocksStorage) {
    let asset_data = AssetData::new(
        COIN_DECIMALS,
        "TOS".to_string(),
        "TOS".to_string(),
        None,
        None,
    );
    let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
    storage.add_asset(&TOS_ASSET, 0, versioned).await.unwrap();
}

/// Create a dummy block for testing
fn create_dummy_block() -> (Block, Hash) {
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&[0u8; 32]);
    let data = writer.as_bytes();

    let mut reader = Reader::new(data);
    let miner = CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey");

    let header = BlockHeader::new_simple(
        BlockVersion::V0,
        vec![],
        0,
        [0u8; EXTRA_NONCE_SIZE],
        miner,
        Hash::zero(),
    );

    let block = Block::new(Immutable::Owned(header), vec![]);
    let hash = block.hash();
    (block, hash)
}

/// Create a signed transfer transaction
fn create_transfer_transaction(
    sender: &KeyPair,
    receiver: &CompressedPublicKey,
    amount: u64,
    fee: u64,
    nonce: u64,
) -> Transaction {
    let transfer = TransferBuilder {
        destination: receiver.to_address(false),
        amount,
        asset: TOS_ASSET,
        extra_data: None,
    };

    let tx_type = TransactionTypeBuilder::Transfers(vec![transfer]);
    let fee_builder = FeeBuilder::Value(fee);

    let builder = TransactionBuilder::new(
        TxVersion::T0,
        sender.get_public_key().compress(),
        None,
        tx_type,
        fee_builder,
    )
    .with_fee_type(FeeType::TOS);

    let mut state = MockAccountState::new()
        .with_balance(TOS_ASSET, 1_000 * COIN_VALUE)
        .with_nonce(nonce);

    builder.build(&mut state, sender).unwrap()
}

/// Setup account in RocksDB storage (NO deadlock risk, NO delays needed!)
async fn setup_account(
    storage: &Arc<RwLock<RocksStorage>>,
    pubkey: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
) {
    let mut guard = storage.write().await;
    guard
        .set_last_nonce_to(pubkey, 0, &VersionedNonce::new(nonce, Some(0)))
        .await
        .unwrap();
    guard
        .set_account_registration_topoheight(pubkey, 0)
        .await
        .unwrap();
    guard
        .set_last_balance_to(
            pubkey,
            &TOS_ASSET,
            0,
            &VersionedBalance::new(balance, Some(0)),
        )
        .await
        .unwrap();
    // RocksDB handles concurrency correctly - no delays needed!
}

/// Snapshot balances and nonces from storage
async fn snapshot_accounts(
    storage: &Arc<RwLock<RocksStorage>>,
    accounts: &[(&'static str, &CompressedPublicKey)],
) -> HashMap<&'static str, (u64, u64)> {
    let guard = storage.read().await;
    let mut result = HashMap::new();

    for (label, key) in accounts {
        let nonce = guard
            .get_nonce_at_maximum_topoheight(key, TopoHeight::MAX)
            .await
            .unwrap()
            .map(|(_, versioned)| versioned.get_nonce())
            .unwrap_or(0);

        let balance = guard
            .get_balance_at_maximum_topoheight(key, &TOS_ASSET, TopoHeight::MAX)
            .await
            .unwrap()
            .map(|(_, versioned)| versioned.get_balance())
            .unwrap_or(0);

        result.insert(*label, (balance, nonce));
    }

    result
}

/// Execute transactions sequentially
async fn execute_sequential(
    storage: &Arc<RwLock<RocksStorage>>,
    environment: &Environment,
    block: &Block,
    block_hash: &Hash,
    topoheight: TopoHeight,
    transactions: &[Arc<Transaction>],
) {
    let mut guard = storage.write().await;
    let mut chain_state = ApplicableChainState::new(
        &mut *guard,
        environment,
        topoheight.saturating_sub(1),
        topoheight,
        BlockVersion::V0,
        0,
        block_hash,
        block,
    );

    let txs_with_hash: Vec<(Arc<Transaction>, Hash)> = transactions
        .iter()
        .map(|tx| (Arc::clone(tx), tx.hash()))
        .collect();

    for (tx, tx_hash) in txs_with_hash.iter() {
        tx.apply_with_partial_verify(tx_hash, &mut chain_state)
            .await
            .unwrap();
    }

    chain_state.apply_changes().await.unwrap();
}

/// Execute transactions in parallel
async fn execute_parallel(
    storage: &Arc<RwLock<RocksStorage>>,
    environment: Arc<Environment>,
    block: Block,
    block_hash: Hash,
    topoheight: TopoHeight,
    transactions: &[Arc<Transaction>],
) {
    let parallel_state = ParallelChainState::new(
        Arc::clone(storage),
        environment,
        topoheight.saturating_sub(1),
        topoheight,
        BlockVersion::V0,
        block,
        block_hash,
    )
    .await;

    let executor = ParallelExecutor::with_parallelism(get_optimal_parallelism());
    let tx_clones: Vec<Transaction> = transactions.iter().map(|tx| (**tx).clone()).collect();
    let results = executor
        .execute_batch(Arc::clone(&parallel_state), tx_clones)
        .await;

    for result in &results {
        assert!(
            result.success,
            "Parallel execution failed: {:?}",
            result.error
        );
    }

    let mut guard = storage.write().await;
    parallel_state.commit(&mut *guard).await.unwrap();
}

/// Prepare test environment with independent RocksDB storages
async fn prepare_test_environment() -> (
    Arc<RwLock<RocksStorage>>,
    Arc<RwLock<RocksStorage>>,
    Arc<Environment>,
) {
    let create_storage = || {
        let temp_dir = TempDir::new("tos_parallel_parity").unwrap();
        let dir_path = temp_dir.path().to_string_lossy().to_string();
        let config = RocksDBConfig::default();

        let storage = RocksStorage::new(
            &dir_path,
            Network::Devnet,
            Some(1024 * 1024),
            &config,
        );

        // Keep temp_dir alive
        std::mem::forget(temp_dir);
        storage
    };

    let storage_seq = Arc::new(RwLock::new(create_storage()));
    let storage_par = Arc::new(RwLock::new(create_storage()));

    {
        let mut guard = storage_seq.write().await;
        register_tos_asset(&mut *guard).await;
    }
    {
        let mut guard = storage_par.write().await;
        register_tos_asset(&mut *guard).await;
    }

    (storage_seq, storage_par, Arc::new(Environment::new()))
}

// ============================================================================
// TESTS (Previously #[ignore] with SledStorage, now working with RocksDB!)
// ============================================================================

#[tokio::test]
async fn test_parallel_matches_sequential_receive_then_spend() {
    // Accounts: Alice sends to Bob, Bob immediately spends to Charlie.
    let (storage_seq, storage_par, environment) = prepare_test_environment().await;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let charlie = KeyPair::new();

    // Setup accounts in both storages
    setup_account(
        &storage_seq,
        &alice.get_public_key().compress(),
        100 * COIN_VALUE,
        0,
    )
    .await;
    setup_account(
        &storage_seq,
        &bob.get_public_key().compress(),
        50 * COIN_VALUE,
        0,
    )
    .await;
    setup_account(&storage_seq, &charlie.get_public_key().compress(), 0, 0).await;

    setup_account(
        &storage_par,
        &alice.get_public_key().compress(),
        100 * COIN_VALUE,
        0,
    )
    .await;
    setup_account(
        &storage_par,
        &bob.get_public_key().compress(),
        50 * COIN_VALUE,
        0,
    )
    .await;
    setup_account(&storage_par, &charlie.get_public_key().compress(), 0, 0).await;

    // NO flush needed with RocksDB!

    // Create transactions
    let tx1 = Arc::new(create_transfer_transaction(
        &alice,
        &bob.get_public_key().compress(),
        30 * COIN_VALUE,
        10,
        0,
    ));

    let tx2 = Arc::new(create_transfer_transaction(
        &bob,
        &charlie.get_public_key().compress(),
        20 * COIN_VALUE,
        5,
        0,
    ));

    let transactions = vec![tx1.clone(), tx2.clone()];

    // Execute sequentially
    let (block, block_hash) = create_dummy_block();
    execute_sequential(
        &storage_seq,
        &environment,
        &block,
        &block_hash,
        1,
        &transactions,
    )
    .await;

    // Execute in parallel
    let (block_parallel, hash_parallel) = create_dummy_block();
    execute_parallel(
        &storage_par,
        Arc::clone(&environment),
        block_parallel,
        hash_parallel,
        1,
        &transactions,
    )
    .await;

    // Compare results
    let accounts = [
        ("alice", &alice.get_public_key().compress()),
        ("bob", &bob.get_public_key().compress()),
        ("charlie", &charlie.get_public_key().compress()),
    ];

    let seq_snapshot = snapshot_accounts(&storage_seq, &accounts).await;
    let par_snapshot = snapshot_accounts(&storage_par, &accounts).await;

    assert_eq!(
        seq_snapshot, par_snapshot,
        "Sequential and parallel states diverged"
    );
    assert_eq!(seq_snapshot["alice"].0, 70 * COIN_VALUE);
    assert_eq!(seq_snapshot["bob"].0, 60 * COIN_VALUE);
    assert_eq!(seq_snapshot["charlie"].0, 20 * COIN_VALUE);
}

#[tokio::test]
async fn test_parallel_matches_sequential_multiple_spends() {
    // Alice executes two outgoing transfers within the same block
    let (storage_seq, storage_par, environment) = prepare_test_environment().await;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let charlie = KeyPair::new();

    // Setup accounts
    setup_account(
        &storage_seq,
        &alice.get_public_key().compress(),
        100 * COIN_VALUE,
        0,
    )
    .await;
    setup_account(&storage_seq, &bob.get_public_key().compress(), 0, 0).await;
    setup_account(&storage_seq, &charlie.get_public_key().compress(), 0, 0).await;

    setup_account(
        &storage_par,
        &alice.get_public_key().compress(),
        100 * COIN_VALUE,
        0,
    )
    .await;
    setup_account(&storage_par, &bob.get_public_key().compress(), 0, 0).await;
    setup_account(&storage_par, &charlie.get_public_key().compress(), 0, 0).await;

    // Create transactions
    let tx1 = Arc::new(create_transfer_transaction(
        &alice,
        &bob.get_public_key().compress(),
        20 * COIN_VALUE,
        10,
        0,
    ));

    let tx2 = Arc::new(create_transfer_transaction(
        &alice,
        &charlie.get_public_key().compress(),
        30 * COIN_VALUE,
        10,
        1,
    ));

    let transactions = vec![tx1.clone(), tx2.clone()];

    // Execute sequentially
    let (block, block_hash) = create_dummy_block();
    execute_sequential(
        &storage_seq,
        &environment,
        &block,
        &block_hash,
        1,
        &transactions,
    )
    .await;

    // Execute in parallel
    let (block_parallel, hash_parallel) = create_dummy_block();
    execute_parallel(
        &storage_par,
        Arc::clone(&environment),
        block_parallel,
        hash_parallel,
        1,
        &transactions,
    )
    .await;

    // Compare results
    let accounts = [
        ("alice", &alice.get_public_key().compress()),
        ("bob", &bob.get_public_key().compress()),
        ("charlie", &charlie.get_public_key().compress()),
    ];

    let seq_snapshot = snapshot_accounts(&storage_seq, &accounts).await;
    let par_snapshot = snapshot_accounts(&storage_par, &accounts).await;

    assert_eq!(
        seq_snapshot, par_snapshot,
        "Sequential and parallel states diverged"
    );
    assert_eq!(seq_snapshot["alice"].0, 40 * COIN_VALUE);
    assert_eq!(seq_snapshot["bob"].0, 20 * COIN_VALUE);
    assert_eq!(seq_snapshot["charlie"].0, 30 * COIN_VALUE);
}
