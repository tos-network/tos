use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        EscrowProvider, PendingReleaseKey,
    },
};
use async_trait::async_trait;
use log::trace;
use rocksdb::Direction;
use tos_common::{crypto::Hash, escrow::EscrowAccount, serializer::Serializer};

#[async_trait]
impl EscrowProvider for RocksStorage {
    async fn get_escrow(&self, escrow_id: &Hash) -> Result<Option<EscrowAccount>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get escrow {}", escrow_id);
        }
        self.load_optional_from_disk(Column::EscrowAccounts, escrow_id.as_bytes())
    }

    async fn set_escrow(&mut self, escrow: &EscrowAccount) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set escrow {}", escrow.id);
        }
        self.insert_into_disk(Column::EscrowAccounts, escrow.id.as_bytes(), escrow)
    }

    async fn add_pending_release(
        &mut self,
        release_at: u64,
        escrow_id: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("add pending release {} for {}", release_at, escrow_id);
        }
        let key = PendingReleaseKey {
            release_at,
            escrow_id: escrow_id.clone(),
        };
        self.insert_into_disk(Column::EscrowPendingRelease, key.to_bytes(), escrow_id)
    }

    async fn remove_pending_release(
        &mut self,
        release_at: u64,
        escrow_id: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("remove pending release {} for {}", release_at, escrow_id);
        }
        let key = PendingReleaseKey {
            release_at,
            escrow_id: escrow_id.clone(),
        };
        self.remove_from_disk(Column::EscrowPendingRelease, key.to_bytes())
    }

    async fn list_pending_releases(
        &self,
        up_to: u64,
        limit: usize,
    ) -> Result<Vec<(u64, Hash)>, BlockchainError> {
        let mut out = Vec::new();
        let start = 0u64.to_be_bytes();
        let iter = self.iter::<PendingReleaseKey, Hash>(
            Column::EscrowPendingRelease,
            IteratorMode::From(&start, Direction::Forward),
        )?;

        for item in iter {
            let (key, value) = item?;
            if key.release_at > up_to {
                break;
            }
            out.push((key.release_at, value));
            if out.len() >= limit {
                break;
            }
        }

        Ok(out)
    }
}
