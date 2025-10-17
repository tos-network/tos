use std::sync::Arc;
use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::{Block, BlockHeader},
    crypto::{Hash, Hashable},
    difficulty::Difficulty,
    immutable::Immutable,
    transaction::Transaction,
    varuint::VarUint
};
use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{
            BlockDifficulty,
            Column,
        },
        sled::{BLOCKS_COUNT, TXS_COUNT},
        BlockProvider,
        BlocksAtHeightProvider,
        DifficultyProvider,
        RocksStorage,
        TransactionProvider
    }
};

#[async_trait]
impl BlockProvider for RocksStorage {
    // Check if the storage has blocks
    async fn has_blocks(&self) -> Result<bool, BlockchainError> {
        trace!("has blocks");
        self.is_empty(Column::Blocks).map(|v| !v)
    }

    // Count the number of blocks stored
    async fn count_blocks(&self) -> Result<u64, BlockchainError> {
        trace!("count blocks");
        self.load_optional_from_disk(Column::Common, BLOCKS_COUNT)
            .map(|v| v.unwrap_or(0))
    }

    async fn decrease_blocks_count(&mut self, minus: u64) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("decrease blocks count by {}", minus);
        }
        let count = self.count_blocks().await?;
        self.insert_into_disk(Column::Common, BLOCKS_COUNT, &(count.saturating_sub(minus)))
    }

    // Check if the block exists using its hash
    async fn has_block_with_hash(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        trace!("has block with hash");
        self.contains_data(Column::Blocks, hash)
    }

    // Get a block with transactions using its hash
    async fn get_block_by_hash(&self, hash: &Hash) -> Result<Block, BlockchainError> {
        trace!("get block by hash");
        let header = self.get_block_header_by_hash(hash).await?;

        // TIP-2 Phase 1 fix: Load transaction hashes from BlockTransactions column
        let tx_hashes: Vec<Hash> = self.load_optional_from_disk(Column::BlockTransactions, hash.as_bytes())?
            .unwrap_or_default();

        // Load each transaction from storage
        let mut transactions = Vec::with_capacity(tx_hashes.len());
        for tx_hash in tx_hashes {
            let tx = self.get_transaction(&tx_hash).await?;
            transactions.push(tx.into_arc());
        }

        Ok(Block::new(header, transactions))
    }

    // Save a new block with its transactions and difficulty
    // Hash is Immutable to be stored efficiently in caches and sharing the same object
    // with others caches (like P2p or GetWork)
    async fn save_block(&mut self, block: Arc<BlockHeader>, txs: &[Arc<Transaction>], difficulty: Difficulty, covariance: VarUint, hash: Immutable<Hash>) -> Result<(), BlockchainError> {
        trace!("save block");

        let mut count_txs = 0;
        let mut tx_hashes = Vec::with_capacity(txs.len());
        for transaction in txs.iter() {
            let tx_hash = (**transaction).hash();
            tx_hashes.push(tx_hash.clone());
            if !self.has_transaction(&tx_hash).await? {
                self.add_transaction(&tx_hash, &transaction).await?;
                count_txs += 1;
            }
        }

        // V-22 Fix: Use fsync for critical block data
        self.insert_into_disk_sync(Column::Blocks, hash.as_bytes(), &block)?;

        // TIP-2 Phase 1 fix: Store block → transactions mapping
        self.insert_into_disk_sync(Column::BlockTransactions, hash.as_bytes(), &tx_hashes)?;

        let block_difficulty = BlockDifficulty {
            covariance,
            difficulty,
        };
        // V-22 Fix: Use fsync for critical block difficulty data
        self.insert_into_disk_sync(Column::BlockDifficulty, hash.as_bytes(), &block_difficulty)?;

        self.add_block_hash_at_blue_score(&hash, block.get_blue_score()).await?;

        if count_txs > 0 {
            count_txs += self.count_transactions().await?;
            self.insert_into_disk(Column::Common, TXS_COUNT, &count_txs)?;
        }

        let blocks_count = self.count_blocks().await?;
        self.insert_into_disk(Column::Common, BLOCKS_COUNT, &(blocks_count + 1))
    }

    // Delete a block using its hash
    async fn delete_block_with_hash(&mut self, hash: &Hash) -> Result<Block, BlockchainError> {
        trace!("delete block with hash");
        let block = self.get_block_by_hash(hash).await?;
        self.remove_from_disk(Column::Blocks, hash)?;
        // TIP-2 Phase 1 fix: Also delete the block → transactions mapping
        self.remove_from_disk(Column::BlockTransactions, hash)?;

        Ok(block)
    }
}