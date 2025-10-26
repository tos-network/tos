// Parallel Transaction Scheduler
//
// This module implements a simplified parallel transaction scheduler that uses
// ThreadAwareAccountLocks to detect conflicts and enable parallel execution.
//
// # Architecture
//
// The scheduler is a CONFLICT DETECTION utility, not an executor.
// It identifies which transactions can run in parallel by analyzing account dependencies.
//
// # Usage Pattern
//
// ```ignore
// // 1. Create scheduler
// let mut scheduler = ParallelScheduler::new(num_threads);
//
// // 2. Find parallelizable transactions
// let results = scheduler.schedule_batch(transactions);
//
// // 3. Execute transactions (placeholder - integrate with ChainState)
// for result in results {
//     // TODO: Call tx.apply_without_verify(state) here
// }
// ```
//
// # Integration Points
//
// - Blockchain block processing (daemon/src/core/blockchain.rs)
// - Transaction application (Transaction::apply_without_verify)
// - ChainState management (daemon/src/core/state/chain_state)
//
// # Current Status
//
// ✅ Conflict detection complete
// ✅ Lock management complete
// ⚠️  Execution is placeholder (needs ChainState integration)
//
// Reference: ~/tos-network/memo/TOS_PARALLEL_EXECUTION_DESIGN_V2.md

use super::account_locks::{ThreadAwareAccountLocks, ThreadId, TryLockError};
use tos_common::transaction::Transaction;
use std::sync::Arc;

// ============================================================================
// Execution Result Types
// ============================================================================

/// Result of executing a single transaction
#[derive(Debug, Clone)]
pub struct TransactionExecutionResult {
    /// Transaction hash
    pub tx_hash: String,

    /// Whether execution succeeded
    pub success: bool,

    /// Error message if execution failed
    pub error: Option<String>,

    /// Thread ID that executed this transaction
    pub thread_id: ThreadId,
}

/// Statistics for a batch execution
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    /// Total transactions processed
    pub total_txs: usize,

    /// Successfully executed transactions
    pub successful_txs: usize,

    /// Failed transactions
    pub failed_txs: usize,

    /// Transactions that couldn't be scheduled (conflicts)
    pub unschedulable_txs: usize,

    /// Average parallelism (concurrent executions)
    pub avg_parallelism: f64,
}

// ============================================================================
// Parallel Scheduler
// ============================================================================

/// Simplified parallel transaction scheduler
///
/// This scheduler uses ThreadAwareAccountLocks to detect conflicts and
/// schedule non-conflicting transactions for parallel execution.
///
/// # Current Architecture
///
/// - Synchronous execution model (no worker threads yet)
/// - Sequential scan for schedulable transactions
/// - Immediate execution when locks are acquired
///
/// # Future Enhancements
///
/// - Worker thread pool for true parallel execution
/// - Look-ahead optimization (scan 256 transactions ahead)
/// - Async execution with tokio
/// - Priority scheduling based on fees
pub struct ParallelScheduler {
    /// Lock manager for conflict detection
    account_locks: ThreadAwareAccountLocks,

    /// Number of "virtual" threads (for lock management)
    #[allow(dead_code)] // Used for worker pool in future implementation
    num_threads: usize,

    /// Execution statistics
    stats: ExecutionStats,
}

impl ParallelScheduler {
    /// Create a new parallel scheduler
    ///
    /// # Arguments
    ///
    /// * `num_threads` - Number of virtual threads for lock management (max 64)
    ///
    /// # Panics
    ///
    /// Panics if num_threads exceeds 64 (MAX_THREADS limit)
    pub fn new(num_threads: usize) -> Self {
        Self {
            account_locks: ThreadAwareAccountLocks::new(num_threads),
            num_threads,
            stats: ExecutionStats::default(),
        }
    }

    /// Schedule and execute a batch of transactions
    ///
    /// This method attempts to execute transactions in parallel by:
    /// 1. Scanning for transactions that can be scheduled (no conflicts)
    /// 2. Acquiring locks for schedulable transactions
    /// 3. Executing transactions (placeholder for now)
    /// 4. Releasing locks
    /// 5. Repeating until all transactions are processed
    ///
    /// # Arguments
    ///
    /// * `transactions` - Batch of transactions to execute
    ///
    /// # Returns
    ///
    /// Vector of execution results, one per transaction
    ///
    /// # Note
    ///
    /// Current implementation executes transactions synchronously.
    /// Future versions will use worker thread pools for true parallelism.
    pub fn schedule_batch(
        &mut self,
        transactions: Vec<Arc<Transaction>>,
    ) -> Vec<TransactionExecutionResult> {
        let mut results = Vec::with_capacity(transactions.len());
        let mut remaining: Vec<(usize, Arc<Transaction>)> = transactions
            .into_iter()
            .enumerate()
            .collect();

        // Reset stats
        self.stats = ExecutionStats {
            total_txs: remaining.len(),
            ..Default::default()
        };

        // Process transactions until all are executed or unschedulable
        let mut rounds = 0;
        while !remaining.is_empty() {
            rounds += 1;

            // Find transactions that can be scheduled this round
            let schedulable = self.find_schedulable_transactions(&remaining);

            if schedulable.is_empty() {
                // No progress possible - all remaining transactions conflict
                // Mark them as unschedulable and break
                if log::log_enabled!(log::Level::Warn) {
                    log::warn!(
                        "Parallel scheduler: {} transactions unschedulable after {} rounds",
                        remaining.len(),
                        rounds
                    );
                }
                self.stats.unschedulable_txs = remaining.len();
                break;
            }

            // Execute schedulable transactions
            let mut executed_indices = Vec::new();
            for (idx, tx, thread_id) in schedulable {
                let result = self.execute_transaction(tx.clone(), thread_id);
                results.push((idx, result));
                executed_indices.push(idx);
            }

            // Remove executed transactions from remaining
            remaining.retain(|(idx, _)| !executed_indices.contains(idx));

            // Log progress
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "Parallel scheduler round {}: executed {}, remaining {}",
                    rounds,
                    executed_indices.len(),
                    remaining.len()
                );
            }
        }

        // Sort results by original transaction index
        results.sort_by_key(|(idx, _)| *idx);
        let final_results: Vec<TransactionExecutionResult> = results
            .into_iter()
            .map(|(_, result)| result)
            .collect();

        // Update stats
        self.stats.successful_txs = final_results.iter().filter(|r| r.success).count();
        self.stats.failed_txs = final_results.iter().filter(|r| !r.success).count();
        self.stats.avg_parallelism = self.stats.total_txs as f64 / rounds as f64;

        final_results
    }

    /// Find transactions that can be scheduled without conflicts
    ///
    /// # Arguments
    ///
    /// * `transactions` - Remaining transactions to scan
    ///
    /// # Returns
    ///
    /// Vector of (index, transaction, thread_id) tuples for schedulable transactions
    fn find_schedulable_transactions(
        &mut self,
        transactions: &[(usize, Arc<Transaction>)],
    ) -> Vec<(usize, Arc<Transaction>, ThreadId)> {
        let mut schedulable = Vec::new();

        for (idx, tx) in transactions {
            // Check if transaction supports parallel execution
            if !tx.supports_parallel_execution() {
                // V0/V1 transactions fall back to sequential execution
                // For now, skip them (they'll be handled in sequential fallback)
                continue;
            }

            // Get transaction account dependencies
            let writable = tx.writable_accounts();
            let readonly = tx.readonly_accounts();

            // Find which threads can execute this transaction
            let schedulable_threads = self.account_locks.schedulable_threads(&writable, &readonly);

            // Try to schedule on first available thread
            for thread_id in schedulable_threads.iter() {
                // Try to acquire locks for this thread
                match self.account_locks.try_lock_accounts(thread_id, &writable, &readonly) {
                    Ok(()) => {
                        // Successfully acquired locks - schedule for execution
                        schedulable.push((*idx, tx.clone(), thread_id));
                        break; // Move to next transaction
                    }
                    Err(TryLockError::InvalidThreadId { .. }) => {
                        // Shouldn't happen - schedulable_threads should only return valid IDs
                        if log::log_enabled!(log::Level::Error) {
                            log::error!("Parallel scheduler: invalid thread ID {}", thread_id);
                        }
                        break;
                    }
                    Err(_) => {
                        // Lock conflict - try next thread
                        continue;
                    }
                }
            }
        }

        schedulable
    }

    /// Execute a single transaction
    ///
    /// # Arguments
    ///
    /// * `tx` - Transaction to execute
    /// * `thread_id` - Thread ID (for lock tracking)
    ///
    /// # Returns
    ///
    /// Execution result
    ///
    /// # Note
    ///
    /// This is a placeholder implementation. The actual execution logic
    /// will be integrated with ChainState in the next phase.
    fn execute_transaction(
        &mut self,
        tx: Arc<Transaction>,
        thread_id: ThreadId,
    ) -> TransactionExecutionResult {
        // TODO: Integrate with ChainState.apply_transaction()
        // For now, return a placeholder success result

        // Placeholder: use debug format of transaction pointer
        let tx_hash = format!("tx_{:p}", Arc::as_ptr(&tx));

        // Simulate execution (placeholder)
        let success = true;
        let error = None;

        // Release locks after execution
        let writable = tx.writable_accounts();
        let readonly = tx.readonly_accounts();
        self.account_locks.unlock_accounts(thread_id, &writable, &readonly);

        TransactionExecutionResult {
            tx_hash,
            success,
            error,
            thread_id,
        }
    }

    /// Get execution statistics for the last batch
    pub fn stats(&self) -> &ExecutionStats {
        &self.stats
    }

    /// Reset the scheduler state (clear all locks)
    pub fn reset(&mut self) {
        self.account_locks.clear();
        self.stats = ExecutionStats::default();
    }

    /// Get number of currently locked accounts
    pub fn locked_account_count(&self) -> usize {
        self.account_locks.locked_account_count()
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_creation() {
        let scheduler = ParallelScheduler::new(4);
        assert_eq!(scheduler.num_threads, 4);
        assert_eq!(scheduler.locked_account_count(), 0);
    }

    #[test]
    fn test_empty_batch() {
        let mut scheduler = ParallelScheduler::new(4);
        let results = scheduler.schedule_batch(vec![]);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_stats_initialization() {
        let scheduler = ParallelScheduler::new(4);
        assert_eq!(scheduler.stats().total_txs, 0);
        assert_eq!(scheduler.stats().successful_txs, 0);
        assert_eq!(scheduler.stats().failed_txs, 0);
    }

    #[test]
    fn test_reset() {
        let mut scheduler = ParallelScheduler::new(4);
        scheduler.reset();
        assert_eq!(scheduler.locked_account_count(), 0);
        assert_eq!(scheduler.stats().total_txs, 0);
    }

    // TODO: Add more comprehensive tests once ChainState integration is complete
    // Tests to add:
    // - test_non_conflicting_transactions (should execute in parallel)
    // - test_conflicting_transactions (should execute sequentially)
    // - test_mixed_conflicts (some parallel, some sequential)
    // - test_v0_v1_fallback (legacy transactions)
    // - test_execution_stats (verify parallelism metrics)
}
