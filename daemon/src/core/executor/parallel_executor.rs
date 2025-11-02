// Parallel Transaction Executor - V3 Simplified Architecture
// Uses tokio JoinSet for concurrency and simple conflict detection

use crate::core::{
    state::parallel_chain_state::{ParallelChainState, TransactionResult},
    storage::Storage,
};
use futures::FutureExt;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tos_common::{
    crypto::{Hash, Hashable, PublicKey},
    transaction::Transaction,
};

/// Parallel transaction executor with automatic conflict detection
pub struct ParallelExecutor {
    /// Maximum number of parallel tasks (defaults to CPU count)
    max_parallelism: usize,
}

impl ParallelExecutor {
    /// Create new parallel executor
    pub fn new() -> Self {
        Self {
            max_parallelism: num_cpus::get(),
        }
    }

    /// Create executor with custom parallelism limit
    pub fn with_parallelism(max_parallelism: usize) -> Self {
        Self { max_parallelism }
    }

    /// Execute transactions in parallel batches
    /// Returns results in the same order as input transactions
    pub async fn execute_batch<S: Storage>(
        &self,
        state: Arc<ParallelChainState<S>>,
        transactions: Vec<Transaction>,
    ) -> Vec<TransactionResult> {
        use log::{debug, info};

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "[PARALLEL] execute_batch ENTRY: {} transactions",
                transactions.len()
            );
        }

        if transactions.is_empty() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("[PARALLEL] execute_batch EXIT: empty batch");
            }
            return Vec::new();
        }

        if log::log_enabled!(log::Level::Info) {
            info!(
                "[PARALLEL] Executing batch of {} transactions with max parallelism {}",
                transactions.len(),
                self.max_parallelism
            );
        }

        // Group transactions by conflict-free batches
        if log::log_enabled!(log::Level::Debug) {
            debug!("[PARALLEL] Calling group_by_conflicts...");
        }
        let batches = self.group_by_conflicts(&transactions);
        let batch_count = batches.len(); // Store length before moving

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "[PARALLEL] Grouped {} transactions into {} conflict-free batches",
                transactions.len(),
                batch_count
            );
        }

        let mut results = Vec::with_capacity(transactions.len());

        for (batch_idx, batch) in batches.into_iter().enumerate() {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "[PARALLEL] Processing batch {}/{} with {} transactions",
                    batch_idx + 1,
                    batch_count,
                    batch.len()
                );
            }

            // Execute batch in parallel
            let batch_results = self.execute_parallel_batch(Arc::clone(&state), batch).await;

            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "[PARALLEL] Batch {} completed with {} results",
                    batch_idx,
                    batch_results.len()
                );
            }

            results.extend(batch_results);
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "[PARALLEL] execute_batch EXIT: {} total results",
                results.len()
            );
        }

        results
    }

    /// Execute a single conflict-free batch in parallel
    async fn execute_parallel_batch<S: Storage>(
        &self,
        state: Arc<ParallelChainState<S>>,
        batch: Vec<(usize, Transaction)>,
    ) -> Vec<TransactionResult> {
        use log::debug;

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "[PARALLEL] execute_parallel_batch ENTRY: {} transactions",
                batch.len()
            );
        }

        let mut join_set = JoinSet::new();

        // SECURITY FIX #4: Use semaphore to limit concurrent tasks
        // This prevents DoS attacks via unbounded parallelism
        // Reference: SECURITY_AUDIT_PARALLEL_EXECUTION.md - Vulnerability #4
        let semaphore = Arc::new(Semaphore::new(self.max_parallelism));

        // Spawn tasks for each transaction
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "[PARALLEL] Spawning {} async tasks (max concurrency: {})...",
                batch.len(),
                self.max_parallelism
            );
        }

        for (index, tx) in batch {
            // Acquire permit before spawning - blocks if max_parallelism limit reached
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .expect("Semaphore acquire failed");
            let state_clone = Arc::clone(&state);
            let tx_hash = tx.hash();

            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "[PARALLEL] Spawning task for index {} (tx: {})",
                    index, tx_hash
                );
            }

            join_set.spawn(async move {
                let task_future = async {
                    // Hold permit for the lifetime of this task
                    let _permit = permit;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "[PARALLEL] Task {} START: applying transaction {}",
                            index, tx_hash
                        );
                    }

                    // Wrap transaction in Arc for apply_with_partial_verify compatibility
                    let tx_arc = Arc::new(tx);
                    let result = state_clone.apply_transaction(tx_arc).await;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "[PARALLEL] Task {} END: result = {:?}",
                            index,
                            result.as_ref().map(|r| &r.success).unwrap_or(&false)
                        );
                    }

                    result
                };

                match AssertUnwindSafe(task_future).catch_unwind().await {
                    Ok(result) => {
                        let tx_result = match result {
                            Ok(tx_res) => tx_res,
                            Err(e) => TransactionResult {
                                tx_hash: tx_hash.clone(),
                                success: false,
                                error: Some(format!("Transaction failed: {:?}", e)),
                                gas_used: 0,
                            },
                        };

                        (index, tx_hash, tx_result)
                    }
                    Err(payload) => {
                        let panic_message = if let Some(&message) = payload.downcast_ref::<&str>() {
                            message.to_string()
                        } else if let Some(message) = payload.downcast_ref::<String>() {
                            message.clone()
                        } else {
                            "Unknown panic payload".to_string()
                        };

                        if log::log_enabled!(log::Level::Error) {
                            log::error!(
                                "[PARALLEL] Task {} panicked while executing tx {}: {}",
                                index,
                                tx_hash,
                                panic_message
                            );
                        }

                        let panic_result = TransactionResult {
                            tx_hash: tx_hash.clone(),
                            success: false,
                            error: Some(format!(
                                "Transaction panicked during parallel execution: {}",
                                panic_message
                            )),
                            gas_used: 0,
                        };

                        (index, tx_hash, panic_result)
                    }
                }
            });
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!("[PARALLEL] All tasks spawned, waiting for completion...");
        }

        // Collect results
        let mut indexed_results = Vec::with_capacity(join_set.len());
        let mut completed = 0;
        while let Some(result) = join_set.join_next().await {
            completed += 1;
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "[PARALLEL] Task completed {}/{}",
                    completed,
                    indexed_results.capacity()
                );
            }

            match result {
                Ok((index, tx_hash, tx_result)) => {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "[PARALLEL] Task {} join OK (tx: {}, success: {})",
                            index, tx_hash, tx_result.success
                        );
                    }
                    indexed_results.push((index, tx_result));
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Error) {
                        log::error!("[PARALLEL] Task join ERROR (unrecoverable panic): {:?}", e);
                    }

                    let error_result = TransactionResult {
                        tx_hash: Hash::zero(),
                        success: false,
                        error: Some(format!("Unrecoverable panic in parallel execution: {}", e)),
                        gas_used: 0,
                    };
                    indexed_results.push((usize::MAX, error_result));
                }
            }
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "[PARALLEL] All tasks joined, sorting {} results...",
                indexed_results.len()
            );
        }

        // Sort by original index to maintain order
        indexed_results.sort_by_key(|(idx, _)| *idx);

        if log::log_enabled!(log::Level::Debug) {
            debug!("[PARALLEL] Results sorted, extracting final results...");
        }

        // Extract results - tasks already returned TransactionResult objects
        let final_results = indexed_results
            .into_iter()
            .map(|(_, result)| result)
            .collect();

        if log::log_enabled!(log::Level::Debug) {
            debug!("[PARALLEL] execute_parallel_batch EXIT");
        }

        final_results
    }

    /// Group transactions into conflict-free batches
    /// Transactions that touch the same accounts must be in different batches
    fn group_by_conflicts(&self, transactions: &[Transaction]) -> Vec<Vec<(usize, Transaction)>> {
        use log::{debug, trace};

        let mut batches = Vec::new();
        let mut current_batch = Vec::new();
        let mut locked_accounts = HashSet::new();

        for (index, tx) in transactions.iter().enumerate() {
            let accounts = self.extract_accounts(tx);

            if log::log_enabled!(log::Level::Trace) {
                trace!("Transaction {} touches {} accounts", index, accounts.len());
            }

            // Check if any account conflicts with current batch
            let has_conflict = accounts.iter().any(|acc| locked_accounts.contains(acc));

            if has_conflict {
                // Start new batch
                if !current_batch.is_empty() {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Conflict detected at transaction {}, starting new batch (current batch size: {})",
                               index, current_batch.len());
                    }
                    batches.push(current_batch);
                    current_batch = Vec::new();
                    locked_accounts.clear();
                }
            }

            // Add to current batch
            current_batch.push((index, tx.clone()));
            locked_accounts.extend(accounts);
        }

        // Add final batch
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        batches
    }

    /// Extract all accounts touched by a transaction
    fn extract_accounts(&self, tx: &Transaction) -> Vec<PublicKey> {
        use tos_common::transaction::TransactionType;

        let mut accounts = vec![tx.get_source().clone()];

        match tx.get_data() {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    accounts.push(transfer.get_destination().clone());
                }
            }
            TransactionType::Burn(_) => {
                // Burn only touches source account
            }
            TransactionType::InvokeContract(_payload) => {
                // Contract invocation touches the contract
                // Note: Contracts are identified by Hash, not PublicKey
                // For now, we only track the source account
            }
            TransactionType::DeployContract(_) => {
                // Deploy only touches source account
            }
            TransactionType::Energy(_) => {
                // Energy only touches source account
            }
            TransactionType::MultiSig(_) => {
                // MultiSig only touches source account
            }
            TransactionType::AIMining(_) => {
                // AI Mining only touches source account
            }
        }

        accounts
    }
}

impl Default for ParallelExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to get optimal parallelism level
pub fn get_optimal_parallelism() -> usize {
    num_cpus::get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimal_parallelism() {
        let parallelism = get_optimal_parallelism();
        assert!(parallelism > 0);
        assert!(parallelism <= 1024); // Sanity check
    }

    #[test]
    fn test_executor_default() {
        let executor = ParallelExecutor::default();
        assert_eq!(executor.max_parallelism, num_cpus::get());
    }

    #[test]
    fn test_executor_custom_parallelism() {
        let executor = ParallelExecutor::with_parallelism(4);
        assert_eq!(executor.max_parallelism, 4);
    }

    // Note: Integration tests for extract_accounts, conflict_detection,
    // and parallel_execution are in daemon/tests/integration/parallel_execution_tests.rs
    // because they require creating real Transaction objects with proper signatures,
    // which is complex and better suited for integration testing.
}
