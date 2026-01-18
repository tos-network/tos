use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Column, ArbiterProvider, NetworkProvider},
};
use async_trait::async_trait;
use log::trace;
use tos_common::{arbitration::ArbiterAccount, crypto::PublicKey};

use crate::core::storage::RocksStorage;

#[async_trait]
impl ArbiterProvider for RocksStorage {
    async fn get_arbiter(
        &self,
        arbiter: &PublicKey,
    ) -> Result<Option<ArbiterAccount>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get arbiter {}", arbiter.as_address(self.is_mainnet()));
        }
        self.load_optional_from_disk(Column::ArbiterAccounts, arbiter.as_bytes())
    }

    async fn set_arbiter(&mut self, arbiter: &ArbiterAccount) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set arbiter {}",
                arbiter.public_key.as_address(self.is_mainnet())
            );
        }
        self.insert_into_disk(
            Column::ArbiterAccounts,
            arbiter.public_key.as_bytes(),
            arbiter,
        )
    }

    async fn remove_arbiter(&mut self, arbiter: &PublicKey) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("remove arbiter {}", arbiter.as_address(self.is_mainnet()));
        }
        self.remove_from_disk(Column::ArbiterAccounts, arbiter.as_bytes())
    }
}
