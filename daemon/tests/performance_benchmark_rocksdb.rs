//! RocksDB Performance Benchmark Test Suite
//!
//! This test suite measures and benchmarks RocksDB performance across various
//! dimensions including storage operations, concurrent access, and parallel
//! chain state operations.
//!
//! # Running the Benchmarks
//!
//! Run all benchmarks:
//! ```bash
//! cargo test --test performance_benchmark_rocksdb --ignored -- --nocapture
//! ```
//!
//! Run specific benchmark:
//! ```bash
//! cargo test --test performance_benchmark_rocksdb benchmark_storage_write_speed --ignored -- --nocapture
//! ```
//!
//! # Interpreting Results
//!
//! Performance baselines (on modern hardware):
//! - Write operations: 10,000-50,000 ops/sec
//! - Read operations: 50,000-200,000 ops/sec
//! - Concurrent operations (10 threads): 5,000-20,000 ops/sec
//! - ParallelChainState creation: < 10ms
//! - State commit: < 100ms for 100 accounts
//!
//! Results significantly below these baselines may indicate performance issues.

use std::sync::Arc;
use std::time::Instant;
use tempdir::TempDir;
use tokio::sync::RwLock;
use tokio::task::JoinSet;

use tos_common::{
    account::{VersionedBalance, VersionedNonce},
    asset::{AssetData, VersionedAssetData},
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    config::{COIN_DECIMALS, COIN_VALUE, TOS_ASSET},
    crypto::{elgamal::CompressedPublicKey, Hash, Hashable, KeyPair},
    immutable::Immutable,
    network::Network,
    versioned_type::Versioned,
};

use tos_daemon::core::{
    config::RocksDBConfig,
    state::parallel_chain_state::ParallelChainState,
    storage::{
        rocksdb::RocksStorage, AccountProvider, AssetProvider, BalanceProvider, NonceProvider,
    },
};

use tos_environment::Environment;

// ============================================================================
// Test Utilities
// ============================================================================

/// Register TOS asset in storage
async fn register_tos_asset(storage: &mut RocksStorage) {
    let asset_data = AssetData::new(
        COIN_DECIMALS,
        "TOS".to_string(),
        "TOS".to_string(),
        None,
        None,
    );
    let versioned_asset_data: VersionedAssetData = Versioned::new(asset_data, Some(0));
    storage
        .add_asset(&TOS_ASSET, 0, versioned_asset_data)
        .await
        .unwrap();
}

/// Setup account with balance and nonce
async fn setup_account(
    storage: &Arc<RwLock<RocksStorage>>,
    account: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
    topoheight: u64,
) {
    let mut guard = storage.write().await;
    guard
        .set_last_nonce_to(
            account,
            topoheight,
            &VersionedNonce::new(nonce, Some(topoheight)),
        )
        .await
        .unwrap();
    guard
        .set_last_balance_to(
            account,
            &TOS_ASSET,
            topoheight,
            &VersionedBalance::new(balance, Some(topoheight)),
        )
        .await
        .unwrap();
    guard
        .set_account_registration_topoheight(account, topoheight)
        .await
        .unwrap();
}

/// Create a dummy block for testing
fn create_dummy_block() -> (Block, Hash) {
    let miner = KeyPair::new().get_public_key().compress();
    let dummy_header = BlockHeader::new(
        BlockVersion::V0,
        vec![],
        0,
        0,
        0u64.into(),
        Hash::zero(),
        0,
        0,
        [0u8; EXTRA_NONCE_SIZE],
        miner,
        Hash::zero(),
        Hash::zero(),
        Hash::zero(),
    );

    let dummy_block = Block::new(Immutable::Arc(Arc::new(dummy_header)), vec![]);

    let block_hash = dummy_block.hash();
    (dummy_block, block_hash)
}

/// Print formatted benchmark results
fn print_benchmark_header(name: &str) {
    println!("\n{}", "=".repeat(80));
    println!("  BENCHMARK: {name}");
    println!("{}", "=".repeat(80));
}

fn print_benchmark_result(label: &str, value: &str, unit: &str) {
    println!("  {label:40} {value:>15} {unit}");
}

fn print_benchmark_footer() {
    println!("{}\n", "=".repeat(80));
}

// ============================================================================
// Benchmark 1: Storage Write Speed
// ============================================================================

/// Benchmark: Measure write operation speed for accounts
///
/// What this measures:
/// - Raw write throughput for account creation
/// - Time to set nonce and balance
/// - Storage backend write performance
///
/// Expected results:
/// - 10,000-50,000 accounts/sec on modern hardware
/// - Linear scaling with number of accounts
/// - No performance degradation over time
#[tokio::test]
#[ignore]
async fn benchmark_storage_write_speed() {
    print_benchmark_header("Storage Write Speed - Account Creation");

    const NUM_ACCOUNTS: usize = 1000;

    let temp_dir = TempDir::new("tos_bench_write").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));

    println!("\n  Creating {NUM_ACCOUNTS} accounts...");

    let start = Instant::now();
    for _i in 0..NUM_ACCOUNTS {
        let keypair = KeyPair::new();
        setup_account(
            &storage,
            &keypair.get_public_key().compress(),
            (_i as u64 + 1) * COIN_VALUE,
            0,
            0,
        )
        .await;
    }
    let elapsed = start.elapsed();

    let accounts_per_sec = NUM_ACCOUNTS as f64 / elapsed.as_secs_f64();
    let avg_time_per_account = elapsed.as_micros() as f64 / NUM_ACCOUNTS as f64;

    print_benchmark_result(
        "Total accounts created",
        &NUM_ACCOUNTS.to_string(),
        "accounts",
    );
    print_benchmark_result(
        "Total time",
        &format!("{:.3}", elapsed.as_secs_f64()),
        "seconds",
    );
    print_benchmark_result(
        "Throughput",
        &format!("{accounts_per_sec:.2}"),
        "accounts/sec",
    );
    print_benchmark_result(
        "Average time per account",
        &format!("{avg_time_per_account:.2}"),
        "microseconds",
    );

    println!("\n  Performance Analysis:");
    if accounts_per_sec > 20000.0 {
        println!(
            "    ✓ EXCELLENT: {}x faster than baseline (10,000 accounts/sec)",
            accounts_per_sec / 10000.0
        );
    } else if accounts_per_sec > 10000.0 {
        println!("    ✓ GOOD: Meets baseline performance");
    } else if accounts_per_sec > 5000.0 {
        println!(
            "    ⚠ ACCEPTABLE: {}x slower than baseline",
            10000.0 / accounts_per_sec
        );
    } else {
        println!(
            "    ✗ SLOW: {}x slower than baseline - investigate!",
            10000.0 / accounts_per_sec
        );
    }

    print_benchmark_footer();
}

// ============================================================================
// Benchmark 2: Storage Read Speed
// ============================================================================

/// Benchmark: Measure read operation speed for account data
///
/// What this measures:
/// - Raw read throughput for balance and nonce queries
/// - Cache effectiveness
/// - Storage backend read performance
///
/// Expected results:
/// - 50,000-200,000 reads/sec on modern hardware
/// - Significantly faster than writes
/// - Consistent performance across queries
#[tokio::test]
#[ignore]
async fn benchmark_storage_read_speed() {
    print_benchmark_header("Storage Read Speed - Balance and Nonce Queries");

    const NUM_ACCOUNTS: usize = 100;
    const NUM_READS: usize = 1000;

    let temp_dir = TempDir::new("tos_bench_read").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));

    // Pre-populate accounts
    println!("\n  Pre-populating {NUM_ACCOUNTS} accounts...");
    let mut accounts = Vec::new();
    for i in 0..NUM_ACCOUNTS {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        setup_account(&storage, &pubkey, (i as u64 + 1) * COIN_VALUE, 0, 0).await;
        accounts.push(pubkey);
    }

    // Benchmark reads
    println!("  Performing {NUM_READS} random reads...");
    let start = Instant::now();
    for i in 0..NUM_READS {
        let account_idx = i % NUM_ACCOUNTS;
        let account = &accounts[account_idx];

        let guard = storage.read().await;
        let _balance = guard
            .get_balance_at_exact_topoheight(account, &TOS_ASSET, 0)
            .await
            .unwrap();
        let _nonce = guard
            .get_nonce_at_exact_topoheight(account, 0)
            .await
            .unwrap();
    }
    let elapsed = start.elapsed();

    let reads_per_sec = NUM_READS as f64 / elapsed.as_secs_f64();
    let avg_time_per_read = elapsed.as_micros() as f64 / NUM_READS as f64;

    print_benchmark_result("Total reads performed", &NUM_READS.to_string(), "reads");
    print_benchmark_result(
        "Total time",
        &format!("{:.3}", elapsed.as_secs_f64()),
        "seconds",
    );
    print_benchmark_result("Throughput", &format!("{reads_per_sec:.2}"), "reads/sec");
    print_benchmark_result(
        "Average time per read",
        &format!("{avg_time_per_read:.2}"),
        "microseconds",
    );

    println!("\n  Performance Analysis:");
    if reads_per_sec > 100000.0 {
        println!(
            "    ✓ EXCELLENT: {}x faster than baseline (50,000 reads/sec)",
            reads_per_sec / 50000.0
        );
    } else if reads_per_sec > 50000.0 {
        println!("    ✓ GOOD: Meets baseline performance");
    } else if reads_per_sec > 25000.0 {
        println!(
            "    ⚠ ACCEPTABLE: {}x slower than baseline",
            50000.0 / reads_per_sec
        );
    } else {
        println!(
            "    ✗ SLOW: {}x slower than baseline - investigate!",
            50000.0 / reads_per_sec
        );
    }

    print_benchmark_footer();
}

// ============================================================================
// Benchmark 3: Update Operations Speed
// ============================================================================

/// Benchmark: Measure update operation speed (read-modify-write)
///
/// What this measures:
/// - Combined read + write performance
/// - Transaction-like workload simulation
/// - Nonce increment and balance update speed
///
/// Expected results:
/// - 5,000-20,000 updates/sec
/// - Slower than pure reads or writes due to RMW cycle
/// - Good indicator of transaction processing speed
#[tokio::test]
#[ignore]
async fn benchmark_storage_update_speed() {
    print_benchmark_header("Storage Update Speed - Read-Modify-Write Operations");

    const NUM_ACCOUNTS: usize = 100;
    const NUM_UPDATES: usize = 1000;

    let temp_dir = TempDir::new("tos_bench_update").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));

    // Pre-populate accounts
    println!("\n  Pre-populating {NUM_ACCOUNTS} accounts...");
    let mut accounts = Vec::new();
    for _i in 0..NUM_ACCOUNTS {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        setup_account(&storage, &pubkey, 1000 * COIN_VALUE, 0, 0).await;
        accounts.push(pubkey);
    }

    // Benchmark updates
    println!("  Performing {NUM_UPDATES} update operations...");
    let start = Instant::now();
    for i in 0..NUM_UPDATES {
        let account_idx = i % NUM_ACCOUNTS;
        let account = &accounts[account_idx];

        // Read current values
        let (current_balance, current_nonce) = {
            let guard = storage.read().await;
            let balance = guard
                .get_balance_at_maximum_topoheight(account, &TOS_ASSET, i as u64)
                .await
                .unwrap()
                .map(|(_, v)| v.get_balance())
                .unwrap_or(0);
            let nonce = guard
                .get_nonce_at_maximum_topoheight(account, i as u64)
                .await
                .unwrap()
                .map(|(_, v)| v.get_nonce())
                .unwrap_or(0);
            (balance, nonce)
        };

        // Modify and write back
        {
            let mut guard = storage.write().await;
            let new_balance = current_balance.saturating_sub(COIN_VALUE);
            let new_nonce = current_nonce + 1;

            guard
                .set_last_balance_to(
                    account,
                    &TOS_ASSET,
                    i as u64,
                    &VersionedBalance::new(new_balance, Some(i as u64)),
                )
                .await
                .unwrap();
            guard
                .set_last_nonce_to(
                    account,
                    i as u64,
                    &VersionedNonce::new(new_nonce, Some(i as u64)),
                )
                .await
                .unwrap();
        }
    }
    let elapsed = start.elapsed();

    let updates_per_sec = NUM_UPDATES as f64 / elapsed.as_secs_f64();
    let avg_time_per_update = elapsed.as_micros() as f64 / NUM_UPDATES as f64;

    print_benchmark_result(
        "Total updates performed",
        &NUM_UPDATES.to_string(),
        "updates",
    );
    print_benchmark_result(
        "Total time",
        &format!("{:.3}", elapsed.as_secs_f64()),
        "seconds",
    );
    print_benchmark_result(
        "Throughput",
        &format!("{updates_per_sec:.2}"),
        "updates/sec",
    );
    print_benchmark_result(
        "Average time per update",
        &format!("{avg_time_per_update:.2}"),
        "microseconds",
    );

    println!("\n  Performance Analysis:");
    if updates_per_sec > 10000.0 {
        println!(
            "    ✓ EXCELLENT: {}x faster than baseline (5,000 updates/sec)",
            updates_per_sec / 5000.0
        );
    } else if updates_per_sec > 5000.0 {
        println!("    ✓ GOOD: Meets baseline performance");
    } else if updates_per_sec > 2500.0 {
        println!(
            "    ⚠ ACCEPTABLE: {}x slower than baseline",
            5000.0 / updates_per_sec
        );
    } else {
        println!(
            "    ✗ SLOW: {}x slower than baseline - investigate!",
            5000.0 / updates_per_sec
        );
    }

    print_benchmark_footer();
}

// ============================================================================
// Benchmark 4: Concurrent Access Performance (10 workers)
// ============================================================================

/// Benchmark: Measure concurrent access with 10 worker threads
///
/// What this measures:
/// - Lock contention under moderate concurrency
/// - Scalability of storage backend
/// - Concurrent read/write performance
///
/// Expected results:
/// - 5,000-20,000 ops/sec with 10 workers
/// - Good parallelization (near-linear speedup for reads)
/// - Some contention on writes (expected)
#[tokio::test]
#[ignore]
async fn benchmark_concurrent_access_10_workers() {
    print_benchmark_header("Concurrent Access - 10 Workers");

    const NUM_WORKERS: usize = 10;
    const OPS_PER_WORKER: usize = 100;
    const TOTAL_OPS: usize = NUM_WORKERS * OPS_PER_WORKER;

    let temp_dir = TempDir::new("tos_bench_concurrent_10").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));

    println!("\n  Spawning {NUM_WORKERS} workers performing {OPS_PER_WORKER} operations each...");

    let start = Instant::now();
    let mut join_set = JoinSet::new();

    for worker_id in 0..NUM_WORKERS {
        let storage_clone = Arc::clone(&storage);
        join_set.spawn(async move {
            for i in 0..OPS_PER_WORKER {
                let keypair = KeyPair::new();
                let pubkey = keypair.get_public_key().compress();

                // Write operation
                {
                    let mut guard = storage_clone.write().await;
                    guard
                        .set_last_nonce_to(&pubkey, 0, &VersionedNonce::new(i as u64, Some(0)))
                        .await
                        .unwrap();
                    guard
                        .set_last_balance_to(
                            &pubkey,
                            &TOS_ASSET,
                            0,
                            &VersionedBalance::new(
                                (worker_id * 100 + i) as u64 * COIN_VALUE,
                                Some(0),
                            ),
                        )
                        .await
                        .unwrap();
                    guard
                        .set_account_registration_topoheight(&pubkey, 0)
                        .await
                        .unwrap();
                }

                // Read operation
                {
                    let guard = storage_clone.read().await;
                    let _balance = guard
                        .get_balance_at_exact_topoheight(&pubkey, &TOS_ASSET, 0)
                        .await;
                }
            }
            worker_id
        });
    }

    // Wait for all workers to complete
    while let Some(_result) = join_set.join_next().await {}

    let elapsed = start.elapsed();
    let ops_per_sec = TOTAL_OPS as f64 / elapsed.as_secs_f64();
    let avg_time_per_op = elapsed.as_micros() as f64 / TOTAL_OPS as f64;

    print_benchmark_result("Number of workers", &NUM_WORKERS.to_string(), "workers");
    print_benchmark_result("Operations per worker", &OPS_PER_WORKER.to_string(), "ops");
    print_benchmark_result("Total operations", &TOTAL_OPS.to_string(), "ops");
    print_benchmark_result(
        "Total time",
        &format!("{:.3}", elapsed.as_secs_f64()),
        "seconds",
    );
    print_benchmark_result("Throughput", &format!("{ops_per_sec:.2}"), "ops/sec");
    print_benchmark_result(
        "Average time per op",
        &format!("{avg_time_per_op:.2}"),
        "microseconds",
    );

    println!("\n  Performance Analysis:");
    if ops_per_sec > 10000.0 {
        println!(
            "    ✓ EXCELLENT: {}x faster than baseline (5,000 ops/sec)",
            ops_per_sec / 5000.0
        );
    } else if ops_per_sec > 5000.0 {
        println!("    ✓ GOOD: Meets baseline performance");
    } else if ops_per_sec > 2500.0 {
        println!(
            "    ⚠ ACCEPTABLE: {}x slower than baseline",
            5000.0 / ops_per_sec
        );
    } else {
        println!(
            "    ✗ SLOW: {}x slower than baseline - investigate!",
            5000.0 / ops_per_sec
        );
    }

    print_benchmark_footer();
}

// ============================================================================
// Benchmark 5: Concurrent Access Performance (50 workers)
// ============================================================================

/// Benchmark: Measure concurrent access with 50 worker threads
///
/// What this measures:
/// - Lock contention under high concurrency
/// - Scalability limits of storage backend
/// - Performance degradation with many workers
///
/// Expected results:
/// - 5,000-15,000 ops/sec with 50 workers
/// - Some contention overhead (not linear scaling)
/// - Still maintains reasonable throughput
#[tokio::test]
#[ignore]
async fn benchmark_concurrent_access_50_workers() {
    print_benchmark_header("Concurrent Access - 50 Workers (High Contention)");

    const NUM_WORKERS: usize = 50;
    const OPS_PER_WORKER: usize = 50;
    const TOTAL_OPS: usize = NUM_WORKERS * OPS_PER_WORKER;

    let temp_dir = TempDir::new("tos_bench_concurrent_50").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));

    println!("\n  Spawning {NUM_WORKERS} workers performing {OPS_PER_WORKER} operations each...");

    let start = Instant::now();
    let mut join_set = JoinSet::new();

    for worker_id in 0..NUM_WORKERS {
        let storage_clone = Arc::clone(&storage);
        join_set.spawn(async move {
            for i in 0..OPS_PER_WORKER {
                let keypair = KeyPair::new();
                let pubkey = keypair.get_public_key().compress();

                {
                    let mut guard = storage_clone.write().await;
                    guard
                        .set_last_nonce_to(&pubkey, 0, &VersionedNonce::new(i as u64, Some(0)))
                        .await
                        .unwrap();
                    guard
                        .set_last_balance_to(
                            &pubkey,
                            &TOS_ASSET,
                            0,
                            &VersionedBalance::new(
                                (worker_id * 100 + i) as u64 * COIN_VALUE,
                                Some(0),
                            ),
                        )
                        .await
                        .unwrap();
                    guard
                        .set_account_registration_topoheight(&pubkey, 0)
                        .await
                        .unwrap();
                }
            }
            worker_id
        });
    }

    // Wait for all workers
    while let Some(_result) = join_set.join_next().await {}

    let elapsed = start.elapsed();
    let ops_per_sec = TOTAL_OPS as f64 / elapsed.as_secs_f64();
    let avg_time_per_op = elapsed.as_micros() as f64 / TOTAL_OPS as f64;

    print_benchmark_result("Number of workers", &NUM_WORKERS.to_string(), "workers");
    print_benchmark_result("Operations per worker", &OPS_PER_WORKER.to_string(), "ops");
    print_benchmark_result("Total operations", &TOTAL_OPS.to_string(), "ops");
    print_benchmark_result(
        "Total time",
        &format!("{:.3}", elapsed.as_secs_f64()),
        "seconds",
    );
    print_benchmark_result("Throughput", &format!("{ops_per_sec:.2}"), "ops/sec");
    print_benchmark_result(
        "Average time per op",
        &format!("{avg_time_per_op:.2}"),
        "microseconds",
    );

    println!("\n  Performance Analysis:");
    if ops_per_sec > 10000.0 {
        println!(
            "    ✓ EXCELLENT: {}x faster than baseline (5,000 ops/sec)",
            ops_per_sec / 5000.0
        );
    } else if ops_per_sec > 5000.0 {
        println!("    ✓ GOOD: Meets baseline performance");
    } else if ops_per_sec > 2500.0 {
        println!(
            "    ⚠ ACCEPTABLE: {}x slower than baseline",
            5000.0 / ops_per_sec
        );
    } else {
        println!(
            "    ✗ SLOW: {}x slower than baseline - high contention detected!",
            5000.0 / ops_per_sec
        );
    }

    print_benchmark_footer();
}

// ============================================================================
// Benchmark 6: Concurrent Access Performance (100 workers)
// ============================================================================

/// Benchmark: Measure concurrent access with 100 worker threads
///
/// What this measures:
/// - Extreme lock contention
/// - Maximum scalability of storage backend
/// - Performance under stress conditions
///
/// Expected results:
/// - 3,000-10,000 ops/sec with 100 workers
/// - Significant contention overhead
/// - Tests system stability under load
#[tokio::test]
#[ignore]
async fn benchmark_concurrent_access_100_workers() {
    print_benchmark_header("Concurrent Access - 100 Workers (Extreme Contention)");

    const NUM_WORKERS: usize = 100;
    const OPS_PER_WORKER: usize = 30;
    const TOTAL_OPS: usize = NUM_WORKERS * OPS_PER_WORKER;

    let temp_dir = TempDir::new("tos_bench_concurrent_100").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));

    println!("\n  Spawning {NUM_WORKERS} workers performing {OPS_PER_WORKER} operations each...");

    let start = Instant::now();
    let mut join_set = JoinSet::new();

    for worker_id in 0..NUM_WORKERS {
        let storage_clone = Arc::clone(&storage);
        join_set.spawn(async move {
            for i in 0..OPS_PER_WORKER {
                let keypair = KeyPair::new();
                let pubkey = keypair.get_public_key().compress();

                {
                    let mut guard = storage_clone.write().await;
                    guard
                        .set_last_nonce_to(&pubkey, 0, &VersionedNonce::new(i as u64, Some(0)))
                        .await
                        .unwrap();
                    guard
                        .set_last_balance_to(
                            &pubkey,
                            &TOS_ASSET,
                            0,
                            &VersionedBalance::new(
                                (worker_id * 100 + i) as u64 * COIN_VALUE,
                                Some(0),
                            ),
                        )
                        .await
                        .unwrap();
                    guard
                        .set_account_registration_topoheight(&pubkey, 0)
                        .await
                        .unwrap();
                }
            }
            worker_id
        });
    }

    // Wait for all workers
    while let Some(_result) = join_set.join_next().await {}

    let elapsed = start.elapsed();
    let ops_per_sec = TOTAL_OPS as f64 / elapsed.as_secs_f64();
    let avg_time_per_op = elapsed.as_micros() as f64 / TOTAL_OPS as f64;

    print_benchmark_result("Number of workers", &NUM_WORKERS.to_string(), "workers");
    print_benchmark_result("Operations per worker", &OPS_PER_WORKER.to_string(), "ops");
    print_benchmark_result("Total operations", &TOTAL_OPS.to_string(), "ops");
    print_benchmark_result(
        "Total time",
        &format!("{:.3}", elapsed.as_secs_f64()),
        "seconds",
    );
    print_benchmark_result("Throughput", &format!("{ops_per_sec:.2}"), "ops/sec");
    print_benchmark_result(
        "Average time per op",
        &format!("{avg_time_per_op:.2}"),
        "microseconds",
    );

    println!("\n  Performance Analysis:");
    if ops_per_sec > 8000.0 {
        println!(
            "    ✓ EXCELLENT: {}x faster than baseline (3,000 ops/sec)",
            ops_per_sec / 3000.0
        );
    } else if ops_per_sec > 3000.0 {
        println!("    ✓ GOOD: Meets baseline performance under extreme load");
    } else if ops_per_sec > 1500.0 {
        println!(
            "    ⚠ ACCEPTABLE: {}x slower than baseline",
            3000.0 / ops_per_sec
        );
    } else {
        println!(
            "    ✗ SLOW: {}x slower than baseline - severe contention!",
            3000.0 / ops_per_sec
        );
    }

    print_benchmark_footer();
}

// ============================================================================
// Benchmark 7: ParallelChainState Creation Speed
// ============================================================================

/// Benchmark: Measure ParallelChainState creation time
///
/// What this measures:
/// - Initialization overhead of parallel execution state
/// - Network info caching performance
/// - Arc/DashMap allocation overhead
///
/// Expected results:
/// - < 10ms per state creation
/// - Minimal memory overhead
/// - No deadlocks or hangs
#[tokio::test]
#[ignore]
async fn benchmark_parallel_chain_state_creation() {
    print_benchmark_header("ParallelChainState Creation Speed");

    const NUM_CREATIONS: usize = 100;

    let temp_dir = TempDir::new("tos_bench_pcs_create").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let (dummy_block, block_hash) = create_dummy_block();

    println!("\n  Creating {NUM_CREATIONS} ParallelChainState instances...");

    let start = Instant::now();
    let mut states = Vec::new();
    for i in 0..NUM_CREATIONS {
        let state = ParallelChainState::new(
            Arc::clone(&storage),
            Arc::clone(&environment),
            i as u64,
            i as u64 + 1,
            BlockVersion::V0,
            dummy_block.clone(),
            block_hash.clone(),
        )
        .await;
        states.push(state);
    }
    let elapsed = start.elapsed();

    let creations_per_sec = NUM_CREATIONS as f64 / elapsed.as_secs_f64();
    let avg_time_per_creation = elapsed.as_micros() as f64 / NUM_CREATIONS as f64;

    print_benchmark_result("Total creations", &NUM_CREATIONS.to_string(), "instances");
    print_benchmark_result(
        "Total time",
        &format!("{:.3}", elapsed.as_secs_f64()),
        "seconds",
    );
    print_benchmark_result(
        "Average creation time",
        &format!("{:.2}", avg_time_per_creation / 1000.0),
        "milliseconds",
    );
    print_benchmark_result(
        "Throughput",
        &format!("{creations_per_sec:.2}"),
        "creations/sec",
    );

    println!("\n  Performance Analysis:");
    if avg_time_per_creation / 1000.0 < 5.0 {
        println!(
            "    ✓ EXCELLENT: {}x faster than baseline (10ms)",
            10.0 / (avg_time_per_creation / 1000.0)
        );
    } else if avg_time_per_creation / 1000.0 < 10.0 {
        println!("    ✓ GOOD: Meets baseline performance");
    } else if avg_time_per_creation / 1000.0 < 20.0 {
        println!(
            "    ⚠ ACCEPTABLE: {}x slower than baseline",
            (avg_time_per_creation / 1000.0) / 10.0
        );
    } else {
        println!(
            "    ✗ SLOW: {}x slower than baseline - investigate!",
            (avg_time_per_creation / 1000.0) / 10.0
        );
    }

    print_benchmark_footer();
}

// ============================================================================
// Benchmark 8: ParallelChainState Commit Speed
// ============================================================================

/// Benchmark: Measure state commit time with various account counts
///
/// What this measures:
/// - Bulk write performance for state merging
/// - Modification tracking overhead
/// - Commit batching effectiveness
///
/// Expected results:
/// - < 100ms for 100 accounts
/// - Linear scaling with account count
/// - Efficient bulk write batching
#[tokio::test]
#[ignore]
async fn benchmark_parallel_chain_state_commit() {
    print_benchmark_header("ParallelChainState Commit Speed");

    const NUM_ACCOUNTS_LIST: &[usize] = &[10, 50, 100, 200];

    for &num_accounts in NUM_ACCOUNTS_LIST {
        println!("\n  Testing commit with {num_accounts} accounts...");

        let temp_dir = TempDir::new("tos_bench_pcs_commit").unwrap();
        let dir_path = temp_dir.path().to_string_lossy().to_string();
        let config = RocksDBConfig::default();

        let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
        register_tos_asset(&mut storage).await;
        let storage = Arc::new(RwLock::new(storage));
        let environment = Arc::new(Environment::new());

        let (dummy_block, block_hash) = create_dummy_block();

        // Create ParallelChainState
        let state = ParallelChainState::new(
            Arc::clone(&storage),
            environment,
            0,
            1,
            BlockVersion::V0,
            dummy_block,
            block_hash,
        )
        .await;

        // Load and modify accounts
        for _i in 0..num_accounts {
            let keypair = KeyPair::new();
            let pubkey = keypair.get_public_key().compress();

            // Setup initial state
            setup_account(&storage, &pubkey, 1000 * COIN_VALUE, 0, 0).await;

            // Load and modify through ParallelChainState
            state.ensure_account_loaded(&pubkey).await.unwrap();
            state
                .ensure_balance_loaded(&pubkey, &TOS_ASSET)
                .await
                .unwrap();
            state.set_nonce(&pubkey, _i as u64 + 1);
            #[allow(deprecated)]
            state.set_balance(&pubkey, &TOS_ASSET, (1000 - _i as u64) * COIN_VALUE);
        }

        // Benchmark commit
        let start = Instant::now();
        {
            let mut guard = storage.write().await;
            state.commit(&mut *guard).await.unwrap();
        }
        let elapsed = start.elapsed();

        let commits_per_sec = 1.0 / elapsed.as_secs_f64();
        let time_ms = elapsed.as_secs_f64() * 1000.0;
        let time_per_account_us = (elapsed.as_micros() as f64) / (num_accounts as f64);

        print_benchmark_result(
            &format!("  Commit time (n={num_accounts})"),
            &format!("{time_ms:.2}"),
            "milliseconds",
        );
        print_benchmark_result(
            &format!("  Time per account (n={num_accounts})"),
            &format!("{time_per_account_us:.2}"),
            "microseconds",
        );
        print_benchmark_result(
            &format!("  Throughput (n={num_accounts})"),
            &format!("{commits_per_sec:.2}"),
            "commits/sec",
        );

        if num_accounts == 100 {
            println!("\n  Performance Analysis (100 accounts baseline):");
            if time_ms < 50.0 {
                println!(
                    "    ✓ EXCELLENT: {}x faster than baseline (100ms)",
                    100.0 / time_ms
                );
            } else if time_ms < 100.0 {
                println!("    ✓ GOOD: Meets baseline performance");
            } else if time_ms < 200.0 {
                println!(
                    "    ⚠ ACCEPTABLE: {}x slower than baseline",
                    time_ms / 100.0
                );
            } else {
                println!(
                    "    ✗ SLOW: {}x slower than baseline - investigate!",
                    time_ms / 100.0
                );
            }
        }
    }

    print_benchmark_footer();
}

// ============================================================================
// Benchmark 9: Account Loading Performance
// ============================================================================

/// Benchmark: Measure account data loading speed into ParallelChainState
///
/// What this measures:
/// - ensure_account_loaded() performance
/// - ensure_balance_loaded() performance
/// - Storage read overhead in parallel context
/// - Semaphore serialization overhead
///
/// Expected results:
/// - < 500 microseconds per account load (with balance)
/// - Dominated by storage read time
/// - Minimal caching overhead
#[tokio::test]
#[ignore]
async fn benchmark_account_loading() {
    print_benchmark_header("Account Loading Performance");

    const NUM_ACCOUNTS: usize = 200;

    let temp_dir = TempDir::new("tos_bench_account_load").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    // Pre-populate accounts in storage
    println!("\n  Pre-populating {NUM_ACCOUNTS} accounts in storage...");
    let mut accounts = Vec::new();
    for i in 0..NUM_ACCOUNTS {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        setup_account(&storage, &pubkey, (i as u64 + 1) * COIN_VALUE, i as u64, 0).await;
        accounts.push(pubkey);
    }

    let (dummy_block, block_hash) = create_dummy_block();

    let state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        dummy_block,
        block_hash,
    )
    .await;

    // Benchmark account loading (nonce + balance)
    println!("  Loading {NUM_ACCOUNTS} accounts into ParallelChainState...");
    let start = Instant::now();
    for account in &accounts {
        state.ensure_account_loaded(account).await.unwrap();
        state
            .ensure_balance_loaded(account, &TOS_ASSET)
            .await
            .unwrap();
    }
    let elapsed = start.elapsed();

    let loads_per_sec = NUM_ACCOUNTS as f64 / elapsed.as_secs_f64();
    let avg_time_per_load = elapsed.as_micros() as f64 / NUM_ACCOUNTS as f64;

    print_benchmark_result(
        "Total accounts loaded",
        &NUM_ACCOUNTS.to_string(),
        "accounts",
    );
    print_benchmark_result(
        "Total time",
        &format!("{:.3}", elapsed.as_secs_f64()),
        "seconds",
    );
    print_benchmark_result("Throughput", &format!("{loads_per_sec:.2}"), "loads/sec");
    print_benchmark_result(
        "Average load time",
        &format!("{avg_time_per_load:.2}"),
        "microseconds",
    );

    println!("\n  Performance Analysis:");
    if avg_time_per_load < 250.0 {
        println!(
            "    ✓ EXCELLENT: {}x faster than baseline (500us)",
            500.0 / avg_time_per_load
        );
    } else if avg_time_per_load < 500.0 {
        println!("    ✓ GOOD: Meets baseline performance");
    } else if avg_time_per_load < 1000.0 {
        println!(
            "    ⚠ ACCEPTABLE: {}x slower than baseline",
            avg_time_per_load / 500.0
        );
    } else {
        println!(
            "    ✗ SLOW: {}x slower than baseline - investigate!",
            avg_time_per_load / 500.0
        );
    }

    // Test cache hit performance (should be much faster)
    println!("\n  Testing cache hit performance (re-loading same accounts)...");
    let cache_start = Instant::now();
    for account in &accounts {
        state.ensure_account_loaded(account).await.unwrap();
        state
            .ensure_balance_loaded(account, &TOS_ASSET)
            .await
            .unwrap();
    }
    let cache_elapsed = cache_start.elapsed();
    let cache_time_per_load = cache_elapsed.as_micros() as f64 / NUM_ACCOUNTS as f64;

    print_benchmark_result(
        "Cache hit time per account",
        &format!("{cache_time_per_load:.2}"),
        "microseconds",
    );

    println!("\n  Cache Effectiveness:");
    let speedup = avg_time_per_load / cache_time_per_load;
    println!("    Cache hits are {speedup}x faster than storage loads");
    if speedup > 10.0 {
        println!("    ✓ EXCELLENT: Cache is very effective");
    } else if speedup > 5.0 {
        println!("    ✓ GOOD: Cache provides significant benefit");
    } else {
        println!("    ⚠ WARNING: Cache may not be working correctly");
    }

    print_benchmark_footer();
}

// ============================================================================
// Summary Report
// ============================================================================

/// Run all benchmarks and generate summary report
#[tokio::test]
#[ignore]
async fn benchmark_all_summary() {
    println!("\n\n");
    println!("################################################################################");
    println!("#                                                                              #");
    println!("#               RocksDB Performance Benchmark Suite - Summary                 #");
    println!("#                                                                              #");
    println!("################################################################################");
    println!("\n");
    println!("Running complete benchmark suite...");
    println!("This will take several minutes. Please wait...\n");

    // Note: In a real implementation, you would call each benchmark and collect results
    // For now, this is a placeholder that tells users how to run the full suite

    println!("To run the full benchmark suite, execute:");
    println!("  cargo test --test performance_benchmark_rocksdb --ignored -- --nocapture");
    println!("\nTo run individual benchmarks:");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_storage_write_speed --ignored -- --nocapture");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_storage_read_speed --ignored -- --nocapture");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_storage_update_speed --ignored -- --nocapture");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_concurrent_access_10_workers --ignored -- --nocapture");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_concurrent_access_50_workers --ignored -- --nocapture");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_concurrent_access_100_workers --ignored -- --nocapture");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_parallel_chain_state_creation --ignored -- --nocapture");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_parallel_chain_state_commit --ignored -- --nocapture");
    println!("  cargo test --test performance_benchmark_rocksdb benchmark_account_loading --ignored -- --nocapture");

    println!("\n\n");
    println!("################################################################################");
    println!("#                           Expected Performance                               #");
    println!("################################################################################");
    println!("\n  Baseline Performance Targets (Modern Hardware):");
    println!("  ------------------------------------------------");
    println!("  Storage Writes:             10,000-50,000 accounts/sec");
    println!("  Storage Reads:              50,000-200,000 reads/sec");
    println!("  Storage Updates:            5,000-20,000 updates/sec");
    println!("  Concurrent (10 workers):    5,000-20,000 ops/sec");
    println!("  Concurrent (50 workers):    5,000-15,000 ops/sec");
    println!("  Concurrent (100 workers):   3,000-10,000 ops/sec");
    println!("  ParallelChainState Create:  < 10ms per instance");
    println!("  ParallelChainState Commit:  < 100ms for 100 accounts");
    println!("  Account Loading:            < 500us per account");
    println!("\n");
}
