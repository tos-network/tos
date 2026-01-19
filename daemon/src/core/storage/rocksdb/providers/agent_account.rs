use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        snapshot::Direction,
        AgentAccountProvider, NetworkProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    account::{AgentAccountMeta, SessionKey},
    crypto::PublicKey,
    serializer::RawBytes,
};

fn session_key_storage_key(account: &PublicKey, key_id: u64) -> [u8; 40] {
    let mut key = [0u8; 40];
    key[..32].copy_from_slice(account.as_bytes());
    key[32..].copy_from_slice(&key_id.to_be_bytes());
    key
}

#[async_trait]
impl AgentAccountProvider for RocksStorage {
    async fn get_agent_account_meta(
        &self,
        account: &PublicKey,
    ) -> Result<Option<AgentAccountMeta>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get agent account meta for {}",
                account.as_address(self.is_mainnet())
            );
        }
        self.load_optional_from_disk(Column::AgentAccountMeta, account.as_bytes())
    }

    async fn set_agent_account_meta(
        &mut self,
        account: &PublicKey,
        meta: &AgentAccountMeta,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set agent account meta for {}",
                account.as_address(self.is_mainnet())
            );
        }
        self.insert_into_disk(Column::AgentAccountMeta, account.as_bytes(), meta)
    }

    async fn delete_agent_account_meta(
        &mut self,
        account: &PublicKey,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete agent account meta for {}",
                account.as_address(self.is_mainnet())
            );
        }
        self.remove_from_disk(Column::AgentAccountMeta, account.as_bytes())
    }

    async fn get_session_key(
        &self,
        account: &PublicKey,
        key_id: u64,
    ) -> Result<Option<SessionKey>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get agent session key {} for {}",
                key_id,
                account.as_address(self.is_mainnet())
            );
        }
        let key = session_key_storage_key(account, key_id);
        self.load_optional_from_disk(Column::AgentSessionKeys, &key)
    }

    async fn set_session_key(
        &mut self,
        account: &PublicKey,
        session_key: &SessionKey,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set agent session key {} for {}",
                session_key.key_id,
                account.as_address(self.is_mainnet())
            );
        }
        let key = session_key_storage_key(account, session_key.key_id);
        self.insert_into_disk(Column::AgentSessionKeys, &key, session_key)
    }

    async fn delete_session_key(
        &mut self,
        account: &PublicKey,
        key_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete agent session key {} for {}",
                key_id,
                account.as_address(self.is_mainnet())
            );
        }
        let key = session_key_storage_key(account, key_id);
        self.remove_from_disk(Column::AgentSessionKeys, &key)
    }

    async fn get_session_keys_for_account(
        &self,
        account: &PublicKey,
    ) -> Result<Vec<SessionKey>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get agent session keys for {}",
                account.as_address(self.is_mainnet())
            );
        }
        let prefix = account.as_bytes().to_vec();
        let iter = self.iter::<RawBytes, SessionKey>(
            Column::AgentSessionKeys,
            IteratorMode::WithPrefix(&prefix, Direction::Forward),
        )?;
        let mut keys = Vec::new();
        for result in iter {
            let (key, value) = result?;
            if !key.as_ref().starts_with(&prefix) {
                break;
            }
            keys.push(value);
        }
        Ok(keys)
    }
}
