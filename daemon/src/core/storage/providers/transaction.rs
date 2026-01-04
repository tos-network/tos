use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    crypto::Hash,
    immutable::Immutable,
    transaction::{Transaction, TransactionResult},
};

#[async_trait]
pub trait TransactionProvider {
    // Get the transaction using its hash
    async fn get_transaction(&self, hash: &Hash)
        -> Result<Immutable<Transaction>, BlockchainError>;

    // Get the transaction size using its hash
    async fn get_transaction_size(&self, hash: &Hash) -> Result<usize, BlockchainError>;

    // Count the number of transactions stored
    async fn count_transactions(&self) -> Result<u64, BlockchainError>;

    // Get all the unexecuted transactions
    async fn get_unexecuted_transactions<'a>(
        &'a self,
    ) -> Result<impl Iterator<Item = Result<Hash, BlockchainError>> + 'a, BlockchainError>;

    // Check if the transaction exists
    async fn has_transaction(&self, hash: &Hash) -> Result<bool, BlockchainError>;

    // Store a new transaction
    async fn add_transaction(
        &mut self,
        hash: &Hash,
        transaction: &Transaction,
    ) -> Result<(), BlockchainError>;

    // Delete a transaction from the storage using its hash
    async fn delete_transaction(
        &mut self,
        hash: &Hash,
    ) -> Result<Immutable<Transaction>, BlockchainError>;
}

/// Provider for transaction execution results (Stake 2.0)
///
/// Stores the actual fee burned and energy consumed after transaction execution.
/// This separates input (fee_limit) from output (actual costs).
#[async_trait]
pub trait TransactionResultProvider {
    /// Get the execution result for a transaction
    async fn get_transaction_result(
        &self,
        hash: &Hash,
    ) -> Result<Option<TransactionResult>, BlockchainError>;

    /// Store the execution result for a transaction
    async fn set_transaction_result(
        &mut self,
        hash: &Hash,
        result: &TransactionResult,
    ) -> Result<(), BlockchainError>;

    /// Check if a transaction has an execution result stored
    async fn has_transaction_result(&self, hash: &Hash) -> Result<bool, BlockchainError>;

    /// Delete the execution result for a transaction
    async fn delete_transaction_result(&mut self, hash: &Hash) -> Result<(), BlockchainError>;
}
