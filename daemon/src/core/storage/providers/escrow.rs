use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    crypto::Hash,
    crypto::PublicKey,
    escrow::EscrowAccount,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PendingReleaseKey {
    pub release_at: u64,
    pub escrow_id: Hash,
}

impl Serializer for PendingReleaseKey {
    fn write(&self, writer: &mut Writer) {
        writer.write_u64(&self.release_at);
        self.escrow_id.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let release_at = reader.read_u64()?;
        let escrow_id = Hash::read(reader)?;
        Ok(Self {
            release_at,
            escrow_id,
        })
    }

    fn size(&self) -> usize {
        8 + self.escrow_id.size()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EscrowHistoryKey {
    pub escrow_id: Hash,
    pub topoheight: u64,
    pub tx_hash: Hash,
}

impl Serializer for EscrowHistoryKey {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        writer.write_u64(&self.topoheight);
        self.tx_hash.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let topoheight = reader.read_u64()?;
        let tx_hash = Hash::read(reader)?;
        Ok(Self {
            escrow_id,
            topoheight,
            tx_hash,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size() + 8 + self.tx_hash.size()
    }
}

#[async_trait]
pub trait EscrowProvider: Send + Sync {
    // ===== Bootstrap Sync =====

    /// List all escrow accounts with skip/limit pagination (returns key-value pairs)
    async fn list_all_escrows(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Hash, EscrowAccount)>, BlockchainError>;

    async fn get_escrow(&self, escrow_id: &Hash) -> Result<Option<EscrowAccount>, BlockchainError>;
    async fn set_escrow(&mut self, escrow: &EscrowAccount) -> Result<(), BlockchainError>;
    async fn add_escrow_history(
        &mut self,
        escrow_id: &Hash,
        topoheight: u64,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError>;
    async fn remove_escrow_history(
        &mut self,
        escrow_id: &Hash,
        topoheight: u64,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError>;
    async fn list_escrow_history(
        &self,
        escrow_id: &Hash,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(u64, Hash)>, BlockchainError>;
    async fn list_escrow_history_desc(
        &self,
        escrow_id: &Hash,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(u64, Hash)>, BlockchainError>;
    async fn list_escrows(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<EscrowAccount>, BlockchainError>;
    async fn get_escrows_by_payer(
        &self,
        payer: &PublicKey,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<EscrowAccount>, BlockchainError>;
    async fn get_escrows_by_payee(
        &self,
        payee: &PublicKey,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<EscrowAccount>, BlockchainError>;
    async fn get_escrows_by_task_id(
        &self,
        task_id: &str,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<EscrowAccount>, BlockchainError>;

    async fn add_pending_release(
        &mut self,
        release_at: u64,
        escrow_id: &Hash,
    ) -> Result<(), BlockchainError>;

    async fn remove_pending_release(
        &mut self,
        release_at: u64,
        escrow_id: &Hash,
    ) -> Result<(), BlockchainError>;

    async fn list_pending_releases(
        &self,
        up_to: u64,
        limit: usize,
    ) -> Result<Vec<(u64, Hash)>, BlockchainError>;
}
