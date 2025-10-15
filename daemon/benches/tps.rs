//! TOS pipeline benchmark.
//!
//! Builds real transfer transactions and replays them through three execution
//! stages so we can observe the extra work introduced by proof verification and
//! disk persistence:
//! 1. `execution_only` – simple ledger updates (upper bound).
//! 2. `execution_with_proofs` – full `Transaction::verify`, including ZK proof checks.
//! 3. `execution_with_proofs_and_storage` – verification plus RocksDB writes that
//!    approximate block persistence overhead.

use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rocksdb::DB;
use tempdir::TempDir;
use tokio::runtime::{Builder, Runtime};

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tos_common::{
    account::{CiphertextCache, Nonce},
    block::BlockVersion,
    config::{COIN_VALUE, TOS_ASSET},
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey, KeyPair},
        Hash, Hashable,
    },
    transaction::{
        builder::{AccountState, FeeBuilder, FeeHelper, TransactionBuilder, TransactionTypeBuilder, TransferBuilder},
        verify::NoZKPCache,
        FeeType, MultiSigPayload, Reference, Transaction, TransactionType, TxVersion,
    },
};
use tos_vm::{Environment, Module};

// -------------------------------------------------------------------------------------------------
// Transaction construction helpers
// -------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct BalanceEntry {
    cache: CiphertextCache,
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
        balances.insert(
            TOS_ASSET,
            BalanceEntry { amount, cache: CiphertextCache::Decompressed(keypair.get_public_key().encrypt(amount)) },
        );
        Self { keypair, balances, nonce: 0 }
    }

    fn update_from_state(&mut self, state: &AccountStateImpl) {
        self.balances = state.balances.clone();
        self.nonce = state.nonce;
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
    fn is_mainnet(&self) -> bool { false }

    fn get_account_balance(&self, asset: &Hash) -> Result<u64, Self::Error> {
        Ok(self.balances.get(asset).map(|b| b.amount).unwrap_or_default())
    }

    fn get_reference(&self) -> Reference { self.reference.clone() }

    fn get_account_ciphertext(&self, asset: &Hash) -> Result<CiphertextCache, Self::Error> {
        Ok(self
            .balances
            .get(asset)
            .map(|b| b.cache.clone())
            .unwrap_or_else(|| CiphertextCache::Decompressed(Ciphertext::zero())))
    }

    fn update_account_balance(&mut self, asset: &Hash, new_balance: u64, ciphertext: Ciphertext) -> Result<(), Self::Error> {
        self.balances.insert(
            asset.clone(),
            BalanceEntry { amount: new_balance, cache: CiphertextCache::Decompressed(ciphertext) },
        );
        Ok(())
    }

    fn get_nonce(&self) -> Result<u64, Self::Error> { Ok(self.nonce) }

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

        let sender_balances = self.balances.get_mut(sender).ok_or("missing sender balance")?;
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
                    .or_insert_with(HashMap::new);
                *dest_balances.entry(transfer.get_asset().clone()).or_insert(0) += per_transfer;
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

#[derive(Clone)]
struct VerificationAccountState {
    balances: HashMap<Hash, Ciphertext>,
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
            let mut balances = HashMap::new();
            for (asset, balance) in &account.balances {
                let mut cache = balance.cache.clone();
                let ciphertext = cache.computable().expect("ciphertext").clone();
                balances.insert(asset.clone(), ciphertext);
            }
            state.accounts.insert(
                account.keypair.get_public_key().compress(),
                VerificationAccountState { balances, nonce: account.nonce },
            );
        }

        state
    }
}

#[async_trait]
impl<'a> tos_common::transaction::verify::BlockchainVerificationState<'a, ()> for VerificationState {
    async fn pre_verify_tx<'b>(&'b mut self, _tx: &Transaction) -> Result<(), ()> {
        Ok(())
    }

    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, ()> {
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
    ) -> Result<&'b mut Ciphertext, ()> {
        self.accounts
            .get_mut(account)
            .and_then(|account| account.balances.get_mut(asset))
            .ok_or(())
    }

    async fn add_sender_output(&mut self, _account: &'a CompressedPublicKey, _asset: &'a Hash, _output: Ciphertext) -> Result<(), ()> {
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
// Block generation
// -------------------------------------------------------------------------------------------------

struct GeneratedBlock {
    transactions: Vec<Arc<Transaction>>,
    hashes: Vec<Hash>,
    baseline: ExecutionLedger,
    verification: VerificationState,
    transfer_amount: u64,
    fee: u64,
}

fn generate_block(tx_count: usize, amount: u64, fee: u64) -> GeneratedBlock {
    let mut sender = BenchAccount::new_with_balance(tx_count as u64 * (amount + fee) + 10 * COIN_VALUE);
    let receiver = BenchAccount::new_with_balance(0);

    let initial_accounts = vec![sender.clone(), receiver.clone()];

    let mut transactions = Vec::with_capacity(tx_count);
    for _ in 0..tx_count {
        let mut builder_state = AccountStateImpl::from_account(&sender);
        let transfer = TransferBuilder {
            asset: TOS_ASSET,
            amount,
            destination: receiver.keypair.get_public_key().compress().to_address(false),
            extra_data: None,
            encrypt_extra_data: true,
        };

        let tx = TransactionBuilder::new(
            TxVersion::T0,
            sender.keypair.get_public_key().compress(),
            None,
            TransactionTypeBuilder::Transfers(vec![transfer]),
            FeeBuilder::Value(fee),
        )
        .with_fee_type(FeeType::TOS)
        .build(&mut builder_state, &sender.keypair)
        .expect("build transaction");

        sender.update_from_state(&builder_state);

        transactions.push(Arc::new(tx));
    }

    let hashes: Vec<Hash> = transactions.iter().map(|tx| tx.hash()).collect();

    GeneratedBlock {
        transactions,
        hashes,
        baseline: ExecutionLedger::from_accounts(&initial_accounts),
        verification: VerificationState::from_accounts(&initial_accounts),
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

    fn persist(self) -> bool { matches!(self, PipelineMode::ExecutionWithProofsAndStorage) }
}

fn build_runtime() -> Runtime {
    Builder::new_current_thread().enable_all().build().expect("tokio runtime")
}

fn run_pipeline(c: &mut Criterion, mode: PipelineMode) {
    let mut group = c.benchmark_group(mode.label());

    for &tx_count in &[16usize, 64, 128, 256] {
        let GeneratedBlock {
            transactions,
            hashes,
            baseline,
            verification,
            transfer_amount,
            fee,
        } = generate_block(tx_count, 50 * COIN_VALUE, 5_000);

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
                                state.apply_transaction(tx, transfer_amount).expect("baseline");
                            }
                            total += start.elapsed();
                        }
                        total
                    });
                });
            }
            _ => {
                group.bench_with_input(BenchmarkId::from_parameter(tx_count), &tx_count, |b, _| {
                    b.iter_custom(|iters| {
                        let runtime = build_runtime();
                        let cache = NoZKPCache;
                        let mut total = Duration::ZERO;

                        for iter_idx in 0..iters {
                            let mut state = verification.clone();
                            let temp_dir =
                                mode.persist().then(|| TempDir::new("tos-bench").expect("temp dir"));
                            let db = temp_dir
                                .as_ref()
                                .map(|dir| DB::open_default(dir.path()).expect("open RocksDB"));
                            let db_ref = db.as_ref();

                            let start = Instant::now();
                            runtime.block_on(async {
                                for (i, tx) in transactions.iter().enumerate() {
                                    tx.verify(&hashes[i], &mut state, &cache)
                                        .await
                                        .expect("tx verify");

                                    if let Some(db) = db_ref {
                                        let key = format!("iter:{}:tx:{}", iter_idx, i);
                                        let mut value = Vec::with_capacity(32 + 16);
                                        value.extend_from_slice(hashes[i].as_bytes());
                                        value.extend_from_slice(&transfer_amount.to_le_bytes());
                                        value.extend_from_slice(&fee.to_le_bytes());
                                        db.put(key.as_bytes(), &value).expect("db put");
                                    }
                                }
                            });

                            if let Some(db) = db.as_ref() {
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
    run_pipeline(c, PipelineMode::ExecutionOnly);
    run_pipeline(c, PipelineMode::ExecutionWithProofs);
    run_pipeline(c, PipelineMode::ExecutionWithProofsAndStorage);
}

criterion_group!(tps_benches, bench_tps);
criterion_main!(tps_benches);
