// TOS Transaction Generator Tool
//
// Purpose: Generate and submit batches of signed transactions to devnet/testnet
// for testing parallel transaction execution performance.
//
// Features:
// - Generate N signed transfer transactions
// - Submit via RPC to local/remote daemon
// - Support batch sizes: 10, 20, 50, 100, 200
// - Automatic keypair management
// - Nonce tracking
// - Performance measurement (TPS, latency)
//
// Usage:
//   cargo run --bin tx_generator -- --count 50 --daemon http://127.0.0.1:8080

// Allow disallowed methods in this test tool binary
#![allow(clippy::disallowed_methods)]

use anyhow::{Context, Result};
use clap::Parser;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tos_common::{
    config::TOS_ASSET,
    crypto::{elgamal::KeyPair, Hash, Hashable},
    network::Network,
    serializer::Serializer,
    transaction::{
        builder::{
            AccountState, FeeBuilder, TransactionBuilder, TransactionTypeBuilder, TransferBuilder,
        },
        FeeType, Reference, Transaction, TxVersion,
    },
};

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "tx_generator")]
#[command(about = "Generate and submit transactions to TOS devnet/testnet", long_about = None)]
struct Args {
    /// Number of transactions to generate
    #[arg(short, long, default_value_t = 25)]
    count: usize,

    /// Daemon RPC address
    #[arg(short, long, default_value = "http://127.0.0.1:8080/json_rpc")]
    daemon: String,

    /// Batch size for submission (submit N transactions at once)
    #[arg(short, long, default_value_t = 1)]
    batch_size: usize,

    /// Delay between batches in milliseconds
    #[arg(long, default_value_t = 100)]
    delay_ms: u64,

    /// Use different senders (conflict-free) instead of same sender
    #[arg(long, default_value_t = false)]
    different_senders: bool,

    /// Amount to transfer (in nanoTOS)
    #[arg(short, long, default_value_t = 1000)]
    amount: u64,

    /// Fee per transaction (in nanoTOS)
    #[arg(short, long, default_value_t = 100)]
    fee: u64,

    /// Network (devnet, testnet, mainnet)
    #[arg(short, long, default_value = "devnet")]
    network: String,

    /// Enable verbose logging
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

// ============================================================================
// Transaction Builder State (Minimal for Testing)
// ============================================================================

/// Minimal account state for building test transactions
struct TestAccountState {
    balance: u64,
    nonce: u64,
    is_mainnet: bool,
    reference: Reference,
}

impl TestAccountState {
    fn new(balance: u64, nonce: u64, is_mainnet: bool, reference: Reference) -> Self {
        Self {
            balance,
            nonce,
            is_mainnet,
            reference,
        }
    }
}

impl tos_common::transaction::builder::FeeHelper for TestAccountState {
    type Error = String;

    fn account_exists(&self, _key: &tos_common::crypto::PublicKey) -> Result<bool, Self::Error> {
        Ok(true) // Assume all accounts exist for testing
    }
}

impl AccountState for TestAccountState {
    fn is_mainnet(&self) -> bool {
        self.is_mainnet
    }

    fn get_account_balance(&self, _asset: &Hash) -> Result<u64, Self::Error> {
        Ok(self.balance)
    }

    fn get_reference(&self) -> Reference {
        self.reference.clone()
    }

    fn update_account_balance(
        &mut self,
        _asset: &Hash,
        new_balance: u64,
    ) -> Result<(), Self::Error> {
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

    fn is_account_registered(
        &self,
        _key: &tos_common::crypto::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true) // Assume all accounts are registered for testing
    }
}

// ============================================================================
// RPC Client
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct RpcResponse<T> {
    jsonrpc: String,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Deserialize)]
struct GetInfoResult {
    topoheight: u64,
    stable_blue_score: u64,
    top_block_hash: String,
    // We only need these fields, but daemon returns more
    // Using #[serde(default)] for fields we don't care about
}

struct RpcClient {
    daemon_url: String,
    client: reqwest::Client,
    request_id: std::sync::atomic::AtomicU64,
}

impl RpcClient {
    fn new(daemon_url: String) -> Self {
        Self {
            daemon_url,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            request_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.request_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("RPC Request: {method} with params: {params}");
        }

        let request = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id(),
            method: method.to_string(),
            params,
        };

        let response = self
            .client
            .post(&self.daemon_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send RPC request")?;

        let rpc_response: RpcResponse<T> = response
            .json()
            .await
            .context("Failed to parse RPC response")?;

        if let Some(error) = rpc_response.error {
            anyhow::bail!("RPC error {}: {}", error.code, error.message);
        }

        rpc_response
            .result
            .ok_or_else(|| anyhow::anyhow!("RPC response missing result"))
    }

    async fn get_info(&self) -> Result<GetInfoResult> {
        // get_info doesn't take parameters - omit params field entirely
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "get_info"
        });

        let response = self
            .client
            .post(&self.daemon_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send RPC request")?;

        let rpc_response: RpcResponse<GetInfoResult> = response
            .json()
            .await
            .context("Failed to parse RPC response")?;

        if let Some(error) = rpc_response.error {
            anyhow::bail!("RPC error {}: {}", error.code, error.message);
        }

        rpc_response
            .result
            .ok_or_else(|| anyhow::anyhow!("RPC response missing result"))
    }

    async fn submit_transaction(&self, tx_hex: String) -> Result<String> {
        self.call("submit_transaction", json!({ "data": tx_hex }))
            .await
    }
}

// ============================================================================
// Transaction Generator
// ============================================================================

struct TransactionGenerator {
    is_mainnet: bool,
    sender_keypairs: Vec<KeyPair>,
    receiver_keypair: KeyPair,
    reference: Reference,
    base_nonce: u64,
}

impl TransactionGenerator {
    fn new(is_mainnet: bool, num_senders: usize) -> Self {
        info!("Generating {num_senders} sender keypairs...");
        let sender_keypairs: Vec<KeyPair> = (0..num_senders).map(|_| KeyPair::new()).collect();

        let receiver_keypair = KeyPair::new();

        info!("Sender addresses:");
        for (i, kp) in sender_keypairs.iter().enumerate() {
            info!(
                "  Sender {}: {}",
                i,
                kp.get_public_key().to_address(is_mainnet)
            );
        }
        info!(
            "Receiver address: {}",
            receiver_keypair.get_public_key().to_address(is_mainnet)
        );

        Self {
            is_mainnet,
            sender_keypairs,
            receiver_keypair,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
            base_nonce: 0,
        }
    }

    fn update_reference(&mut self, reference: Reference) {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Updated reference to topoheight {} hash {}",
                reference.topoheight, reference.hash
            );
        }
        self.reference = reference;
    }

    fn generate_transactions(
        &self,
        count: usize,
        amount: u64,
        fee: u64,
        different_senders: bool,
    ) -> Result<Vec<Transaction>> {
        info!("Generating {count} transactions (different_senders: {different_senders})...");

        let mut transactions = Vec::with_capacity(count);
        let initial_balance = 1_000_000_000_000u64; // 1M TOS = 1e15 nanoTOS

        for i in 0..count {
            // Select sender keypair
            let sender_keypair = if different_senders {
                // Use different sender for each transaction (conflict-free)
                &self.sender_keypairs[i % self.sender_keypairs.len()]
            } else {
                // Use same sender for all transactions (conflicting)
                &self.sender_keypairs[0]
            };

            // Calculate nonce
            let nonce = if different_senders {
                self.base_nonce // Each sender starts from base_nonce
            } else {
                self.base_nonce + i as u64 // Same sender increments nonce
            };

            // Create account state
            let mut state = TestAccountState::new(
                initial_balance,
                nonce,
                self.is_mainnet,
                self.reference.clone(),
            );

            // Build transfer
            let transfer = TransferBuilder {
                asset: TOS_ASSET,
                amount,
                destination: self
                    .receiver_keypair
                    .get_public_key()
                    .to_address(self.is_mainnet),
                extra_data: None,
            };

            // Build transaction
            let tx = TransactionBuilder::new(
                TxVersion::T0,
                sender_keypair.get_public_key().compress(),
                None,
                TransactionTypeBuilder::Transfers(vec![transfer]),
                FeeBuilder::Value(fee),
            )
            .with_fee_type(FeeType::TOS)
            .build(&mut state, sender_keypair)
            .context(format!("Failed to build transaction {i}"))?;

            if log::log_enabled!(log::Level::Trace) {
                debug!("Generated tx {}: hash={}, nonce={}", i, tx.hash(), nonce);
            }

            transactions.push(tx);
        }

        info!("Successfully generated {count} transactions");
        Ok(transactions)
    }
}

// ============================================================================
// Transaction Submitter
// ============================================================================

struct TransactionSubmitter {
    rpc_client: Arc<RpcClient>,
}

impl TransactionSubmitter {
    fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    async fn submit_batch(
        &self,
        transactions: &[Transaction],
        batch_name: &str,
    ) -> Result<Duration> {
        info!(
            "Submitting batch '{}' with {} transactions...",
            batch_name,
            transactions.len()
        );

        let start = Instant::now();
        let mut submitted = 0;
        let mut errors = 0;

        for (i, tx) in transactions.iter().enumerate() {
            // Serialize transaction to hex
            let tx_bytes = tx.to_bytes();
            let tx_hex = hex::encode(&tx_bytes);

            // Submit via RPC
            match self.rpc_client.submit_transaction(tx_hex).await {
                Ok(tx_hash) => {
                    submitted += 1;
                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "  TX {}/{}: {} submitted successfully",
                            i + 1,
                            transactions.len(),
                            tx_hash
                        );
                    }
                }
                Err(e) => {
                    errors += 1;
                    warn!(
                        "  TX {}/{}: submission failed: {}",
                        i + 1,
                        transactions.len(),
                        e
                    );
                }
            }
        }

        let elapsed = start.elapsed();
        let tps = submitted as f64 / elapsed.as_secs_f64();

        info!(
            "Batch '{}' complete: {}/{} submitted, {} errors, {:.2} TPS, {:?} elapsed",
            batch_name,
            submitted,
            transactions.len(),
            errors,
            tps,
            elapsed
        );

        Ok(elapsed)
    }
}

// ============================================================================
// Performance Tracker
// ============================================================================

struct PerformanceTracker {
    total_transactions: usize,
    total_submitted: usize,
    total_errors: usize,
    total_duration: Duration,
    batch_durations: Vec<Duration>,
}

impl PerformanceTracker {
    fn new() -> Self {
        Self {
            total_transactions: 0,
            total_submitted: 0,
            total_errors: 0,
            total_duration: Duration::ZERO,
            batch_durations: Vec::new(),
        }
    }

    fn record_batch(&mut self, tx_count: usize, submitted: usize, duration: Duration) {
        self.total_transactions += tx_count;
        self.total_submitted += submitted;
        self.total_errors += tx_count - submitted;
        self.total_duration += duration;
        self.batch_durations.push(duration);
    }

    fn print_summary(&self) {
        println!("\n{}", "=".repeat(70));
        println!("PERFORMANCE SUMMARY");
        println!("{}", "=".repeat(70));
        println!("Total transactions generated: {}", self.total_transactions);
        println!("Total transactions submitted: {}", self.total_submitted);
        println!("Total errors:                 {}", self.total_errors);
        println!(
            "Success rate:                 {:.2}%",
            (self.total_submitted as f64 / self.total_transactions as f64) * 100.0
        );
        println!("Total duration:               {:?}", self.total_duration);
        println!(
            "Average TPS:                  {:.2}",
            self.total_submitted as f64 / self.total_duration.as_secs_f64()
        );

        if !self.batch_durations.is_empty() {
            let avg_batch =
                self.batch_durations.iter().sum::<Duration>() / self.batch_durations.len() as u32;
            let min_batch = self.batch_durations.iter().min().unwrap();
            let max_batch = self.batch_durations.iter().max().unwrap();
            println!(
                "Batch durations:              min={min_batch:?}, avg={avg_batch:?}, max={max_batch:?}"
            );
        }
        println!("{}", "=".repeat(70));
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    env_logger::Builder::from_default_env()
        .filter_level(log_level)
        .format_timestamp_millis()
        .init();

    info!("TOS Transaction Generator");
    info!("========================");
    info!("Configuration:");
    info!("  Transaction count: {}", args.count);
    info!("  Daemon URL:        {}", args.daemon);
    info!("  Batch size:        {}", args.batch_size);
    info!("  Delay between batches: {}ms", args.delay_ms);
    info!("  Different senders: {}", args.different_senders);
    info!("  Amount per tx:     {} nanoTOS", args.amount);
    info!("  Fee per tx:        {} nanoTOS", args.fee);
    info!("  Network:           {}", args.network);
    info!("");

    // Determine network type
    let is_mainnet = args.network.to_lowercase() == "mainnet";
    let _network = match args.network.to_lowercase().as_str() {
        "devnet" => Network::Devnet,
        "testnet" => Network::Testnet,
        "mainnet" => Network::Mainnet,
        _ => {
            error!("Invalid network: {}", args.network);
            std::process::exit(1);
        }
    };

    // Create RPC client
    let rpc_client = Arc::new(RpcClient::new(args.daemon.clone()));

    // Get chain info
    info!("Fetching chain info from daemon...");
    let chain_info = rpc_client
        .get_info()
        .await
        .context("Failed to get chain info. Is the daemon running?")?;

    info!("Chain info:");
    info!("  Topoheight:    {}", chain_info.topoheight);
    info!("  Stable score:  {}", chain_info.stable_blue_score);
    info!("  Top block:     {}", chain_info.top_block_hash);
    info!("");

    // Create reference from chain info
    let reference = Reference {
        topoheight: chain_info.stable_blue_score,
        hash: Hash::from_hex(&chain_info.top_block_hash).unwrap_or_else(|_| Hash::zero()),
    };

    // Determine number of senders needed
    let num_senders = if args.different_senders {
        args.count // Need one sender per transaction
    } else {
        1 // All transactions from same sender
    };

    // Create transaction generator
    let mut generator = TransactionGenerator::new(is_mainnet, num_senders);
    generator.update_reference(reference);

    // Generate all transactions
    let all_transactions = generator.generate_transactions(
        args.count,
        args.amount,
        args.fee,
        args.different_senders,
    )?;

    // Create submitter and performance tracker
    let submitter = TransactionSubmitter::new(Arc::clone(&rpc_client));
    let mut perf_tracker = PerformanceTracker::new();

    // Submit transactions in batches
    let num_batches = args.count.div_ceil(args.batch_size);
    info!(
        "Submitting {} transactions in {} batches...",
        args.count, num_batches
    );
    info!("");

    for (batch_idx, batch) in all_transactions.chunks(args.batch_size).enumerate() {
        let batch_name = format!("Batch {}/{}", batch_idx + 1, num_batches);

        match submitter.submit_batch(batch, &batch_name).await {
            Ok(duration) => {
                perf_tracker.record_batch(batch.len(), batch.len(), duration);
            }
            Err(e) => {
                error!("Batch submission failed: {e}");
                perf_tracker.record_batch(batch.len(), 0, Duration::ZERO);
            }
        }

        // Delay between batches (except for last batch)
        if batch_idx < num_batches - 1 && args.delay_ms > 0 {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Waiting {}ms before next batch...", args.delay_ms);
            }
            tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
        }
    }

    // Print performance summary
    perf_tracker.print_summary();

    info!("");
    info!("Transaction generation complete!");
    info!("Check daemon logs for parallel execution activity.");
    info!(
        "Look for blocks with {}+ transactions to trigger parallel path.",
        20
    ); // MIN_TXS_FOR_PARALLEL

    Ok(())
}
