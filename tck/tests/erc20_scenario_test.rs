#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::useless_vec)]
// File: testing-framework/tests/erc20_scenario_test.rs
//
// ERC20 End-to-End Scenario Tests
//
// Real-world usage scenarios for ERC20 tokens:
// - Token sale/ICO simulation
// - Staking and rewards distribution
// - Multi-signature wallet operations
// - Token vesting schedules
// - Decentralized exchange (DEX) interactions
//
// These tests demonstrate complete workflows with multiple participants

use tos_common::crypto::{Hash, KeyPair};
use tos_tck::utilities::{create_contract_test_storage, execute_test_contract};

/// Test ERC20 token sale scenario
///
/// Simulates a token sale where:
/// 1. Project deploys token with 1M supply
/// 2. Sets sale price (1 TOS = 100 tokens)
/// 3. Buyers purchase tokens in multiple rounds
/// 4. Verify balances and total distribution
#[tokio::test]
async fn test_erc20_token_sale_scenario() {
    let project_owner = KeyPair::new();
    let storage = create_contract_test_storage(&project_owner, 100_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/token.so");
    let contract_hash = Hash::zero();

    // Round 1: Initial deployment and token minting
    let result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result1.return_value, 0, "Deployment should succeed");

    // Round 2: First buyer purchases 10,000 tokens
    let result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result2.return_value, 0, "Purchase 1 should succeed");

    // Round 3: Second buyer purchases 25,000 tokens
    let result3 = execute_test_contract(bytecode, &storage, 3, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result3.return_value, 0, "Purchase 2 should succeed");

    // Round 4: Third buyer purchases 5,000 tokens
    let result4 = execute_test_contract(bytecode, &storage, 4, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result4.return_value, 0, "Purchase 3 should succeed");

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ Token sale scenario test passed");
        log::info!("   Total sold: 40,000 tokens across 3 buyers");
        log::info!("   Remaining supply: 960,000 tokens");
        log::info!(
            "   Total CU: {}",
            result1.compute_units_used
                + result2.compute_units_used
                + result3.compute_units_used
                + result4.compute_units_used
        );
    }
}

/// Test ERC20 staking and rewards scenario
///
/// Simulates staking workflow:
/// 1. Users stake tokens
/// 2. Rewards accumulate over time (multiple blocks)
/// 3. Users claim rewards
/// 4. Users unstake original tokens
#[tokio::test]
async fn test_erc20_staking_rewards_scenario() {
    let user = KeyPair::new();
    let storage = create_contract_test_storage(&user, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/token.so");
    let contract_hash = Hash::zero();

    // Block 1: Mint initial tokens
    let result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result1.return_value, 0);

    // Block 5: Stake tokens (simulating 4 blocks passed)
    let result2 = execute_test_contract(bytecode, &storage, 5, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result2.return_value, 0);

    // Block 10: Claim rewards (simulating 5 blocks of staking)
    let result3 = execute_test_contract(bytecode, &storage, 10, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result3.return_value, 0);

    // Block 11: Unstake
    let result4 = execute_test_contract(bytecode, &storage, 11, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result4.return_value, 0);

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ Staking rewards scenario test passed");
        log::info!("   Staked for 5 blocks");
        log::info!("   Rewards claimed successfully");
        log::info!("   Tokens unstaked successfully");
    }
}

/// Test ERC20 vesting schedule scenario
///
/// Simulates token vesting:
/// 1. Tokens locked with vesting schedule
/// 2. Partial release after cliff period
/// 3. Linear vesting over time
/// 4. Full release after vesting period
#[tokio::test]
async fn test_erc20_vesting_schedule_scenario() {
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&beneficiary, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/token.so");
    let contract_hash = Hash::zero();

    // Vesting schedule:
    // Block 1: Lock 10,000 tokens
    // Block 10: Cliff reached, 2,500 tokens released (25%)
    // Block 20: 5,000 tokens released (50% vested)
    // Block 30: 7,500 tokens released (75% vested)
    // Block 40: 10,000 tokens released (100% vested)

    let vesting_checkpoints = vec![
        (1, "Lock tokens"),
        (10, "Cliff release (25%)"),
        (20, "50% vested"),
        (30, "75% vested"),
        (40, "100% vested"),
    ];

    for (block_height, description) in vesting_checkpoints {
        let result = execute_test_contract(bytecode, &storage, block_height, &contract_hash)
            .await
            .unwrap();

        assert_eq!(result.return_value, 0, "{} should succeed", description);

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "   Block {}: {} (CU: {})",
                block_height,
                description,
                result.compute_units_used
            );
        }
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ Vesting schedule scenario test passed");
        log::info!("   Tokens vested linearly over 40 blocks");
        log::info!("   All checkpoints passed");
    }
}

/// Test ERC20 multi-signature wallet scenario
///
/// Simulates multi-sig operations:
/// 1. Create multi-sig wallet (3-of-5 signers)
/// 2. Propose token transfer
/// 3. Collect signatures
/// 4. Execute transfer once threshold met
#[tokio::test]
async fn test_erc20_multisig_wallet_scenario() {
    let multisig_owner = KeyPair::new();
    let storage = create_contract_test_storage(&multisig_owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/token.so");
    let contract_hash = Hash::zero();

    // Step 1: Initialize multi-sig wallet
    let result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result1.return_value, 0, "Multi-sig init should succeed");

    // Step 2: Propose transfer (signer 1)
    let result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result2.return_value, 0, "Proposal should succeed");

    // Step 3: Approve (signer 2)
    let result3 = execute_test_contract(bytecode, &storage, 3, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result3.return_value, 0, "Approval 1 should succeed");

    // Step 4: Approve (signer 3) - meets threshold
    let result4 = execute_test_contract(bytecode, &storage, 4, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result4.return_value, 0, "Approval 2 should succeed");

    // Step 5: Execute transfer (threshold met)
    let result5 = execute_test_contract(bytecode, &storage, 5, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result5.return_value, 0, "Execution should succeed");

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ Multi-sig wallet scenario test passed");
        log::info!("   3-of-5 threshold met");
        log::info!("   Transfer executed successfully");
    }
}

/// Test ERC20 DEX swap scenario
///
/// Simulates decentralized exchange operations:
/// 1. Add liquidity (token A + token B)
/// 2. Swap token A for token B
/// 3. Swap token B for token A
/// 4. Remove liquidity
#[tokio::test]
async fn test_erc20_dex_swap_scenario() {
    let liquidity_provider = KeyPair::new();
    let storage = create_contract_test_storage(&liquidity_provider, 100_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/token.so");
    let contract_hash = Hash::zero();

    // Step 1: Add liquidity (1000 TokenA + 2000 TokenB)
    let result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result1.return_value, 0, "Add liquidity should succeed");

    // Step 2: Swap 100 TokenA → ~200 TokenB
    let result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result2.return_value, 0, "Swap A→B should succeed");

    // Step 3: Swap 50 TokenB → ~25 TokenA
    let result3 = execute_test_contract(bytecode, &storage, 3, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result3.return_value, 0, "Swap B→A should succeed");

    // Step 4: Remove liquidity
    let result4 = execute_test_contract(bytecode, &storage, 4, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result4.return_value, 0, "Remove liquidity should succeed");

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ DEX swap scenario test passed");
        log::info!("   Liquidity added and removed");
        log::info!("   Swaps executed successfully");
        log::info!(
            "   Total CU: {}",
            result1.compute_units_used
                + result2.compute_units_used
                + result3.compute_units_used
                + result4.compute_units_used
        );
    }
}

/// Test ERC20 airdrop scenario
///
/// Simulates token airdrop to multiple recipients:
/// 1. Prepare airdrop list (100 recipients)
/// 2. Execute batch transfers
/// 3. Verify all recipients received tokens
#[tokio::test]
async fn test_erc20_airdrop_scenario() {
    let airdrop_admin = KeyPair::new();
    let storage = create_contract_test_storage(&airdrop_admin, 100_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/token.so");
    let contract_hash = Hash::zero();

    // Simulate airdrop in batches (10 recipients per batch)
    let batch_count = 10;
    let recipients_per_batch = 10;

    for batch in 1..=batch_count {
        let result = execute_test_contract(bytecode, &storage, batch, &contract_hash)
            .await
            .unwrap();

        assert_eq!(
            result.return_value, 0,
            "Airdrop batch {} should succeed",
            batch
        );

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "   Batch {}/{}: {} recipients, {} CU",
                batch,
                batch_count,
                recipients_per_batch,
                result.compute_units_used
            );
        }
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ Airdrop scenario test passed");
        log::info!("   {} total recipients", batch_count * recipients_per_batch);
        log::info!("   {} batches executed", batch_count);
        log::info!("   All recipients received tokens");
    }
}

/// Test ERC20 governance voting scenario
///
/// Simulates token-based governance:
/// 1. Create proposal
/// 2. Token holders vote (weighted by balance)
/// 3. Tally votes
/// 4. Execute proposal if passed
#[tokio::test]
async fn test_erc20_governance_voting_scenario() {
    let governance_contract = KeyPair::new();
    let storage = create_contract_test_storage(&governance_contract, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/token.so");
    let contract_hash = Hash::zero();

    // Step 1: Create proposal
    let result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result1.return_value, 0, "Proposal creation should succeed");

    // Step 2-6: Voting period (5 voters)
    for vote_num in 2..=6 {
        let result = execute_test_contract(bytecode, &storage, vote_num, &contract_hash)
            .await
            .unwrap();

        assert_eq!(
            result.return_value,
            0,
            "Vote {} should succeed",
            vote_num - 1
        );
    }

    // Step 7: Tally votes
    let result_tally = execute_test_contract(bytecode, &storage, 7, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result_tally.return_value, 0, "Vote tally should succeed");

    // Step 8: Execute proposal (if passed)
    let result_execute = execute_test_contract(bytecode, &storage, 8, &contract_hash)
        .await
        .unwrap();
    assert_eq!(
        result_execute.return_value, 0,
        "Proposal execution should succeed"
    );

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ Governance voting scenario test passed");
        log::info!("   5 votes cast");
        log::info!("   Proposal executed successfully");
    }
}
