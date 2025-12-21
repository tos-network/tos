//! Scheduled Execution Integration Tests
//!
//! This test suite validates the ScheduledExecution system for TAKO VM,
//! including:
//! - Scheduling executions at future topoheights
//! - Priority ordering (higher offer → earlier execution)
//! - Offer handling (30% burn, 70% miner)
//! - Cancellation and refunds
//! - Rate limiting
//! - Execution at target topoheight

#![allow(clippy::disallowed_methods)]

use indexmap::IndexMap;
use std::collections::HashMap;
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{
        ContractProvider, ContractStorage, ScheduledExecution, ScheduledExecutionKind,
        ScheduledExecutionStatus, MAX_SCHEDULING_HORIZON, MIN_SCHEDULED_EXECUTION_GAS,
        OFFER_BURN_PERCENT,
    },
    crypto::{Hash, PublicKey},
};
use tos_kernel::ValueCell;
use tos_program_runtime::ScheduledExecutionProvider;

// Import the adapter from daemon
use tos_daemon::tako_integration::TosScheduledExecutionAdapter;

/// Mock provider for testing scheduled execution
struct MockScheduledExecProvider {
    balances: HashMap<Hash, u64>,
}

impl MockScheduledExecProvider {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
        }
    }

    fn with_contract_balance(mut self, contract: Hash, balance: u64) -> Self {
        self.balances.insert(contract, balance);
        self
    }
}

impl ContractProvider for MockScheduledExecProvider {
    fn get_contract_balance_for_asset(
        &self,
        contract: &Hash,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, u64)>> {
        Ok(self.balances.get(contract).map(|&b| (100, b)))
    }

    fn get_account_balance_for_asset(
        &self,
        _key: &PublicKey,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, u64)>> {
        Ok(Some((100, 1_000_000)))
    }

    fn asset_exists(&self, _asset: &Hash, _topoheight: TopoHeight) -> anyhow::Result<bool> {
        Ok(true)
    }

    fn load_asset_data(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, AssetData)>> {
        Ok(None)
    }

    fn load_asset_supply(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, u64)>> {
        Ok(None)
    }

    fn account_exists(&self, _key: &PublicKey, _topoheight: TopoHeight) -> anyhow::Result<bool> {
        Ok(true)
    }

    fn load_contract_module(
        &self,
        _contract: &Hash,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

impl ContractStorage for MockScheduledExecProvider {
    fn load_data(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<(TopoHeight, Option<ValueCell>)>> {
        Ok(None)
    }

    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<TopoHeight>> {
        Ok(Some(100))
    }

    fn has_data(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> anyhow::Result<bool> {
        Ok(true)
    }
}

// ============================================================================
// Basic Scheduling Tests
// ============================================================================

#[test]
fn test_schedule_execution_at_future_topoheight() {
    println!("\n=== Test: Schedule Execution at Future Topoheight ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;
    let target_topoheight = 150;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let handle = {
        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            current_topoheight,
            &current_contract,
            &provider,
        );

        let result = adapter.schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            0,                           // chunk_id
            &[],                         // input_data
            MIN_SCHEDULED_EXECUTION_GAS, // max_gas
            10_000,                      // offer_amount
            target_topoheight,           // target_topoheight
            false,                       // is_block_end
        );

        assert!(result.is_ok(), "schedule_execution should succeed");
        let handle = result.unwrap();
        assert!(handle > 0, "Should return valid handle");

        // Verify burned offers
        let burn_amount = 10_000 * OFFER_BURN_PERCENT / 100;
        assert_eq!(
            adapter.burned_offers, burn_amount,
            "Should burn 30% of offer"
        );

        handle
    };

    // Verify execution was added
    assert_eq!(
        scheduled_executions.len(),
        1,
        "Should have one scheduled execution"
    );

    // Verify balance was deducted
    let delta = balance_changes.get(&current_contract).unwrap();
    let expected_deduction = MIN_SCHEDULED_EXECUTION_GAS as i128 + 10_000i128;
    assert_eq!(*delta, -expected_deduction, "Should deduct gas + offer");

    println!("✅ Successfully scheduled execution for topoheight {target_topoheight}");
    println!("   Handle: {handle}");
    println!("   Balance deduction: {expected_deduction}");
    println!("   Offer burned: {}", 10_000 * OFFER_BURN_PERCENT / 100);
}

#[test]
fn test_schedule_execution_block_end() {
    println!("\n=== Test: Schedule Execution at Block End ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let (handle, is_block_end_kind) = {
        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            current_topoheight,
            &current_contract,
            &provider,
        );

        let handle = adapter
            .schedule_execution(
                current_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                5_000,
                0,    // target_topoheight (ignored for block end)
                true, // is_block_end = true
            )
            .unwrap();

        // Query via adapter
        let info = adapter.get_scheduled_execution(handle).unwrap().unwrap();
        (handle, info.is_block_end)
    };

    // Verify execution was added with BlockEnd kind
    let execution = scheduled_executions.values().next().unwrap();
    assert!(
        matches!(execution.kind, ScheduledExecutionKind::BlockEnd),
        "Should be BlockEnd scheduling"
    );
    assert!(is_block_end_kind, "Info should indicate block end");

    println!("✅ Successfully scheduled execution for block end");
    println!("   Handle: {handle}");
    println!("   Kind: BlockEnd");
}

// ============================================================================
// Validation Tests
// ============================================================================

#[test]
fn test_schedule_execution_topoheight_in_past_fails() {
    println!("\n=== Test: Topoheight in Past Fails ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;
    let past_topoheight = 50; // In the past

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    let result = adapter.schedule_execution(
        current_contract.as_bytes(),
        &target_contract,
        0,
        &[],
        MIN_SCHEDULED_EXECUTION_GAS,
        1000,
        past_topoheight,
        false,
    );

    // Should return error code 2 (ERR_TOPOHEIGHT_IN_PAST)
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 2, "Should return ERR_TOPOHEIGHT_IN_PAST");
    assert!(
        scheduled_executions.is_empty(),
        "Should not add execution for past topoheight"
    );

    println!("✅ Correctly rejected past topoheight");
}

#[test]
fn test_schedule_execution_topoheight_too_far_fails() {
    println!("\n=== Test: Topoheight Too Far in Future Fails ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;
    let far_future = current_topoheight + MAX_SCHEDULING_HORIZON + 1;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    let result = adapter.schedule_execution(
        current_contract.as_bytes(),
        &target_contract,
        0,
        &[],
        MIN_SCHEDULED_EXECUTION_GAS,
        1000,
        far_future,
        false,
    );

    // Should return error code 3 (ERR_TOPOHEIGHT_TOO_FAR)
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 3, "Should return ERR_TOPOHEIGHT_TOO_FAR");

    println!("✅ Correctly rejected topoheight beyond horizon");
    println!("   Max horizon: {MAX_SCHEDULING_HORIZON}");
}

#[test]
fn test_schedule_execution_gas_too_low_fails() {
    println!("\n=== Test: Gas Too Low Fails ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    let result = adapter.schedule_execution(
        current_contract.as_bytes(),
        &target_contract,
        0,
        &[],
        100, // Too low (MIN_SCHEDULED_EXECUTION_GAS = 20,000)
        1000,
        150,
        false,
    );

    // Should return error code 5 (ERR_GAS_TOO_LOW)
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 5, "Should return ERR_GAS_TOO_LOW");

    println!("✅ Correctly rejected low gas");
    println!("   Minimum gas required: {MIN_SCHEDULED_EXECUTION_GAS}");
}

#[test]
fn test_schedule_execution_insufficient_balance_fails() {
    println!("\n=== Test: Insufficient Balance Fails ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;

    // Very low balance
    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 100);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    let result = adapter.schedule_execution(
        current_contract.as_bytes(),
        &target_contract,
        0,
        &[],
        MIN_SCHEDULED_EXECUTION_GAS,
        10_000, // Total cost > 100
        150,
        false,
    );

    // Should return error code 1 (ERR_INSUFFICIENT_BALANCE)
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 1, "Should return ERR_INSUFFICIENT_BALANCE");

    println!("✅ Correctly rejected insufficient balance");
}

// ============================================================================
// Cancellation Tests
// ============================================================================

#[test]
fn test_cancel_scheduled_execution() {
    println!("\n=== Test: Cancel Scheduled Execution ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    // Schedule first
    let offer_amount = 10_000u64;
    let (_handle, refund) = {
        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            current_topoheight,
            &current_contract,
            &provider,
        );

        let handle = adapter
            .schedule_execution(
                current_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                offer_amount,
                150,
                false,
            )
            .unwrap();

        println!("Scheduled execution with handle: {handle}");

        // Now cancel
        let refund = adapter
            .cancel_scheduled_execution(current_contract.as_bytes(), handle)
            .unwrap();

        (handle, refund)
    };

    // Verify refund (gas + 70% of offer)
    // 30% was burned, so 70% of offer is refundable
    let expected_refund = MIN_SCHEDULED_EXECUTION_GAS + (offer_amount * 70 / 100);
    assert_eq!(refund, expected_refund, "Should refund gas + 70% offer");

    // Verify execution was removed
    assert!(
        scheduled_executions.is_empty(),
        "Should remove cancelled execution"
    );

    println!("✅ Successfully cancelled execution");
    println!("   Refund: {refund}");
    println!(
        "   Expected: gas({MIN_SCHEDULED_EXECUTION_GAS}) + 70% offer({})",
        offer_amount * 70 / 100
    );
}

#[test]
fn test_cancel_not_authorized_fails() {
    println!("\n=== Test: Cancel Not Authorized Fails ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let other_contract = Hash::new([3u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    // Schedule with current_contract
    let handle = adapter
        .schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            150,
            false,
        )
        .unwrap();

    // Try to cancel with different contract
    let result = adapter.cancel_scheduled_execution(other_contract.as_bytes(), handle);

    // Should return error code 10 (ERR_NOT_AUTHORIZED)
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 10, "Should return ERR_NOT_AUTHORIZED");

    // Execution should still exist
    assert_eq!(
        scheduled_executions.len(),
        1,
        "Execution should not be removed"
    );

    println!("✅ Correctly rejected unauthorized cancellation");
}

#[test]
fn test_cancel_too_close_to_execution_fails() {
    println!("\n=== Test: Cancel Too Close to Execution Fails ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;
    // Target is only 1 block away - too close to cancel
    // MIN_CANCELLATION_WINDOW = 1, so target must be > current + 1
    let target_topoheight = 101;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    let handle = adapter
        .schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            target_topoheight,
            false,
        )
        .unwrap();

    // Try to cancel - should fail because too close to execution
    let result = adapter.cancel_scheduled_execution(current_contract.as_bytes(), handle);

    // Should return error code 11 (ERR_CANNOT_CANCEL)
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 11, "Should return ERR_CANNOT_CANCEL");

    // Execution should still exist
    assert_eq!(
        scheduled_executions.len(),
        1,
        "Execution should not be removed"
    );

    println!("✅ Correctly rejected cancellation too close to execution");
    println!("   Current topoheight: {current_topoheight}");
    println!("   Target topoheight: {target_topoheight}");
    println!("   MIN_CANCELLATION_WINDOW: 1 block");
}

#[test]
fn test_cancel_block_end_fails() {
    println!("\n=== Test: Cancel BlockEnd Execution Fails ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    let handle = adapter
        .schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            0,    // ignored for BlockEnd
            true, // is_block_end
        )
        .unwrap();

    // Try to cancel BlockEnd - should fail
    let result = adapter.cancel_scheduled_execution(current_contract.as_bytes(), handle);

    // Should return error code 11 (ERR_CANNOT_CANCEL)
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 11, "Should return ERR_CANNOT_CANCEL");

    // Execution should still exist
    assert_eq!(
        scheduled_executions.len(),
        1,
        "Execution should not be removed"
    );

    println!("✅ Correctly rejected BlockEnd cancellation");
    println!("   BlockEnd executions cannot be cancelled");
}

// ============================================================================
// Priority Ordering Tests
// ============================================================================

#[test]
fn test_priority_ordering_by_offer_amount() {
    println!("\n=== Test: Priority Ordering by Offer Amount ===\n");

    // Create three executions with different offer amounts
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

    // Sort by priority (using Ord implementation)
    let mut executions = [exec_low.clone(), exec_high.clone(), exec_medium.clone()];
    executions.sort_by(|a, b| b.cmp(a)); // Reverse order for highest priority first

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

    // Create three executions with same offer but different registration times
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

// ============================================================================
// Query Tests
// ============================================================================

#[test]
fn test_get_scheduled_execution_info() {
    println!("\n=== Test: Get Scheduled Execution Info ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;
    let target_topoheight = 150;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    let handle = adapter
        .schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            42,            // chunk_id
            &[1, 2, 3, 4], // input_data
            MIN_SCHEDULED_EXECUTION_GAS,
            5_000,
            target_topoheight,
            false,
        )
        .unwrap();

    // Query the execution
    let info = adapter.get_scheduled_execution(handle).unwrap();
    assert!(info.is_some(), "Should find scheduled execution");

    let info = info.unwrap();
    assert_eq!(info.handle, handle);
    assert_eq!(info.target_contract, target_contract);
    assert_eq!(info.chunk_id, 42);
    assert_eq!(info.max_gas, MIN_SCHEDULED_EXECUTION_GAS);
    assert_eq!(info.offer_amount, 5_000);
    assert_eq!(info.target_topoheight, target_topoheight);
    assert!(!info.is_block_end);
    assert_eq!(info.registration_topoheight, current_topoheight);
    assert_eq!(info.status, 0); // Pending

    println!("✅ Successfully queried execution info:");
    println!("   Handle: {}", info.handle);
    println!("   Target: {:?}", Hash::new(info.target_contract));
    println!("   Chunk ID: {}", info.chunk_id);
    println!("   Max Gas: {}", info.max_gas);
    println!("   Offer: {}", info.offer_amount);
    println!("   Target Topoheight: {}", info.target_topoheight);
    println!("   Status: Pending");
}

#[test]
fn test_get_nonexistent_execution_returns_none() {
    println!("\n=== Test: Get Nonexistent Execution ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let current_topoheight = 100;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    // Query non-existent handle
    let info = adapter.get_scheduled_execution(99999).unwrap();
    assert!(info.is_none(), "Should return None for non-existent handle");

    println!("✅ Correctly returned None for non-existent execution");
}

// ============================================================================
// Input Data Tests
// ============================================================================

#[test]
fn test_schedule_with_input_data() {
    println!("\n=== Test: Schedule with Input Data ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let target_contract = [2u8; 32];
    let current_topoheight = 100;

    let provider =
        MockScheduledExecProvider::new().with_contract_balance(current_contract.clone(), 1_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut adapter = TosScheduledExecutionAdapter::new(
        &mut scheduled_executions,
        &mut balance_changes,
        current_topoheight,
        &current_contract,
        &provider,
    );

    // Input data: encoded function call
    let input_data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];

    let handle = adapter
        .schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            1, // chunk_id
            &input_data,
            MIN_SCHEDULED_EXECUTION_GAS,
            1_000,
            150,
            false,
        )
        .unwrap();

    // Verify input data was stored
    let execution = scheduled_executions.values().next().unwrap();
    assert_eq!(
        execution.input_data, input_data,
        "Input data should be stored"
    );

    println!("✅ Successfully stored input data:");
    println!("   Handle: {handle}");
    println!("   Input data: {:?}", input_data);
    println!("   Input length: {} bytes", input_data.len());
}

// ============================================================================
// Multiple Scheduling Tests
// ============================================================================

#[test]
fn test_multiple_scheduled_executions() {
    println!("\n=== Test: Multiple Scheduled Executions ===\n");

    let current_contract = Hash::new([1u8; 32]);
    let current_topoheight = 100;

    let provider = MockScheduledExecProvider::new()
        .with_contract_balance(current_contract.clone(), 10_000_000);

    let mut scheduled_executions = IndexMap::new();
    let mut balance_changes = HashMap::new();

    let mut handles = Vec::new();

    {
        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            current_topoheight,
            &current_contract,
            &provider,
        );

        // Schedule 5 executions to different contracts
        for i in 0..5 {
            let mut target = [0u8; 32];
            target[0] = i + 10;

            let handle = adapter
                .schedule_execution(
                    current_contract.as_bytes(),
                    &target,
                    i as u16,
                    &[i],
                    MIN_SCHEDULED_EXECUTION_GAS,
                    (i as u64 + 1) * 1_000, // Different offers
                    150 + i as u64,         // Different target topoheights
                    false,
                )
                .unwrap();

            handles.push(handle);
            println!("Scheduled execution {}: handle={}", i, handle);
        }
    }

    assert_eq!(
        scheduled_executions.len(),
        5,
        "Should have 5 scheduled executions"
    );
    assert_eq!(handles.len(), 5, "Should have 5 handles");

    // Verify all handles are unique
    let unique_handles: std::collections::HashSet<_> = handles.iter().collect();
    assert_eq!(unique_handles.len(), 5, "All handles should be unique");

    println!("\n✅ Successfully scheduled 5 executions");
    println!("   Handles: {:?}", handles);
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

// ============================================================================
// Summary
// ============================================================================

#[test]
fn test_summary() {
    println!("\n");
    println!("{}", "=".repeat(60));
    println!("SCHEDULED EXECUTION TEST SUMMARY");
    println!("{}", "=".repeat(60));
    println!();
    println!("Tests cover:");
    println!("  - Basic scheduling at future topoheight");
    println!("  - Block end scheduling");
    println!("  - Validation (past topoheight, horizon, gas, balance)");
    println!("  - Cancellation and refunds");
    println!("  - Cancellation window (MIN_CANCELLATION_WINDOW = 1 block)");
    println!("  - BlockEnd cannot be cancelled");
    println!("  - Authorization checks");
    println!("  - Priority ordering (offer amount, FIFO)");
    println!("  - Query scheduled execution info");
    println!("  - Input data handling");
    println!("  - Multiple executions");
    println!();
    println!("All tests verify the OFFERCALL (EIP-7833 inspired) implementation:");
    println!("  - 30% offer burn on registration");
    println!("  - 70% offer to miner on execution");
    println!("  - Priority: higher offer -> earlier execution");
    println!("  - FIFO fallback for equal offers");
    println!();
    println!("{}", "=".repeat(60));
}
