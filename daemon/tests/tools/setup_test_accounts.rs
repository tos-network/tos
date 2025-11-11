// TOS Test Account Setup Tool
//
// Purpose: Generate test accounts, mine TOS to account A, and distribute to B-E
//
// Usage:
//   cargo run --bin setup_test_accounts -- --output test_accounts.json

use anyhow::Result;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use tos_common::crypto::elgamal::KeyPair;

#[derive(Parser, Debug)]
#[command(name = "setup_test_accounts")]
#[command(about = "Generate test keypairs for devnet testing")]
struct Args {
    /// Output file for keypairs
    #[arg(short, long, default_value = "test_accounts.json")]
    output: PathBuf,

    /// Network (devnet, testnet, mainnet)
    #[arg(short, long, default_value = "devnet")]
    network: String,
}

#[derive(Serialize, Deserialize)]
struct TestAccount {
    name: String,
    address: String,
    // Note: In production, never store private keys in plain text!
    // This is for testing purposes only
    #[serde(skip_serializing_if = "Option::is_none")]
    private_key_hex: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct TestAccounts {
    network: String,
    accounts: Vec<TestAccount>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("TOS Test Account Setup");
    println!("=====================");
    println!("Network: {}", args.network);
    println!();

    let is_mainnet = args.network.to_lowercase() == "mainnet";

    // Generate 5 keypairs (A, B, C, D, E)
    let names = vec!["A (Miner)", "B", "C", "D", "E"];
    let mut accounts = Vec::new();

    for name in names {
        let keypair = KeyPair::new();
        let address = keypair.get_public_key().to_address(is_mainnet);

        // Serialize private key (for testing only!)
        let private_key = keypair.get_private_key();
        let private_key_bytes = private_key.to_bytes();
        let private_key_hex = hex::encode(private_key_bytes);

        println!("Account {name}: {address}");

        accounts.push(TestAccount {
            name: name.to_string(),
            address: address.to_string(),
            private_key_hex: Some(private_key_hex),
        });
    }

    // Save to file
    let test_accounts = TestAccounts {
        network: args.network.clone(),
        accounts,
    };

    let json = serde_json::to_string_pretty(&test_accounts)?;
    fs::write(&args.output, json)?;

    println!();
    println!("✅ Keypairs saved to: {:?}", args.output);
    println!();
    println!("Next steps:");
    println!(
        "1. Start daemon: ./target/release/tos_daemon --network devnet --dir-path ~/tos_devnet/"
    );
    println!("2. Start miner: ./target/release/tos_miner --miner-address <Account A address> --daemon-address 127.0.0.1:8080");
    println!("3. Wait for ~300 blocks to mine (Account A will have 20+ TOS)");
    println!("4. Use tx_generator to transfer from A to B, C, D, E");

    Ok(())
}
