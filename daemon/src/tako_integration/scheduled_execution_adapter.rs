//! Scheduled Execution Adapter: TOS ContractManager → TAKO ScheduledExecutionProvider
//!
//! This module bridges TOS's scheduled execution system with TAKO's tos_offer_call syscalls.
//! It translates between TOS's IndexMap-based scheduling and TAKO's trait interface.
//!
//! # Architecture
//!
//! ```text
//! TAKO syscall tos_offer_call(...)
//!     ↓
//! TosScheduledExecutionAdapter::schedule_execution()
//!     ↓
//! 1. Validate topoheight (not past, within horizon)
//! 2. Validate max_gas >= MIN_SCHEDULED_EXECUTION_GAS
//! 3. Deduct offer + gas from contract balance
//! 4. Burn 30% of offer immediately
//! 5. Create ScheduledExecution with registration_topoheight
//! 6. Add to scheduled_executions IndexMap
//! 7. Return handle (hash of execution)
//! ```
//!
//! # Offer Handling (EIP-7833 Inspired)
//!
//! - 30% of offer is burned on registration (anti-spam, consistent with TOS gas model)
//! - 70% of offer is paid to miner on execution (incentive)
//! - Priority: Higher offer = higher priority, FIFO for equal offers

use indexmap::IndexMap;
use std::collections::HashMap;
use tos_common::{
    block::TopoHeight,
    contract::{
        ContractProvider, ScheduledExecution, ScheduledExecutionKind, ScheduledExecutionStatus,
        MAX_SCHEDULES_PER_CONTRACT_PER_WINDOW, MAX_SCHEDULING_HORIZON, MIN_OFFER_AMOUNT,
        MIN_SCHEDULED_EXECUTION_GAS, OFFER_BURN_PERCENT, RATE_LIMIT_BYPASS_OFFER,
        SCHEDULE_RATE_LIMIT_WINDOW,
    },
    crypto::Hash,
};
use tos_program_runtime::{ScheduledExecutionInfo, ScheduledExecutionProvider};
use tos_tbpf::error::EbpfError;

/// Error codes returned by scheduled execution operations
#[allow(dead_code)]
pub mod error_codes {
    /// Insufficient contract balance for offer + gas
    pub const ERR_INSUFFICIENT_BALANCE: u64 = 1;
    /// Target topoheight is in the past
    pub const ERR_TOPOHEIGHT_IN_PAST: u64 = 2;
    /// Target topoheight exceeds maximum scheduling horizon
    pub const ERR_TOPOHEIGHT_TOO_FAR: u64 = 3;
    /// Execution already scheduled at this topoheight for this contract
    pub const ERR_ALREADY_SCHEDULED: u64 = 4;
    /// Max gas below minimum threshold
    pub const ERR_GAS_TOO_LOW: u64 = 5;
    /// Invalid params pointer or memory access
    pub const ERR_INVALID_PARAMS: u64 = 6;
    /// Offer amount below minimum
    pub const ERR_OFFER_TOO_LOW: u64 = 7;
    /// Rate limit exceeded for this contract
    pub const ERR_RATE_LIMIT_EXCEEDED: u64 = 8;
    /// Scheduled execution not found
    pub const ERR_NOT_FOUND: u64 = 9;
    /// Not authorized to cancel (not the scheduler)
    pub const ERR_NOT_AUTHORIZED: u64 = 10;
    /// Cannot cancel - already executed or too close to execution
    pub const ERR_CANNOT_CANCEL: u64 = 11;
}

/// Adapter that wraps TOS's scheduled execution system to implement TAKO's ScheduledExecutionProvider
///
/// # Design Notes
///
/// This adapter operates on a mutable reference to the scheduled_executions IndexMap
/// from the ContractManager. It tracks:
/// - Handle to execution mapping (for queries and cancellation)
/// - Rate limiting per contract
/// - Burned offer amounts (for accounting)
///
/// # Example
///
/// ```ignore
/// use tos_daemon::tako_integration::TosScheduledExecutionAdapter;
///
/// // During contract execution
/// let mut adapter = TosScheduledExecutionAdapter::new(
///     &mut scheduled_executions,
///     &mut balance_changes,
///     current_topoheight,
///     &current_contract,
///     &provider,
/// );
///
/// // TAKO will call these methods via syscalls
/// adapter.schedule_execution(...)?;
/// ```
pub struct TosScheduledExecutionAdapter<'a> {
    /// Reference to the scheduled executions map from ContractManager
    scheduled_executions: &'a mut IndexMap<Hash, ScheduledExecution>,
    /// Balance changes tracking (for deducting offer + gas)
    /// Maps contract hash → balance delta (negative for deductions)
    balance_changes: &'a mut HashMap<Hash, i128>,
    /// Current topoheight (for validation)
    current_topoheight: TopoHeight,
    /// Current contract being executed (scheduler)
    current_contract: Hash,
    /// TOS contract provider (for balance queries)
    provider: &'a (dyn ContractProvider + Send),
    /// Handle to hash mapping (for queries)
    handle_to_hash: HashMap<u64, Hash>,
    /// Next handle to assign
    next_handle: u64,
    /// Rate limit tracking: contract → (window_start, count)
    rate_limit_tracker: HashMap<Hash, (TopoHeight, u64)>,
    /// Total burned offers during this execution
    pub burned_offers: u64,
}

impl<'a> TosScheduledExecutionAdapter<'a> {
    /// Create a new scheduled execution adapter
    ///
    /// # Arguments
    ///
    /// * `scheduled_executions` - Mutable reference to scheduled executions map
    /// * `balance_changes` - Mutable reference to balance changes tracking
    /// * `current_topoheight` - Current blockchain topoheight
    /// * `current_contract` - Hash of the currently executing contract
    /// * `provider` - TOS contract provider for balance queries
    pub fn new(
        scheduled_executions: &'a mut IndexMap<Hash, ScheduledExecution>,
        balance_changes: &'a mut HashMap<Hash, i128>,
        current_topoheight: TopoHeight,
        current_contract: &Hash,
        provider: &'a (dyn ContractProvider + Send),
    ) -> Self {
        Self {
            scheduled_executions,
            balance_changes,
            current_topoheight,
            current_contract: current_contract.clone(),
            provider,
            handle_to_hash: HashMap::new(),
            next_handle: 1, // Start handles at 1 (0 = error)
            rate_limit_tracker: HashMap::new(),
            burned_offers: 0,
        }
    }

    /// Get the current contract balance (accounting for pending changes)
    fn get_effective_balance(&self, contract: &Hash) -> Result<u64, EbpfError> {
        // Get base balance from provider
        let base_balance = self
            .provider
            .get_contract_balance_for_asset(contract, &Hash::zero(), self.current_topoheight)
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to get contract balance: {}", e),
                )))
            })?
            .map(|(_, balance)| balance)
            .unwrap_or(0);

        // Apply pending balance changes
        let delta = self.balance_changes.get(contract).copied().unwrap_or(0);
        let effective = (base_balance as i128).saturating_add(delta);

        if effective < 0 {
            Ok(0)
        } else {
            Ok(effective as u64)
        }
    }

    /// Deduct amount from contract balance
    fn deduct_balance(&mut self, contract: &Hash, amount: u64) {
        let delta = self.balance_changes.entry(contract.clone()).or_insert(0);
        *delta = delta.saturating_sub(amount as i128);
    }

    /// Check rate limiting for a contract
    fn check_rate_limit(&mut self, contract: &Hash, offer_amount: u64) -> bool {
        // High-value offers bypass rate limiting
        if offer_amount >= RATE_LIMIT_BYPASS_OFFER {
            return true;
        }

        let window_start = self
            .current_topoheight
            .saturating_sub(SCHEDULE_RATE_LIMIT_WINDOW);

        let entry = self
            .rate_limit_tracker
            .entry(contract.clone())
            .or_insert((window_start, 0));

        // Reset window if expired
        if entry.0 < window_start {
            *entry = (self.current_topoheight, 0);
        }

        // Check limit
        if entry.1 >= MAX_SCHEDULES_PER_CONTRACT_PER_WINDOW {
            return false;
        }

        // Increment count
        entry.1 = entry.1.saturating_add(1);
        true
    }

    /// Generate a unique handle for a scheduled execution
    fn generate_handle(&mut self, execution_hash: &Hash) -> u64 {
        let handle = self.next_handle;
        self.next_handle = self.next_handle.saturating_add(1);
        self.handle_to_hash.insert(handle, execution_hash.clone());
        handle
    }

    /// Get total burned offers during this adapter's lifetime
    pub fn get_burned_offers(&self) -> u64 {
        self.burned_offers
    }
}

impl<'a> ScheduledExecutionProvider for TosScheduledExecutionAdapter<'a> {
    fn schedule_execution(
        &mut self,
        scheduler: &[u8; 32],
        target_contract: &[u8; 32],
        chunk_id: u16,
        input_data: &[u8],
        max_gas: u64,
        offer_amount: u64,
        target_topoheight: u64,
        is_block_end: bool,
    ) -> Result<u64, EbpfError> {
        let scheduler_hash = Hash::new(*scheduler);
        let target_hash = Hash::new(*target_contract);

        // Verify scheduler is the current contract
        if scheduler_hash != self.current_contract {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Scheduler must be the current contract",
            ))));
        }

        // 1. Validate max_gas >= MIN_SCHEDULED_EXECUTION_GAS
        if max_gas < MIN_SCHEDULED_EXECUTION_GAS {
            return Ok(error_codes::ERR_GAS_TOO_LOW);
        }

        // 2. Validate offer amount (MIN_OFFER_AMOUNT is 0, allowing free scheduled executions)
        // This check is kept for future-proofing if MIN_OFFER_AMOUNT changes
        #[allow(clippy::absurd_extreme_comparisons)]
        if offer_amount < MIN_OFFER_AMOUNT {
            return Ok(error_codes::ERR_OFFER_TOO_LOW);
        }

        // 3. Validate target topoheight
        let kind = if is_block_end {
            ScheduledExecutionKind::BlockEnd
        } else {
            // Check not in past
            if target_topoheight <= self.current_topoheight {
                return Ok(error_codes::ERR_TOPOHEIGHT_IN_PAST);
            }

            // Check not too far in future
            if target_topoheight
                > self
                    .current_topoheight
                    .saturating_add(MAX_SCHEDULING_HORIZON)
            {
                return Ok(error_codes::ERR_TOPOHEIGHT_TOO_FAR);
            }

            ScheduledExecutionKind::TopoHeight(target_topoheight)
        };

        // 4. Check rate limiting
        if !self.check_rate_limit(&scheduler_hash, offer_amount) {
            return Ok(error_codes::ERR_RATE_LIMIT_EXCEEDED);
        }

        // 5. Calculate total cost (offer + gas)
        // Gas is reserved but not burned; refunded if unused
        let total_cost = offer_amount.saturating_add(max_gas);

        // 6. Check contract balance
        let balance = self.get_effective_balance(&scheduler_hash)?;
        if balance < total_cost {
            return Ok(error_codes::ERR_INSUFFICIENT_BALANCE);
        }

        // 7. Deduct total cost from contract balance
        self.deduct_balance(&scheduler_hash, total_cost);

        // 8. Burn 30% of offer immediately
        let burn_amount = offer_amount
            .saturating_mul(OFFER_BURN_PERCENT)
            .saturating_div(100);
        self.burned_offers = self.burned_offers.saturating_add(burn_amount);

        // 9. Create ScheduledExecution
        let execution = ScheduledExecution::new_offercall(
            target_hash.clone(),
            chunk_id,
            input_data.to_vec(),
            max_gas,
            offer_amount,
            scheduler_hash.clone(),
            kind,
            self.current_topoheight,
        );

        let execution_hash = execution.hash.clone();

        // 10. Check for duplicate (same contract at same topoheight)
        if self.scheduled_executions.contains_key(&target_hash) {
            // Refund the deducted amount
            let delta = self
                .balance_changes
                .entry(scheduler_hash.clone())
                .or_insert(0);
            *delta = delta.saturating_add(total_cost as i128);
            self.burned_offers = self.burned_offers.saturating_sub(burn_amount);
            return Ok(error_codes::ERR_ALREADY_SCHEDULED);
        }

        // 11. Add to scheduled executions
        self.scheduled_executions
            .insert(execution_hash.clone(), execution);

        // 12. Generate and return handle
        let handle = self.generate_handle(&execution_hash);

        Ok(handle)
    }

    fn get_scheduled_execution(
        &self,
        handle: u64,
    ) -> Result<Option<ScheduledExecutionInfo>, EbpfError> {
        // Look up hash from handle
        let hash = match self.handle_to_hash.get(&handle) {
            Some(h) => h,
            None => return Ok(None),
        };

        // Look up execution from hash
        let execution = match self.scheduled_executions.get(hash) {
            Some(e) => e,
            None => return Ok(None),
        };

        // Convert to ScheduledExecutionInfo
        let (target_topoheight, is_block_end) = match execution.kind {
            ScheduledExecutionKind::TopoHeight(topo) => (topo, false),
            ScheduledExecutionKind::BlockEnd => (0, true),
        };

        let status = match execution.status {
            ScheduledExecutionStatus::Pending => 0,
            ScheduledExecutionStatus::Executed => 1,
            ScheduledExecutionStatus::Cancelled => 2,
            ScheduledExecutionStatus::Failed => 3,
            ScheduledExecutionStatus::Expired => 4,
        };

        Ok(Some(ScheduledExecutionInfo {
            handle,
            target_contract: *execution.contract.as_bytes(),
            chunk_id: execution.chunk_id,
            max_gas: execution.max_gas,
            offer_amount: execution.offer_amount,
            target_topoheight,
            is_block_end,
            registration_topoheight: execution.registration_topoheight,
            status,
        }))
    }

    fn cancel_scheduled_execution(
        &mut self,
        scheduler: &[u8; 32],
        handle: u64,
    ) -> Result<u64, EbpfError> {
        let scheduler_hash = Hash::new(*scheduler);

        // Look up hash from handle
        let hash = match self.handle_to_hash.get(&handle) {
            Some(h) => h.clone(),
            None => return Ok(error_codes::ERR_NOT_FOUND),
        };

        // Look up execution
        let execution = match self.scheduled_executions.get(&hash) {
            Some(e) => e,
            None => return Ok(error_codes::ERR_NOT_FOUND),
        };

        // Verify authorization (only scheduler can cancel)
        if execution.scheduler_contract != scheduler_hash {
            return Ok(error_codes::ERR_NOT_AUTHORIZED);
        }

        // Check if cancellable (must be at least MIN_CANCELLATION_WINDOW blocks before execution)
        if !execution.can_cancel(self.current_topoheight) {
            return Ok(error_codes::ERR_CANNOT_CANCEL);
        }

        // Calculate refund (gas + remaining offer after burn)
        // 30% was already burned, so 70% of offer is refundable
        let offer_refund = execution
            .offer_amount
            .saturating_mul(100u64.saturating_sub(OFFER_BURN_PERCENT))
            .saturating_div(100);
        let refund_amount = execution.max_gas.saturating_add(offer_refund);

        // Remove execution
        self.scheduled_executions.shift_remove(&hash);
        self.handle_to_hash.remove(&handle);

        // Credit refund to scheduler
        let delta = self
            .balance_changes
            .entry(scheduler_hash.clone())
            .or_insert(0);
        *delta = delta.saturating_add(refund_amount as i128);

        Ok(refund_amount)
    }

    fn get_current_topoheight(&self) -> u64 {
        self.current_topoheight
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::asset::AssetData;
    use tos_common::contract::ContractStorage;
    use tos_common::crypto::PublicKey;
    use tos_kernel::ValueCell;

    // Mock provider for testing
    struct MockProvider {
        balance: u64,
    }

    impl ContractProvider for MockProvider {
        fn get_contract_balance_for_asset(
            &self,
            _contract: &Hash,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(Some((0, self.balance)))
        }

        fn get_account_balance_for_asset(
            &self,
            _key: &PublicKey,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }

        fn asset_exists(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(true)
        }

        fn load_asset_data(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, AssetData)>, anyhow::Error> {
            Ok(None)
        }

        fn load_asset_supply(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }

        fn account_exists(
            &self,
            _key: &PublicKey,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(true)
        }

        fn load_contract_module(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<Vec<u8>>, anyhow::Error> {
            Ok(None)
        }
    }

    impl ContractStorage for MockProvider {
        fn load_data(
            &self,
            _contract: &Hash,
            _key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> {
            Ok(None)
        }

        fn load_data_latest_topoheight(
            &self,
            _contract: &Hash,
            _key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<TopoHeight>, anyhow::Error> {
            Ok(None)
        }

        fn has_data(
            &self,
            _contract: &Hash,
            _key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }

        fn has_contract(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(true)
        }
    }

    #[test]
    fn test_schedule_execution_success() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 1_000_000 };

        let (handle, burned_offers) = {
            let mut adapter = TosScheduledExecutionAdapter::new(
                &mut scheduled_executions,
                &mut balance_changes,
                100, // current topoheight
                &current_contract,
                &provider,
            );

            let target_contract = [2u8; 32];
            let result = adapter.schedule_execution(
                current_contract.as_bytes(),
                &target_contract,
                0,                           // chunk_id
                &[],                         // input_data
                MIN_SCHEDULED_EXECUTION_GAS, // max_gas
                1000,                        // offer_amount
                150,                         // target_topoheight
                false,                       // is_block_end
            );

            assert!(result.is_ok());
            let handle = result.unwrap();
            assert!(handle > 0); // Valid handle
            (handle, adapter.burned_offers)
        };

        // After adapter is dropped, we can check the mutable data
        // Verify execution was added
        assert_eq!(scheduled_executions.len(), 1);

        // Verify balance was deducted
        let delta = balance_changes.get(&current_contract).unwrap();
        let expected_deduction = MIN_SCHEDULED_EXECUTION_GAS as i128 + 1000i128;
        assert_eq!(*delta, -expected_deduction);

        // Verify offer was burned (30% of 1000 = 300)
        assert_eq!(burned_offers, 300);

        // Verify handle was assigned
        assert!(handle > 0);
    }

    #[test]
    fn test_schedule_execution_insufficient_balance() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 100 }; // Very low balance

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            100,
            &current_contract,
            &provider,
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            150,
            false,
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), error_codes::ERR_INSUFFICIENT_BALANCE);
    }

    #[test]
    fn test_schedule_execution_gas_too_low() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 1_000_000 };

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            100,
            &current_contract,
            &provider,
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            100, // Too low
            1000,
            150,
            false,
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), error_codes::ERR_GAS_TOO_LOW);
    }

    #[test]
    fn test_schedule_execution_topoheight_in_past() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 1_000_000 };

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            100, // current topoheight
            &current_contract,
            &provider,
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            current_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            50, // In the past
            false,
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), error_codes::ERR_TOPOHEIGHT_IN_PAST);
    }

    #[test]
    fn test_cancel_scheduled_execution() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 1_000_000 };

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            100,
            &current_contract,
            &provider,
        );

        // Schedule first
        let target_contract = [2u8; 32];
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

        // Now cancel
        let refund = adapter
            .cancel_scheduled_execution(current_contract.as_bytes(), handle)
            .unwrap();

        // Verify refund (gas + 70% of offer)
        // 70% of 1000 = 700, plus MIN_SCHEDULED_EXECUTION_GAS
        let expected_refund = MIN_SCHEDULED_EXECUTION_GAS + 700;
        assert_eq!(refund, expected_refund);

        // Verify execution was removed
        assert!(scheduled_executions.is_empty());
    }

    #[test]
    fn test_get_scheduled_execution() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 1_000_000 };

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            100,
            &current_contract,
            &provider,
        );

        // Schedule first
        let target_contract = [2u8; 32];
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

        // Query
        let info = adapter.get_scheduled_execution(handle).unwrap().unwrap();

        assert_eq!(info.handle, handle);
        assert_eq!(info.target_contract, target_contract);
        assert_eq!(info.max_gas, MIN_SCHEDULED_EXECUTION_GAS);
        assert_eq!(info.offer_amount, 1000);
        assert_eq!(info.target_topoheight, 150);
        assert!(!info.is_block_end);
        assert_eq!(info.status, 0); // Pending
    }

    #[test]
    fn test_block_end_scheduling() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 1_000_000 };

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            100,
            &current_contract,
            &provider,
        );

        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                current_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                0,    // target_topoheight (ignored for block end)
                true, // is_block_end
            )
            .unwrap();

        let info = adapter.get_scheduled_execution(handle).unwrap().unwrap();
        assert!(info.is_block_end);
    }

    #[test]
    fn test_cancel_too_close_to_execution_fails() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 1_000_000 };

        // Current topoheight = 100, target = 101
        // With MIN_CANCELLATION_WINDOW = 1, cannot cancel when target <= current + 1
        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            100, // current_topoheight
            &current_contract,
            &provider,
        );

        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                current_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                101, // target_topoheight = current + 1 (too close)
                false,
            )
            .unwrap();

        // Try to cancel - should fail because too close to execution
        let result = adapter
            .cancel_scheduled_execution(current_contract.as_bytes(), handle)
            .unwrap();

        // Should return ERR_CANNOT_CANCEL (11)
        assert_eq!(result, error_codes::ERR_CANNOT_CANCEL);

        // Execution should still exist
        assert_eq!(scheduled_executions.len(), 1);
    }

    #[test]
    fn test_cancel_block_end_fails() {
        let mut scheduled_executions = IndexMap::new();
        let mut balance_changes = HashMap::new();
        let current_contract = Hash::new([1u8; 32]);
        let provider = MockProvider { balance: 1_000_000 };

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut scheduled_executions,
            &mut balance_changes,
            100,
            &current_contract,
            &provider,
        );

        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                current_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                0,    // ignored for block end
                true, // is_block_end
            )
            .unwrap();

        // Try to cancel BlockEnd - should fail
        let result = adapter
            .cancel_scheduled_execution(current_contract.as_bytes(), handle)
            .unwrap();

        // Should return ERR_CANNOT_CANCEL (11)
        assert_eq!(result, error_codes::ERR_CANNOT_CANCEL);

        // Execution should still exist
        assert_eq!(scheduled_executions.len(), 1);
    }
}
