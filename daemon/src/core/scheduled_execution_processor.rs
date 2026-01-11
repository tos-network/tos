// Scheduled Execution Processor
//
// This module handles the processing of scheduled executions at block boundaries.
// It processes OFFERCALL-scheduled contract executions in priority order and
// handles gas accounting, offer payments, and execution status updates.

use std::collections::HashMap;

use log::{debug, error, info, trace, warn};
use tos_common::{
    block::TopoHeight,
    contract::{
        ContractProvider, ScheduledExecution, ScheduledExecutionStatus, TransferOutput,
        MAX_SCHEDULED_EXECUTIONS_PER_BLOCK, MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK,
    },
    crypto::{Hash, PublicKey},
};

use crate::core::{error::BlockchainError, storage::ContractScheduledExecutionProvider};
use crate::tako_integration::{calculate_offer_miner_reward, TakoExecutor};

/// Error category for scheduled execution failures
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledExecutionErrorKind {
    /// Execution completed successfully
    None,
    /// Contract bytecode not found
    ContractNotFound,
    /// Execution ran out of gas
    OutOfGas,
    /// Contract returned a non-zero exit code
    ContractError,
    /// Maximum deferrals reached, execution expired
    Expired,
    /// Internal system error (storage, I/O, etc.)
    InternalError,
    /// Unknown or uncategorized error
    Unknown,
}

impl Default for ScheduledExecutionErrorKind {
    fn default() -> Self {
        Self::None
    }
}

/// Result of processing a single scheduled execution
#[derive(Debug)]
pub struct ScheduledExecutionResult {
    /// The execution that was processed
    pub execution: ScheduledExecution,
    /// Whether execution succeeded
    pub success: bool,
    /// Compute units used
    pub compute_units_used: u64,
    /// Error message if failed (detailed, human-readable)
    pub error: Option<String>,
    /// Error category for programmatic handling
    pub error_kind: ScheduledExecutionErrorKind,
    /// Miner reward from offer
    pub miner_reward: u64,
    /// Events emitted during execution
    pub events: Vec<tos_program_runtime::Event>,
    /// Log messages from execution
    pub log_messages: Vec<String>,
    /// Transfers requested during execution
    pub transfers: Vec<TransferOutput>,
}

/// Result of processing all scheduled executions at a topoheight
#[derive(Debug, Default)]
pub struct BlockScheduledExecutionResults {
    /// All execution results
    pub results: Vec<ScheduledExecutionResult>,
    /// Total gas consumed by scheduled executions
    pub total_gas_used: u64,
    /// Total miner rewards from offers
    pub total_miner_rewards: u64,
    /// Number of successful executions
    pub success_count: u32,
    /// Number of failed executions
    pub failure_count: u32,
    /// Number of deferred executions
    pub deferred_count: u32,
    /// Aggregated transfers from all successful executions
    /// Key: destination public key, Value: map of asset hash to amount
    pub aggregated_transfers: HashMap<PublicKey, HashMap<Hash, u64>>,
}

/// Configuration for scheduled execution processing
#[derive(Debug, Clone)]
pub struct ScheduledExecutionConfig {
    /// Maximum executions to process per block
    pub max_executions_per_block: u32,
    /// Maximum gas to consume per block
    pub max_gas_per_block: u64,
    /// Minimum gas remaining to attempt next execution
    pub min_gas_for_execution: u64,
}

impl Default for ScheduledExecutionConfig {
    fn default() -> Self {
        Self {
            max_executions_per_block: MAX_SCHEDULED_EXECUTIONS_PER_BLOCK as u32,
            max_gas_per_block: MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK,
            min_gas_for_execution: 100_000, // 100k minimum to attempt execution
        }
    }
}

/// Process all scheduled executions at the given topoheight
///
/// This function:
/// 1. Retrieves all scheduled executions in priority order
/// 2. Executes each one within the block's gas budget
/// 3. Pays miners their offer rewards (70% of offer amount)
/// 4. Updates execution status and handles deferrals
/// 5. Returns aggregate results for inclusion in block
///
/// # Arguments
///
/// * `storage` - Storage implementing both ContractScheduledExecutionProvider and ContractProvider
/// * `topoheight` - Current block's topoheight
/// * `block_hash` - Current block hash
/// * `block_height` - Current block height
/// * `block_timestamp` - Current block timestamp
/// * `config` - Processing configuration
///
/// # Returns
///
/// `BlockScheduledExecutionResults` containing all execution outcomes
pub async fn process_scheduled_executions<S>(
    storage: &mut S,
    topoheight: TopoHeight,
    block_hash: &Hash,
    block_height: u64,
    block_timestamp: u64,
    config: &ScheduledExecutionConfig,
) -> Result<BlockScheduledExecutionResults, BlockchainError>
where
    S: ContractScheduledExecutionProvider + ContractProvider + Send,
{
    if log::log_enabled!(log::Level::Info) {
        info!(
            "Processing scheduled executions at topoheight {} (max: {}, gas: {})",
            topoheight, config.max_executions_per_block, config.max_gas_per_block
        );
    }

    let mut results = BlockScheduledExecutionResults::default();
    let mut gas_remaining = config.max_gas_per_block;
    let mut execution_count = 0u32;

    // Get priority-sorted executions for this topoheight
    let executions_iter = storage
        .get_priority_sorted_scheduled_executions_at_topoheight(topoheight)
        .await?;

    // Collect into a Vec for processing (need ownership for mutations)
    let executions: Vec<ScheduledExecution> = executions_iter
        .filter_map(|r| match r {
            Ok(e) => Some(e),
            Err(e) => {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Error loading scheduled execution: {:?}", e);
                }
                None
            }
        })
        .collect();

    if log::log_enabled!(log::Level::Debug) {
        debug!(
            "Found {} scheduled executions at topoheight {}",
            executions.len(),
            topoheight
        );
    }

    for mut execution in executions {
        // Check block limits
        if execution_count >= config.max_executions_per_block {
            if log::log_enabled!(log::Level::Info) {
                info!(
                    "Reached max executions per block: {}",
                    config.max_executions_per_block
                );
            }
            break;
        }

        // Check if we have enough gas for this execution
        if gas_remaining < config.min_gas_for_execution {
            if log::log_enabled!(log::Level::Info) {
                info!(
                    "Insufficient gas remaining: {} < {}",
                    gas_remaining, config.min_gas_for_execution
                );
            }
            break;
        }

        // Cap execution gas to remaining budget
        let exec_gas = execution.max_gas.min(gas_remaining);

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Processing execution {} for contract {} (gas: {}, offer: {})",
                execution.hash,
                execution.contract,
                exec_gas,
                execution.offer_amount
            );
        }

        // Execute the contract
        let exec_result = execute_scheduled(
            &execution,
            storage,
            topoheight,
            block_hash,
            block_height,
            block_timestamp,
            exec_gas,
        )
        .await;

        match exec_result {
            Ok(result) => {
                // Update gas accounting
                gas_remaining = gas_remaining.saturating_sub(result.compute_units_used);
                results.total_gas_used = results
                    .total_gas_used
                    .saturating_add(result.compute_units_used);

                // Calculate and track miner reward
                let miner_reward = calculate_offer_miner_reward(execution.offer_amount);
                results.total_miner_rewards =
                    results.total_miner_rewards.saturating_add(miner_reward);

                // Update execution status
                execution.status = ScheduledExecutionStatus::Executed;

                // Delete completed execution from storage
                if let Err(e) = storage
                    .delete_contract_scheduled_execution(&execution.contract, &execution)
                    .await
                {
                    if log::log_enabled!(log::Level::Error) {
                        error!("Failed to delete completed execution: {:?}", e);
                    }
                }

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Execution {} succeeded: gas={}, miner_reward={}",
                        execution.hash, result.compute_units_used, miner_reward
                    );
                }

                results.success_count = results.success_count.saturating_add(1);

                // Aggregate transfers from this execution
                for transfer in &result.transfers {
                    let asset_map = results
                        .aggregated_transfers
                        .entry(transfer.destination.clone())
                        .or_default();
                    let current = asset_map.entry(transfer.asset.clone()).or_insert(0);
                    *current = current.saturating_add(transfer.amount);
                }

                results.results.push(ScheduledExecutionResult {
                    execution,
                    success: true,
                    compute_units_used: result.compute_units_used,
                    error: None,
                    error_kind: ScheduledExecutionErrorKind::None,
                    miner_reward,
                    events: result.events,
                    log_messages: result.log_messages,
                    transfers: result.transfers,
                });

                // Successful executions consume a block slot
                execution_count = execution_count.saturating_add(1);
            }
            Err(e) => {
                // Create detailed error message with context
                let error_msg = format!(
                    "Scheduled execution failed: contract={}, execution_hash={}, chunk_id={}, error={}",
                    execution.contract,
                    execution.hash,
                    execution.chunk_id,
                    e
                );

                // Determine if we should defer or mark as failed
                let should_defer = execution.defer_count < tos_common::contract::MAX_DEFER_COUNT
                    && is_retryable_error(&e);

                if should_defer {
                    // Increment defer count
                    let max_reached = execution.defer();

                    if max_reached {
                        // Max deferrals reached - mark as expired
                        execution.status = ScheduledExecutionStatus::Expired;
                        if let Err(e) = storage
                            .delete_contract_scheduled_execution(&execution.contract, &execution)
                            .await
                        {
                            if log::log_enabled!(log::Level::Error) {
                                error!("Failed to delete expired execution: {:?}", e);
                            }
                        }
                        results.failure_count = results.failure_count.saturating_add(1);

                        if log::log_enabled!(log::Level::Warn) {
                            warn!(
                                "Execution {} expired after {} deferrals: {}",
                                execution.hash, execution.defer_count, error_msg
                            );
                        }
                    } else {
                        // Reschedule for next block by re-inserting with updated execution topoheight
                        // First, delete the old entry
                        if let Err(e) = storage
                            .delete_contract_scheduled_execution(&execution.contract, &execution)
                            .await
                        {
                            if log::log_enabled!(log::Level::Error) {
                                error!(
                                    "Failed to delete deferred execution for re-insertion: {:?}",
                                    e
                                );
                            }
                        }

                        // Update execution to target next topoheight
                        let next_topoheight = topoheight.saturating_add(1);
                        execution.kind = tos_common::contract::ScheduledExecutionKind::TopoHeight(
                            next_topoheight,
                        );

                        // Re-insert with updated defer_count and new target topoheight
                        // Keep original registration_topoheight but update execution_topoheight
                        if let Err(e) = storage
                            .set_contract_scheduled_execution_at_topoheight(
                                &execution.contract,
                                execution.registration_topoheight, // Keep original registration
                                &execution,
                                next_topoheight, // New execution target
                            )
                            .await
                        {
                            if log::log_enabled!(log::Level::Error) {
                                error!("Failed to re-insert deferred execution: {:?}", e);
                            }
                            // Mark as failed if we can't reschedule
                            execution.status = ScheduledExecutionStatus::Failed;
                            results.failure_count = results.failure_count.saturating_add(1);

                            // Failed to defer counts as a consumed slot
                            execution_count = execution_count.saturating_add(1);

                            // Failed defers still pay miner reward
                            let miner_reward = calculate_offer_miner_reward(execution.offer_amount);
                            results.total_miner_rewards =
                                results.total_miner_rewards.saturating_add(miner_reward);

                            results.results.push(ScheduledExecutionResult {
                                execution,
                                success: false,
                                compute_units_used: 0,
                                error: Some(error_msg),
                                error_kind: ScheduledExecutionErrorKind::InternalError,
                                miner_reward,
                                events: vec![],
                                log_messages: vec![],
                                transfers: vec![],
                            });
                        } else {
                            // Successfully deferred - does NOT consume block slot or miner reward
                            results.deferred_count = results.deferred_count.saturating_add(1);

                            if log::log_enabled!(log::Level::Debug) {
                                debug!(
                                    "Execution {} deferred to topoheight {} (count: {}): {}",
                                    execution.hash,
                                    next_topoheight,
                                    execution.defer_count,
                                    error_msg
                                );
                            }
                            // Note: No execution_count increment, no miner reward, no results push
                            // The execution will be processed in a future block
                        }
                        continue; // Skip the rest of the error handling
                    }
                } else {
                    // Permanent failure
                    execution.status = ScheduledExecutionStatus::Failed;
                    if let Err(e) = storage
                        .delete_contract_scheduled_execution(&execution.contract, &execution)
                        .await
                    {
                        if log::log_enabled!(log::Level::Error) {
                            error!("Failed to delete failed execution: {:?}", e);
                        }
                    }
                    results.failure_count = results.failure_count.saturating_add(1);

                    if log::log_enabled!(log::Level::Warn) {
                        warn!(
                            "Execution {} failed permanently: {}",
                            execution.hash, error_msg
                        );
                    }
                }

                // Failed/expired executions pay miner reward (to prevent spam)
                let miner_reward = calculate_offer_miner_reward(execution.offer_amount);
                results.total_miner_rewards =
                    results.total_miner_rewards.saturating_add(miner_reward);

                // Determine error kind based on execution status and error type
                let error_kind = if execution.status == ScheduledExecutionStatus::Expired {
                    ScheduledExecutionErrorKind::Expired
                } else {
                    categorize_error(&e)
                };

                results.results.push(ScheduledExecutionResult {
                    execution,
                    success: false,
                    compute_units_used: 0,
                    error: Some(error_msg),
                    error_kind,
                    miner_reward,
                    events: vec![],
                    log_messages: vec![],
                    transfers: vec![],
                });

                // Non-deferred failures consume a block slot
                execution_count = execution_count.saturating_add(1);
            }
        }
    }

    if log::log_enabled!(log::Level::Info) {
        info!(
            "Processed {} scheduled executions: {} succeeded, {} failed, {} deferred, gas={}, miner_rewards={}",
            execution_count,
            results.success_count,
            results.failure_count,
            results.deferred_count,
            results.total_gas_used,
            results.total_miner_rewards
        );
    }

    Ok(results)
}

/// Execute a single scheduled execution
async fn execute_scheduled<S>(
    execution: &ScheduledExecution,
    storage: &mut S,
    topoheight: TopoHeight,
    block_hash: &Hash,
    block_height: u64,
    block_timestamp: u64,
    max_gas: u64,
) -> Result<ExecutionOutput, BlockchainError>
where
    S: ContractProvider + Send,
{
    // Load contract bytecode
    let bytecode = storage
        .load_contract_module(&execution.contract, topoheight)?
        .ok_or_else(|| BlockchainError::ContractNotFound(execution.contract.clone()))?;

    // Prepare input data
    // Format: [2 bytes chunk_id] [input_data...]
    let mut input_data = Vec::with_capacity(2 + execution.input_data.len());
    input_data.extend_from_slice(&execution.chunk_id.to_le_bytes());
    input_data.extend_from_slice(&execution.input_data);

    // Execute the contract
    let result = TakoExecutor::execute(
        &bytecode,
        storage,
        topoheight,
        &execution.contract,
        block_hash,
        block_height,
        block_timestamp,
        &execution.hash, // Use execution hash as tx_hash
        &execution.scheduler_contract,
        &input_data,
        Some(max_gas),
    )
    .map_err(|e| BlockchainError::ModuleError(format!("Scheduled execution failed: {}", e)))?;

    Ok(ExecutionOutput {
        return_value: result.return_value,
        compute_units_used: result.compute_units_used,
        events: result.events,
        log_messages: result.log_messages,
        transfers: result.transfers,
    })
}

/// Output from a successful scheduled execution
#[allow(dead_code)]
struct ExecutionOutput {
    return_value: u64,
    compute_units_used: u64,
    events: Vec<tos_program_runtime::Event>,
    log_messages: Vec<String>,
    transfers: Vec<TransferOutput>,
}

/// Check if an error is retryable (should defer rather than fail)
///
/// Retryable errors are transient conditions that might resolve in the next block:
/// - Resource contention (semaphores, locks)
/// - I/O errors (temporary disk issues)
/// - Syncing state (chain catching up)
/// - Contract not yet deployed (might be deployed soon)
///
/// Non-retryable errors are permanent failures:
/// - Invalid signatures, proofs, or formats
/// - Logic errors in contract execution
/// - Missing data that should exist
fn is_retryable_error(error: &BlockchainError) -> bool {
    matches!(
        error,
        // Generic/unknown errors might be temporary
        BlockchainError::Unknown
        // Resource contention errors
        | BlockchainError::SemaphoreError(_)
        | BlockchainError::PoisonError(_)
        // I/O errors might be temporary
        | BlockchainError::ErrorStd(_)
        | BlockchainError::DatabaseError(_)
        // Chain syncing - might work when sync completes
        | BlockchainError::IsSyncing
        // Contract not found might resolve if deployed in a concurrent transaction
        | BlockchainError::ContractNotFound(_)
    )
}

/// Categorize a blockchain error into an error kind for reporting
fn categorize_error(error: &BlockchainError) -> ScheduledExecutionErrorKind {
    match error {
        BlockchainError::ContractNotFound(_) => ScheduledExecutionErrorKind::ContractNotFound,
        BlockchainError::ModuleError(msg) if msg.contains("out of gas") => {
            ScheduledExecutionErrorKind::OutOfGas
        }
        BlockchainError::ModuleError(_) => ScheduledExecutionErrorKind::ContractError,
        BlockchainError::DatabaseError(_)
        | BlockchainError::ErrorStd(_)
        | BlockchainError::SemaphoreError(_)
        | BlockchainError::PoisonError(_)
        | BlockchainError::IsSyncing => ScheduledExecutionErrorKind::InternalError,
        _ => ScheduledExecutionErrorKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ScheduledExecutionConfig::default();
        assert_eq!(
            config.max_executions_per_block,
            MAX_SCHEDULED_EXECUTIONS_PER_BLOCK as u32
        );
        assert_eq!(
            config.max_gas_per_block,
            MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK
        );
    }

    #[test]
    fn test_results_default() {
        let results = BlockScheduledExecutionResults::default();
        assert_eq!(results.total_gas_used, 0);
        assert_eq!(results.total_miner_rewards, 0);
        assert_eq!(results.success_count, 0);
        assert_eq!(results.failure_count, 0);
        assert_eq!(results.deferred_count, 0);
    }
}
