//! Tier 1 TestBlockchain Demo
//!
//! This example demonstrates the core features of the TestBlockchain
//! component for V3.0 Testing Framework Phase 1.
//!
//! Run with: `cargo run --example tier1_blockchain_demo`

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(unused)]
#![allow(clippy::needless_range_loop)]

use std::sync::Arc;
use tos_common::crypto::Hash;
use tos_testing_framework::orchestrator::{Clock, PausedClock, SystemClock};
use tos_testing_framework::tier1_component::{
    AccountState, BlockchainCounters, TestBlockchain, TestBlockchainBuilder, TestTransaction,
};
use tos_testing_framework::utilities::create_temp_rocksdb;

fn create_test_pubkey(id: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = id;
    for i in 1..32 {
        bytes[i] = (id.wrapping_mul(i as u8)).wrapping_add(i as u8);
    }
    Hash::new(bytes)
}

fn create_test_hash(id: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = id;
    Hash::new(bytes)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("=== TOS Testing Framework V3.0 ===");
    println!("Tier 1: TestBlockchain Demo\n");

    // Demo 1: Basic blockchain creation with builder
    demo_1_basic_creation().await?;

    // Demo 2: Transaction submission and mining
    demo_2_transactions().await?;

    // Demo 3: O(1) counters for invariant checking
    demo_3_counters().await?;

    // Demo 4: State equivalence checking
    demo_4_state_equivalence().await?;

    // Demo 5: Clock injection for deterministic testing
    demo_5_clock_injection().await?;

    println!("\nAll demos completed successfully!");

    Ok(())
}

async fn demo_1_basic_creation() -> anyhow::Result<()> {
    println!("--- Demo 1: Basic Blockchain Creation ---");

    // Create blockchain with 10 funded accounts
    let blockchain = TestBlockchainBuilder::new()
        .with_funded_account_count(10)
        .with_default_balance(1_000_000_000_000) // 1000 TOS per account
        .build()
        .await?;

    println!("Created blockchain with 10 funded accounts");
    println!("Genesis height: {}", blockchain.get_tip_height().await?);

    // Check first account balance
    let alice = create_test_pubkey(1);
    let balance = blockchain.get_balance(&alice).await?;
    println!("Alice balance: {} nanoTOS (1000 TOS)", balance);

    // Check counters
    let counters = blockchain.read_counters().await?;
    println!("Total supply: {} nanoTOS", counters.supply);
    println!("Total balances: {} nanoTOS\n", counters.balances_total);

    Ok(())
}

async fn demo_2_transactions() -> anyhow::Result<()> {
    println!("--- Demo 2: Transaction Submission and Mining ---");

    let blockchain = TestBlockchainBuilder::new()
        .with_funded_account_count(2)
        .with_default_balance(10_000_000_000_000) // 10,000 TOS
        .build()
        .await?;

    let alice = create_test_pubkey(1);
    let bob = create_test_pubkey(2);
    let bob_hash = create_test_hash(2);

    println!(
        "Alice initial balance: {} nanoTOS",
        blockchain.get_balance(&alice).await?
    );
    println!("Alice nonce: {}", blockchain.get_nonce(&alice).await?);

    // Create and submit transaction
    let tx = TestTransaction {
        hash: create_test_hash(100),
        sender: alice.clone(),
        recipient: bob_hash,
        amount: 1_000_000_000_000, // 1000 TOS
        fee: 100_000_000,          // 0.1 TOS
        nonce: 1,
    };

    println!(
        "\nSubmitting transaction: {} TOS + {} fee",
        tx.amount / 1_000_000_000,
        tx.fee / 1_000_000_000
    );
    blockchain.submit_transaction(tx).await?;

    // Mine block
    println!("Mining block...");
    let block = blockchain.mine_block().await?;
    println!(
        "Block {} mined with {} transactions",
        block.height,
        block.transactions.len()
    );
    println!("Block reward: {} nanoTOS", block.reward);

    // Check updated balances and nonces
    println!(
        "\nAlice final balance: {} nanoTOS",
        blockchain.get_balance(&alice).await?
    );
    println!("Alice nonce: {}", blockchain.get_nonce(&alice).await?);
    println!(
        "Blockchain height: {}\n",
        blockchain.get_tip_height().await?
    );

    Ok(())
}

async fn demo_3_counters() -> anyhow::Result<()> {
    println!("--- Demo 3: O(1) Counters for Invariant Checking ---");

    let blockchain = TestBlockchainBuilder::new()
        .with_funded_account_count(5)
        .with_default_balance(5_000_000_000_000)
        .build()
        .await?;

    let alice = create_test_pubkey(1);
    let bob_hash = create_test_hash(2);

    // Submit multiple transactions
    for i in 1..=3 {
        let tx = TestTransaction {
            hash: create_test_hash(100 + i),
            sender: alice.clone(),
            recipient: bob_hash.clone(),
            amount: 100_000_000_000, // 100 TOS
            fee: 10_000_000,         // 0.01 TOS
            nonce: i as u64,
        };
        blockchain.submit_transaction(tx).await?;
    }

    // Mine block
    blockchain.mine_block().await?;

    // Read counters (O(1) operation)
    let counters = blockchain.read_counters().await?;

    println!("Blockchain Counters (O(1) reads):");
    println!("  Total supply: {} nanoTOS", counters.supply);
    println!("  Total balances: {} nanoTOS", counters.balances_total);
    println!("  Fees burned: {} nanoTOS", counters.fees_burned);
    println!("  Fees to miner: {} nanoTOS", counters.fees_miner);
    println!("  Fees to treasury: {} nanoTOS", counters.fees_treasury);
    println!("  Rewards emitted: {} nanoTOS", counters.rewards_emitted);

    // Verify invariant: supply = balances_total + fees_burned
    let expected_supply = counters.balances_total + counters.fees_burned as u128;
    println!("\nInvariant check:");
    println!("  balances_total + fees_burned = {}", expected_supply);
    println!("  supply = {}", counters.supply);
    println!("  Match: {}\n", expected_supply == counters.supply);

    Ok(())
}

async fn demo_4_state_equivalence() -> anyhow::Result<()> {
    println!("--- Demo 4: State Equivalence Checking ---");

    // Create two identical blockchains
    let blockchain1 = TestBlockchainBuilder::new()
        .with_funded_account_count(3)
        .with_default_balance(1_000_000_000_000)
        .build()
        .await?;

    let blockchain2 = TestBlockchainBuilder::new()
        .with_funded_account_count(3)
        .with_default_balance(1_000_000_000_000)
        .build()
        .await?;

    // Get state roots
    let state_root1 = blockchain1.state_root().await?;
    let state_root2 = blockchain2.state_root().await?;

    println!("Blockchain 1 state root: {}", state_root1);
    println!("Blockchain 2 state root: {}", state_root2);
    println!("Identical: {}", state_root1 == state_root2);

    // Get full account state for detailed comparison
    let accounts1 = blockchain1.accounts_kv().await?;
    let accounts2 = blockchain2.accounts_kv().await?;

    println!("\nAccount count in blockchain 1: {}", accounts1.len());
    println!("Account count in blockchain 2: {}", accounts2.len());
    println!("Accounts match: {}\n", accounts1 == accounts2);

    Ok(())
}

async fn demo_5_clock_injection() -> anyhow::Result<()> {
    println!("--- Demo 5: Clock Injection for Deterministic Testing ---");

    // Create blockchain with SystemClock (real time)
    println!("Creating blockchain with SystemClock...");
    let blockchain_real = TestBlockchainBuilder::new()
        .with_clock(Arc::new(SystemClock))
        .with_funded_account_count(1)
        .build()
        .await?;

    let clock = blockchain_real.clock();
    let start = clock.now();

    // Wait 100ms
    clock.sleep(tokio::time::Duration::from_millis(100)).await;

    let elapsed = clock.now() - start;
    println!("Elapsed time with SystemClock: {:?}", elapsed);

    // Create blockchain with PausedClock (deterministic time)
    println!("\nCreating blockchain with PausedClock...");
    let paused_clock = Arc::new(PausedClock::new());

    let blockchain_test = TestBlockchainBuilder::new()
        .with_clock(paused_clock.clone())
        .with_funded_account_count(1)
        .build()
        .await?;

    let test_clock = blockchain_test.clock();
    let test_start = test_clock.now();

    // Manually advance time by 1 hour (instant)
    paused_clock
        .advance(tokio::time::Duration::from_secs(3600))
        .await;

    let test_elapsed = test_clock.now() - test_start;
    println!("Elapsed time with PausedClock: {:?}", test_elapsed);
    println!(
        "Time advanced instantly: {}\n",
        test_elapsed == tokio::time::Duration::from_secs(3600)
    );

    Ok(())
}
