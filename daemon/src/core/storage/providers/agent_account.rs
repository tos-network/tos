use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    account::{AgentAccountMeta, SessionKey},
    crypto::PublicKey,
};

#[async_trait]
pub trait AgentAccountProvider {
    async fn get_agent_account_meta(
        &self,
        account: &PublicKey,
    ) -> Result<Option<AgentAccountMeta>, BlockchainError>;

    async fn set_agent_account_meta(
        &mut self,
        account: &PublicKey,
        meta: &AgentAccountMeta,
    ) -> Result<(), BlockchainError>;

    async fn delete_agent_account_meta(
        &mut self,
        account: &PublicKey,
    ) -> Result<(), BlockchainError>;

    async fn get_session_key(
        &self,
        account: &PublicKey,
        key_id: u64,
    ) -> Result<Option<SessionKey>, BlockchainError>;

    async fn set_session_key(
        &mut self,
        account: &PublicKey,
        session_key: &SessionKey,
    ) -> Result<(), BlockchainError>;

    async fn delete_session_key(
        &mut self,
        account: &PublicKey,
        key_id: u64,
    ) -> Result<(), BlockchainError>;

    async fn get_session_keys_for_account(
        &self,
        account: &PublicKey,
    ) -> Result<Vec<SessionKey>, BlockchainError>;
}
