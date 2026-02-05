use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Column, GenesisStateHashProvider, RocksStorage},
};
use async_trait::async_trait;
use log::trace;
use tos_common::crypto::Hash;

/// DB key for genesis state hash
const GENESIS_STATE_HASH_KEY: &[u8; 18] = b"genesis_state_hash";

#[async_trait]
impl GenesisStateHashProvider for RocksStorage {
    async fn get_genesis_state_hash(&self) -> Result<Option<Hash>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get genesis state hash");
        }
        self.load_optional_from_disk(Column::Common, GENESIS_STATE_HASH_KEY)
    }

    async fn set_genesis_state_hash(&mut self, hash: &Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set genesis state hash to {}", hash);
        }
        self.insert_into_disk(Column::Common, GENESIS_STATE_HASH_KEY, hash)
    }

    async fn has_genesis_state_hash(&self) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has genesis state hash");
        }
        self.contains_data(Column::Common, GENESIS_STATE_HASH_KEY)
    }
}
