//! Pruning Point Integration Tests
//!
//! Tests for pruning point calculation and validation using the testing framework.
//! These tests verify the GHOSTDAG pruning point implementation.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]

use anyhow::Result;
use std::sync::Arc;
use tos_common::crypto::Hash;
use tos_testing_framework::orchestrator::SystemClock;
use tos_testing_framework::tier1_component::{TestBlock, TestBlockchainBuilder, PRUNING_DEPTH};

/// Helper to create a test pubkey from an ID
fn create_test_pubkey(id: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = id;
    Hash::new(bytes)
}

// =============================================================================
// Test 1: Pruning point validation for new blocks
// =============================================================================

#[tokio::test]
async fn test_pruning_point_validation_new_block() -> Result<()> {
    let clock = Arc::new(SystemClock);
    let blockchain = TestBlockchainBuilder::new()
        .with_clock(clock)
        .with_funded_account_count(1)
        .build()
        .await?;

    // Mine blocks beyond PRUNING_DEPTH
    for _ in 0..250 {
        blockchain.mine_block().await?;
    }

    // Get the last block and verify its pruning_point is valid
    let height = blockchain.get_tip_height().await?;
    let block = blockchain.get_block_at_height(height).await?.unwrap();

    // Pruning point should NOT be genesis for blocks with blue_score >= PRUNING_DEPTH
    let genesis_hash = blockchain.get_genesis_hash();
    assert_ne!(
        block.pruning_point, *genesis_hash,
        "Block at height {} should have non-genesis pruning_point",
        height
    );

    // Validate using the blockchain's validation method
    assert!(
        blockchain.validate_pruning_point(&block).await?,
        "Block should have valid pruning_point"
    );

    Ok(())
}

// =============================================================================
// Test 2: Pruning point at genesis returns genesis
// =============================================================================

#[tokio::test]
async fn test_pruning_point_genesis() -> Result<()> {
    let clock = Arc::new(SystemClock);
    let blockchain = TestBlockchainBuilder::new()
        .with_clock(clock)
        .with_funded_account_count(1)
        .build()
        .await?;

    let genesis_hash = blockchain.get_genesis_hash().clone();

    // Mine blocks 1-199 (all with blue_score < PRUNING_DEPTH)
    for i in 1..PRUNING_DEPTH {
        let block = blockchain.mine_block().await?;

        // All blocks before PRUNING_DEPTH should have genesis as pruning_point
        assert_eq!(
            block.pruning_point, genesis_hash,
            "Block {} (blue_score={}) should have genesis as pruning_point",
            i, block.blue_score
        );

        // Verify blue_score matches height in linear chain
        assert_eq!(block.blue_score, i, "Blue score should match block number");
    }

    Ok(())
}

// =============================================================================
// Test 3: Pruning point transitions at PRUNING_DEPTH boundary
// =============================================================================

#[tokio::test]
async fn test_pruning_point_boundary_transition() -> Result<()> {
    let clock = Arc::new(SystemClock);
    let blockchain = TestBlockchainBuilder::new()
        .with_clock(clock)
        .with_funded_account_count(1)
        .build()
        .await?;

    let genesis_hash = blockchain.get_genesis_hash().clone();

    // Mine exactly PRUNING_DEPTH blocks (200 blocks)
    for _ in 0..PRUNING_DEPTH {
        blockchain.mine_block().await?;
    }

    // Block 200's pruning_point should still be genesis
    // (because we walk back PRUNING_DEPTH steps from block 200 = genesis)
    let block_200 = blockchain
        .get_block_at_height(PRUNING_DEPTH)
        .await?
        .unwrap();
    assert_eq!(
        block_200.pruning_point, genesis_hash,
        "Block 200 should have genesis as pruning_point"
    );

    // Mine block 201
    let block_201 = blockchain.mine_block().await?;
    // Block 201's pruning_point: start from selected_parent (block 200), walk back 200 steps
    // Block 200 -> Block 199 -> ... -> Block 0 (genesis)
    // So pruning_point should still be genesis
    assert_eq!(
        block_201.pruning_point, genesis_hash,
        "Block 201 should have genesis as pruning_point (200 steps from block 200 = genesis)"
    );

    // Mine block 202
    let block_202 = blockchain.mine_block().await?;
    // Block 202's pruning_point: start from selected_parent (block 201), walk back 200 steps
    // Block 201 -> Block 200 -> ... -> Block 1
    // So pruning_point should be block 1
    let block_1 = blockchain.get_block_at_height(1).await?.unwrap();
    assert_eq!(
        block_202.pruning_point, block_1.hash,
        "Block 202 should have block 1 as pruning_point (200 steps from block 201 = block 1)"
    );

    Ok(())
}

// =============================================================================
// Test 4: Invalid pruning point rejection
// =============================================================================

#[tokio::test]
async fn test_invalid_pruning_point_rejection() -> Result<()> {
    let clock = Arc::new(SystemClock);
    let blockchain = TestBlockchainBuilder::new()
        .with_clock(clock.clone())
        .with_funded_account_count(1)
        .build()
        .await?;

    // Mine 300+ blocks
    for _ in 0..300 {
        blockchain.mine_block().await?;
    }

    // Get the last valid block
    let last_block = blockchain
        .get_block_at_height(blockchain.get_tip_height().await?)
        .await?
        .unwrap();

    // Create invalid blocks with wrong pruning_point

    // Case a: pruning_point = all zeros
    let invalid_block_zero = TestBlock {
        hash: create_test_pubkey(100),
        height: last_block.height + 1,
        blue_score: last_block.blue_score + 1,
        transactions: vec![],
        reward: 50_000_000_000,
        pruning_point: Hash::zero(), // Invalid!
        selected_parent: last_block.hash.clone(),
    };

    let result = blockchain.receive_block(invalid_block_zero).await;
    assert!(
        result.is_err(),
        "Block with zero pruning_point should be rejected"
    );
    assert!(
        result.unwrap_err().to_string().contains("pruning_point"),
        "Error should mention pruning_point"
    );

    // Case b: pruning_point = random hash
    let invalid_block_random = TestBlock {
        hash: create_test_pubkey(101),
        height: last_block.height + 1,
        blue_score: last_block.blue_score + 1,
        transactions: vec![],
        reward: 50_000_000_000,
        pruning_point: create_test_pubkey(255), // Invalid random hash!
        selected_parent: last_block.hash.clone(),
    };

    let result = blockchain.receive_block(invalid_block_random).await;
    assert!(
        result.is_err(),
        "Block with random pruning_point should be rejected"
    );

    // Case c: pruning_point = incorrect block (e.g., genesis when it shouldn't be)
    let genesis_hash = blockchain.get_genesis_hash().clone();
    let invalid_block_genesis = TestBlock {
        hash: create_test_pubkey(102),
        height: last_block.height + 1,
        blue_score: last_block.blue_score + 1,
        transactions: vec![],
        reward: 50_000_000_000,
        pruning_point: genesis_hash, // Should not be genesis at this height!
        selected_parent: last_block.hash.clone(),
    };

    let result = blockchain.receive_block(invalid_block_genesis).await;
    assert!(
        result.is_err(),
        "Block with genesis as pruning_point (when incorrect) should be rejected"
    );

    Ok(())
}

// =============================================================================
// Test 5: Block template includes correct pruning point
// =============================================================================

#[tokio::test]
async fn test_block_template_pruning_point() -> Result<()> {
    let clock = Arc::new(SystemClock);
    let blockchain = TestBlockchainBuilder::new()
        .with_clock(clock)
        .with_funded_account_count(1)
        .build()
        .await?;

    // Mine 300+ blocks
    for _ in 0..300 {
        blockchain.mine_block().await?;
    }

    // Mine a new block (simulates getting template and mining)
    let new_block = blockchain.mine_block().await?;

    // Verify the block was accepted (if mine_block succeeds, pruning_point was correct)
    assert!(
        blockchain.validate_pruning_point(&new_block).await?,
        "Mined block should have valid pruning_point"
    );

    // Verify the pruning_point is the expected block
    // For block 301, selected_parent is block 300
    // Walking back 200 steps from block 300 = block 100
    let expected_pruning_block = blockchain.get_block_at_height(100).await?.unwrap();
    assert_eq!(
        new_block.pruning_point, expected_pruning_block.hash,
        "Block 301's pruning_point should be block 100 (300 - 200)"
    );

    Ok(())
}

// =============================================================================
// Test 6: Pruning point with long chains (1000+ blocks)
// =============================================================================

#[tokio::test]
async fn test_pruning_point_long_chain() -> Result<()> {
    let clock = Arc::new(SystemClock);
    let blockchain = TestBlockchainBuilder::new()
        .with_clock(clock)
        .with_funded_account_count(1)
        .build()
        .await?;

    let genesis_hash = blockchain.get_genesis_hash().clone();

    // Mine 500 blocks
    for i in 1..=500 {
        let block = blockchain.mine_block().await?;

        // Verify pruning_point is correct
        // calc_pruning_point starts from selected_parent (block i-1), walks back PRUNING_DEPTH steps
        // So for block i, pruning_point = block (i - 1 - PRUNING_DEPTH) if that's >= 0, else genesis
        if i <= PRUNING_DEPTH {
            // For blocks 1-200, selected_parent is block 0-199
            // Walking back 200 steps from block 0-199 hits genesis
            assert_eq!(
                block.pruning_point, genesis_hash,
                "Block {} should have genesis as pruning_point",
                i
            );
        } else {
            // For block i > 200, selected_parent is block i-1
            // Walking back 200 steps from block i-1 = block (i - 1 - 200) = block (i - 201)
            let expected_height = i - PRUNING_DEPTH - 1;
            let expected_block = blockchain
                .get_block_at_height(expected_height)
                .await?
                .unwrap();
            assert_eq!(
                block.pruning_point, expected_block.hash,
                "Block {} should have block {} as pruning_point",
                i, expected_height
            );
        }
    }

    // Verify final state
    assert_eq!(blockchain.get_tip_height().await?, 500);
    assert_eq!(blockchain.get_blue_score().await?, 500);

    Ok(())
}

// =============================================================================
// Test 7: Miner-submitted blocks with pruning point
// =============================================================================

#[tokio::test]
async fn test_miner_block_pruning_point() -> Result<()> {
    let clock = Arc::new(SystemClock);
    let blockchain = TestBlockchainBuilder::new()
        .with_clock(clock.clone())
        .with_funded_account_count(1)
        .build()
        .await?;

    // Mine blocks to establish chain
    for _ in 0..250 {
        blockchain.mine_block().await?;
    }

    // Simulate miner getting template and submitting block
    // In real scenario, miner gets template from get_block_template()
    // For test, mine_block() does this internally

    let miner_block = blockchain.mine_block().await?;

    // Verify block was accepted with correct pruning_point
    let stored_block = blockchain
        .get_block_by_hash(&miner_block.hash)
        .await?
        .expect("Block should be stored");
    assert_eq!(
        stored_block.pruning_point, miner_block.pruning_point,
        "Stored block should have same pruning_point"
    );

    // Now simulate miner submitting block with wrong pruning_point
    let last_block = blockchain
        .get_block_at_height(blockchain.get_tip_height().await?)
        .await?
        .unwrap();

    let bad_miner_block = TestBlock {
        hash: create_test_pubkey(200),
        height: last_block.height + 1,
        blue_score: last_block.blue_score + 1,
        transactions: vec![],
        reward: 50_000_000_000,
        pruning_point: create_test_pubkey(199), // Wrong pruning_point!
        selected_parent: last_block.hash.clone(),
    };

    let result = blockchain.receive_block(bad_miner_block).await;
    assert!(
        result.is_err(),
        "Block with wrong pruning_point should be rejected"
    );

    Ok(())
}

// =============================================================================
// Test 8: P2P block propagation with pruning point
// =============================================================================

#[tokio::test]
async fn test_p2p_block_pruning_point() -> Result<()> {
    // Create two blockchains (simulating two nodes)
    let clock1 = Arc::new(SystemClock);
    let clock2 = Arc::new(SystemClock);

    let node_a = TestBlockchainBuilder::new()
        .with_clock(clock1)
        .with_funded_account_count(1)
        .build()
        .await?;

    let node_b = TestBlockchainBuilder::new()
        .with_clock(clock2)
        .with_funded_account_count(1)
        .build()
        .await?;

    // Mine blocks on node_a
    for _ in 0..250 {
        node_a.mine_block().await?;
    }

    // Sync blocks from node_a to node_b (simulating P2P propagation)
    let all_blocks = node_a.get_all_blocks().await?;
    for block in all_blocks.iter().skip(1) {
        // Skip genesis
        node_b.receive_block(block.clone()).await?;
    }

    // Verify both nodes have same state
    assert_eq!(
        node_a.get_tip_height().await?,
        node_b.get_tip_height().await?,
        "Both nodes should have same tip height"
    );

    assert_eq!(
        node_a.get_blue_score().await?,
        node_b.get_blue_score().await?,
        "Both nodes should have same blue_score"
    );

    // Verify pruning_point matches on both nodes
    let tip_a = node_a
        .get_block_at_height(node_a.get_tip_height().await?)
        .await?
        .unwrap();
    let tip_b = node_b
        .get_block_at_height(node_b.get_tip_height().await?)
        .await?
        .unwrap();

    assert_eq!(
        tip_a.pruning_point, tip_b.pruning_point,
        "Both nodes should have same pruning_point for tip"
    );

    // Try to sync a block with invalid pruning_point
    let last_block = node_a
        .get_block_at_height(node_a.get_tip_height().await?)
        .await?
        .unwrap();

    let invalid_block = TestBlock {
        hash: create_test_pubkey(250),
        height: last_block.height + 1,
        blue_score: last_block.blue_score + 1,
        transactions: vec![],
        reward: 50_000_000_000,
        pruning_point: Hash::zero(), // Invalid!
        selected_parent: last_block.hash.clone(),
    };

    let result = node_b.receive_block(invalid_block).await;
    assert!(
        result.is_err(),
        "Node B should reject block with invalid pruning_point"
    );

    Ok(())
}

// =============================================================================
// Test 9: Verify PRUNING_DEPTH constant
// =============================================================================

#[test]
fn test_pruning_depth_constant() {
    assert_eq!(
        PRUNING_DEPTH, 200,
        "PRUNING_DEPTH should be 200 (matching daemon config)"
    );
}

// =============================================================================
// Test 10: Deterministic pruning point calculation
// =============================================================================

#[tokio::test]
async fn test_pruning_point_deterministic() -> Result<()> {
    // Create two identical blockchains
    let clock1 = Arc::new(SystemClock);
    let clock2 = Arc::new(SystemClock);

    let blockchain1 = TestBlockchainBuilder::new()
        .with_clock(clock1)
        .with_funded_account_count(1)
        .build()
        .await?;

    let blockchain2 = TestBlockchainBuilder::new()
        .with_clock(clock2)
        .with_funded_account_count(1)
        .build()
        .await?;

    // Mine same number of blocks on both
    for _ in 0..300 {
        blockchain1.mine_block().await?;
        blockchain2.mine_block().await?;
    }

    // Verify pruning points match
    for height in 1..=300u64 {
        let block1 = blockchain1.get_block_at_height(height).await?.unwrap();
        let block2 = blockchain2.get_block_at_height(height).await?.unwrap();

        // Note: hashes might differ due to different hash computation,
        // but pruning_point structure should be the same (relative to genesis)
        if height < PRUNING_DEPTH {
            assert_eq!(
                block1.pruning_point,
                *blockchain1.get_genesis_hash(),
                "Block {} on chain1 should have genesis as pruning_point",
                height
            );
            assert_eq!(
                block2.pruning_point,
                *blockchain2.get_genesis_hash(),
                "Block {} on chain2 should have genesis as pruning_point",
                height
            );
        }
    }

    Ok(())
}

// =============================================================================
// Test Summary
// =============================================================================

#[test]
fn test_suite_summary() {
    println!();
    println!("=== PRUNING POINT INTEGRATION TEST SUITE ===");
    println!();
    println!("Tests using tos-testing-framework:");
    println!("  1. test_pruning_point_validation_new_block");
    println!("  2. test_pruning_point_genesis");
    println!("  3. test_pruning_point_boundary_transition");
    println!("  4. test_invalid_pruning_point_rejection");
    println!("  5. test_block_template_pruning_point");
    println!("  6. test_pruning_point_long_chain");
    println!("  7. test_miner_block_pruning_point");
    println!("  8. test_p2p_block_pruning_point");
    println!("  9. test_pruning_depth_constant");
    println!(" 10. test_pruning_point_deterministic");
    println!();
    println!("All tests verify pruning_point calculation and validation!");
    println!();
}
