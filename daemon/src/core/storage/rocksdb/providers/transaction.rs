use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode},
        ClientProtocolProvider, RocksStorage, TransactionProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use std::sync::Arc;
use tos_common::{crypto::Hash, immutable::Immutable, transaction::Transaction};

#[async_trait]
impl TransactionProvider for RocksStorage {
    // Get the transaction using its hash
    async fn get_transaction(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<Transaction>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get transaction {}", hash);
        }
        let use_cache = self
            .snapshot
            .as_ref()
            .and_then(|s| s.contains(Column::Transactions, hash.as_bytes()))
            .is_none();

        if use_cache {
            if let Some(objects) = &self.cache().objects {
                if let Some(transaction) = objects.transactions_cache.lock().await.get(hash) {
                    return Ok(Immutable::Arc(transaction.clone()));
                }
            }
        }

        let transaction: Arc<Transaction> =
            Arc::new(self.load_from_disk(Column::Transactions, hash)?);
        if use_cache {
            if let Some(objects) = &self.cache().objects {
                objects
                    .transactions_cache
                    .lock()
                    .await
                    .put(hash.clone(), transaction.clone());
            }
        }

        Ok(Immutable::Arc(transaction))
    }

    // Get the transaction size using its hash
    async fn get_transaction_size(&self, hash: &Hash) -> Result<usize, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get transaction size {}", hash);
        }
        self.get_size_from_disk(Column::Transactions, hash)
    }

    // Count the number of transactions stored
    async fn count_transactions(&self) -> Result<u64, BlockchainError> {
        trace!("count transactions");
        Ok(self.cache().counter.transactions_count)
    }

    // Get all the unexecuted transactions
    // Those were not executed by the DAG
    async fn get_unexecuted_transactions<'a>(
        &'a self,
    ) -> Result<impl Iterator<Item = Result<Hash, BlockchainError>> + 'a, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get unexecuted transactions");
        }
        let iter = self.iter_keys(Column::Transactions, IteratorMode::Start)?;
        Ok(iter
            .map(|res| {
                let hash = res?;

                if self.is_tx_executed_in_a_block(&hash)? {
                    return Ok(None);
                }

                Ok(Some(hash))
            })
            .filter_map(Result::transpose))
    }

    // Check if the transaction exists
    async fn has_transaction(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has transaction {}", hash);
        }
        self.contains_data(Column::Transactions, hash)
    }

    // Check if the transaction exists
    async fn add_transaction(
        &mut self,
        hash: &Hash,
        transaction: &Transaction,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("add transaction {}", hash);
        }
        // Write to disk first, then update cache (prevents stale cache on disk failure)
        self.insert_into_disk(Column::Transactions, hash, transaction)?;
        if let Some(objects) = self.cache_mut().objects.as_mut() {
            objects
                .transactions_cache
                .get_mut()
                .put(hash.clone(), Arc::new(transaction.clone()));
        }
        Ok(())
    }

    // Delete a transaction from the storage using its hash
    async fn delete_transaction(
        &mut self,
        hash: &Hash,
    ) -> Result<Immutable<Transaction>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete transaction {}", hash);
        }
        let transaction = self.get_transaction(hash).await?;
        self.remove_from_disk(Column::Transactions, hash)?;
        if let Some(objects) = &self.cache().objects {
            objects.transactions_cache.lock().await.pop(hash);
        }
        Ok(transaction)
    }
}
