// Contract Scheduled Execution Provider Trait
// Manages storage of scheduled contract executions

use async_trait::async_trait;
use futures::Stream;
use tos_common::{block::TopoHeight, contract::ScheduledExecution, crypto::Hash};

use crate::core::error::BlockchainError;

#[async_trait]
pub trait ContractScheduledExecutionProvider {
    /// Set contract scheduled execution at provided topoheight.
    /// Caller must ensure that the topoheight configured is >= current topoheight.
    async fn set_contract_scheduled_execution_at_topoheight(
        &mut self,
        contract: &Hash,
        topoheight: TopoHeight,
        execution: &ScheduledExecution,
        execution_topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Check if a contract has a scheduled execution registered at the provided topoheight.
    /// Only one scheduled execution per contract per topoheight can exist.
    async fn has_contract_scheduled_execution_at_topoheight(
        &self,
        contract: &Hash,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError>;

    /// Get the contract scheduled execution registered at the provided topoheight.
    async fn get_contract_scheduled_execution_at_topoheight(
        &self,
        contract: &Hash,
        topoheight: TopoHeight,
    ) -> Result<ScheduledExecution, BlockchainError>;

    /// Get the registered scheduled executions at the provided topoheight.
    /// Returns iterator of (execution_topoheight, contract_hash).
    async fn get_registered_contract_scheduled_executions_at_topoheight<'a>(
        &'a self,
        topoheight: TopoHeight,
    ) -> Result<
        impl Iterator<Item = Result<(TopoHeight, Hash), BlockchainError>> + Send + 'a,
        BlockchainError,
    >;

    /// Get the scheduled executions planned for execution at the provided topoheight.
    async fn get_contract_scheduled_executions_at_topoheight<'a>(
        &'a self,
        topoheight: TopoHeight,
    ) -> Result<
        impl Iterator<Item = Result<ScheduledExecution, BlockchainError>> + Send + 'a,
        BlockchainError,
    >;

    /// Get the registered scheduled executions in a topoheight range (inclusive).
    /// Returns a stream of (execution_topoheight, registration_topoheight, execution).
    async fn get_registered_contract_scheduled_executions_in_range<'a>(
        &'a self,
        minimum_topoheight: TopoHeight,
        maximum_topoheight: TopoHeight,
    ) -> Result<
        impl Stream<Item = Result<(TopoHeight, TopoHeight, ScheduledExecution), BlockchainError>>
            + Send
            + 'a,
        BlockchainError,
    >;

    /// Get scheduled executions at topoheight, sorted by priority (OFFERCALL ordering).
    /// Priority order: higher offer first, then FIFO by registration time, then by contract ID.
    /// This is used by the execution engine to process high-priority executions first.
    async fn get_priority_sorted_scheduled_executions_at_topoheight<'a>(
        &'a self,
        topoheight: TopoHeight,
    ) -> Result<
        impl Iterator<Item = Result<ScheduledExecution, BlockchainError>> + Send + 'a,
        BlockchainError,
    >;

    /// Delete a scheduled execution and its priority index entry.
    /// Called when execution is complete, cancelled, or failed.
    async fn delete_contract_scheduled_execution(
        &mut self,
        contract: &Hash,
        execution: &ScheduledExecution,
    ) -> Result<(), BlockchainError>;
}
