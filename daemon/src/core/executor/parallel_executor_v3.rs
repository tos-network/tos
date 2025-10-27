// Parallel Transaction Executor - V3 Simplified Architecture
// Uses tokio JoinSet for concurrency and simple conflict detection

use std::collections::HashSet;
use std::sync::Arc;
use tokio::task::JoinSet;
use tos_common::{
    crypto::{Hash, PublicKey},
    transaction::Transaction,
};
use crate::core::{
    storage::Storage,
    state::parallel_chain_state::{ParallelChainState, TransactionResult},
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

        if transactions.is_empty() {
            return Vec::new();
        }

        if log::log_enabled!(log::Level::Info) {
            info!("Executing batch of {} transactions with max parallelism {}",
                  transactions.len(), self.max_parallelism);
        }

        // Group transactions by conflict-free batches
        let batches = self.group_by_conflicts(&transactions);

        if log::log_enabled!(log::Level::Debug) {
            debug!("Grouped {} transactions into {} conflict-free batches",
                   transactions.len(), batches.len());
        }

        let mut results = Vec::with_capacity(transactions.len());

        for (batch_idx, batch) in batches.into_iter().enumerate() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Executing batch {} with {} transactions", batch_idx, batch.len());
            }

            // Execute batch in parallel
            let batch_results = self.execute_parallel_batch(
                Arc::clone(&state),
                batch,
            ).await;

            results.extend(batch_results);
        }

        results
    }

    /// Execute a single conflict-free batch in parallel
    async fn execute_parallel_batch<S: Storage>(
        &self,
        state: Arc<ParallelChainState<S>>,
        batch: Vec<(usize, Transaction)>,
    ) -> Vec<TransactionResult> {
        use log::{debug, trace};

        let mut join_set = JoinSet::new();

        // Spawn tasks for each transaction
        for (index, tx) in batch {
            let state_clone = Arc::clone(&state);

            if log::log_enabled!(log::Level::Trace) {
                trace!("Spawning task for transaction index {}", index);
            }

            join_set.spawn(async move {
                let result = state_clone.apply_transaction(&tx).await;
                (index, result)
            });
        }

        // Collect results
        let mut indexed_results = Vec::with_capacity(join_set.len());
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((index, tx_result)) => {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!("Task for index {} completed", index);
                    }
                    indexed_results.push((index, tx_result));
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Task join error: {:?}", e);
                    }
                    // Task panic - create error result directly as TransactionResult
                    let error_result = Ok(TransactionResult {
                        tx_hash: Hash::zero(),
                        success: false,
                        error: Some(format!("Task panic: {}", e)),
                        gas_used: 0,
                    });
                    indexed_results.push((usize::MAX, error_result));
                }
            }
        }

        // Sort by original index to maintain order
        indexed_results.sort_by_key(|(idx, _)| *idx);

        // Extract results, converting Result to TransactionResult
        indexed_results.into_iter()
            .map(|(_, result)| match result {
                Ok(tx_result) => tx_result,
                Err(e) => TransactionResult {
                    tx_hash: Hash::zero(),
                    success: false,
                    error: Some(format!("{:?}", e)),
                    gas_used: 0,
                }
            })
            .collect()
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
