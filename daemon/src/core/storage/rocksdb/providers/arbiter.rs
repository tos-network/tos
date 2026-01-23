use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        ArbiterProvider, NetworkProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{arbitration::ArbiterAccount, crypto::PublicKey};

#[async_trait]
impl ArbiterProvider for RocksStorage {
    async fn list_all_arbiters(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(PublicKey, ArbiterAccount)>, BlockchainError> {
        let iter =
            self.iter::<PublicKey, ArbiterAccount>(Column::ArbiterAccounts, IteratorMode::Start)?;
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for item in iter {
            let (key, value) = item?;
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push((key, value));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

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
