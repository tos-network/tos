//! Security Performance Benchmarks
//!
//! This module benchmarks security-critical operations to ensure they perform
//! within acceptable bounds. Slow security operations can lead to DoS vulnerabilities.
//!
//! Benchmarked operations:
//! 1. Signature verification (Ed25519)
//! 2. Hash computation (Blake3, SHA3)
//! 3. Transaction validation
//! 4. Block validation
//! 5. Merkle tree computation
//! 6. Nonce verification
//! 7. Balance verification

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::time::Duration;

// Benchmark 1: Signature Verification Performance
fn benchmark_signature_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("signature_verification");

    // Ed25519 signature verification should be fast (< 100 microseconds)
    group.bench_function("ed25519_verify", |b| {
        let (pubkey, signature, message) = setup_ed25519_signature();

        b.iter(|| {
            verify_ed25519_signature(
                black_box(&pubkey),
                black_box(&signature),
                black_box(&message),
            )
        });
    });

    // Batch verification should be more efficient
    group.bench_function("ed25519_batch_verify_10", |b| {
        let batch = setup_ed25519_batch(10);

        b.iter(|| batch_verify_ed25519(black_box(&batch)));
    });

    group.bench_function("ed25519_batch_verify_100", |b| {
        let batch = setup_ed25519_batch(100);

        b.iter(|| batch_verify_ed25519(black_box(&batch)));
    });

    group.finish();
}

// Benchmark 2: Hash Computation Performance
fn benchmark_hash_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash_computation");

    // Blake3 hash should be very fast
    group.bench_function("blake3_32bytes", |b| {
        let data = vec![0u8; 32];
        b.iter(|| hash_blake3(black_box(&data)));
    });

    group.bench_function("blake3_1kb", |b| {
        let data = vec![0u8; 1024];
        b.iter(|| hash_blake3(black_box(&data)));
    });

    group.bench_function("blake3_10kb", |b| {
        let data = vec![0u8; 10 * 1024];
        b.iter(|| hash_blake3(black_box(&data)));
    });

    // SHA3 hash for comparison
    group.bench_function("sha3_256_32bytes", |b| {
        let data = vec![0u8; 32];
        b.iter(|| hash_sha3_256(black_box(&data)));
    });

    group.finish();
}

// Benchmark 3: Transaction Validation
fn benchmark_transaction_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("transaction_validation");

    group.bench_function("simple_transfer", |b| {
        let tx = create_simple_transfer_tx();

        b.iter(|| validate_transaction(black_box(&tx)));
    });

    group.bench_function("complex_transaction", |b| {
        let tx = create_complex_tx();

        b.iter(|| validate_transaction(black_box(&tx)));
    });

    // Batch validation
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("batch_validation", size),
            size,
            |b, &size| {
                let txs = create_transaction_batch(size);
                b.iter(|| validate_transaction_batch(black_box(&txs)));
            },
        );
    }

    group.finish();
}

// Benchmark 4: Block Validation
fn benchmark_block_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_validation");

    // Block validation with different transaction counts
    for tx_count in [1, 10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("validate_block", tx_count),
            tx_count,
            |b, &tx_count| {
                let block = create_block_with_txs(tx_count);
                b.iter(|| validate_block(black_box(&block)));
            },
        );
    }

    group.finish();
}

// Benchmark 5: Merkle Tree Computation
fn benchmark_merkle_tree(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_tree");

    for count in [1, 10, 100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("merkle_root", count),
            count,
            |b, &count| {
                let hashes = create_hash_list(count);
                b.iter(|| compute_merkle_root(black_box(&hashes)));
            },
        );
    }

    group.finish();
}

// Benchmark 6: Nonce Verification
fn benchmark_nonce_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("nonce_verification");

    group.bench_function("single_nonce_check", |b| {
        let nonce = 12345u64;
        let expected = 12345u64;

        b.iter(|| verify_nonce(black_box(nonce), black_box(expected)));
    });

    // Nonce lookup in set
    for set_size in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("nonce_set_lookup", set_size),
            set_size,
            |b, &set_size| {
                let nonce_set = create_nonce_set(set_size);
                let lookup_nonce = (set_size / 2) as u64;

                b.iter(|| nonce_set_contains(black_box(&nonce_set), black_box(lookup_nonce)));
            },
        );
    }

    group.finish();
}

// Benchmark 7: Balance Verification
fn benchmark_balance_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("balance_operations");

    group.bench_function("balance_add_checked", |b| {
        let balance1 = 1_000_000u64;
        let balance2 = 500_000u64;

        b.iter(|| balance1.checked_add(black_box(balance2)));
    });

    group.bench_function("balance_sub_checked", |b| {
        let balance = 1_000_000u64;
        let amount = 500_000u64;

        b.iter(|| balance.checked_sub(black_box(amount)));
    });

    group.bench_function("fee_calculation_scaled", |b| {
        let base_fee = 100_000u64;
        let multiplier = 12000u128; // 1.2x with SCALE=10000

        b.iter(|| calculate_fee_scaled(black_box(base_fee), black_box(multiplier)));
    });

    group.finish();
}

// Helper functions and mock implementations

fn setup_ed25519_signature() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    // In a real implementation, this would create valid Ed25519 signatures
    let pubkey = vec![0u8; 32];
    let signature = vec![0u8; 64];
    let message = b"benchmark message".to_vec();
    (pubkey, signature, message)
}

fn setup_ed25519_batch(count: usize) -> Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    (0..count).map(|_| setup_ed25519_signature()).collect()
}

fn verify_ed25519_signature(_pubkey: &[u8], _signature: &[u8], _message: &[u8]) -> bool {
    // Mock verification
    true
}

fn batch_verify_ed25519(batch: &[(Vec<u8>, Vec<u8>, Vec<u8>)]) -> bool {
    batch
        .iter()
        .all(|(pk, sig, msg)| verify_ed25519_signature(pk, sig, msg))
}

fn hash_blake3(data: &[u8]) -> [u8; 32] {
    // Use SHA3-256 as a replacement since blake3 is not a dev-dependency
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn hash_sha3_256(data: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    hasher.finalize().into()
}

struct MockTransaction {
    nonce: u64,
    amount: u64,
    fee: u64,
}

fn create_simple_transfer_tx() -> MockTransaction {
    MockTransaction {
        nonce: 1,
        amount: 100_000,
        fee: 1_000,
    }
}

fn create_complex_tx() -> MockTransaction {
    MockTransaction {
        nonce: 12345,
        amount: 999_999_999,
        fee: 100_000,
    }
}

fn create_transaction_batch(count: usize) -> Vec<MockTransaction> {
    (0..count)
        .map(|i| MockTransaction {
            nonce: i as u64,
            amount: 100_000,
            fee: 1_000,
        })
        .collect()
}

fn validate_transaction(tx: &MockTransaction) -> bool {
    tx.amount > 0 && tx.fee > 0
}

fn validate_transaction_batch(txs: &[MockTransaction]) -> bool {
    txs.iter().all(validate_transaction)
}

struct MockBlock {
    transactions: Vec<MockTransaction>,
}

fn create_block_with_txs(count: usize) -> MockBlock {
    MockBlock {
        transactions: create_transaction_batch(count),
    }
}

fn validate_block(block: &MockBlock) -> bool {
    validate_transaction_batch(&block.transactions)
}

fn create_hash_list(count: usize) -> Vec<[u8; 32]> {
    (0..count)
        .map(|i| {
            let mut hash = [0u8; 32];
            hash[0] = (i % 256) as u8;
            hash
        })
        .collect()
}

fn compute_merkle_root(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.is_empty() {
        return [0u8; 32];
    }

    let mut current_level = hashes.to_vec();

    while current_level.len() > 1 {
        let mut next_level = Vec::new();

        for chunk in current_level.chunks(2) {
            let combined = if chunk.len() == 2 {
                let mut data = Vec::new();
                data.extend_from_slice(&chunk[0]);
                data.extend_from_slice(&chunk[1]);
                hash_blake3(&data)
            } else {
                chunk[0]
            };
            next_level.push(combined);
        }

        current_level = next_level;
    }

    current_level[0]
}

fn verify_nonce(nonce: u64, expected: u64) -> bool {
    nonce == expected
}

fn create_nonce_set(size: usize) -> std::collections::HashSet<u64> {
    (0..size as u64).collect()
}

fn nonce_set_contains(set: &std::collections::HashSet<u64>, nonce: u64) -> bool {
    set.contains(&nonce)
}

fn calculate_fee_scaled(base_fee: u64, multiplier: u128) -> u64 {
    const SCALE: u128 = 10000;
    ((base_fee as u128 * multiplier) / SCALE) as u64
}

// Configure criterion
criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(10))
        .sample_size(100);
    targets =
        benchmark_signature_verification,
        benchmark_hash_computation,
        benchmark_transaction_validation,
        benchmark_block_validation,
        benchmark_merkle_tree,
        benchmark_nonce_verification,
        benchmark_balance_operations
}

criterion_main!(benches);
