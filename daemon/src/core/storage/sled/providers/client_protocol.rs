use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{ClientProtocolProvider, SledStorage, Tips},
};
use async_trait::async_trait;
use log::trace;
use std::{borrow::Cow, collections::HashSet};
use tos_common::{crypto::Hash, serializer::Serializer};

#[async_trait]
impl ClientProtocolProvider for SledStorage {
    fn get_block_executor_for_tx(&self, tx: &Hash) -> Result<Hash, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block executer for tx {}", tx);
        }
        self.load_from_disk(
            &self.txs_executed,
            tx.as_bytes(),
            DiskContext::BlockExecutorForTx,
        )
    }

    fn mark_tx_as_executed_in_block(
        &mut self,
        tx: &Hash,
        block: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set tx {} executed in block {}", tx, block);
        }
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.txs_executed,
            tx.as_bytes(),
            block.as_bytes(),
        )?;
        Ok(())
    }

    fn unmark_tx_from_executed(&mut self, tx: &Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("remove tx {} executed", tx);
        }
        Self::remove_from_disk_without_reading(
            self.snapshot.as_mut(),
            &self.txs_executed,
            tx.as_bytes(),
        )?;

        Ok(())
    }

    fn is_tx_executed_in_a_block(&self, tx: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is tx {} executed in a block", tx);
        }
        self.contains_data(&self.txs_executed, tx.as_bytes())
    }

    fn is_tx_executed_in_block(&self, tx: &Hash, block: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is tx {} executed in block {}", tx, block);
        }
        if let Ok(hash) = self.get_block_executor_for_tx(tx) {
            return Ok(hash == *block);
        }
        Ok(false)
    }

    fn has_tx_blocks(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has tx blocks {}", hash);
        }
        self.contains_data(&self.tx_blocks, hash.as_bytes())
    }

    fn has_block_linked_to_tx(&self, tx: &Hash, block: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has block {} linked to tx {}", block, tx);
        }
        Ok(self.has_tx_blocks(tx)? && self.get_blocks_for_tx(tx)?.contains(block))
    }

    fn add_block_linked_to_tx_if_not_present(
        &mut self,
        tx: &Hash,
        block: &Hash,
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("add block {} linked to tx {} if not present", block, tx);
        }
        let mut hashes: HashSet<Cow<'_, Hash>> = if self.has_tx_blocks(tx)? {
            self.load_from_disk(&self.tx_blocks, tx.as_bytes(), DiskContext::TxBlocks)?
        } else {
            HashSet::new()
        };

        let insert = hashes.insert(Cow::Borrowed(block));
        if insert {
            Self::insert_into_disk(
                self.snapshot.as_mut(),
                &self.tx_blocks,
                tx.as_bytes(),
                hashes.to_bytes(),
            )?;
        }

        Ok(insert)
    }

    fn get_blocks_for_tx(&self, hash: &Hash) -> Result<Tips, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get blocks for tx {}", hash);
        }
        self.load_from_disk(&self.tx_blocks, hash.as_bytes(), DiskContext::TxBlocks)
    }

    fn set_blocks_for_tx(&mut self, tx: &Hash, blocks: &Tips) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set blocks ({}) for tx {} ", blocks.len(), tx);
        }
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.tx_blocks,
            tx.as_bytes(),
            blocks.to_bytes(),
        )?;
        Ok(())
    }
}
