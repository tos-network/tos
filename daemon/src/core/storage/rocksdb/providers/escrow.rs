use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        snapshot::Direction,
        EscrowHistoryKey, EscrowProvider, PendingReleaseKey,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{crypto::Hash, crypto::PublicKey, escrow::EscrowAccount, serializer::Serializer};

#[async_trait]
impl EscrowProvider for RocksStorage {
    async fn list_all_escrows(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Hash, EscrowAccount)>, BlockchainError> {
        let iter = self.iter::<Hash, EscrowAccount>(Column::EscrowAccounts, IteratorMode::Start)?;
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

    async fn add_escrow_history(
        &mut self,
        escrow_id: &Hash,
        topoheight: u64,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add escrow history {} at topoheight {} for {}",
                tx_hash,
                topoheight,
                escrow_id
            );
        }
        let key = EscrowHistoryKey {
            escrow_id: escrow_id.clone(),
            topoheight,
            tx_hash: tx_hash.clone(),
        };
        self.insert_into_disk(Column::EscrowHistory, key.to_bytes(), tx_hash)
    }

    async fn remove_escrow_history(
        &mut self,
        escrow_id: &Hash,
        topoheight: u64,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove escrow history {} at topoheight {} for {}",
                tx_hash,
                topoheight,
                escrow_id
            );
        }
        let key = EscrowHistoryKey {
            escrow_id: escrow_id.clone(),
            topoheight,
            tx_hash: tx_hash.clone(),
        };
        self.remove_from_disk(Column::EscrowHistory, key.to_bytes())
    }

    async fn list_escrow_history(
        &self,
        escrow_id: &Hash,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(u64, Hash)>, BlockchainError> {
        let mut out = Vec::new();
        let start_key = EscrowHistoryKey {
            escrow_id: escrow_id.clone(),
            topoheight: 0,
            tx_hash: Hash::zero(),
        };
        let iter = self.iter::<EscrowHistoryKey, Hash>(
            Column::EscrowHistory,
            IteratorMode::From(&start_key.to_bytes(), Direction::Forward),
        )?;
        let mut skipped = 0usize;
        for item in iter {
            let (key, value) = item?;
            if key.escrow_id != *escrow_id {
                break;
            }
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push((key.topoheight, value));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    async fn list_escrow_history_desc(
        &self,
        escrow_id: &Hash,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(u64, Hash)>, BlockchainError> {
        let mut out = Vec::new();
        let start_key = EscrowHistoryKey {
            escrow_id: escrow_id.clone(),
            topoheight: u64::MAX,
            tx_hash: Hash::max(),
        };
        let iter = self.iter::<EscrowHistoryKey, Hash>(
            Column::EscrowHistory,
            IteratorMode::From(&start_key.to_bytes(), Direction::Reverse),
        )?;
        let mut skipped = 0usize;
        for item in iter {
            let (key, value) = item?;
            if key.escrow_id != *escrow_id {
                break;
            }
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push((key.topoheight, value));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    async fn list_escrows(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<EscrowAccount>, BlockchainError> {
        let mut out = Vec::new();
        let iter = self.iter::<Hash, EscrowAccount>(Column::EscrowAccounts, IteratorMode::Start)?;
        let mut skipped = 0usize;
        for item in iter {
            let (_, escrow) = item?;
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push(escrow);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    async fn get_escrows_by_payer(
        &self,
        payer: &PublicKey,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<EscrowAccount>, BlockchainError> {
        let mut out = Vec::new();
        let iter = self.iter::<Hash, EscrowAccount>(Column::EscrowAccounts, IteratorMode::Start)?;
        let mut skipped = 0usize;
        for item in iter {
            let (_, escrow) = item?;
            if &escrow.payer != payer {
                continue;
            }
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push(escrow);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    async fn get_escrows_by_payee(
        &self,
        payee: &PublicKey,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<EscrowAccount>, BlockchainError> {
        let mut out = Vec::new();
        let iter = self.iter::<Hash, EscrowAccount>(Column::EscrowAccounts, IteratorMode::Start)?;
        let mut skipped = 0usize;
        for item in iter {
            let (_, escrow) = item?;
            if &escrow.payee != payee {
                continue;
            }
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push(escrow);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    async fn get_escrows_by_task_id(
        &self,
        task_id: &str,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<EscrowAccount>, BlockchainError> {
        let mut out = Vec::new();
        let iter = self.iter::<Hash, EscrowAccount>(Column::EscrowAccounts, IteratorMode::Start)?;
        let mut skipped = 0usize;
        for item in iter {
            let (_, escrow) = item?;
            if escrow.task_id != task_id {
                continue;
            }
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push(escrow);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
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
