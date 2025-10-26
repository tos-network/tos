//! End-to-End TPS Benchmark for TOS Blockchain
//!
//! This benchmark provides realistic TPS measurement under production-like conditions,
//! following Kaspa's best practices. Unlike the simple `tps.rs` benchmark which only
//! measures transaction verification performance (~14,300 TPS), this benchmark simulates:
//!
//! 1. **TPS Throttling**: Realistic transaction arrival rate (e.g., 100 TPS)
//! 2. **Mempool Management**: Tracks mempool size and prevents overflow
//! 3. **Network Latency**: Simulates P2P propagation delays (100-500ms)
//! 4. **Block Processing**: Simulates 1 BPS block production
//! 5. **Transaction Confirmation**: Measures time from submission to confirmation
//!
//! Expected results: 100-500 TPS (realistic production throughput)
//!
//! Reference: /Users/tomisetsu/tos-network/memo/TPS_BENCHMARK_ANALYSIS.md

#![allow(dead_code)]  // Benchmark code has intentionally unused helper methods for future tests

use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, Criterion};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    runtime::{Builder, Runtime},
    sync::{Mutex as AsyncMutex, RwLock},
    time::sleep,
};

use tos_common::{
    account::Nonce,
    block::BlockVersion,
    config::{COIN_VALUE, TOS_ASSET},
    crypto::{
        elgamal::{CompressedPublicKey, KeyPair},
        Hash, Hashable,
    },
    transaction::{
        builder::{AccountState, FeeBuilder, FeeHelper, TransactionBuilder, TransactionTypeBuilder, TransferBuilder},
        verify::NoZKPCache,
        FeeType, MultiSigPayload, Reference, Transaction, TxVersion,
    },
};
use tos_vm::{Environment, Module};

// -------------------------------------------------------------------------------------------------
// Configuration
// -------------------------------------------------------------------------------------------------

/// End-to-end TPS benchmark configuration
#[derive(Clone, Debug)]
struct E2ETPSConfig {
    /// Total number of transactions to submit
    tx_count: usize,
    /// Target TPS pressure (transactions per second throttle)
    tps_pressure: u64,
    /// Target mempool size (pause submission when exceeded)
    mempool_target: u64,
    /// Block time in milliseconds (1000ms = 1 BPS)
    block_time_ms: u64,
    /// Network delay simulation in milliseconds (100-500ms)
    network_delay_ms: u64,
    /// Test duration in seconds
    test_duration_secs: u64,
    /// Transfer amount per transaction (in TOS coins)
    transfer_amount: u64,
    /// Fee per transaction (in base units)
    fee: u64,
}

impl Default for E2ETPSConfig {
    fn default() -> Self {
        Self {
            tx_count: 10_000,
            tps_pressure: 100,
            mempool_target: 500,
            block_time_ms: 1000,
            network_delay_ms: 200,
            test_duration_secs: 60,
            transfer_amount: 50,
            fee: 5_000,
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Simplified Account and State Management
// -------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct BalanceEntry {
    amount: u64,
}

#[derive(Clone)]
struct BenchAccount {
    keypair: KeyPair,
    balances: HashMap<Hash, BalanceEntry>,
    nonce: Nonce,
}

impl BenchAccount {
    fn new_with_balance(amount: u64) -> Self {
        let keypair = KeyPair::new();
        let mut balances = HashMap::new();
        balances.insert(TOS_ASSET, BalanceEntry { amount });
        Self { keypair, balances, nonce: 0 }
    }

    fn credit(&mut self, asset: &Hash, value: u64) {
        let entry = self.balances.entry(asset.clone()).or_insert_with(|| BalanceEntry { amount: 0 });
        entry.amount = entry.amount.saturating_add(value);
    }

    fn debit(&mut self, asset: &Hash, value: u64) -> Result<(), &'static str> {
        let entry = self.balances.get_mut(asset).ok_or("asset not found")?;
        if entry.amount < value {
            return Err("insufficient balance");
        }
        entry.amount -= value;
        Ok(())
    }
}

struct AccountStateImpl {
    balances: HashMap<Hash, BalanceEntry>,
    reference: Reference,
    nonce: Nonce,
}

impl AccountStateImpl {
    fn from_account(account: &BenchAccount) -> Self {
        Self {
            balances: account.balances.clone(),
            reference: Reference { topoheight: 0, hash: Hash::zero() },
            nonce: account.nonce,
        }
    }
}

impl FeeHelper for AccountStateImpl {
    type Error = String;

    fn account_exists(&self, _key: &CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

impl AccountState for AccountStateImpl {
    fn is_mainnet(&self) -> bool {
        false
    }

    fn get_account_balance(&self, asset: &Hash) -> Result<u64, Self::Error> {
        Ok(self.balances.get(asset).map(|b| b.amount).unwrap_or_default())
    }

    fn get_reference(&self) -> Reference {
        self.reference.clone()
    }

    fn update_account_balance(&mut self, asset: &Hash, new_balance: u64) -> Result<(), Self::Error> {
        self.balances.insert(asset.clone(), BalanceEntry { amount: new_balance });
        Ok(())
    }

    fn get_nonce(&self) -> Result<u64, Self::Error> {
        Ok(self.nonce)
    }

    fn update_nonce(&mut self, new_nonce: u64) -> Result<(), Self::Error> {
        self.nonce = new_nonce;
        Ok(())
    }

    fn is_account_registered(&self, _key: &CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

// -------------------------------------------------------------------------------------------------
// Verification State for Transaction Verification
// -------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct VerificationAccountState {
    balances: HashMap<Hash, u64>,
    nonce: Nonce,
}

#[derive(Clone)]
struct VerificationState {
    accounts: HashMap<CompressedPublicKey, VerificationAccountState>,
    multisig: HashMap<CompressedPublicKey, MultiSigPayload>,
    contracts: HashMap<Hash, Module>,
    env: Environment,
}

impl VerificationState {
    fn from_accounts(accounts: &[BenchAccount]) -> Self {
        let mut state = Self {
            accounts: HashMap::new(),
            multisig: HashMap::new(),
            contracts: HashMap::new(),
            env: Environment::new(),
        };

        for account in accounts {
            let balances: HashMap<Hash, u64> =
                account.balances.iter().map(|(asset, balance)| (asset.clone(), balance.amount)).collect();
            state.accounts.insert(
                account.keypair.get_public_key().compress(),
                VerificationAccountState { balances, nonce: account.nonce },
            );
        }

        state
    }

    fn update_balance(&mut self, account: &CompressedPublicKey, asset: &Hash, new_balance: u64) {
        if let Some(acc_state) = self.accounts.get_mut(account) {
            acc_state.balances.insert(asset.clone(), new_balance);
        }
    }

    fn increment_nonce(&mut self, account: &CompressedPublicKey) {
        if let Some(acc_state) = self.accounts.get_mut(account) {
            acc_state.nonce += 1;
        }
    }
}

#[async_trait]
impl<'a> tos_common::transaction::verify::BlockchainVerificationState<'a, ()> for VerificationState {
    async fn pre_verify_tx<'b>(&'b mut self, _tx: &Transaction) -> Result<(), ()> {
        Ok(())
    }

    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: std::borrow::Cow<'a, CompressedPublicKey>,
        asset: std::borrow::Cow<'a, Hash>,
    ) -> Result<&'b mut u64, ()> {
        self.accounts
            .get_mut(account.as_ref())
            .and_then(|account| account.balances.get_mut(asset.as_ref()))
            .ok_or(())
    }

    async fn get_sender_balance<'b>(
        &'b mut self,
        account: &'a CompressedPublicKey,
        asset: &'a Hash,
        _reference: &Reference,
    ) -> Result<&'b mut u64, ()> {
        self.accounts.get_mut(account).and_then(|account| account.balances.get_mut(asset)).ok_or(())
    }

    async fn add_sender_output(&mut self, _account: &'a CompressedPublicKey, _asset: &'a Hash, _output: u64) -> Result<(), ()> {
        Ok(())
    }

    async fn get_account_nonce(&mut self, account: &'a CompressedPublicKey) -> Result<Nonce, ()> {
        self.accounts.get(account).map(|account| account.nonce).ok_or(())
    }

    async fn update_account_nonce(&mut self, account: &'a CompressedPublicKey, new_nonce: Nonce) -> Result<(), ()> {
        let entry = self.accounts.get_mut(account).ok_or(())?;
        entry.nonce = new_nonce;
        Ok(())
    }

    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        expected: Nonce,
        new_value: Nonce,
    ) -> Result<bool, ()> {
        let current = self.get_account_nonce(account).await?;
        if current == expected {
            self.update_account_nonce(account, new_value).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get_block_version(&self) -> BlockVersion {
        BlockVersion::V0
    }

    async fn set_multisig_state(&mut self, account: &'a CompressedPublicKey, config: &MultiSigPayload) -> Result<(), ()> {
        self.multisig.insert(account.clone(), config.clone());
        Ok(())
    }

    async fn get_multisig_state(&mut self, account: &'a CompressedPublicKey) -> Result<Option<&MultiSigPayload>, ()> {
        Ok(self.multisig.get(account))
    }

    async fn get_environment(&mut self) -> Result<&Environment, ()> {
        Ok(&self.env)
    }

    async fn set_contract_module(&mut self, hash: &'a Hash, module: &'a Module) -> Result<(), ()> {
        self.contracts.insert(hash.clone(), module.clone());
        Ok(())
    }

    async fn load_contract_module(&mut self, hash: &'a Hash) -> Result<bool, ()> {
        Ok(self.contracts.contains_key(hash))
    }

    async fn get_contract_module_with_environment(&self, hash: &'a Hash) -> Result<(&Module, &Environment), ()> {
        let module = self.contracts.get(hash).ok_or(())?;
        Ok((module, &self.env))
    }
}

// -------------------------------------------------------------------------------------------------
// Transaction and Mempool Tracking
// -------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct TrackedTransaction {
    transaction: Arc<Transaction>,
    hash: Hash,
    submitted_at: Instant,
    confirmed_at: Option<Instant>,
    block_height: Option<u64>,
}

struct Mempool {
    transactions: AsyncMutex<VecDeque<TrackedTransaction>>,
    max_size: u64,
}

impl Mempool {
    fn new(max_size: u64) -> Self {
        Self { transactions: AsyncMutex::new(VecDeque::new()), max_size }
    }

    async fn size(&self) -> usize {
        self.transactions.lock().await.len()
    }

    async fn submit(&self, tx: TrackedTransaction) -> Result<(), &'static str> {
        let mut txs = self.transactions.lock().await;
        if txs.len() >= self.max_size as usize {
            return Err("mempool full");
        }
        txs.push_back(tx);
        Ok(())
    }

    async fn take_batch(&self, count: usize) -> Vec<TrackedTransaction> {
        let mut txs = self.transactions.lock().await;
        let take = count.min(txs.len());
        txs.drain(0..take).collect()
    }

    async fn clear(&self) {
        self.transactions.lock().await.clear();
    }
}

// -------------------------------------------------------------------------------------------------
// Metrics Collection
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct E2EMetrics {
    txs_submitted: Arc<AtomicUsize>,
    txs_confirmed: Arc<AtomicUsize>,
    blocks_produced: Arc<AtomicUsize>,
    total_confirmation_time_ms: Arc<AtomicU64>,
    mempool_size_samples: Arc<AsyncMutex<Vec<usize>>>,
    start_time: Option<Instant>,
}

impl E2EMetrics {
    fn new() -> Self {
        Self {
            txs_submitted: Arc::new(AtomicUsize::new(0)),
            txs_confirmed: Arc::new(AtomicUsize::new(0)),
            blocks_produced: Arc::new(AtomicUsize::new(0)),
            total_confirmation_time_ms: Arc::new(AtomicU64::new(0)),
            mempool_size_samples: Arc::new(AsyncMutex::new(Vec::new())),
            start_time: Some(Instant::now()),
        }
    }

    fn submit_tx(&self) {
        self.txs_submitted.fetch_add(1, Ordering::Relaxed);
    }

    fn confirm_tx(&self, confirmation_time_ms: u64) {
        self.txs_confirmed.fetch_add(1, Ordering::Relaxed);
        self.total_confirmation_time_ms.fetch_add(confirmation_time_ms, Ordering::Relaxed);
    }

    fn produce_block(&self) {
        self.blocks_produced.fetch_add(1, Ordering::Relaxed);
    }

    async fn record_mempool_size(&self, size: usize) {
        self.mempool_size_samples.lock().await.push(size);
    }

    async fn print_report(&self, config: &E2ETPSConfig) {
        let submitted = self.txs_submitted.load(Ordering::Relaxed);
        let confirmed = self.txs_confirmed.load(Ordering::Relaxed);
        let blocks = self.blocks_produced.load(Ordering::Relaxed);
        let total_conf_time = self.total_confirmation_time_ms.load(Ordering::Relaxed);

        let elapsed = self.start_time.map(|t| t.elapsed().as_secs_f64()).unwrap_or(1.0);
        let submitted_tps = submitted as f64 / elapsed;
        let confirmed_tps = confirmed as f64 / elapsed;

        let avg_conf_time = if confirmed > 0 { total_conf_time as f64 / confirmed as f64 } else { 0.0 };

        let samples = self.mempool_size_samples.lock().await;
        let mempool_avg = if !samples.is_empty() { samples.iter().sum::<usize>() as f64 / samples.len() as f64 } else { 0.0 };
        let mempool_min = samples.iter().min().copied().unwrap_or(0);
        let mempool_max = samples.iter().max().copied().unwrap_or(0);

        println!("\n========================================");
        println!("=== End-to-End TPS Benchmark Results ===");
        println!("========================================");
        println!("\nConfiguration:");
        println!("  Target TPS Pressure: {} TPS", config.tps_pressure);
        println!("  Mempool Target: {} txs", config.mempool_target);
        println!("  Block Time: {} ms ({} BPS)", config.block_time_ms, 1000 / config.block_time_ms);
        println!("  Network Delay: {} ms", config.network_delay_ms);
        println!("  Test Duration: {:.2} seconds", elapsed);
        println!("\nTransaction Metrics:");
        println!("  Submitted: {} txs", submitted);
        println!("  Confirmed: {} txs", confirmed);
        println!("  Confirmation Rate: {:.1}%", if submitted > 0 { confirmed as f64 / submitted as f64 * 100.0 } else { 0.0 });
        println!("\nThroughput Metrics:");
        println!("  Submitted TPS: {:.2} TPS", submitted_tps);
        println!("  Confirmed TPS: {:.2} TPS", confirmed_tps);
        println!("  Average Confirmation Time: {:.2} ms", avg_conf_time);
        println!("\nMempool Metrics:");
        println!("  Min Size: {} txs", mempool_min);
        println!("  Avg Size: {:.2} txs", mempool_avg);
        println!("  Max Size: {} txs", mempool_max);
        println!("\nBlock Metrics:");
        println!("  Blocks Produced: {}", blocks);
        println!("  Avg Txs/Block: {:.2}", if blocks > 0 { confirmed as f64 / blocks as f64 } else { 0.0 });
        println!("  Real BPS: {:.2}", blocks as f64 / elapsed);
        println!("========================================\n");
    }
}

// -------------------------------------------------------------------------------------------------
// Transaction Generator with TPS Throttling
// -------------------------------------------------------------------------------------------------

async fn transaction_generator(
    sender: Arc<AsyncMutex<BenchAccount>>,
    receiver: Arc<AsyncMutex<BenchAccount>>,
    mempool: Arc<Mempool>,
    metrics: Arc<E2EMetrics>,
    config: E2ETPSConfig,
    stop_signal: Arc<AtomicBool>,
) {
    let tx_interval = Duration::from_secs_f64(1.0 / config.tps_pressure as f64);
    let network_delay = Duration::from_millis(config.network_delay_ms);

    let mut tx_count = 0;
    while tx_count < config.tx_count && !stop_signal.load(Ordering::Relaxed) {
        // TPS throttling: sleep between transactions
        sleep(tx_interval).await;

        // Check mempool size and pause if over target
        let mempool_size = mempool.size().await;
        metrics.record_mempool_size(mempool_size).await;

        if mempool_size >= config.mempool_target as usize {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!("Mempool full ({} txs), pausing submission", mempool_size);
            }
            sleep(Duration::from_millis(100)).await;
            continue;
        }

        // Build transaction
        let tx = {
            let mut sender_account = sender.lock().await;
            let receiver_account = receiver.lock().await;

            let mut builder_state = AccountStateImpl::from_account(&sender_account);
            let transfer = TransferBuilder {
                asset: TOS_ASSET,
                amount: config.transfer_amount * COIN_VALUE,
                destination: receiver_account.keypair.get_public_key().compress().to_address(false),
                extra_data: None,
            };

            let tx = TransactionBuilder::new(
                TxVersion::T0,
                sender_account.keypair.get_public_key().compress(),
                None,
                TransactionTypeBuilder::Transfers(vec![transfer]),
                FeeBuilder::Value(config.fee),
            )
            .with_fee_type(FeeType::TOS)
            .build(&mut builder_state, &sender_account.keypair)
            .expect("build transaction");

            // Update sender state
            sender_account.debit(&TOS_ASSET, config.transfer_amount * COIN_VALUE + config.fee).expect("debit");
            sender_account.nonce += 1;

            Arc::new(tx)
        };

        let tx_hash = tx.hash();
        let submitted_at = Instant::now();

        // Network latency simulation
        let mempool_clone = mempool.clone();
        let metrics_clone = metrics.clone();
        tokio::spawn(async move {
            sleep(network_delay).await;

            let tracked = TrackedTransaction {
                transaction: tx,
                hash: tx_hash,
                submitted_at,
                confirmed_at: None,
                block_height: None,
            };

            let _ = mempool_clone.submit(tracked).await;
            metrics_clone.submit_tx();
        });

        tx_count += 1;

        if tx_count % 100 == 0 && log::log_enabled!(log::Level::Info) {
            log::info!("Submitted {} transactions, mempool size: {}", tx_count, mempool_size);
        }
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!("Transaction generator finished: {} txs submitted", tx_count);
    }
}

// -------------------------------------------------------------------------------------------------
// Block Producer (Simulates Mining at 1 BPS)
// -------------------------------------------------------------------------------------------------

async fn block_producer(
    mempool: Arc<Mempool>,
    verification_state: Arc<RwLock<VerificationState>>,
    metrics: Arc<E2EMetrics>,
    config: E2ETPSConfig,
    stop_signal: Arc<AtomicBool>,
) {
    let block_interval = Duration::from_millis(config.block_time_ms);
    let cache = NoZKPCache;

    let mut block_height = 0u64;

    while !stop_signal.load(Ordering::Relaxed) {
        sleep(block_interval).await;

        // Take transactions from mempool (simulate max block size)
        let max_txs_per_block = 1000; // Simulate ~2MB block with 2KB avg tx size
        let txs = mempool.take_batch(max_txs_per_block).await;

        if txs.is_empty() {
            continue;
        }

        // Verify and confirm transactions
        let mut state = verification_state.write().await;
        let mut confirmed_count = 0;
        let block_time = Instant::now();

        for mut tx in txs {
            match tx.transaction.verify(&tx.hash, &mut *state, &cache).await {
                Ok(_) => {
                    tx.confirmed_at = Some(Instant::now());
                    tx.block_height = Some(block_height);

                    let conf_time = tx.confirmed_at.unwrap().duration_since(tx.submitted_at).as_millis() as u64;
                    metrics.confirm_tx(conf_time);

                    confirmed_count += 1;
                }
                Err(_e) => {
                    // Transaction verification failed, skip
                    continue;
                }
            }
        }

        block_height += 1;
        metrics.produce_block();

        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Block {} produced: {} txs confirmed in {:.2} ms",
                block_height,
                confirmed_count,
                block_time.elapsed().as_millis()
            );
        }
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!("Block producer finished: {} blocks produced", block_height);
    }
}

// -------------------------------------------------------------------------------------------------
// Main End-to-End TPS Benchmark
// -------------------------------------------------------------------------------------------------

async fn run_e2e_tps_benchmark(config: E2ETPSConfig) -> Arc<E2EMetrics> {
    // Initialize accounts with sufficient balance
    let total_cost = (config.transfer_amount * COIN_VALUE + config.fee) * config.tx_count as u64;
    let sender = Arc::new(AsyncMutex::new(BenchAccount::new_with_balance(total_cost + 100 * COIN_VALUE)));
    let receiver = Arc::new(AsyncMutex::new(BenchAccount::new_with_balance(0)));

    // Initialize mempool
    let mempool = Arc::new(Mempool::new(config.mempool_target * 2));

    // Initialize verification state
    let sender_snapshot = sender.lock().await.clone();
    let receiver_snapshot = receiver.lock().await.clone();
    let verification_state = Arc::new(RwLock::new(VerificationState::from_accounts(&[sender_snapshot, receiver_snapshot])));

    // Initialize metrics
    let metrics = Arc::new(E2EMetrics::new());

    // Stop signal
    let stop_signal = Arc::new(AtomicBool::new(false));

    // Spawn transaction generator
    let tx_gen_handle = {
        let sender = sender.clone();
        let receiver = receiver.clone();
        let mempool = mempool.clone();
        let metrics = metrics.clone();
        let config = config.clone();
        let stop_signal = stop_signal.clone();
        tokio::spawn(async move {
            transaction_generator(sender, receiver, mempool, metrics, config, stop_signal).await;
        })
    };

    // Spawn block producer
    let block_prod_handle = {
        let mempool = mempool.clone();
        let verification_state = verification_state.clone();
        let metrics = metrics.clone();
        let config = config.clone();
        let stop_signal = stop_signal.clone();
        tokio::spawn(async move {
            block_producer(mempool, verification_state, metrics, config, stop_signal).await;
        })
    };

    // Wait for test duration
    sleep(Duration::from_secs(config.test_duration_secs)).await;

    // Signal stop
    stop_signal.store(true, Ordering::Relaxed);

    // Wait for tasks to complete
    let _ = tokio::join!(tx_gen_handle, block_prod_handle);

    // Wait for mempool to drain
    sleep(Duration::from_secs(2)).await;

    metrics
}

fn build_runtime() -> Runtime {
    Builder::new_multi_thread().worker_threads(4).enable_all().build().expect("tokio runtime")
}

// -------------------------------------------------------------------------------------------------
// Criterion Benchmark Entry Point
// -------------------------------------------------------------------------------------------------

fn bench_e2e_tps_quick(c: &mut Criterion) {
    // Quick test configuration (10 seconds, 100 TPS)
    let config = E2ETPSConfig {
        tx_count: 1_000,
        tps_pressure: 100,
        mempool_target: 500,
        block_time_ms: 1000,
        network_delay_ms: 200,
        test_duration_secs: 10,
        transfer_amount: 50,
        fee: 5_000,
    };

    let runtime = build_runtime();

    c.bench_function("e2e_tps_quick_10s_100tps", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let metrics = run_e2e_tps_benchmark(config.clone()).await;
                metrics.print_report(&config).await;
            });
        });
    });
}

fn bench_e2e_tps_standard(c: &mut Criterion) {
    // Standard test configuration (60 seconds, 100 TPS)
    let config = E2ETPSConfig::default();

    let runtime = build_runtime();

    c.bench_function("e2e_tps_standard_60s_100tps", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let metrics = run_e2e_tps_benchmark(config.clone()).await;
                metrics.print_report(&config).await;
            });
        });
    });
}

fn bench_e2e_tps_high_pressure(c: &mut Criterion) {
    // High pressure test (30 seconds, 500 TPS)
    let config = E2ETPSConfig {
        tx_count: 15_000,
        tps_pressure: 500,
        mempool_target: 2000,
        block_time_ms: 1000,
        network_delay_ms: 200,
        test_duration_secs: 30,
        transfer_amount: 50,
        fee: 5_000,
    };

    let runtime = build_runtime();

    c.bench_function("e2e_tps_high_pressure_30s_500tps", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let metrics = run_e2e_tps_benchmark(config.clone()).await;
                metrics.print_report(&config).await;
            });
        });
    });
}

criterion_group! {
    name = e2e_tps_benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(10));
    targets = bench_e2e_tps_quick
}

criterion_group! {
    name = e2e_tps_standard_benches;
    config = Criterion::default()
        .sample_size(5)
        .measurement_time(Duration::from_secs(60));
    targets = bench_e2e_tps_standard
}

criterion_group! {
    name = e2e_tps_high_pressure_benches;
    config = Criterion::default()
        .sample_size(5)
        .measurement_time(Duration::from_secs(30));
    targets = bench_e2e_tps_high_pressure
}

criterion_main!(e2e_tps_benches);
