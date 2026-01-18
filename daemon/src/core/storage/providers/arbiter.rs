use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{arbitration::ArbiterAccount, crypto::PublicKey};

#[async_trait]
pub trait ArbiterProvider: Send + Sync {
    async fn get_arbiter(
        &self,
        arbiter: &PublicKey,
    ) -> Result<Option<ArbiterAccount>, BlockchainError>;

    async fn set_arbiter(&mut self, arbiter: &ArbiterAccount) -> Result<(), BlockchainError>;

    async fn remove_arbiter(&mut self, arbiter: &PublicKey) -> Result<(), BlockchainError>;
}
