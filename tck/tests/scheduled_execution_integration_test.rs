//! Scheduled Execution Integration Tests
//!
//! This test suite validates the ScheduledExecution system for TAKO VM,
//! including:
//! - Priority ordering (higher offer → earlier execution)
//! - Offer handling (30% burn, 70% miner)
//! - FIFO for equal offers
//! - Execution status transitions

#![allow(clippy::disallowed_methods)]

use tos_common::{
    contract::{
        ScheduledExecution, ScheduledExecutionKind, ScheduledExecutionStatus,
        MAX_SCHEDULING_HORIZON, MIN_SCHEDULED_EXECUTION_GAS, OFFER_BURN_PERCENT,
    },
    crypto::Hash,
};

// Import offer calculation functions from daemon
use tos_daemon::tako_integration::{calculate_offer_burn, calculate_offer_miner_reward};

// ============================================================================
// Offer Calculation Tests
// ============================================================================

#[test]
fn test_offer_burn_calculation() {
    println!("\n=== Test: Offer Burn Calculation ===\n");

    // Test with 1000 tokens
    let offer = 1000u64;
    let burn = calculate_offer_burn(offer);
    let expected_burn = offer * OFFER_BURN_PERCENT / 100;

    assert_eq!(
        burn, expected_burn,
        "Burn should be {}% of offer",
        OFFER_BURN_PERCENT
    );
    assert_eq!(burn, 300, "30% of 1000 should be 300");

    println!("✅ Offer: {}", offer);
    println!("   Burn ({}%): {}", OFFER_BURN_PERCENT, burn);
}

#[test]
fn test_offer_miner_reward_calculation() {
    println!("\n=== Test: Offer Miner Reward Calculation ===\n");

    let offer = 1000u64;
    let miner = calculate_offer_miner_reward(offer);
    let burn = calculate_offer_burn(offer);

    assert_eq!(miner, 700, "70% of 1000 should be 700");
    assert_eq!(burn + miner, offer, "Burn + miner should equal offer");

    println!("✅ Offer: {}", offer);
    println!("   Miner reward: {} ({}%)", miner, 100 - OFFER_BURN_PERCENT);
}

#[test]
fn test_offer_calculations_rounding() {
    println!("\n=== Test: Offer Calculations with Rounding ===\n");

    // Test with amount that doesn't divide evenly
    let offer = 101u64;
    let burn = calculate_offer_burn(offer);
    let miner = calculate_offer_miner_reward(offer);

    // 101 * 30 / 100 = 30.3 → 30 (truncated)
    assert_eq!(burn, 30, "101 * 30% should truncate to 30");
    assert_eq!(miner, 71, "Miner gets 71 (101 - 30)");
    assert_eq!(burn + miner, offer, "Burn + miner should equal offer");

    println!("✅ Offer: {}", offer);
    println!("   Burn: {} (truncated)", burn);
    println!("   Miner: {}", miner);
}

// ============================================================================
// Priority Ordering Tests
// ============================================================================

#[test]
fn test_priority_ordering_by_offer_amount() {
    println!("\n=== Test: Priority Ordering by Offer Amount ===\n");

    let current_topoheight = 100u64;
    let target_topoheight = 150u64;

    let exec_low = ScheduledExecution::new_offercall(
        Hash::new([1u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000, // Low offer
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        current_topoheight,
    );

    let exec_high = ScheduledExecution::new_offercall(
        Hash::new([2u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        100_000, // High offer
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        current_topoheight,
    );

    let exec_medium = ScheduledExecution::new_offercall(
        Hash::new([3u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        10_000, // Medium offer
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        current_topoheight,
    );

    // Sort by priority (using Ord implementation - reverse for highest first)
    let mut executions = [exec_low.clone(), exec_high.clone(), exec_medium.clone()];
    executions.sort_by(|a, b| b.cmp(a));

    // Verify order: high → medium → low
    assert_eq!(
        executions[0].offer_amount, 100_000,
        "Highest offer should be first"
    );
    assert_eq!(
        executions[1].offer_amount, 10_000,
        "Medium offer should be second"
    );
    assert_eq!(
        executions[2].offer_amount, 1_000,
        "Lowest offer should be last"
    );

    println!("✅ Priority ordering correct:");
    println!("   1. Offer: 100,000 (highest priority)");
    println!("   2. Offer: 10,000");
    println!("   3. Offer: 1,000 (lowest priority)");
}

#[test]
fn test_priority_ordering_fifo_for_equal_offers() {
    println!("\n=== Test: FIFO for Equal Offers ===\n");

    let target_topoheight = 150u64;
    let offer_amount = 10_000u64;

    let exec_first = ScheduledExecution::new_offercall(
        Hash::new([1u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        offer_amount,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        100, // Registered first
    );

    let exec_second = ScheduledExecution::new_offercall(
        Hash::new([2u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        offer_amount,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        101, // Registered second
    );

    let exec_third = ScheduledExecution::new_offercall(
        Hash::new([3u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        offer_amount,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        102, // Registered third
    );

    // Sort by priority
    let mut executions = [exec_third.clone(), exec_first.clone(), exec_second.clone()];
    executions.sort_by(|a, b| b.cmp(a));

    // Verify FIFO order (earlier registration = higher priority)
    assert_eq!(
        executions[0].registration_topoheight, 100,
        "First registered should be first"
    );
    assert_eq!(
        executions[1].registration_topoheight, 101,
        "Second registered should be second"
    );
    assert_eq!(
        executions[2].registration_topoheight, 102,
        "Third registered should be last"
    );

    println!("✅ FIFO ordering correct for equal offers:");
    println!("   1. Registered at: 100 (first)");
    println!("   2. Registered at: 101");
    println!("   3. Registered at: 102 (last)");
}

#[test]
fn test_priority_hash_tiebreaker() {
    println!("\n=== Test: Hash Tiebreaker for Equal Offers and Registration ===\n");

    let target_topoheight = 150u64;
    let offer_amount = 10_000u64;
    let reg_topoheight = 100u64;

    // Same offer, same registration time, different contract hashes
    let exec_a = ScheduledExecution::new_offercall(
        Hash::new([0xAAu8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        offer_amount,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        reg_topoheight,
    );

    let exec_b = ScheduledExecution::new_offercall(
        Hash::new([0xBBu8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        offer_amount,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        reg_topoheight,
    );

    // Compare - should use contract hash as tiebreaker
    let cmp = exec_a.cmp(&exec_b);
    assert!(cmp != std::cmp::Ordering::Equal, "Hash should break tie");

    println!("✅ Hash tiebreaker working:");
    println!("   Contract 0xAA vs 0xBB: {:?}", cmp);
}

// ============================================================================
// Execution Status Tests
// ============================================================================

#[test]
fn test_execution_status_pending() {
    println!("\n=== Test: Execution Status is Pending ===\n");

    let target_topoheight = 150u64;

    let execution = ScheduledExecution::new_offercall(
        Hash::new([1u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target_topoheight),
        100,
    );

    assert!(
        matches!(execution.status, ScheduledExecutionStatus::Pending),
        "New execution should be Pending"
    );

    println!("✅ New execution has Pending status");
}

#[test]
fn test_execution_can_cancel() {
    println!("\n=== Test: Execution Cancellation Window ===\n");

    let current_topoheight = 100u64;
    let far_future = 200u64;
    let near_future = 101u64;

    // Far future execution - can cancel
    let exec_far = ScheduledExecution::new_offercall(
        Hash::new([1u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(far_future),
        current_topoheight,
    );

    // Near future execution - too close to cancel
    let exec_near = ScheduledExecution::new_offercall(
        Hash::new([2u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(near_future),
        current_topoheight,
    );

    // BlockEnd execution - cannot cancel
    let exec_block_end = ScheduledExecution::new_offercall(
        Hash::new([3u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::BlockEnd,
        current_topoheight,
    );

    assert!(
        exec_far.can_cancel(current_topoheight),
        "Far future should be cancellable"
    );
    assert!(
        !exec_near.can_cancel(current_topoheight),
        "Near future should NOT be cancellable"
    );
    assert!(
        !exec_block_end.can_cancel(current_topoheight),
        "BlockEnd should NOT be cancellable"
    );

    println!("✅ Cancellation window logic correct:");
    println!("   Far future (200): cancellable = true");
    println!("   Near future (101): cancellable = false");
    println!("   BlockEnd: cancellable = false");
}

// ============================================================================
// Constants Tests
// ============================================================================

#[test]
fn test_scheduling_horizon_reasonable() {
    println!("\n=== Test: Scheduling Horizon ===\n");

    // MAX_SCHEDULING_HORIZON should be reasonable (not too far in future)
    // Currently ~7 days at 6-second blocks = 100,800
    // Use runtime binding to avoid clippy::assertions_on_constants
    let horizon = MAX_SCHEDULING_HORIZON;
    assert!(
        horizon <= 1_000_000,
        "Horizon should be <= 1,000,000 blocks (~69 days)"
    );
    assert!(horizon >= 100, "Horizon should be >= 100 blocks");

    println!("✅ MAX_SCHEDULING_HORIZON: {} blocks", horizon);
}

#[test]
fn test_min_gas_reasonable() {
    println!("\n=== Test: Minimum Gas ===\n");

    // MIN_SCHEDULED_EXECUTION_GAS should allow basic execution
    // Use runtime binding to avoid clippy::assertions_on_constants
    let min_gas = MIN_SCHEDULED_EXECUTION_GAS;
    assert!(min_gas >= 10_000, "Min gas should be >= 10,000");
    assert!(min_gas <= 1_000_000, "Min gas should be <= 1,000,000");

    println!("✅ MIN_SCHEDULED_EXECUTION_GAS: {} units", min_gas);
}

#[test]
fn test_offer_burn_percentage() {
    println!("\n=== Test: Offer Burn Percentage ===\n");

    // Use runtime binding to avoid clippy::assertions_on_constants
    let burn_percent = OFFER_BURN_PERCENT;
    assert_eq!(burn_percent, 30, "Burn should be 30%");
    assert!(burn_percent < 100, "Burn should be less than 100%");
    assert!(burn_percent > 0, "Burn should be greater than 0%");

    println!("✅ OFFER_BURN_PERCENT: {}%", burn_percent);
    println!("   Miner receives: {}%", 100 - burn_percent);
}

// ============================================================================
// Execution Kind Tests
// ============================================================================

#[test]
fn test_execution_kind_topoheight() {
    println!("\n=== Test: Execution Kind - TopoHeight ===\n");

    let target = 150u64;
    let execution = ScheduledExecution::new_offercall(
        Hash::new([1u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(target),
        100,
    );

    match execution.kind {
        ScheduledExecutionKind::TopoHeight(topo) => {
            assert_eq!(topo, target, "Target topoheight should match");
        }
        _ => panic!("Expected TopoHeight kind"),
    }

    println!("✅ TopoHeight execution created with target: {}", target);
}

#[test]
fn test_execution_kind_block_end() {
    println!("\n=== Test: Execution Kind - BlockEnd ===\n");

    let execution = ScheduledExecution::new_offercall(
        Hash::new([1u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::BlockEnd,
        100,
    );

    assert!(
        matches!(execution.kind, ScheduledExecutionKind::BlockEnd),
        "Should be BlockEnd kind"
    );

    println!("✅ BlockEnd execution created");
}

// ============================================================================
// Input Data Tests
// ============================================================================

#[test]
fn test_execution_with_input_data() {
    println!("\n=== Test: Execution with Input Data ===\n");

    let input_data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];

    let execution = ScheduledExecution::new_offercall(
        Hash::new([1u8; 32]),
        42, // chunk_id
        input_data.clone(),
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(150),
        100,
    );

    assert_eq!(
        execution.input_data, input_data,
        "Input data should be stored"
    );
    assert_eq!(execution.chunk_id, 42, "Chunk ID should be stored");

    println!("✅ Input data stored:");
    println!("   Chunk ID: {}", execution.chunk_id);
    println!("   Input data: {:?}", input_data);
    println!("   Input length: {} bytes", input_data.len());
}

// ============================================================================
// Defer Tests
// ============================================================================

#[test]
fn test_execution_defer() {
    println!("\n=== Test: Execution Defer ===\n");

    let mut execution = ScheduledExecution::new_offercall(
        Hash::new([1u8; 32]),
        0,
        vec![],
        MIN_SCHEDULED_EXECUTION_GAS,
        1_000,
        Hash::new([10u8; 32]),
        ScheduledExecutionKind::TopoHeight(150),
        100,
    );

    assert_eq!(execution.defer_count, 0, "Initial defer count should be 0");

    // Defer once
    let max_reached = execution.defer();
    assert!(!max_reached, "Should not reach max on first defer");
    assert_eq!(execution.defer_count, 1, "Defer count should be 1");

    // Defer again
    let max_reached = execution.defer();
    assert!(!max_reached, "Should not reach max on second defer");
    assert_eq!(execution.defer_count, 2, "Defer count should be 2");

    println!("✅ Defer count incrementing:");
    println!("   After 2 defers: defer_count = {}", execution.defer_count);
}

// ============================================================================
// Summary
// ============================================================================

#[test]
fn test_summary() {
    println!("\n");
    println!("{}", "=".repeat(60));
    println!("SCHEDULED EXECUTION INTEGRATION TEST SUMMARY");
    println!("{}", "=".repeat(60));
    println!();
    println!("Tests cover:");
    println!("  - Offer burn/miner split (30%/70%)");
    println!("  - Priority ordering (offer amount)");
    println!("  - FIFO for equal offers");
    println!("  - Hash tiebreaker");
    println!("  - Execution status (Pending)");
    println!("  - Cancellation window logic");
    println!("  - Scheduling constants");
    println!("  - Execution kinds (TopoHeight, BlockEnd)");
    println!("  - Input data handling");
    println!("  - Defer mechanism");
    println!();
    println!("OFFERCALL (EIP-7833 inspired) implementation:");
    println!("  - 30% offer burn on registration");
    println!("  - 70% offer to miner on execution");
    println!("  - Priority: higher offer -> earlier execution");
    println!("  - FIFO fallback for equal offers");
    println!();
    println!("{}", "=".repeat(60));
}
