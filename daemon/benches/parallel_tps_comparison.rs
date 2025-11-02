// TOS Parallel vs Sequential Transaction Execution Performance Comparison
//
// Comprehensive TPS benchmark comparing parallel and sequential execution modes.
// Measures throughput (TPS), latency, and speedup ratio for different scenarios.
//
// Metrics:
// - Total execution time (microseconds)
// - Throughput (transactions per second, calculated as u64)
// - Speedup ratio (sequential time / parallel time, using u128 scaled integers)
//
// Requirements:
// - All comments in English only
// - Use log level checks for performance-critical logging
// - NO f64 in consensus-critical paths (use u64 for TPS, u128 for ratios)
// - Follow CLAUDE.md code quality standards

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::runtime::Runtime;
use tempdir::TempDir;

use tos_common::{
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    config::TOS_ASSET,
    crypto::{Hash, Hashable, elgamal::{KeyPair, CompressedPublicKey}},
    immutable::Immutable,
    network::Network,
    serializer::{Serializer, Writer, Reader},
    transaction::{
        Transaction,
        builder::{
            TransactionBuilder, TransactionTypeBuilder, TransferBuilder,
            FeeBuilder, AccountState,
        },
        FeeType, TxVersion, Reference,
    },
};
use tos_daemon::core::{
    storage::sled::{SledStorage, StorageMode},
    state::parallel_chain_state::ParallelChainState,
    executor::ParallelExecutor,
};
use tos_environment::Environment;

// ============================================================================
// Constants for scaled integer arithmetic (CLAUDE.md compliance)
// ============================================================================

// SCALE factor for fixed-point arithmetic (avoids f64 in critical paths)
// Used for speedup ratio calculations: ratio = (sequential_time * SCALE) / parallel_time
const SCALE: u128 = 10000; // Represents 1.0

// ============================================================================
// Helper Functions for Creating Test Blocks
// ============================================================================

/// Helper function to create a test public key from bytes
fn create_test_pubkey(bytes: [u8; 32]) -> CompressedPublicKey {
    // Use serialization to create a CompressedPublicKey from bytes
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&bytes);
    let data = writer.as_bytes();

    // Create a Reader and deserialize
    let mut reader = Reader::new(data);
    CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey")
}

/// Create a minimal block for benchmarking (empty block with zero parents)
fn create_minimal_block() -> Block {
    let miner = create_test_pubkey([0u8; 32]);
    let header = BlockHeader::new_simple(
        BlockVersion::V0,
        vec![], // No parents
        0, // timestamp
        [0u8; EXTRA_NONCE_SIZE], // extra_nonce
        miner,
        Hash::zero(), // merkle root
    );
    Block::new(Immutable::Owned(header), vec![])
}

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

/// Generate conflict-free transfer transactions (different senders)
/// This creates the best-case scenario for parallel execution
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

/// Generate transactions with 50% conflict ratio
/// Half the transactions share the same sender, forcing sequential execution
fn generate_mixed_conflict_transactions(count: usize) -> Vec<Transaction> {
    let receiver_keypair = KeyPair::new();
    let shared_sender_keypair = KeyPair::new(); // Used for conflicting transactions

    let mut transactions = Vec::with_capacity(count);

    for i in 0..count {
        let use_shared_sender = i % 2 == 0; // 50% conflict ratio

        let (sender_keypair, nonce) = if use_shared_sender {
            (shared_sender_keypair.clone(), (i / 2) as u64)
        } else {
            (KeyPair::new(), 0)
        };

        let mut state = BenchAccountState::new(1_000_000_000, nonce);

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
// Sequential Execution Baseline
// ============================================================================

/// Execute transactions sequentially (baseline for comparison)
async fn execute_sequential<S: tos_daemon::core::storage::Storage>(
    state: Arc<ParallelChainState<S>>,
    transactions: Vec<Transaction>,
) -> Duration {
    let start = Instant::now();

    for tx in transactions {
        let tx_arc = Arc::new(tx);
        // Clone state because apply_transaction takes ownership
        let _result = state.clone().apply_transaction(tx_arc).await;
    }

    start.elapsed()
}

// ============================================================================
// Parallel Execution
// ============================================================================

/// Execute transactions in parallel using ParallelExecutor
async fn execute_parallel<S: tos_daemon::core::storage::Storage>(
    state: Arc<ParallelChainState<S>>,
    transactions: Vec<Transaction>,
) -> Duration {
    let start = Instant::now();

    let executor = ParallelExecutor::new();
    let _results = executor.execute_batch(state, transactions).await;

    start.elapsed()
}

// ============================================================================
// Performance Metrics Calculation (NO f64 in critical path)
// ============================================================================

/// Calculate TPS using integer arithmetic only
/// TPS = (tx_count * 1_000_000) / elapsed_micros
/// Returns TPS as u64 (no floating point)
fn calculate_tps_integer(tx_count: usize, elapsed: Duration) -> u64 {
    let tx_count_u64 = tx_count as u64;
    let elapsed_micros = elapsed.as_micros() as u64;

    if elapsed_micros == 0 {
        return 0;
    }

    // TPS = (transactions * 1_000_000 microseconds/second) / elapsed_microseconds
    (tx_count_u64 * 1_000_000) / elapsed_micros
}

/// Calculate speedup ratio using u128 scaled integer arithmetic
/// speedup = (sequential_time * SCALE) / parallel_time
/// Returns scaled ratio (e.g., 15000 means 1.5x speedup)
fn calculate_speedup_ratio(sequential_micros: u128, parallel_micros: u128) -> u128 {
    if parallel_micros == 0 {
        return 0;
    }

    (sequential_micros * SCALE) / parallel_micros
}

// SAFE: f64 for display/reporting only, not consensus-critical
#[allow(dead_code)]
fn print_performance_comparison(
    mode: &str,
    tx_count: usize,
    sequential_time: Duration,
    parallel_time: Duration,
) {
    let seq_micros = sequential_time.as_micros() as u128;
    let par_micros = parallel_time.as_micros() as u128;

    let seq_tps = calculate_tps_integer(tx_count, sequential_time);
    let par_tps = calculate_tps_integer(tx_count, parallel_time);
    let speedup = calculate_speedup_ratio(seq_micros, par_micros);

    // SAFE: f64 for display formatting only
    let speedup_display = speedup as f64 / SCALE as f64;

    println!("[{}] txs={} | Sequential: {}µs ({}tx/s) | Parallel: {}µs ({}tx/s) | Speedup: {:.2}x",
        mode, tx_count, seq_micros, seq_tps, par_micros, par_tps, speedup_display);
}

// ============================================================================
// Benchmark 1: Sequential Baseline - 10 Transactions
// ============================================================================

fn bench_sequential_10_txs(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_execution");
    group.sample_size(20);
    group.throughput(Throughput::Elements(10));

    let runtime = Runtime::new().unwrap();

    group.bench_function("10_txs", |b| {
        b.iter(|| {
            runtime.block_on(async {
                // Create temporary storage
                let temp_dir = TempDir::new("tos-bench-seq-10").expect("temp dir");
                let storage = SledStorage::new(
                    temp_dir.path().to_string_lossy().to_string(),
                    Some(1024 * 1024),
                    Network::Devnet,
                    1024 * 1024,
                    StorageMode::HighThroughput,
                ).expect("storage");

                let storage_arc = Arc::new(RwLock::new(storage));
                let environment = Arc::new(Environment::new());

                // Create a minimal block for state initialization
                let block = create_minimal_block();
                let block_hash = block.hash();

                let state = ParallelChainState::new(
                    storage_arc,
                    environment,
                    0, // stable_topoheight
                    1, // topoheight
                    BlockVersion::V0,
                    block,
                    block_hash,
                ).await;

                let transactions = generate_conflict_free_transactions(10);
                let _duration = execute_sequential(state, transactions).await;
            })
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 2: Parallel Execution - 10 Transactions
// ============================================================================

fn bench_parallel_10_txs(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_execution");
    group.sample_size(20);
    group.throughput(Throughput::Elements(10));

    let runtime = Runtime::new().unwrap();

    group.bench_function("10_txs", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let temp_dir = TempDir::new("tos-bench-par-10").expect("temp dir");
                let storage = SledStorage::new(
                    temp_dir.path().to_string_lossy().to_string(),
                    Some(1024 * 1024),
                    Network::Devnet,
                    1024 * 1024,
                    StorageMode::HighThroughput,
                ).expect("storage");

                let storage_arc = Arc::new(RwLock::new(storage));
                let environment = Arc::new(Environment::new());

                let block = create_minimal_block();
                let block_hash = block.hash();

                let state = ParallelChainState::new(
                    storage_arc,
                    environment,
                    0,
                    1,
                    BlockVersion::V0,
                    block,
                    block_hash,
                ).await;

                let transactions = generate_conflict_free_transactions(10);
                let _duration = execute_parallel(state, transactions).await;
            })
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 3: Sequential Baseline - 100 Transactions
// ============================================================================

fn bench_sequential_100_txs(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_execution");
    group.sample_size(10);
    group.throughput(Throughput::Elements(100));

    let runtime = Runtime::new().unwrap();

    group.bench_function("100_txs", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let temp_dir = TempDir::new("tos-bench-seq-100").expect("temp dir");
                let storage = SledStorage::new(
                    temp_dir.path().to_string_lossy().to_string(),
                    Some(1024 * 1024),
                    Network::Devnet,
                    1024 * 1024,
                    StorageMode::HighThroughput,
                ).expect("storage");

                let storage_arc = Arc::new(RwLock::new(storage));
                let environment = Arc::new(Environment::new());

                let block = create_minimal_block();
                let block_hash = block.hash();

                let state = ParallelChainState::new(
                    storage_arc,
                    environment,
                    0,
                    1,
                    BlockVersion::V0,
                    block,
                    block_hash,
                ).await;

                let transactions = generate_conflict_free_transactions(100);
                let _duration = execute_sequential(state, transactions).await;
            })
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 4: Parallel Execution - 100 Transactions
// ============================================================================

fn bench_parallel_100_txs(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_execution");
    group.sample_size(10);
    group.throughput(Throughput::Elements(100));

    let runtime = Runtime::new().unwrap();

    group.bench_function("100_txs", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let temp_dir = TempDir::new("tos-bench-par-100").expect("temp dir");
                let storage = SledStorage::new(
                    temp_dir.path().to_string_lossy().to_string(),
                    Some(1024 * 1024),
                    Network::Devnet,
                    1024 * 1024,
                    StorageMode::HighThroughput,
                ).expect("storage");

                let storage_arc = Arc::new(RwLock::new(storage));
                let environment = Arc::new(Environment::new());

                let block = create_minimal_block();
                let block_hash = block.hash();

                let state = ParallelChainState::new(
                    storage_arc,
                    environment,
                    0,
                    1,
                    BlockVersion::V0,
                    block,
                    block_hash,
                ).await;

                let transactions = generate_conflict_free_transactions(100);
                let _duration = execute_parallel(state, transactions).await;
            })
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 5: Mixed Conflict Ratio (50% conflicts)
// ============================================================================

fn bench_conflict_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("conflict_ratio_50pct");
    group.sample_size(10);
    group.throughput(Throughput::Elements(50));

    let runtime = Runtime::new().unwrap();

    // Sequential execution with conflicts
    group.bench_function("sequential_50_txs", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let temp_dir = TempDir::new("tos-bench-conflict-seq").expect("temp dir");
                let storage = SledStorage::new(
                    temp_dir.path().to_string_lossy().to_string(),
                    Some(1024 * 1024),
                    Network::Devnet,
                    1024 * 1024,
                    StorageMode::HighThroughput,
                ).expect("storage");

                let storage_arc = Arc::new(RwLock::new(storage));
                let environment = Arc::new(Environment::new());

                let block = create_minimal_block();
                let block_hash = block.hash();

                let state = ParallelChainState::new(
                    storage_arc,
                    environment,
                    0,
                    1,
                    BlockVersion::V0,
                    block,
                    block_hash,
                ).await;

                let transactions = generate_mixed_conflict_transactions(50);
                let _duration = execute_sequential(state, transactions).await;
            })
        });
    });

    // Parallel execution with conflicts
    group.bench_function("parallel_50_txs", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let temp_dir = TempDir::new("tos-bench-conflict-par").expect("temp dir");
                let storage = SledStorage::new(
                    temp_dir.path().to_string_lossy().to_string(),
                    Some(1024 * 1024),
                    Network::Devnet,
                    1024 * 1024,
                    StorageMode::HighThroughput,
                ).expect("storage");

                let storage_arc = Arc::new(RwLock::new(storage));
                let environment = Arc::new(Environment::new());

                let block = create_minimal_block();
                let block_hash = block.hash();

                let state = ParallelChainState::new(
                    storage_arc,
                    environment,
                    0,
                    1,
                    BlockVersion::V0,
                    block,
                    block_hash,
                ).await;

                let transactions = generate_mixed_conflict_transactions(50);
                let _duration = execute_parallel(state, transactions).await;
            })
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark 6: Direct TPS Comparison (Side-by-side)
// ============================================================================

fn bench_tps_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("tps_comparison");
    group.sample_size(10);

    let runtime = Runtime::new().unwrap();

    for tx_count in [10, 50, 100].iter() {
        // Fixed: Use standard b.iter() to include all overhead in measurement
        group.bench_with_input(
            BenchmarkId::new("sequential", format!("{}_txs", tx_count)),
            tx_count,
            |b, &count| {
                b.iter(|| {
                    runtime.block_on(async {
                        // Include storage creation in measurement (same as other benchmarks)
                        let temp_dir = TempDir::new("tos-bench-tps-seq").expect("temp dir");
                        let storage = SledStorage::new(
                            temp_dir.path().to_string_lossy().to_string(),
                            Some(1024 * 1024),
                            Network::Devnet,
                            1024 * 1024,
                            StorageMode::HighThroughput,
                        ).expect("storage");

                        let storage_arc = Arc::new(RwLock::new(storage));
                        let environment = Arc::new(Environment::new());

                        let block = create_minimal_block();
                        let block_hash = block.hash();

                        let state = ParallelChainState::new(
                            storage_arc,
                            environment,
                            0,
                            1,
                            BlockVersion::V0,
                            block,
                            block_hash,
                        ).await;

                        let transactions = generate_conflict_free_transactions(count);
                        let _duration = execute_sequential(state, transactions).await;
                    })
                });
            },
        );

        // Fixed: Use standard b.iter() to include all overhead in measurement
        group.bench_with_input(
            BenchmarkId::new("parallel", format!("{}_txs", tx_count)),
            tx_count,
            |b, &count| {
                b.iter(|| {
                    runtime.block_on(async {
                        // Include storage creation in measurement (same as other benchmarks)
                        let temp_dir = TempDir::new("tos-bench-tps-par").expect("temp dir");
                        let storage = SledStorage::new(
                            temp_dir.path().to_string_lossy().to_string(),
                            Some(1024 * 1024),
                            Network::Devnet,
                            1024 * 1024,
                            StorageMode::HighThroughput,
                        ).expect("storage");

                        let storage_arc = Arc::new(RwLock::new(storage));
                        let environment = Arc::new(Environment::new());

                        let block = create_minimal_block();
                        let block_hash = block.hash();

                        let state = ParallelChainState::new(
                            storage_arc,
                            environment,
                            0,
                            1,
                            BlockVersion::V0,
                            block,
                            block_hash,
                        ).await;

                        let transactions = generate_conflict_free_transactions(count);
                        let _duration = execute_parallel(state, transactions).await;
                    })
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    parallel_tps_benches,
    bench_sequential_10_txs,
    bench_parallel_10_txs,
    bench_sequential_100_txs,
    bench_parallel_100_txs,
    bench_conflict_ratio,
    bench_tps_comparison,
);

criterion_main!(parallel_tps_benches);
