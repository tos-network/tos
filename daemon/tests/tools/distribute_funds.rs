// TOS Fund Distribution Tool
//
// Purpose: Distribute funds from account A to accounts B, C, D, E
//
// Usage:
//   cargo run --bin distribute_funds -- --accounts test_accounts.json

use std::fs;
use std::path::PathBuf;
use clap::Parser;
use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use log::{info, error};

use tos_common::{
    config::TOS_ASSET,
    crypto::{Hash, Hashable, PublicKey, elgamal::{KeyPair, PrivateKey}},
    serializer::Serializer,
    transaction::{
        Transaction,
        builder::{TransactionBuilder, TransactionTypeBuilder, TransferBuilder, FeeBuilder, AccountState, FeeHelper},
        FeeType, TxVersion, Reference,
    },
};

#[derive(Parser, Debug)]
#[command(name = "distribute_funds")]
#[command(about = "Distribute funds from account A to B, C, D, E")]
struct Args {
    /// Path to test_accounts.json
    #[arg(short, long, default_value = "test_accounts.json")]
    accounts: PathBuf,

    /// Amount to send to each account (in nanoTOS)
    #[arg(short = 'm', long, default_value_t = 5000000000000)]
    amount: u64,

    /// Fee per transaction (in nanoTOS)
    #[arg(short, long, default_value_t = 120000)]
    fee: u64,

    /// Daemon RPC URL
    #[arg(short, long, default_value = "http://127.0.0.1:8080/json_rpc")]
    daemon: String,

    /// Network (devnet, testnet, mainnet)
    #[arg(short, long, default_value = "devnet")]
    network: String,
}

#[derive(Deserialize)]
struct TestAccount {
    name: String,
    address: String,
    private_key_hex: String,
}

#[derive(Deserialize)]
struct TestAccounts {
    network: String,
    accounts: Vec<TestAccount>,
}

#[derive(Deserialize)]
struct GetInfoResult {
    topoheight: u64,
    stable_blue_score: u64,
    top_block_hash: String,
}

#[derive(Deserialize)]
struct BalanceResult {
    balance: u64,
    topoheight: u64,
}

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

impl FeeHelper for TestAccountState {
    type Error = String;

    fn account_exists(&self, _key: &PublicKey) -> Result<bool, Self::Error> {
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

    fn is_account_registered(&self, _key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(true) // Assume all accounts are registered for testing
    }
}

struct RpcClient {
    client: reqwest::Client,
    daemon_url: String,
    request_id: std::sync::atomic::AtomicU64,
}

impl RpcClient {
    fn new(daemon_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            daemon_url,
            request_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    async fn get_info(&self) -> Result<GetInfoResult> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "get_info"
        });

        let response = self.client
            .post(&self.daemon_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send get_info request")?;

        let body: serde_json::Value = response.json().await.context("Failed to parse get_info response")?;

        if let Some(error) = body.get("error") {
            bail!("RPC error: {}", error);
        }

        let result = body.get("result").context("No result in response")?;
        Ok(serde_json::from_value(result.clone())?)
    }

    async fn get_balance(&self, address: &str) -> Result<u64> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "get_balance",
            "params": {
                "address": address,
                "asset": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        });

        let response = self.client
            .post(&self.daemon_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send get_balance request")?;

        let body: serde_json::Value = response.json().await.context("Failed to parse get_balance response")?;

        if let Some(error) = body.get("error") {
            bail!("RPC error: {}", error);
        }

        let result: BalanceResult = serde_json::from_value(
            body.get("result").context("No result in response")?.clone()
        )?;

        Ok(result.balance)
    }

    async fn submit_transaction(&self, tx: &Transaction) -> Result<String> {
        let tx_hex = hex::encode(tx.to_bytes());

        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "submit_transaction",
            "params": {
                "data": tx_hex
            }
        });

        let response = self.client
            .post(&self.daemon_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send submit_transaction request")?;

        let body: serde_json::Value = response.json().await.context("Failed to parse submit_transaction response")?;

        if let Some(error) = body.get("error") {
            bail!("RPC error: {}", error);
        }

        Ok(tx.hash().to_string())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    info!("TOS Fund Distribution Tool");
    info!("==========================");
    info!("");

    // Load accounts
    let accounts_json = fs::read_to_string(&args.accounts)
        .context("Failed to read test accounts file")?;
    let test_accounts: TestAccounts = serde_json::from_str(&accounts_json)?;

    if test_accounts.accounts.len() < 5 {
        bail!("Need at least 5 accounts (A, B, C, D, E)");
    }

    let is_mainnet = args.network.to_lowercase() == "mainnet";

    // Parse account A (sender)
    let account_a = &test_accounts.accounts[0];
    let private_key_bytes = hex::decode(&account_a.private_key_hex)?;
    if private_key_bytes.len() != 32 {
        bail!("Invalid private key length: {} bytes", private_key_bytes.len());
    }
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&private_key_bytes);
    let private_key = PrivateKey::from_bytes(&key_array)
        .map_err(|_| anyhow::anyhow!("Failed to parse private key"))?;
    let keypair_a = KeyPair::from_private_key(private_key)
        .map_err(|_| anyhow::anyhow!("Failed to create keypair from private key"))?;

    info!("Sender: {} ({})", account_a.name, account_a.address);

    // Connect to daemon
    let rpc = RpcClient::new(args.daemon.clone());
    let chain_info = rpc.get_info().await?;
    info!("Connected to daemon at topoheight {}", chain_info.topoheight);

    // Check account A balance
    let balance_a = rpc.get_balance(&account_a.address).await?;
    let balance_tos = balance_a as f64 / 1_000_000_000_000.0;
    info!("Account A balance: {:.6} TOS ({} nanoTOS)", balance_tos, balance_a);

    // Calculate total needed
    let recipients = &test_accounts.accounts[1..5]; // B, C, D, E
    let total_needed = (args.amount + args.fee) * recipients.len() as u64;
    let total_tos = total_needed as f64 / 1_000_000_000_000.0;

    info!("");
    info!("Distribution plan:");
    info!("  Amount per recipient: {:.6} TOS", args.amount as f64 / 1_000_000_000_000.0);
    info!("  Fee per transaction: {:.6} TOS", args.fee as f64 / 1_000_000_000_000.0);
    info!("  Total needed: {:.6} TOS", total_tos);
    info!("");

    if balance_a < total_needed {
        bail!("Insufficient balance! Have {:.6} TOS, need {:.6} TOS", balance_tos, total_tos);
    }

    // Create reference
    let reference = Reference {
        topoheight: chain_info.topoheight,
        hash: Hash::from_hex(&chain_info.top_block_hash)?,
    };

    // Send transactions
    let mut nonce = 0u64;
    for (i, recipient) in recipients.iter().enumerate() {
        info!("Sending {:.6} TOS to {} ({})...",
            args.amount as f64 / 1_000_000_000_000.0,
            recipient.name,
            recipient.address
        );

        let mut state = TestAccountState::new(balance_a, nonce, is_mainnet, reference.clone());

        let transfer = TransferBuilder {
            asset: TOS_ASSET,
            amount: args.amount,
            destination: recipient.address.parse()?,
            extra_data: None,
        };

        let tx = TransactionBuilder::new(
            TxVersion::T0,
            keypair_a.get_public_key().compress(),
            None,
            TransactionTypeBuilder::Transfers(vec![transfer]),
            FeeBuilder::Value(args.fee),
        )
        .with_fee_type(FeeType::TOS)
        .build(&mut state, &keypair_a)?;

        let tx_hash = rpc.submit_transaction(&tx).await?;
        info!("  ✓ Transaction submitted: {}", tx_hash);

        nonce += 1;

        // Small delay between transactions
        if i < recipients.len() - 1 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    info!("");
    info!("✅ Distribution complete!");
    info!("Wait ~10 seconds for transactions to be included in blocks");

    Ok(())
}
