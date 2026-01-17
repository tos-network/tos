use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    crypto::Hash,
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

#[async_trait]
pub trait EscrowProvider: Send + Sync {
    async fn get_escrow(&self, escrow_id: &Hash) -> Result<Option<EscrowAccount>, BlockchainError>;
    async fn set_escrow(&mut self, escrow: &EscrowAccount) -> Result<(), BlockchainError>;

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
