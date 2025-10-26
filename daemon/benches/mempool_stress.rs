// Mempool Stress Test Benchmark
//
// Tests TOS blockchain mempool behavior under various load scenarios.
//
// Run with: cargo bench --bench mempool_stress

#![allow(dead_code)]  // Benchmark code has intentionally unused helper methods for future tests
#![allow(unused_assignments)]  // Benchmark snapshots may be collected but not always used

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tos_common::{
    config::{COIN_VALUE, TOS_ASSET},
    crypto::{KeyPair, Hashable},
    transaction::{
        FeeType,
        builder::{TransactionBuilder, TransferBuilder, TransactionTypeBuilder, FeeBuilder, FeeHelper},
        Transaction,
        TxVersion,
    },
    crypto::elgamal::CompressedPublicKey,
};
use std::collections::HashMap;

// Snapshot of mempool state for monitoring
#[derive(Debug, Clone)]
#[allow(dead_code)]  // Fields used for future analysis/logging
struct MempoolSnapshot {
    timestamp: Duration,       // Time since test start
    size: usize,               // Number of transactions in mempool
    accepted_count: u64,       // Total accepted transactions
    rejected_count: u64,       // Total rejected transactions
    memory_rss: u64,          // RSS memory in bytes
}

// Simplified mempool for stress testing
struct StressTestMempool {
    txs: Arc<Mutex<HashMap<tos_common::crypto::Hash, Arc<Transaction>>>>,
    max_size: usize,
    accepted_count: Arc<Mutex<u64>>,
    rejected_count: Arc<Mutex<u64>>,
}

impl StressTestMempool {
    fn new(max_size: usize) -> Self {
        Self {
            txs: Arc::new(Mutex::new(HashMap::new())),
            max_size,
            accepted_count: Arc::new(Mutex::new(0)),
            rejected_count: Arc::new(Mutex::new(0)),
        }
    }

    async fn add_transaction(&self, tx: Arc<Transaction>) -> Result<(), String> {
        let hash = tx.hash();
        let mut txs = self.txs.lock().await;

        // Reject if mempool is full
        if txs.len() >= self.max_size {
            let mut rejected = self.rejected_count.lock().await;
            *rejected += 1;
            return Err("Mempool full".to_string());
        }

        // Check for duplicate
        if txs.contains_key(&hash) {
            let mut rejected = self.rejected_count.lock().await;
            *rejected += 1;
            return Err("Transaction already in mempool".to_string());
        }

        // Add transaction
        txs.insert(hash, tx);
        let mut accepted = self.accepted_count.lock().await;
        *accepted += 1;

        Ok(())
    }

    async fn remove_transactions(&self, count: usize) -> Vec<Arc<Transaction>> {
        let mut txs = self.txs.lock().await;
        let mut removed = Vec::with_capacity(count);

        // Remove oldest transactions (simulating block inclusion)
        let keys_to_remove: Vec<_> = txs.keys().take(count).cloned().collect();
        for key in keys_to_remove {
            if let Some(tx) = txs.remove(&key) {
                removed.push(tx);
            }
        }

        removed
    }

    async fn size(&self) -> usize {
        self.txs.lock().await.len()
    }

    async fn get_stats(&self) -> (usize, u64, u64) {
        let size = self.txs.lock().await.len();
        let accepted = *self.accepted_count.lock().await;
        let rejected = *self.rejected_count.lock().await;
        (size, accepted, rejected)
    }

    async fn clear(&self) {
        self.txs.lock().await.clear();
    }
}

// Mock account state for transaction building
struct MockAccountState {
    balances: HashMap<tos_common::crypto::Hash, u64>,
    nonce: u64,
}

impl MockAccountState {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
            nonce: 0,
        }
    }

    fn set_balance(&mut self, asset: tos_common::crypto::Hash, amount: u64) {
        self.balances.insert(asset, amount);
    }
}

impl FeeHelper for MockAccountState {
    type Error = Box<dyn std::error::Error>;

    fn account_exists(&self, _account: &CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(true) // All accounts exist in mock
    }
}

impl tos_common::transaction::builder::AccountState for MockAccountState {
    fn is_mainnet(&self) -> bool {
        false
    }

    fn get_account_balance(&self, asset: &tos_common::crypto::Hash) -> Result<u64, Self::Error> {
        Ok(self.balances.get(asset).copied().unwrap_or(1000000 * COIN_VALUE))
    }

    fn get_reference(&self) -> tos_common::transaction::Reference {
        tos_common::transaction::Reference {
            topoheight: 0,
            hash: tos_common::crypto::Hash::zero(),
        }
    }

    fn update_account_balance(&mut self, asset: &tos_common::crypto::Hash, new_balance: u64) -> Result<(), Self::Error> {
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

    fn is_account_registered(&self, _account: &CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(true) // All accounts registered in mock
    }
}

// Helper to create a transaction
fn create_test_transaction(
    sender: &KeyPair,
    receiver: &CompressedPublicKey,
    amount: u64,
    fee: u64,
    nonce: u64,
) -> Result<Transaction, String> {
    let transfer = TransferBuilder {
        destination: receiver.clone().to_address(false),
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
        fee_builder
    ).with_fee_type(FeeType::TOS);

    let mut state = MockAccountState::new();
    state.set_balance(TOS_ASSET, 1000000 * COIN_VALUE);
    state.nonce = nonce;

    let tx = builder.build(&mut state, sender).map_err(|e| e.to_string())?;
    Ok(tx)
}

// Get current memory usage (simplified for benchmark)
fn get_memory_usage() -> u64 {
    // Note: Memory tracking requires sysinfo crate which is only in dev-dependencies
    // For now, return 0 as a placeholder. In production, would use actual memory monitoring.
    0
}

// Scenario 1: Sustained High Input
// 1000 TPS continuous input, 100 TPS drain (1 BPS * 100 txs/block)
async fn bench_sustained_high_input() -> Vec<MempoolSnapshot> {
    println!("\n=== Scenario 1: Sustained High Input ===");
    println!("Configuration:");
    println!("  Input Rate:     1000 TPS continuous");
    println!("  Block Rate:     1 BPS");
    println!("  Block Capacity: 100 txs/block");
    println!("  Drain Rate:     100 TPS");
    println!("  Mempool Limit:  10,000 txs");
    println!("  Duration:       120 seconds");
    println!();

    let mempool = Arc::new(StressTestMempool::new(10_000));
    let sender = KeyPair::new();
    let receiver = KeyPair::new().get_public_key().compress();
    let mut snapshots = Vec::new();

    let start = Instant::now();
    let duration = Duration::from_secs(120);

    // Spawn transaction generator (1000 TPS)
    let mempool_clone = mempool.clone();
    let tx_generator = tokio::spawn(async move {
        let mut nonce = 0u64;
        let mut tx_count = 0u64;

        loop {
            if start.elapsed() >= duration {
                break;
            }

            // Generate transaction
            if let Ok(tx) = create_test_transaction(&sender, &receiver, 100, 10, nonce) {
                let _ = mempool_clone.add_transaction(Arc::new(tx)).await;
                nonce += 1;
                tx_count += 1;
            }

            // Sleep to maintain 1000 TPS
            tokio::time::sleep(Duration::from_micros(1000)).await;
        }

        println!("  Transactions generated: {}", tx_count);
    });

    // Spawn block processor (100 TPS drain rate = 1 BPS * 100 txs/block)
    let mempool_clone = mempool.clone();
    let block_processor = tokio::spawn(async move {
        let mut blocks = 0u64;
        let mut total_txs = 0u64;

        loop {
            if start.elapsed() >= duration {
                break;
            }

            // Process block every 1 second (1 BPS)
            tokio::time::sleep(Duration::from_secs(1)).await;

            // Remove 100 transactions (simulating block inclusion)
            let txs = mempool_clone.remove_transactions(100).await;
            total_txs += txs.len() as u64;
            blocks += 1;
        }

        println!("  Blocks processed: {}", blocks);
        println!("  Transactions drained: {}", total_txs);
    });

    // Monitor mempool state
    let mempool_clone = mempool.clone();
    let monitor = tokio::spawn(async move {
        let mut snapshots = Vec::new();

        loop {
            if start.elapsed() >= duration {
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            let elapsed = start.elapsed();
            let (size, accepted, rejected) = mempool_clone.get_stats().await;
            let memory = get_memory_usage();

            snapshots.push(MempoolSnapshot {
                timestamp: elapsed,
                size,
                accepted_count: accepted,
                rejected_count: rejected,
                memory_rss: memory,
            });
        }

        snapshots
    });

    // Wait for all tasks
    tx_generator.await.unwrap();
    block_processor.await.unwrap();
    snapshots = monitor.await.unwrap();

    // Print statistics
    let (final_size, accepted, rejected) = mempool.get_stats().await;
    println!("\nResults:");
    println!("  Final mempool size: {}", final_size);
    println!("  Total accepted:     {}", accepted);
    println!("  Total rejected:     {}", rejected);
    println!("  Acceptance rate:    {:.2}%", (accepted as f64 / (accepted + rejected) as f64) * 100.0);

    // Calculate average mempool size
    let avg_size = snapshots.iter().map(|s| s.size as f64).sum::<f64>() / snapshots.len() as f64;
    println!("  Average mempool:    {:.0} txs", avg_size);

    snapshots
}

// Scenario 2: Burst Traffic
// 5000 TPS for 10 seconds (50,000 txs burst), then 100 TPS sustained
async fn bench_burst_traffic() -> Vec<MempoolSnapshot> {
    println!("\n=== Scenario 2: Burst Traffic ===");
    println!("Configuration:");
    println!("  Burst Phase:    5000 TPS for 10 seconds (50,000 txs)");
    println!("  Sustained:      100 TPS after burst");
    println!("  Block Rate:     1 BPS");
    println!("  Block Capacity: 100 txs/block");
    println!("  Mempool Limit:  50,000 txs");
    println!("  Total Duration: 600 seconds (10 min)");
    println!();

    let mempool = Arc::new(StressTestMempool::new(50_000));
    let sender = KeyPair::new();
    let receiver = KeyPair::new().get_public_key().compress();
    let mut snapshots = Vec::new();

    let start = Instant::now();
    let burst_duration = Duration::from_secs(10);
    let total_duration = Duration::from_secs(600);

    // Spawn transaction generator
    let mempool_clone = mempool.clone();
    let tx_generator = tokio::spawn(async move {
        let mut nonce = 0u64;
        let mut burst_count = 0u64;
        let mut sustained_count = 0u64;

        loop {
            let elapsed = start.elapsed();
            if elapsed >= total_duration {
                break;
            }

            // Burst phase: 5000 TPS
            if elapsed < burst_duration {
                if let Ok(tx) = create_test_transaction(&sender, &receiver, 100, 10, nonce) {
                    let _ = mempool_clone.add_transaction(Arc::new(tx)).await;
                    nonce += 1;
                    burst_count += 1;
                }
                // Sleep to maintain 5000 TPS
                tokio::time::sleep(Duration::from_micros(200)).await;
            }
            // Sustained phase: 100 TPS
            else {
                if let Ok(tx) = create_test_transaction(&sender, &receiver, 100, 10, nonce) {
                    let _ = mempool_clone.add_transaction(Arc::new(tx)).await;
                    nonce += 1;
                    sustained_count += 1;
                }
                // Sleep to maintain 100 TPS
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        println!("  Burst transactions:     {}", burst_count);
        println!("  Sustained transactions: {}", sustained_count);
    });

    // Spawn block processor (100 TPS drain)
    let mempool_clone = mempool.clone();
    let block_processor = tokio::spawn(async move {
        let mut blocks = 0u64;
        let mut total_txs = 0u64;

        loop {
            if start.elapsed() >= total_duration {
                break;
            }

            tokio::time::sleep(Duration::from_secs(1)).await;

            let txs = mempool_clone.remove_transactions(100).await;
            total_txs += txs.len() as u64;
            blocks += 1;
        }

        println!("  Blocks processed:       {}", blocks);
        println!("  Transactions drained:   {}", total_txs);
    });

    // Monitor mempool state
    let mempool_clone = mempool.clone();
    let monitor = tokio::spawn(async move {
        let mut snapshots = Vec::new();
        let mut peak_size = 0usize;

        loop {
            if start.elapsed() >= total_duration {
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            let elapsed = start.elapsed();
            let (size, accepted, rejected) = mempool_clone.get_stats().await;
            let memory = get_memory_usage();

            if size > peak_size {
                peak_size = size;
            }

            snapshots.push(MempoolSnapshot {
                timestamp: elapsed,
                size,
                accepted_count: accepted,
                rejected_count: rejected,
                memory_rss: memory,
            });
        }

        println!("  Peak mempool size:      {}", peak_size);
        snapshots
    });

    // Wait for all tasks
    tx_generator.await.unwrap();
    block_processor.await.unwrap();
    snapshots = monitor.await.unwrap();

    // Print statistics
    let (final_size, accepted, rejected) = mempool.get_stats().await;
    println!("\nResults:");
    println!("  Final mempool size: {}", final_size);
    println!("  Total accepted:     {}", accepted);
    println!("  Total rejected:     {}", rejected);
    println!("  Acceptance rate:    {:.2}%", (accepted as f64 / (accepted + rejected) as f64) * 100.0);

    snapshots
}

// Scenario 3: Fee-Based Eviction
// Test priority replacement when mempool is full
async fn bench_fee_eviction() -> Vec<MempoolSnapshot> {
    println!("\n=== Scenario 3: Fee-Based Eviction ===");
    println!("Configuration:");
    println!("  Phase 1: Fill mempool with 10,000 low-fee txs (10 nanoTOS fee)");
    println!("  Phase 2: Submit 1,000 high-fee txs (100 nanoTOS fee)");
    println!("  Mempool Limit: 10,000 txs");
    println!("  Expected: High-fee txs should evict low-fee txs");
    println!();

    // Note: This is a simplified version since true fee-based eviction
    // requires mempool implementation changes. This test demonstrates
    // the rejection behavior when mempool is full.

    let mempool = Arc::new(StressTestMempool::new(10_000));
    let sender = KeyPair::new();
    let receiver = KeyPair::new().get_public_key().compress();
    let mut snapshots = Vec::new();

    let start = Instant::now();

    println!("Phase 1: Filling mempool with low-fee transactions...");
    let mut nonce = 0u64;
    let mut low_fee_accepted = 0u64;

    // Fill mempool with low-fee transactions
    for _ in 0..10_000 {
        if let Ok(tx) = create_test_transaction(&sender, &receiver, 100, 10, nonce) {
            if mempool.add_transaction(Arc::new(tx)).await.is_ok() {
                low_fee_accepted += 1;
            }
        }
        nonce += 1;
    }

    let (size_after_phase1, _, _) = mempool.get_stats().await;
    println!("  Low-fee transactions accepted: {}", low_fee_accepted);
    println!("  Mempool size after phase 1: {}", size_after_phase1);

    // Take snapshot
    snapshots.push(MempoolSnapshot {
        timestamp: start.elapsed(),
        size: size_after_phase1,
        accepted_count: low_fee_accepted,
        rejected_count: 0,
        memory_rss: get_memory_usage(),
    });

    println!("\nPhase 2: Submitting high-fee transactions...");
    let mut high_fee_accepted = 0u64;
    let mut high_fee_rejected = 0u64;

    // Try to add high-fee transactions
    for _ in 0..1_000 {
        if let Ok(tx) = create_test_transaction(&sender, &receiver, 100, 100, nonce) {
            match mempool.add_transaction(Arc::new(tx)).await {
                Ok(_) => high_fee_accepted += 1,
                Err(_) => high_fee_rejected += 1,
            }
        }
        nonce += 1;
    }

    let (final_size, total_accepted, total_rejected) = mempool.get_stats().await;
    println!("  High-fee transactions accepted: {}", high_fee_accepted);
    println!("  High-fee transactions rejected: {}", high_fee_rejected);
    println!("  Final mempool size: {}", final_size);

    // Take final snapshot
    snapshots.push(MempoolSnapshot {
        timestamp: start.elapsed(),
        size: final_size,
        accepted_count: total_accepted,
        rejected_count: total_rejected,
        memory_rss: get_memory_usage(),
    });

    println!("\nResults:");
    println!("  Total accepted: {}", total_accepted);
    println!("  Total rejected: {}", total_rejected);
    println!("  Rejection rate: {:.2}%", (total_rejected as f64 / (total_accepted + total_rejected) as f64) * 100.0);
    println!("\nNote: True fee-based eviction requires mempool priority queue implementation.");
    println!("This test demonstrates rejection behavior when mempool is full.");

    snapshots
}

// Main function to run all benchmarks
#[tokio::main]
async fn main() {
    println!("\n========================================");
    println!("TOS Mempool Stress Test Benchmark Suite");
    println!("========================================\n");

    // Run Scenario 1
    bench_sustained_high_input().await;

    println!("\n");

    // Run Scenario 2
    bench_burst_traffic().await;

    println!("\n");

    // Run Scenario 3
    bench_fee_eviction().await;

    println!("\n========================================");
    println!("All mempool stress tests completed!");
    println!("========================================\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mempool_basic_operations() {
        let mempool = StressTestMempool::new(100);
        let sender = KeyPair::new();
        let receiver = KeyPair::new().get_public_key().compress();

        // Add transaction
        let tx = create_test_transaction(&sender, &receiver, 100, 10, 0).unwrap();
        assert!(mempool.add_transaction(Arc::new(tx)).await.is_ok());

        // Check size
        assert_eq!(mempool.size().await, 1);

        // Remove transaction
        let removed = mempool.remove_transactions(1).await;
        assert_eq!(removed.len(), 1);
        assert_eq!(mempool.size().await, 0);
    }

    #[tokio::test]
    async fn test_mempool_rejection_when_full() {
        let mempool = StressTestMempool::new(10);
        let sender = KeyPair::new();
        let receiver = KeyPair::new().get_public_key().compress();

        // Fill mempool
        for nonce in 0..10 {
            let tx = create_test_transaction(&sender, &receiver, 100, 10, nonce).unwrap();
            assert!(mempool.add_transaction(Arc::new(tx)).await.is_ok());
        }

        // Try to add one more (should be rejected)
        let tx = create_test_transaction(&sender, &receiver, 100, 10, 10).unwrap();
        assert!(mempool.add_transaction(Arc::new(tx)).await.is_err());

        let (_, _, rejected) = mempool.get_stats().await;
        assert_eq!(rejected, 1);
    }
}
