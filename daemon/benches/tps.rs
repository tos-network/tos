//! TOS pipeline benchmark.
//!
//! Builds real transfer transactions and replays them through three execution
//! stages so we can observe the extra work introduced by proof verification and
//! disk persistence:
//! 1. `execution_only` – simple ledger updates (upper bound).
//! 2. `execution_with_proofs` – full `Transaction::verify`, including ZK proof checks.
//! 3. `execution_with_proofs_and_storage` – verification plus RocksDB writes that
//!    approximate block persistence overhead.
//!
//! Note: Balance simplification improves TPS by removing encryption/proof overhead.

#![allow(clippy::enum_variant_names)]
#![allow(clippy::disallowed_methods)]

use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rocksdb::{WriteBatch, DB};
use tempdir::TempDir;
use tokio::runtime::{Builder, Runtime};

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tos_common::{
    account::Nonce,
    block::BlockVersion,
    config::{COIN_VALUE, TOS_ASSET},
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey, KeyPair},
        Hash, Hashable,
    },
    network::Network,
    transaction::{
        builder::{
            AccountState, FeeBuilder, FeeHelper, TransactionBuilder, TransactionTypeBuilder,
            TransferBuilder,
        },
        verify::NoZKPCache,
        FeeType, MultiSigPayload, Reference, Transaction, TransactionType, TxVersion,
    },
};
use tos_kernel::{Environment, Module};

// -------------------------------------------------------------------------------------------------
// Transaction construction helpers
// -------------------------------------------------------------------------------------------------

// Balance simplification: Removed CiphertextCache (plaintext u64 only)
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
        Self {
            keypair,
            balances,
            nonce: 0,
        }
    }

    fn update_from_state(&mut self, state: &AccountStateImpl) {
        self.balances = state.balances.clone();
        self.nonce = state.nonce;
    }

    fn credit(&mut self, asset: &Hash, value: u64) {
        let entry = self
            .balances
            .entry(asset.clone())
            .or_insert_with(|| BalanceEntry { amount: 0 });
        entry.amount = entry.amount.saturating_add(value);
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
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
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
        Ok(self
            .balances
            .get(asset)
            .map(|b| b.amount)
            .unwrap_or_default())
    }

    fn get_reference(&self) -> Reference {
        self.reference.clone()
    }

    fn update_account_balance(
        &mut self,
        asset: &Hash,
        new_balance: u64,
    ) -> Result<(), Self::Error> {
        self.balances.insert(
            asset.clone(),
            BalanceEntry {
                amount: new_balance,
            },
        );
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
// Minimal ledger used by the execution_only stage
// -------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct ExecutionLedger {
    balances: HashMap<CompressedPublicKey, HashMap<Hash, u64>>,
    nonces: HashMap<CompressedPublicKey, Nonce>,
}

impl ExecutionLedger {
    fn from_accounts(accounts: &[BenchAccount]) -> Self {
        let balances = accounts
            .iter()
            .map(|account| {
                let map = account
                    .balances
                    .iter()
                    .map(|(asset, balance)| (asset.clone(), balance.amount))
                    .collect();
                (account.keypair.get_public_key().compress(), map)
            })
            .collect();
        let nonces = accounts
            .iter()
            .map(|account| (account.keypair.get_public_key().compress(), account.nonce))
            .collect();
        Self { balances, nonces }
    }

    fn apply_transaction(&mut self, tx: &Transaction, amount: u64) -> Result<(), &'static str> {
        let sender = tx.get_source();
        let expected_nonce = *self.nonces.get(sender).unwrap_or(&0);
        if tx.get_nonce() != expected_nonce {
            return Err("invalid nonce");
        }

        let sender_balances = self
            .balances
            .get_mut(sender)
            .ok_or("missing sender balance")?;
        let sender_balance = sender_balances.entry(TOS_ASSET).or_insert(0);
        let total_cost = amount.checked_add(tx.get_fee()).ok_or("overflow")?;
        if *sender_balance < total_cost {
            return Err("insufficient balance");
        }
        *sender_balance -= total_cost;

        if let TransactionType::Transfers(transfers) = tx.get_data() {
            if transfers.is_empty() {
                return Err("empty transfers");
            }
            let per_transfer = amount / transfers.len() as u64;
            for transfer in transfers {
                let dest_balances = self
                    .balances
                    .entry(transfer.get_destination().clone())
                    .or_default();
                *dest_balances
                    .entry(transfer.get_asset().clone())
                    .or_insert(0) += per_transfer;
            }
        } else {
            return Err("unsupported transaction type");
        }

        self.nonces.insert(sender.clone(), expected_nonce + 1);
        Ok(())
    }
}

// -------------------------------------------------------------------------------------------------
// Simplified verification state implementing blockchain traits
// -------------------------------------------------------------------------------------------------

// Balance simplification: Changed from HashMap<Hash, Ciphertext> to HashMap<Hash, u64>
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
            let balances: HashMap<Hash, u64> = account
                .balances
                .iter()
                .map(|(asset, balance)| (asset.clone(), balance.amount))
                .collect();
            state.accounts.insert(
                account.keypair.get_public_key().compress(),
                VerificationAccountState {
                    balances,
                    nonce: account.nonce,
                },
            );
        }

        state
    }
}

#[async_trait]
impl<'a> tos_common::transaction::verify::BlockchainVerificationState<'a, ()>
    for VerificationState
{
    async fn pre_verify_tx<'b>(&'b mut self, _tx: &Transaction) -> Result<(), ()> {
        Ok(())
    }

    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, ()> {
        self.accounts
            .get_mut(account.as_ref())
            .and_then(|account| account.balances.get_mut(asset.as_ref()))
            .ok_or(())
    }

    async fn get_sender_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
        _reference: &Reference,
    ) -> Result<&'b mut u64, ()> {
        self.accounts
            .get_mut(account.as_ref())
            .and_then(|account| account.balances.get_mut(asset.as_ref()))
            .ok_or(())
    }

    async fn add_sender_output(
        &mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
        _output: u64,
    ) -> Result<(), ()> {
        Ok(())
    }

    async fn get_account_nonce(&mut self, account: &'a CompressedPublicKey) -> Result<Nonce, ()> {
        self.accounts
            .get(account)
            .map(|account| account.nonce)
            .ok_or(())
    }

    async fn account_exists(&mut self, account: &'a CompressedPublicKey) -> Result<bool, ()> {
        Ok(self.accounts.contains_key(account))
    }

    async fn update_account_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        new_nonce: Nonce,
    ) -> Result<(), ()> {
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
        BlockVersion::Nobunaga
    }

    fn get_verification_timestamp(&self) -> u64 {
        // Return current time for benchmarks
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn get_verification_topoheight(&self) -> u64 {
        1000 // Default topoheight for benchmarks
    }

    async fn get_recyclable_tos(&mut self, _account: &'a CompressedPublicKey) -> Result<u64, ()> {
        Ok(0) // No recyclable TOS in benchmarks
    }

    async fn set_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
        config: &MultiSigPayload,
    ) -> Result<(), ()> {
        self.multisig.insert(account.clone(), config.clone());
        Ok(())
    }

    async fn get_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<&MultiSigPayload>, ()> {
        Ok(self.multisig.get(account))
    }

    async fn get_environment(&mut self) -> Result<&Environment, ()> {
        Ok(&self.env)
    }

    async fn set_contract_module(&mut self, hash: &Hash, module: &'a Module) -> Result<(), ()> {
        self.contracts.insert(hash.clone(), module.clone());
        Ok(())
    }

    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, ()> {
        Ok(self.contracts.contains_key(hash))
    }

    async fn get_contract_module_with_environment(
        &self,
        hash: &Hash,
    ) -> Result<(&Module, &Environment), ()> {
        let module = self.contracts.get(hash).ok_or(())?;
        Ok((module, &self.env))
    }

    fn get_network(&self) -> Network {
        Network::Mainnet
    }

    async fn get_receiver_uno_balance<'b>(
        &'b mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, ()> {
        Err(())
    }

    async fn get_sender_uno_balance<'b>(
        &'b mut self,
        _account: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _reference: &Reference,
    ) -> Result<&'b mut Ciphertext, ()> {
        Err(())
    }

    async fn add_sender_uno_output(
        &mut self,
        _account: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _output: Ciphertext,
    ) -> Result<(), ()> {
        Err(())
    }

    // ===== TNS (TOS Name Service) Verification Methods =====

    async fn is_name_registered(&self, _name_hash: &Hash) -> Result<bool, ()> {
        Ok(false)
    }

    async fn account_has_name(&self, _account: &'a CompressedPublicKey) -> Result<bool, ()> {
        Ok(false)
    }

    async fn get_account_name_hash(
        &self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, ()> {
        Ok(None)
    }
}

// -------------------------------------------------------------------------------------------------
// Block generation
// -------------------------------------------------------------------------------------------------

struct GeneratedBlock {
    transactions: Vec<Arc<Transaction>>,
    hashes: Vec<Hash>,
    baseline: ExecutionLedger,
    sender_snapshots: Vec<BenchAccount>,
    receiver_snapshots: Vec<BenchAccount>,
    transfer_amount: u64,
    fee: u64,
}

fn generate_block(tx_count: usize, amount: u64, fee: u64) -> GeneratedBlock {
    let mut sender =
        BenchAccount::new_with_balance(tx_count as u64 * (amount + fee) + 10 * COIN_VALUE);
    let mut receiver = BenchAccount::new_with_balance(0);

    let mut sender_snapshots = Vec::with_capacity(tx_count + 1);
    let mut receiver_snapshots = Vec::with_capacity(tx_count + 1);
    sender_snapshots.push(sender.clone());
    receiver_snapshots.push(receiver.clone());
    let initial_accounts = vec![sender.clone(), receiver.clone()];

    let mut transactions = Vec::with_capacity(tx_count);
    for _ in 0..tx_count {
        let mut builder_state = AccountStateImpl::from_account(&sender);
        let transfer = TransferBuilder {
            asset: TOS_ASSET,
            amount,
            destination: receiver
                .keypair
                .get_public_key()
                .compress()
                .to_address(false),
            extra_data: None,
        };

        let tx = TransactionBuilder::new(
            TxVersion::T1,
            0, // chain_id: 0 for Mainnet (benchmarks use T1 format)
            sender.keypair.get_public_key().compress(),
            None,
            TransactionTypeBuilder::Transfers(vec![transfer]),
            FeeBuilder::Value(fee),
        )
        .with_fee_type(FeeType::TOS)
        .build(&mut builder_state, &sender.keypair)
        .expect("build transaction");

        sender.update_from_state(&builder_state);
        receiver.credit(&TOS_ASSET, amount);

        sender_snapshots.push(sender.clone());
        receiver_snapshots.push(receiver.clone());

        transactions.push(Arc::new(tx));
    }

    let hashes: Vec<Hash> = transactions.iter().map(|tx| tx.hash()).collect();

    GeneratedBlock {
        transactions,
        hashes,
        baseline: ExecutionLedger::from_accounts(&initial_accounts),
        sender_snapshots,
        receiver_snapshots,
        transfer_amount: amount,
        fee,
    }
}

// -------------------------------------------------------------------------------------------------
// Benchmark harness
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum PipelineMode {
    ExecutionOnly,
    ExecutionWithProofs,
    ExecutionWithProofsAndStorage,
}

impl PipelineMode {
    fn label(self) -> &'static str {
        match self {
            PipelineMode::ExecutionOnly => "execution_only",
            PipelineMode::ExecutionWithProofs => "execution_with_proofs",
            PipelineMode::ExecutionWithProofsAndStorage => "execution_with_proofs_and_storage",
        }
    }

    fn persist(self) -> bool {
        matches!(self, PipelineMode::ExecutionWithProofsAndStorage)
    }
}

// Benchmark configuration with environment variable overrides
const DEFAULT_WORKER_THREADS: usize = 4;
const DEFAULT_BATCH_SIZE: usize = 64;
const DEFAULT_TX_COUNTS: &[usize] = &[16, 64, 128, 256, 512];
const DEFAULT_TRANSFER_AMOUNT: u64 = 50; // In TOS coins
const DEFAULT_FEE: u64 = 5_000; // In base units

fn get_worker_threads() -> usize {
    std::env::var("TOS_BENCH_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_WORKER_THREADS)
}

fn get_batch_size() -> usize {
    std::env::var("TOS_BENCH_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_BATCH_SIZE)
}

fn get_sample_size() -> usize {
    std::env::var("TOS_BENCH_SAMPLE_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20)
}

fn get_measurement_time() -> u64 {
    std::env::var("TOS_BENCH_MEASUREMENT_TIME")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10)
}

fn build_runtime() -> Runtime {
    Builder::new_multi_thread()
        .worker_threads(get_worker_threads())
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// Print performance metrics for a benchmark run (available for custom analysis)
#[allow(dead_code)]
fn print_performance_stats(mode: &str, tx_count: usize, duration: Duration) {
    let tx_count_f64 = tx_count as f64;
    let duration_secs = duration.as_secs_f64();
    let tps = tx_count_f64 / duration_secs;
    let latency_ms = (duration_secs * 1000.0) / tx_count_f64;

    println!(
        "[{mode}] tx_count={tx_count} | TPS={tps:.2} | avg_latency={latency_ms:.3}ms | total_time={duration_secs:.3}s"
    );
}

fn run_pipeline(c: &mut Criterion, mode: PipelineMode) {
    let mut group = c.benchmark_group(mode.label());
    group.sample_size(get_sample_size());
    group.measurement_time(Duration::from_secs(get_measurement_time()));

    for &tx_count in DEFAULT_TX_COUNTS {
        let GeneratedBlock {
            transactions,
            hashes,
            baseline,
            sender_snapshots,
            receiver_snapshots,
            transfer_amount,
            fee,
        } = generate_block(tx_count, DEFAULT_TRANSFER_AMOUNT * COIN_VALUE, DEFAULT_FEE);

        group.throughput(Throughput::Elements(tx_count as u64));

        match mode {
            PipelineMode::ExecutionOnly => {
                group.bench_with_input(BenchmarkId::from_parameter(tx_count), &tx_count, |b, _| {
                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;
                        for _ in 0..iters {
                            let mut state = baseline.clone();
                            let start = Instant::now();
                            for tx in &transactions {
                                state
                                    .apply_transaction(tx, transfer_amount)
                                    .expect("baseline");
                            }
                            total += start.elapsed();
                        }
                        total
                    });
                });
            }
            _ => {
                group.bench_with_input(BenchmarkId::from_parameter(tx_count), &tx_count, |b, _| {
                    // Reuse runtime across iterations for better performance
                    let runtime = build_runtime();

                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;

                        for iter_idx in 0..iters {
                            let temp_dir = mode
                                .persist()
                                .then(|| TempDir::new("tos-bench").expect("temp dir"));
                            let db = temp_dir
                                .as_ref()
                                .map(|dir| DB::open_default(dir.path()).expect("open RocksDB"));

                            let start = Instant::now();
                            runtime.block_on(async {
                                // Pre-allocate futures vector with exact capacity
                                let batch_size = get_batch_size();
                                let num_batches = transactions.len().div_ceil(batch_size);
                                let mut futures = Vec::with_capacity(num_batches);

                                // Process transactions in batches
                                for batch_idx in 0..num_batches {
                                    let start_idx = batch_idx * batch_size;
                                    let end_idx = (start_idx + batch_size).min(transactions.len());

                                    // Share transaction and hash data via Arc (already Arc<Transaction>)
                                    let tx_chunk: Vec<_> =
                                        transactions[start_idx..end_idx].to_vec();
                                    let hash_chunk: Vec<_> = hashes[start_idx..end_idx].to_vec();

                                    // Clone account states only once per batch
                                    let sender_state = sender_snapshots[start_idx].clone();
                                    let receiver_state = receiver_snapshots[start_idx].clone();

                                    futures.push(async move {
                                        let cache = NoZKPCache;
                                        let mut state = VerificationState::from_accounts(&[
                                            sender_state,
                                            receiver_state,
                                        ]);

                                        // Verify all transactions in this batch sequentially
                                        for (tx, hash) in tx_chunk.iter().zip(hash_chunk.iter()) {
                                            tx.verify(hash, &mut state, &cache)
                                                .await
                                                .expect("tx verify");
                                        }
                                    });
                                }

                                // Execute all batches in parallel
                                futures::future::join_all(futures).await;
                            });

                            if let Some(db) = db.as_ref() {
                                // Use batched writes for better performance
                                let mut batch = WriteBatch::default();
                                // Pre-allocate key buffer to reduce allocations
                                let iter_prefix = format!("iter:{iter_idx}:hash:");

                                for hash in &hashes {
                                    // Reuse prefix to minimize allocations
                                    let mut key = iter_prefix.clone();
                                    key.push_str(&hash.to_string());

                                    // Pre-allocate value buffer with exact size
                                    let mut value = Vec::with_capacity(32 + 16);
                                    value.extend_from_slice(hash.as_bytes());
                                    value.extend_from_slice(&transfer_amount.to_le_bytes());
                                    value.extend_from_slice(&fee.to_le_bytes());
                                    batch.put(key.as_bytes(), &value);
                                }
                                db.write(batch).expect("db batch write");
                                db.flush().expect("flush RocksDB");
                            }

                            total += start.elapsed();
                        }
                        total
                    });
                });
            }
        }
    }

    group.finish();
}

fn bench_tps(c: &mut Criterion) {
    println!("\n=== TOS TPS Benchmark Configuration ===");
    println!("Worker threads: {}", get_worker_threads());
    println!("Batch size: {}", get_batch_size());
    println!("Sample size: {}", get_sample_size());
    println!("Measurement time: {}s", get_measurement_time());
    println!("Transaction counts: {DEFAULT_TX_COUNTS:?}");
    println!("Transfer amount: {DEFAULT_TRANSFER_AMOUNT} TOS");
    println!("Fee: {DEFAULT_FEE} base units");
    println!("\nEnvironment variables:");
    println!("  TOS_BENCH_THREADS={}", get_worker_threads());
    println!("  TOS_BENCH_BATCH_SIZE={}", get_batch_size());
    println!("  TOS_BENCH_SAMPLE_SIZE={}", get_sample_size());
    println!("  TOS_BENCH_MEASUREMENT_TIME={}", get_measurement_time());
    println!("========================================\n");

    run_pipeline(c, PipelineMode::ExecutionOnly);
    run_pipeline(c, PipelineMode::ExecutionWithProofs);
    run_pipeline(c, PipelineMode::ExecutionWithProofsAndStorage);
}

criterion_group!(tps_benches, bench_tps);
criterion_main!(tps_benches);
