// Scheduled Execution Adapter: TOS ContractScheduledExecutionProvider → TAKO ScheduledExecutionProvider
//
// This module bridges TOS's contract scheduled execution system with TAKO's OFFERCALL syscalls.
// It enables contracts to schedule future executions via the tos_offer_call syscall.

use std::io::{Error as IoError, ErrorKind};
use std::sync::{Arc, Mutex};

use tos_common::{
    block::TopoHeight,
    contract::{
        ScheduledExecution, ScheduledExecutionKind, ScheduledExecutionStatus, MAX_INPUT_DATA_SIZE,
        MAX_OFFER_AMOUNT, MAX_SCHEDULES_PER_CONTRACT_PER_WINDOW, MAX_SCHEDULING_HORIZON,
        MIN_SCHEDULED_EXECUTION_GAS, OFFER_BURN_PERCENT, RATE_LIMIT_BYPASS_OFFER,
        SCHEDULE_RATE_LIMIT_WINDOW,
    },
    crypto::Hash,
    serializer::Serializer,
};
use tos_program_runtime::{ScheduledExecutionInfo, ScheduledExecutionProvider};
use tos_tbpf::error::EbpfError;

use crate::core::storage::ContractScheduledExecutionProvider;

/// Adapter that wraps TOS's ContractScheduledExecutionProvider to implement TAKO's ScheduledExecutionProvider
///
/// # Architecture
///
/// ```text
/// TAKO syscall tos_offer_call(target, chunk_id, input, gas, offer, topoheight, is_block_end)
///     ↓
/// TosScheduledExecutionAdapter::schedule_execution()
///     ↓
/// Validate parameters (topoheight, gas, offer)
///     ↓
/// Burn 30% of offer (deduct from contract balance)
///     ↓
/// Store in ContractScheduledExecutionProvider
///     ↓
/// Return handle (derived from execution hash)
/// ```
///
/// # Priority Model
///
/// Executions are ordered by:
/// 1. Higher offer_amount (priority fee)
/// 2. Earlier registration_topoheight (FIFO for equal offers)
/// 3. Deterministic hash comparison (tiebreaker)
pub struct TosScheduledExecutionAdapter<'a, P> {
    /// TOS scheduled execution storage provider
    provider: Arc<Mutex<&'a mut P>>,
    /// Current topoheight for validation
    current_topoheight: TopoHeight,
    /// Scheduler contract hash (for authorization)
    scheduler_contract: Hash,
}

impl<'a, P: ContractScheduledExecutionProvider + Send> TosScheduledExecutionAdapter<'a, P> {
    /// Create a new scheduled execution adapter
    ///
    /// # Arguments
    ///
    /// * `provider` - TOS scheduled execution storage provider
    /// * `current_topoheight` - Current blockchain topoheight
    /// * `scheduler_contract` - Hash of the contract scheduling executions
    pub fn new(
        provider: &'a mut P,
        current_topoheight: TopoHeight,
        scheduler_contract: Hash,
    ) -> Self {
        Self {
            provider: Arc::new(Mutex::new(provider)),
            current_topoheight,
            scheduler_contract,
        }
    }

    /// Convert a ScheduledExecution to ScheduledExecutionInfo for TAKO
    fn execution_to_info(execution: &ScheduledExecution) -> ScheduledExecutionInfo {
        let (target_topoheight, is_block_end) = match execution.kind {
            ScheduledExecutionKind::TopoHeight(topo) => (topo, false),
            ScheduledExecutionKind::BlockEnd => (execution.registration_topoheight, true),
        };

        let status = match execution.status {
            ScheduledExecutionStatus::Pending => 0,
            ScheduledExecutionStatus::Executed => 1,
            ScheduledExecutionStatus::Cancelled => 2,
            ScheduledExecutionStatus::Failed => 3,
            ScheduledExecutionStatus::Expired => 4,
        };

        ScheduledExecutionInfo {
            handle: Self::hash_to_handle(&execution.hash),
            target_contract: *execution.contract.as_bytes(),
            chunk_id: execution.chunk_id,
            max_gas: execution.max_gas,
            offer_amount: execution.offer_amount,
            target_topoheight,
            is_block_end,
            registration_topoheight: execution.registration_topoheight,
            status,
        }
    }

    /// Convert a hash to a handle (first 8 bytes as u64)
    fn hash_to_handle(hash: &Hash) -> u64 {
        let bytes = hash.as_bytes();
        u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }
}

impl<'a, P: ContractScheduledExecutionProvider + Send> ScheduledExecutionProvider
    for TosScheduledExecutionAdapter<'a, P>
{
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
        // Verify scheduler matches
        if scheduler != self.scheduler_contract.as_bytes() {
            return Err(EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::PermissionDenied,
                "Scheduler contract mismatch",
            ))));
        }

        // Validate gas amount
        if max_gas < MIN_SCHEDULED_EXECUTION_GAS {
            return Err(EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::InvalidInput,
                format!(
                    "Max gas {} below minimum {}",
                    max_gas, MIN_SCHEDULED_EXECUTION_GAS
                ),
            ))));
        }

        // Validate input data size (prevent storage bloat)
        if input_data.len() > MAX_INPUT_DATA_SIZE {
            return Err(EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::InvalidInput,
                format!(
                    "Input data size {} exceeds maximum {}",
                    input_data.len(),
                    MAX_INPUT_DATA_SIZE
                ),
            ))));
        }

        // Validate offer amount (max bound)
        // Note: MIN_OFFER_AMOUNT is 0, so no minimum check needed
        if offer_amount > MAX_OFFER_AMOUNT {
            return Err(EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::InvalidInput,
                format!(
                    "Offer amount {} exceeds maximum {}",
                    offer_amount, MAX_OFFER_AMOUNT
                ),
            ))));
        }

        // Rate limiting: check if scheduler has exceeded rate limit
        // High-value offers (>= RATE_LIMIT_BYPASS_OFFER) bypass rate limiting
        if offer_amount < RATE_LIMIT_BYPASS_OFFER {
            let provider = self.provider.lock().map_err(|_| {
                EbpfError::SyscallError(Box::new(IoError::new(
                    ErrorKind::Other,
                    "Failed to acquire provider lock for rate limit check",
                )))
            })?;

            // Calculate rate limit window
            let window_start = self
                .current_topoheight
                .saturating_sub(SCHEDULE_RATE_LIMIT_WINDOW);
            let window_end = self.current_topoheight;

            // Count recent schedules by this scheduler contract
            let recent_count = tos_common::tokio::try_block_on(
                provider.count_contract_scheduled_executions_in_window(
                    &self.scheduler_contract,
                    window_start,
                    window_end,
                ),
            )
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(IoError::new(
                    ErrorKind::Other,
                    format!("Tokio runtime error during rate limit check: {}", e),
                )))
            })?
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(IoError::new(
                    ErrorKind::Other,
                    format!("Failed to count recent schedules: {}", e),
                )))
            })?;

            // Release lock before potential error return
            drop(provider);

            if recent_count >= MAX_SCHEDULES_PER_CONTRACT_PER_WINDOW {
                return Err(EbpfError::SyscallError(Box::new(IoError::new(
                    ErrorKind::Other,
                    format!(
                        "Rate limit exceeded: {} schedules in window (max {}). Increase offer to {} to bypass.",
                        recent_count, MAX_SCHEDULES_PER_CONTRACT_PER_WINDOW, RATE_LIMIT_BYPASS_OFFER
                    ),
                ))));
            }
        }

        // Determine execution kind
        let kind = if is_block_end {
            ScheduledExecutionKind::BlockEnd
        } else {
            // Validate target topoheight
            if target_topoheight <= self.current_topoheight {
                return Err(EbpfError::SyscallError(Box::new(IoError::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Target topoheight {} must be greater than current {}",
                        target_topoheight, self.current_topoheight
                    ),
                ))));
            }

            // Check scheduling horizon
            let horizon = target_topoheight.saturating_sub(self.current_topoheight);
            if horizon > MAX_SCHEDULING_HORIZON {
                return Err(EbpfError::SyscallError(Box::new(IoError::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Target topoheight {} exceeds max scheduling horizon {}",
                        target_topoheight, MAX_SCHEDULING_HORIZON
                    ),
                ))));
            }

            ScheduledExecutionKind::TopoHeight(target_topoheight)
        };

        // Convert target contract hash
        let contract = match Hash::from_bytes(target_contract) {
            Ok(h) => h,
            Err(_) => {
                return Err(EbpfError::SyscallError(Box::new(IoError::new(
                    ErrorKind::InvalidData,
                    "Invalid target contract hash",
                ))));
            }
        };

        // Create scheduled execution
        let execution = ScheduledExecution::new_offercall(
            contract.clone(),
            chunk_id,
            input_data.to_vec(),
            max_gas,
            offer_amount,
            self.scheduler_contract.clone(),
            kind.clone(),
            self.current_topoheight,
        );

        // Compute handle from hash
        let handle = Self::hash_to_handle(&execution.hash);

        // Get execution topoheight for storage
        let execution_topoheight = match kind {
            ScheduledExecutionKind::TopoHeight(topo) => topo,
            ScheduledExecutionKind::BlockEnd => self.current_topoheight,
        };

        // Store the execution
        // Note: The 30% burn is handled by the calling code before this syscall
        let mut provider = self.provider.lock().map_err(|_| {
            EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::Other,
                "Failed to acquire provider lock",
            )))
        })?;

        // Use block_on to convert async to sync
        tos_common::tokio::try_block_on(provider.set_contract_scheduled_execution_at_topoheight(
            &contract,
            self.current_topoheight,
            &execution,
            execution_topoheight,
        ))
        .map_err(|e| {
            EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::Other,
                format!("Tokio runtime error: {}", e),
            )))
        })?
        .map_err(|e| {
            EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::Other,
                format!("Failed to store scheduled execution: {}", e),
            )))
        })?;

        Ok(handle)
    }

    fn get_scheduled_execution(
        &self,
        handle: u64,
    ) -> Result<Option<ScheduledExecutionInfo>, EbpfError> {
        let provider = self.provider.lock().map_err(|_| {
            EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::Other,
                "Failed to acquire provider lock",
            )))
        })?;

        // Look up execution by handle
        let execution =
            tos_common::tokio::try_block_on(provider.get_scheduled_execution_by_handle(handle))
                .map_err(|e| {
                    EbpfError::SyscallError(Box::new(IoError::new(
                        ErrorKind::Other,
                        format!("Tokio runtime error: {}", e),
                    )))
                })?
                .map_err(|e| {
                    EbpfError::SyscallError(Box::new(IoError::new(
                        ErrorKind::Other,
                        format!("Failed to get scheduled execution: {}", e),
                    )))
                })?;

        // Convert to ScheduledExecutionInfo if found
        Ok(execution.map(|e| Self::execution_to_info(&e)))
    }

    fn cancel_scheduled_execution(
        &mut self,
        scheduler: &[u8; 32],
        handle: u64,
    ) -> Result<u64, EbpfError> {
        // Verify scheduler matches
        if scheduler != self.scheduler_contract.as_bytes() {
            return Err(EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::PermissionDenied,
                "Only the scheduler contract can cancel executions",
            ))));
        }

        let mut provider = self.provider.lock().map_err(|_| {
            EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::Other,
                "Failed to acquire provider lock",
            )))
        })?;

        // Find the execution by handle
        let execution =
            tos_common::tokio::try_block_on(provider.get_scheduled_execution_by_handle(handle))
                .map_err(|e| {
                    EbpfError::SyscallError(Box::new(IoError::new(
                        ErrorKind::Other,
                        format!("Tokio runtime error: {}", e),
                    )))
                })?
                .map_err(|e| {
                    EbpfError::SyscallError(Box::new(IoError::new(
                        ErrorKind::Other,
                        format!("Failed to find scheduled execution: {}", e),
                    )))
                })?;

        let Some(execution) = execution else {
            return Err(EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::NotFound,
                "Scheduled execution not found",
            ))));
        };

        // Verify the scheduler contract matches
        if execution.scheduler_contract != self.scheduler_contract {
            return Err(EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::PermissionDenied,
                "Only the original scheduler can cancel this execution",
            ))));
        }

        // Check if cancellation is allowed
        if !execution.can_cancel(self.current_topoheight) {
            return Err(EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::Other,
                "Cannot cancel: execution is not pending or too close to execution time",
            ))));
        }

        // Delete the execution
        tos_common::tokio::try_block_on(
            provider.delete_contract_scheduled_execution(&execution.contract, &execution),
        )
        .map_err(|e| {
            EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::Other,
                format!("Tokio runtime error: {}", e),
            )))
        })?
        .map_err(|e| {
            EbpfError::SyscallError(Box::new(IoError::new(
                ErrorKind::Other,
                format!("Failed to delete scheduled execution: {}", e),
            )))
        })?;

        // Return the refundable gas (offer_amount minus burned portion)
        // The 30% was already burned on registration, so refund the remaining 70%
        let refund = calculate_offer_miner_reward(execution.offer_amount);
        Ok(refund)
    }

    fn get_current_topoheight(&self) -> u64 {
        self.current_topoheight
    }
}

/// Calculate the burn amount for an offer (30%)
pub fn calculate_offer_burn(offer_amount: u64) -> u64 {
    offer_amount
        .saturating_mul(OFFER_BURN_PERCENT as u64)
        .saturating_div(100)
}

/// Calculate the miner reward for an offer (70%)
pub fn calculate_offer_miner_reward(offer_amount: u64) -> u64 {
    offer_amount.saturating_sub(calculate_offer_burn(offer_amount))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ============================================================================
    // Mock Provider for Testing
    // ============================================================================

    /// A stateful mock provider that tracks stored scheduled executions
    struct MockProvider {
        /// Stored executions: hash -> execution
        executions: HashMap<Hash, ScheduledExecution>,
        /// Rate limit count to return
        rate_limit_count: u64,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                executions: HashMap::new(),
                rate_limit_count: 0,
            }
        }

        fn with_rate_limit_count(mut self, count: u64) -> Self {
            self.rate_limit_count = count;
            self
        }
    }

    #[async_trait::async_trait]
    impl ContractScheduledExecutionProvider for MockProvider {
        async fn set_contract_scheduled_execution_at_topoheight(
            &mut self,
            _contract: &Hash,
            _registration_topoheight: TopoHeight,
            execution: &ScheduledExecution,
            _execution_topoheight: TopoHeight,
        ) -> Result<(), crate::core::error::BlockchainError> {
            self.executions
                .insert(execution.hash.clone(), execution.clone());
            Ok(())
        }

        async fn has_contract_scheduled_execution_at_topoheight(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, crate::core::error::BlockchainError> {
            Ok(false)
        }

        async fn get_contract_scheduled_execution_at_topoheight(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<ScheduledExecution, crate::core::error::BlockchainError> {
            Err(crate::core::error::BlockchainError::Unknown)
        }

        async fn get_registered_contract_scheduled_executions_at_topoheight<'a>(
            &'a self,
            _topoheight: TopoHeight,
        ) -> Result<
            impl Iterator<Item = Result<(TopoHeight, Hash), crate::core::error::BlockchainError>>
                + Send
                + 'a,
            crate::core::error::BlockchainError,
        > {
            Ok(std::iter::empty())
        }

        async fn get_contract_scheduled_executions_at_topoheight<'a>(
            &'a self,
            _topoheight: TopoHeight,
        ) -> Result<
            impl Iterator<Item = Result<ScheduledExecution, crate::core::error::BlockchainError>>
                + Send
                + 'a,
            crate::core::error::BlockchainError,
        > {
            Ok(std::iter::empty())
        }

        async fn get_registered_contract_scheduled_executions_in_range<'a>(
            &'a self,
            _from: TopoHeight,
            _to: TopoHeight,
        ) -> Result<
            impl futures::Stream<
                    Item = Result<
                        (TopoHeight, TopoHeight, ScheduledExecution),
                        crate::core::error::BlockchainError,
                    >,
                > + Send
                + 'a,
            crate::core::error::BlockchainError,
        > {
            Ok(futures::stream::empty())
        }

        async fn get_priority_sorted_scheduled_executions_at_topoheight<'a>(
            &'a self,
            _topoheight: TopoHeight,
        ) -> Result<
            impl Iterator<Item = Result<ScheduledExecution, crate::core::error::BlockchainError>>
                + Send
                + 'a,
            crate::core::error::BlockchainError,
        > {
            Ok(std::iter::empty())
        }

        async fn delete_contract_scheduled_execution(
            &mut self,
            _contract: &Hash,
            execution: &ScheduledExecution,
        ) -> Result<(), crate::core::error::BlockchainError> {
            self.executions.remove(&execution.hash);
            Ok(())
        }

        async fn count_contract_scheduled_executions_in_window(
            &self,
            _contract: &Hash,
            _from: TopoHeight,
            _to: TopoHeight,
        ) -> Result<u64, crate::core::error::BlockchainError> {
            Ok(self.rate_limit_count)
        }

        async fn get_scheduled_execution_by_handle(
            &self,
            handle: u64,
        ) -> Result<Option<ScheduledExecution>, crate::core::error::BlockchainError> {
            // Find execution by handle (first 8 bytes of hash)
            for execution in self.executions.values() {
                let exec_handle =
                    TosScheduledExecutionAdapter::<MockProvider>::hash_to_handle(&execution.hash);
                if exec_handle == handle {
                    return Ok(Some(execution.clone()));
                }
            }
            Ok(None)
        }
    }

    // ============================================================================
    // Offer Calculation Tests
    // ============================================================================

    #[test]
    fn test_offer_calculations() {
        // Test with 1000 tokens
        let offer = 1000u64;
        let burn = calculate_offer_burn(offer);
        let miner = calculate_offer_miner_reward(offer);

        assert_eq!(burn, 300); // 30%
        assert_eq!(miner, 700); // 70%
        assert_eq!(burn + miner, offer);
    }

    #[test]
    fn test_offer_calculations_rounding() {
        // Test with amount that doesn't divide evenly
        let offer = 101u64;
        let burn = calculate_offer_burn(offer);
        let miner = calculate_offer_miner_reward(offer);

        // 101 * 30 / 100 = 30.3 -> 30 (truncated)
        assert_eq!(burn, 30);
        assert_eq!(miner, 71);
        assert_eq!(burn + miner, offer);
    }

    #[test]
    fn test_offer_calculations_zero() {
        let offer = 0u64;
        let burn = calculate_offer_burn(offer);
        let miner = calculate_offer_miner_reward(offer);

        assert_eq!(burn, 0);
        assert_eq!(miner, 0);
    }

    #[test]
    fn test_offer_calculations_large_amount() {
        // Test with maximum allowed offer
        let offer = MAX_OFFER_AMOUNT;
        let burn = calculate_offer_burn(offer);
        let miner = calculate_offer_miner_reward(offer);

        // No overflow should occur
        assert_eq!(burn + miner, offer);
        assert!(burn > 0);
        assert!(miner > 0);
    }

    // ============================================================================
    // Hash to Handle Tests
    // ============================================================================

    #[test]
    fn test_hash_to_handle() {
        let hash = Hash::new([
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ]);

        // First 8 bytes: 0x0102030405060708
        let expected_handle = 0x0102030405060708u64;
        let handle = TosScheduledExecutionAdapter::<MockProvider>::hash_to_handle(&hash);
        assert_eq!(handle, expected_handle);
    }

    #[test]
    fn test_hash_to_handle_zero() {
        let hash = Hash::zero();
        let handle = TosScheduledExecutionAdapter::<MockProvider>::hash_to_handle(&hash);
        assert_eq!(handle, 0);
    }

    #[test]
    fn test_hash_to_handle_max() {
        let hash = Hash::new([0xFF; 32]);
        let handle = TosScheduledExecutionAdapter::<MockProvider>::hash_to_handle(&hash);
        assert_eq!(handle, u64::MAX);
    }

    // ============================================================================
    // Schedule Execution Tests
    // ============================================================================

    #[test]
    fn test_schedule_execution_success() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,                           // chunk_id
            &[],                         // input_data
            MIN_SCHEDULED_EXECUTION_GAS, // max_gas
            1000,                        // offer_amount
            150,                         // target_topoheight (current + 50)
            false,                       // is_block_end
        );

        assert!(result.is_ok());
        let handle = result.expect("test");
        assert!(handle > 0); // Valid handle (non-zero)
    }

    #[test]
    fn test_schedule_execution_with_input_data() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let input_data = vec![0xDE, 0xAD, 0xBE, 0xEF];

        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            1,                           // chunk_id
            &input_data,                 // input_data
            MIN_SCHEDULED_EXECUTION_GAS, // max_gas
            500,                         // offer_amount
            200,                         // target_topoheight
            false,                       // is_block_end
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_schedule_execution_gas_too_low() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            100, // Too low (below MIN_SCHEDULED_EXECUTION_GAS)
            1000,
            150,
            false,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("below minimum"));
    }

    #[test]
    fn test_schedule_execution_topoheight_in_past() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            50, // In the past (< current_topoheight)
            false,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("must be greater than current"));
    }

    #[test]
    fn test_schedule_execution_topoheight_at_current() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            100, // Equal to current (not allowed)
            false,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_schedule_execution_topoheight_too_far() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            current_topoheight + MAX_SCHEDULING_HORIZON + 1, // Beyond horizon
            false,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("exceeds max scheduling horizon"));
    }

    #[test]
    fn test_schedule_execution_at_max_horizon() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            current_topoheight + MAX_SCHEDULING_HORIZON, // Exactly at horizon (allowed)
            false,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_schedule_execution_scheduler_mismatch() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let wrong_scheduler = [99u8; 32];
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract,
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            &wrong_scheduler, // Wrong scheduler
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            150,
            false,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("mismatch"));
    }

    #[test]
    fn test_schedule_execution_input_data_too_large() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let large_input = vec![0u8; MAX_INPUT_DATA_SIZE + 1]; // Too large

        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &large_input,
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            150,
            false,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("exceeds maximum"));
    }

    #[test]
    fn test_schedule_execution_offer_too_high() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            MAX_OFFER_AMOUNT + 1, // Too high
            150,
            false,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("exceeds maximum"));
    }

    #[test]
    fn test_schedule_execution_rate_limit_exceeded() {
        // Create provider with rate limit already at max
        let mut provider =
            MockProvider::new().with_rate_limit_count(MAX_SCHEDULES_PER_CONTRACT_PER_WINDOW);
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            100, // Low offer (subject to rate limiting)
            150,
            false,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("Rate limit exceeded"));
    }

    #[test]
    fn test_schedule_execution_rate_limit_bypass_with_high_offer() {
        // Create provider with rate limit already at max
        let mut provider =
            MockProvider::new().with_rate_limit_count(MAX_SCHEDULES_PER_CONTRACT_PER_WINDOW);
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            RATE_LIMIT_BYPASS_OFFER, // High offer bypasses rate limit
            150,
            false,
        );

        // Should succeed despite rate limit being exceeded
        assert!(result.is_ok());
    }

    // ============================================================================
    // Block End Scheduling Tests
    // ============================================================================

    #[test]
    fn test_block_end_scheduling() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            0,    // target_topoheight (ignored for block end)
            true, // is_block_end
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_block_end_scheduling_ignores_topoheight_validation() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        // Block end should succeed even with a past topoheight (it's ignored)
        let result = adapter.schedule_execution(
            scheduler_contract.as_bytes(),
            &target_contract,
            0,
            &[],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            50,   // Past topoheight (should be ignored for block end)
            true, // is_block_end
        );

        assert!(result.is_ok());
    }

    // ============================================================================
    // Get Scheduled Execution Tests
    // ============================================================================

    #[test]
    fn test_get_scheduled_execution() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        // Schedule first
        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                scheduler_contract.as_bytes(),
                &target_contract,
                5, // chunk_id
                &[0xAB, 0xCD],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                150,
                false,
            )
            .expect("test");

        // Query
        let info = adapter
            .get_scheduled_execution(handle)
            .expect("test")
            .expect("test");

        assert_eq!(info.handle, handle);
        assert_eq!(info.target_contract, target_contract);
        assert_eq!(info.chunk_id, 5);
        assert_eq!(info.max_gas, MIN_SCHEDULED_EXECUTION_GAS);
        assert_eq!(info.offer_amount, 1000);
        assert_eq!(info.target_topoheight, 150);
        assert!(!info.is_block_end);
        assert_eq!(info.registration_topoheight, current_topoheight);
        assert_eq!(info.status, 0); // Pending
    }

    #[test]
    fn test_get_scheduled_execution_not_found() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract,
        );

        // Try to get non-existent execution
        let result = adapter.get_scheduled_execution(12345);

        assert!(result.is_ok());
        assert!(result.expect("test").is_none());
    }

    #[test]
    fn test_get_scheduled_execution_block_end() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                scheduler_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                0,    // ignored
                true, // is_block_end
            )
            .expect("test");

        let info = adapter
            .get_scheduled_execution(handle)
            .expect("test")
            .expect("test");
        assert!(info.is_block_end);
    }

    // ============================================================================
    // Cancel Scheduled Execution Tests
    // ============================================================================

    #[test]
    fn test_cancel_scheduled_execution() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        // Schedule with enough buffer to allow cancellation
        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                scheduler_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                200, // Far enough in future to allow cancellation
                false,
            )
            .expect("test");

        // Verify execution exists
        assert!(adapter
            .get_scheduled_execution(handle)
            .expect("test")
            .is_some());

        // Cancel
        let refund = adapter
            .cancel_scheduled_execution(scheduler_contract.as_bytes(), handle)
            .expect("test");

        // Verify refund is 70% of offer (miner portion)
        let expected_refund = calculate_offer_miner_reward(1000);
        assert_eq!(refund, expected_refund);

        // Verify execution was removed
        assert!(adapter
            .get_scheduled_execution(handle)
            .expect("test")
            .is_none());
    }

    #[test]
    fn test_cancel_scheduled_execution_not_found() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        // Try to cancel non-existent execution
        let result = adapter.cancel_scheduled_execution(scheduler_contract.as_bytes(), 12345);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("not found"));
    }

    #[test]
    fn test_cancel_scheduled_execution_unauthorized() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let wrong_scheduler = [99u8; 32];
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        // Schedule
        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                scheduler_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                200,
                false,
            )
            .expect("test");

        // Try to cancel with wrong scheduler
        let result = adapter.cancel_scheduled_execution(&wrong_scheduler, handle);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("scheduler"));
    }

    #[test]
    fn test_cancel_too_close_to_execution_fails() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        // Schedule at topoheight just 1 ahead (too close to cancel)
        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                scheduler_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                101, // Just 1 topoheight ahead
                false,
            )
            .expect("test");

        // Try to cancel - should fail because too close to execution
        let result = adapter.cancel_scheduled_execution(scheduler_contract.as_bytes(), handle);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("Cannot cancel"));
    }

    #[test]
    fn test_cancel_block_end_fails() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 100u64;

        let mut adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract.clone(),
        );

        // Schedule a BlockEnd execution
        let target_contract = [2u8; 32];
        let handle = adapter
            .schedule_execution(
                scheduler_contract.as_bytes(),
                &target_contract,
                0,
                &[],
                MIN_SCHEDULED_EXECUTION_GAS,
                1000,
                0,    // ignored for block end
                true, // is_block_end
            )
            .expect("test");

        // Try to cancel BlockEnd - should fail
        let result = adapter.cancel_scheduled_execution(scheduler_contract.as_bytes(), handle);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{:?}", err).contains("Cannot cancel"));
    }

    // ============================================================================
    // Get Current Topoheight Test
    // ============================================================================

    #[test]
    fn test_get_current_topoheight() {
        let mut provider = MockProvider::new();
        let scheduler_contract = Hash::new([1u8; 32]);
        let current_topoheight = 12345u64;

        let adapter = TosScheduledExecutionAdapter::new(
            &mut provider,
            current_topoheight,
            scheduler_contract,
        );

        assert_eq!(adapter.get_current_topoheight(), current_topoheight);
    }

    // ============================================================================
    // Execution Info Conversion Tests
    // ============================================================================

    #[test]
    fn test_execution_to_info_pending() {
        let execution = ScheduledExecution::new_offercall(
            Hash::new([2u8; 32]),
            1,
            vec![0xDE, 0xAD],
            50000,
            1000,
            Hash::new([1u8; 32]),
            ScheduledExecutionKind::TopoHeight(150),
            100,
        );

        let info = TosScheduledExecutionAdapter::<MockProvider>::execution_to_info(&execution);

        assert_eq!(info.target_contract, [2u8; 32]);
        assert_eq!(info.chunk_id, 1);
        assert_eq!(info.max_gas, 50000);
        assert_eq!(info.offer_amount, 1000);
        assert_eq!(info.target_topoheight, 150);
        assert!(!info.is_block_end);
        assert_eq!(info.registration_topoheight, 100);
        assert_eq!(info.status, 0); // Pending
    }

    #[test]
    fn test_execution_to_info_block_end() {
        let execution = ScheduledExecution::new_offercall(
            Hash::new([2u8; 32]),
            0,
            vec![],
            50000,
            500,
            Hash::new([1u8; 32]),
            ScheduledExecutionKind::BlockEnd,
            100,
        );

        let info = TosScheduledExecutionAdapter::<MockProvider>::execution_to_info(&execution);

        assert!(info.is_block_end);
        // For BlockEnd, target_topoheight should be registration_topoheight
        assert_eq!(info.target_topoheight, 100);
    }
}
