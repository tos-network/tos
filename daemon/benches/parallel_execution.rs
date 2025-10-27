// TOS Parallel Transaction Execution Performance Benchmarks
// Phase 5: Performance Benchmarking Suite
//
// Benchmarks for parallel transaction execution infrastructure including:
// - ParallelExecutor batch processing with varying batch sizes
// - ParallelChainState creation overhead
// - State merging overhead
// - Conflict detection performance
// - Transaction account extraction
//
// NOTE: These are infrastructure benchmarks measuring overhead and scalability.
// They do NOT require real signed transactions or full blockchain state.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::runtime::Runtime;
use tempdir::TempDir;

use tos_common::{
    block::BlockVersion,
    config::TOS_ASSET,
    crypto::{Hash, PublicKey, elgamal::KeyPair},
    network::Network,
    transaction::{
        Transaction, TransactionType,
        builder::{TransactionBuilder, TransactionTypeBuilder, TransferBuilder, FeeBuilder, AccountState},
        FeeType, TxVersion, Reference,
    },
};
use tos_daemon::core::{
    storage::sled::{SledStorage, StorageMode},
    state::parallel_chain_state::ParallelChainState,
    executor::{ParallelExecutor, get_optimal_parallelism},
};
use tos_environment::Environment;

// ============================================================================
// Helper Types for Transaction Building
// ============================================================================

/// Minimal account state for building benchmark transactions
struct BenchAccountState {
    balance: u64,
    nonce: u64,
    is_mainnet: bool,
}

impl BenchAccountState {
    fn new(balance: u64, nonce: u64) -> Self {
        Self {
            balance,
            nonce,
            is_mainnet: false,
        }
    }
}

impl tos_common::transaction::builder::FeeHelper for BenchAccountState {
    type Error = String;

    fn account_exists(&self, _key: &tos_common::crypto::elgamal::CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

impl AccountState for BenchAccountState {

    fn is_mainnet(&self) -> bool {
        self.is_mainnet
    }

    fn get_account_balance(&self, _asset: &Hash) -> Result<u64, Self::Error> {
        Ok(self.balance)
    }

    fn get_reference(&self) -> Reference {
        Reference {
            topoheight: 0,
            hash: Hash::zero(),
        }
    }

    fn update_account_balance(&mut self, _asset: &Hash, new_balance: u64) -> Result<(), Self::Error> {
        self.balance = new_balance;
        Ok(())
    }

    fn get_nonce(&self) -> Result<u64, Self::Error> {
        Ok(self.nonce)
    }

    fn update_nonce(&mut self, new_nonce: u64) -> Result<(), Self::Error> {
        self.nonce = new_nonce;
        Ok(())
    }

    fn is_account_registered(&self, _key: &tos_common::crypto::elgamal::CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

// ============================================================================
// Transaction Generation Helpers
// ============================================================================

/// Generate a batch of valid transfer transactions for benchmarking
/// Each transaction transfers from sender to receiver
fn generate_transfer_transactions(count: usize) -> Vec<Transaction> {
    let sender_keypair = KeyPair::new();
    let receiver_keypair = KeyPair::new();

    let mut transactions = Vec::with_capacity(count);

    for nonce in 0..count {
        let mut state = BenchAccountState::new(1_000_000_000, nonce as u64);

        let transfer = TransferBuilder {
            asset: TOS_ASSET,
            amount: 1000,
            destination: receiver_keypair.get_public_key().compress().to_address(false),
            extra_data: None,
        };

        let tx = TransactionBuilder::new(
            TxVersion::T0,
            sender_keypair.get_public_key().compress(),
            None,
            TransactionTypeBuilder::Transfers(vec![transfer]),
            FeeBuilder::Value(100),
        )
        .with_fee_type(FeeType::TOS)
        .build(&mut state, &sender_keypair)
        .expect("build transaction");

        transactions.push(tx);
    }

    transactions
}

/// Generate transactions with intentional conflicts (same sender)
fn generate_conflicting_transactions(count: usize) -> Vec<Transaction> {
    let sender_keypair = KeyPair::new();
    let receiver_keypair = KeyPair::new();

    let mut transactions = Vec::with_capacity(count);

    for nonce in 0..count {
        let mut state = BenchAccountState::new(1_000_000_000, nonce as u64);

        let transfer = TransferBuilder {
            asset: TOS_ASSET,
            amount: 1000,
            destination: receiver_keypair.get_public_key().compress().to_address(false),
            extra_data: None,
        };

        let tx = TransactionBuilder::new(
            TxVersion::T0,
            sender_keypair.get_public_key().compress(),
            None,
            TransactionTypeBuilder::Transfers(vec![transfer]),
            FeeBuilder::Value(100),
        )
        .with_fee_type(FeeType::TOS)
        .build(&mut state, &sender_keypair)
        .expect("build transaction");

        transactions.push(tx);
    }

    transactions
}

/// Generate conflict-free transactions (different senders)
fn generate_conflict_free_transactions(count: usize) -> Vec<Transaction> {
    let receiver_keypair = KeyPair::new();

    let mut transactions = Vec::with_capacity(count);

    for _ in 0..count {
        let sender_keypair = KeyPair::new(); // Different sender each time
        let mut state = BenchAccountState::new(1_000_000_000, 0);

        let transfer = TransferBuilder {
            asset: TOS_ASSET,
            amount: 1000,
            destination: receiver_keypair.get_public_key().compress().to_address(false),
            extra_data: None,
        };

        let tx = TransactionBuilder::new(
            TxVersion::T0,
            sender_keypair.get_public_key().compress(),
            None,
            TransactionTypeBuilder::Transfers(vec![transfer]),
            FeeBuilder::Value(100),
        )
        .with_fee_type(FeeType::TOS)
        .build(&mut state, &sender_keypair)
        .expect("build transaction");

        transactions.push(tx);
    }

    transactions
}

// ============================================================================
// Benchmark 1: ParallelChainState Creation Overhead
// ============================================================================

fn bench_parallel_state_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_state_creation");

    let runtime = Runtime::new().unwrap();

    group.bench_function("create_parallel_chain_state", |b| {
        b.iter(|| {
            runtime.block_on(async {
                // Create temporary storage
                let temp_dir = TempDir::new("tos-bench-state").expect("temp dir");
                let storage = SledStorage::new(
                    temp_dir.path().to_string_lossy().to_string(),
                    Some(1024 * 1024),
                    Network::Devnet,
                    1024 * 1024,
                    StorageMode::HighThroughput,
                ).expect("storage");

                let storage_arc = Arc::new(RwLock::new(storage));
                let environment = Arc::new(Environment::new());

                // Measure state creation time
                let _state = ParallelChainState::new(
                    storage_arc,
                    environment,
                    0, // stable_topoheight
                    1, // topoheight
                    BlockVersion::V0,
                ).await;
            })
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 2: ParallelExecutor with Different Batch Sizes
// ============================================================================

fn bench_parallel_executor_batch_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_executor_batch_sizes");
    group.sample_size(10); // Reduce sample size for expensive benchmarks

    let runtime = Runtime::new().unwrap();

    // Test with different batch sizes: 10, 20, 50, 100
    for batch_size in [10, 20, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_txs", batch_size)),
            batch_size,
            |b, &size| {
                b.iter(|| {
                    runtime.block_on(async {
                        // Create temporary storage
                        let temp_dir = TempDir::new("tos-bench-executor").expect("temp dir");
                        let storage = SledStorage::new(
                            temp_dir.path().to_string_lossy().to_string(),
                            Some(1024 * 1024),
                            Network::Devnet,
                            1024 * 1024,
                            StorageMode::HighThroughput,
                        ).expect("storage");

                        let storage_arc = Arc::new(RwLock::new(storage));
                        let environment = Arc::new(Environment::new());

                        let state = ParallelChainState::new(
                            storage_arc,
                            environment,
                            0,
                            1,
                            BlockVersion::V0,
                        ).await;

                        let executor = ParallelExecutor::new();
                        let transactions = generate_transfer_transactions(size);

                        // Measure batch execution time
                        let _results = executor.execute_batch(state, transactions).await;
                    })
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark 3: Conflict Detection Performance
// ============================================================================

fn bench_conflict_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("conflict_detection");

    let _runtime = Runtime::new().unwrap();

    // Benchmark with conflicting transactions (same sender)
    for tx_count in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("conflicting_{}_txs", tx_count)),
            tx_count,
            |b, &count| {
                let transactions = generate_conflicting_transactions(count);
                let _executor = ParallelExecutor::new();

                b.iter(|| {
                    // Extract accounts to trigger conflict detection logic
                    let mut all_accounts = Vec::new();
                    for tx in &transactions {
                        let accounts = extract_transaction_accounts(tx);
                        all_accounts.extend(accounts);
                    }
                    all_accounts
                });
            },
        );
    }

    // Benchmark with conflict-free transactions (different senders)
    for tx_count in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("conflict_free_{}_txs", tx_count)),
            tx_count,
            |b, &count| {
                let transactions = generate_conflict_free_transactions(count);
                let _executor = ParallelExecutor::new();

                b.iter(|| {
                    // Extract accounts to trigger conflict detection logic
                    let mut all_accounts = Vec::new();
                    for tx in &transactions {
                        let accounts = extract_transaction_accounts(tx);
                        all_accounts.extend(accounts);
                    }
                    all_accounts
                });
            },
        );
    }

    group.finish();
}

// Helper function to extract accounts from transaction
fn extract_transaction_accounts(tx: &Transaction) -> Vec<PublicKey> {
    let mut accounts = vec![tx.get_source().clone()];

    match tx.get_data() {
        TransactionType::Transfers(transfers) => {
            for transfer in transfers {
                accounts.push(transfer.get_destination().clone());
            }
        }
        _ => {}
    }

    accounts
}

// ============================================================================
// Benchmark 4: Account Extraction Performance
// ============================================================================

fn bench_account_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("account_extraction");

    let _runtime = Runtime::new().unwrap();

    for tx_count in [10, 50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_txs", tx_count)),
            tx_count,
            |b, &count| {
                let transactions = generate_transfer_transactions(count);

                b.iter(|| {
                    let mut total_accounts = 0;
                    for tx in &transactions {
                        let accounts = extract_transaction_accounts(tx);
                        total_accounts += accounts.len();
                    }
                    total_accounts
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark 5: Executor Parallelism Scalability
// ============================================================================

fn bench_executor_parallelism(c: &mut Criterion) {
    let mut group = c.benchmark_group("executor_parallelism");
    group.sample_size(10);

    let runtime = Runtime::new().unwrap();
    let optimal_parallelism = get_optimal_parallelism();

    // Test with different parallelism levels: 1, 2, 4, optimal
    let parallelism_levels = vec![1, 2, 4, optimal_parallelism];

    for parallelism in parallelism_levels {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("parallelism_{}", parallelism)),
            &parallelism,
            |b, &parallelism| {
                b.iter(|| {
                    runtime.block_on(async {
                        let temp_dir = TempDir::new("tos-bench-parallelism").expect("temp dir");
                        let storage = SledStorage::new(
                            temp_dir.path().to_string_lossy().to_string(),
                            Some(1024 * 1024),
                            Network::Devnet,
                            1024 * 1024,
                            StorageMode::HighThroughput,
                        ).expect("storage");

                        let storage_arc = Arc::new(RwLock::new(storage));
                        let environment = Arc::new(Environment::new());

                        let state = ParallelChainState::new(
                            storage_arc,
                            environment,
                            0,
                            1,
                            BlockVersion::V0,
                        ).await;

                        let executor = ParallelExecutor::with_parallelism(parallelism);
                        let transactions = generate_conflict_free_transactions(50);

                        let _results = executor.execute_batch(state, transactions).await;
                    })
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark 6: State Commit Overhead
// ============================================================================

fn bench_state_commit(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_commit");
    group.sample_size(10);

    let runtime = Runtime::new().unwrap();

    for tx_count in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_txs", tx_count)),
            tx_count,
            |b, &count| {
                b.iter(|| {
                    runtime.block_on(async {
                        let temp_dir = TempDir::new("tos-bench-commit").expect("temp dir");
                        let storage = SledStorage::new(
                            temp_dir.path().to_string_lossy().to_string(),
                            Some(1024 * 1024),
                            Network::Devnet,
                            1024 * 1024,
                            StorageMode::HighThroughput,
                        ).expect("storage");

                        let storage_arc = Arc::new(RwLock::new(storage));
                        let environment = Arc::new(Environment::new());

                        let state = ParallelChainState::new(
                            Arc::clone(&storage_arc),
                            environment,
                            0,
                            1,
                            BlockVersion::V0,
                        ).await;

                        let executor = ParallelExecutor::new();
                        let transactions = generate_conflict_free_transactions(count);

                        // Execute transactions
                        let _results = executor.execute_batch(Arc::clone(&state), transactions).await;

                        // Measure commit time
                        let mut storage_lock = storage_arc.write().await;
                        let _commit_result = state.commit(&mut *storage_lock).await;
                    })
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark 7: Memory Overhead
// ============================================================================

fn bench_memory_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_overhead");

    let runtime = Runtime::new().unwrap();

    group.bench_function("state_memory_footprint", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let temp_dir = TempDir::new("tos-bench-memory").expect("temp dir");
                let storage = SledStorage::new(
                    temp_dir.path().to_string_lossy().to_string(),
                    Some(1024 * 1024),
                    Network::Devnet,
                    1024 * 1024,
                    StorageMode::HighThroughput,
                ).expect("storage");

                let storage_arc = Arc::new(RwLock::new(storage));
                let environment = Arc::new(Environment::new());

                // Create multiple states to measure overhead
                let _state1 = ParallelChainState::new(
                    Arc::clone(&storage_arc),
                    Arc::clone(&environment),
                    0,
                    1,
                    BlockVersion::V0,
                ).await;

                let _state2 = ParallelChainState::new(
                    Arc::clone(&storage_arc),
                    Arc::clone(&environment),
                    1,
                    2,
                    BlockVersion::V0,
                ).await;

                let _state3 = ParallelChainState::new(
                    storage_arc,
                    environment,
                    2,
                    3,
                    BlockVersion::V0,
                ).await;
            })
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    parallel_execution_benches,
    bench_parallel_state_creation,
    bench_parallel_executor_batch_sizes,
    bench_conflict_detection,
    bench_account_extraction,
    bench_executor_parallelism,
    bench_state_commit,
    bench_memory_overhead,
);

criterion_main!(parallel_execution_benches);
