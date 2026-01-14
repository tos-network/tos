use std::{
    collections::HashMap,
    io,
    sync::Mutex,
};

use async_trait::async_trait;
use tos_common::{
    block::TopoHeight,
    contract::ContractProvider,
    crypto::Hash,
    native_asset::{
        AdminDelay, AgentAuthorization, Allowance, BalanceCheckpoint, Checkpoint, Delegation,
        DelegationCheckpoint, Escrow, FreezeState, NativeAssetData, PauseState, RoleConfig, RoleId,
        SupplyCheckpoint, TimelockOperation, TokenKey, TokenValue, TokenLock,
    },
    versioned_type::VersionedState,
};

use crate::core::{
    error::BlockchainError,
    storage::{NativeAssetProvider, StorageWriteBatch},
};

pub struct ContractTokenProvider<'a> {
    provider: &'a (dyn ContractProvider + Send),
    contract_hash: &'a Hash,
    topoheight: TopoHeight,
    cache: Mutex<&'a mut HashMap<TokenKey, (VersionedState, TokenValue)>>,
}

impl<'a> ContractTokenProvider<'a> {
    pub fn new(
        provider: &'a (dyn ContractProvider + Send),
        contract_hash: &'a Hash,
        topoheight: TopoHeight,
        cache: &'a mut HashMap<TokenKey, (VersionedState, TokenValue)>,
    ) -> Self {
        Self {
            provider,
            contract_hash,
            topoheight,
            cache: Mutex::new(cache),
        }
    }

    fn with_cache<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<TokenKey, (VersionedState, TokenValue)>) -> R,
    {
        let mut cache_ref = self.cache.lock().expect("token cache mutex poisoned");
        f(&mut *cache_ref)
    }

    fn map_anyhow(err: anyhow::Error) -> BlockchainError {
        BlockchainError::ErrorStd(io::Error::new(io::ErrorKind::Other, err.to_string()))
    }

    async fn get_cached_value(&self, key: &TokenKey) -> Result<Option<TokenValue>, BlockchainError> {
        if let Some((_, value)) = self.with_cache(|cache| cache.get(key).cloned()) {
            return match value {
                TokenValue::Deleted => Ok(None),
                other => Ok(Some(other)),
            };
        }

        let fetched = self
            .provider
            .get_contract_token_ext(self.contract_hash, key, self.topoheight)
            .map_err(Self::map_anyhow)?;

        if let Some((topo, value)) = fetched {
            self.with_cache(|cache| {
                cache.insert(key.clone(), (VersionedState::FetchedAt(topo), value.clone()));
            });
            return Ok(Some(value));
        }

        Ok(None)
    }

    async fn set_cached_value(
        &self,
        key: TokenKey,
        value: TokenValue,
    ) -> Result<(), BlockchainError> {
        let existing_state = self.with_cache(|cache| cache.get(&key).map(|(s, _)| *s));
        let state = match existing_state {
            Some(mut state) => {
                state.mark_updated();
                state
            }
            None => match self
                .provider
                .get_contract_token_ext(self.contract_hash, &key, self.topoheight)
                .map_err(Self::map_anyhow)?
            {
                Some((topo, _)) => VersionedState::Updated(topo),
                None => VersionedState::New,
            },
        };

        self.with_cache(|cache| {
            cache.insert(key, (state, value));
        });
        Ok(())
    }

    async fn delete_cached_value(&self, key: TokenKey) -> Result<(), BlockchainError> {
        self.set_cached_value(key, TokenValue::Deleted).await
    }
}

#[async_trait(?Send)]
impl NativeAssetProvider for ContractTokenProvider<'_> {
    async fn has_native_asset(&self, asset: &Hash) -> Result<bool, BlockchainError> {
        let key = TokenKey::Asset(asset.clone());
        Ok(self.get_cached_value(&key).await?.is_some())
    }

    async fn get_native_asset(&self, asset: &Hash) -> Result<NativeAssetData, BlockchainError> {
        let key = TokenKey::Asset(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Asset(data)) => Ok(data),
            _ => Err(BlockchainError::AssetNotFound(asset.clone())),
        }
    }

    async fn set_native_asset(
        &mut self,
        asset: &Hash,
        data: &NativeAssetData,
    ) -> Result<(), BlockchainError> {
        self.set_cached_value(TokenKey::Asset(asset.clone()), TokenValue::Asset(data.clone()))
            .await
    }

    async fn get_native_asset_supply(&self, asset: &Hash) -> Result<u64, BlockchainError> {
        let key = TokenKey::Supply(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Supply(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_supply(
        &mut self,
        asset: &Hash,
        supply: u64,
    ) -> Result<(), BlockchainError> {
        self.set_cached_value(TokenKey::Supply(asset.clone()), TokenValue::Supply(supply))
            .await
    }

    async fn get_native_asset_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        let key = TokenKey::Balance {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Balance(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_balance(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        balance: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Balance {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::Balance(balance)).await
    }

    async fn has_native_asset_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        let key = TokenKey::Balance {
            asset: asset.clone(),
            account: *account,
        };
        Ok(self.get_cached_value(&key).await?.is_some())
    }

    async fn get_native_asset_allowance(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<Allowance, BlockchainError> {
        let key = TokenKey::Allowance {
            asset: asset.clone(),
            owner: *owner,
            spender: *spender,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Allowance(value)) => Ok(value),
            _ => Ok(Allowance::default()),
        }
    }

    async fn set_native_asset_allowance(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
        allowance: &Allowance,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Allowance {
            asset: asset.clone(),
            owner: *owner,
            spender: *spender,
        };
        self.set_cached_value(key, TokenValue::Allowance(allowance.clone()))
            .await
    }

    async fn delete_native_asset_allowance(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Allowance {
            asset: asset.clone(),
            owner: *owner,
            spender: *spender,
        };
        self.delete_cached_value(key).await
    }

    async fn get_native_asset_lock(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<TokenLock, BlockchainError> {
        let key = TokenKey::Lock {
            asset: asset.clone(),
            account: *account,
            lock_id,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Lock(value)) => Ok(value),
            _ => Err(BlockchainError::Unknown),
        }
    }

    async fn set_native_asset_lock(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock: &TokenLock,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Lock {
            asset: asset.clone(),
            account: *account,
            lock_id: lock.id,
        };
        self.set_cached_value(key, TokenValue::Lock(lock.clone()))
            .await
    }

    async fn delete_native_asset_lock(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Lock {
            asset: asset.clone(),
            account: *account,
            lock_id,
        };
        self.delete_cached_value(key).await
    }

    async fn get_native_asset_lock_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        let key = TokenKey::LockCount {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::LockCount(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_lock_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::LockCount {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::LockCount(count))
            .await
    }

    async fn get_native_asset_next_lock_id(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        let key = TokenKey::NextLockId {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::NextLockId(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_next_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        next_id: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::NextLockId {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::NextLockId(next_id))
            .await
    }

    async fn get_native_asset_locked_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        let key = TokenKey::LockedBalance {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::LockedBalance(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_locked_balance(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        locked: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::LockedBalance {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::LockedBalance(locked))
            .await
    }

    async fn get_native_asset_role_config(
        &self,
        asset: &Hash,
        role: &RoleId,
    ) -> Result<RoleConfig, BlockchainError> {
        let key = TokenKey::RoleConfig {
            asset: asset.clone(),
            role: *role,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::RoleConfig(value)) => Ok(value),
            _ => Ok(RoleConfig::default()),
        }
    }

    async fn set_native_asset_role_config(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        config: &RoleConfig,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::RoleConfig {
            asset: asset.clone(),
            role: *role,
        };
        self.set_cached_value(key, TokenValue::RoleConfig(config.clone()))
            .await
    }

    async fn has_native_asset_role(
        &self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        let key = TokenKey::RoleMember {
            asset: asset.clone(),
            role: *role,
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::RoleMemberGrantedAt(value)) => Ok(value > 0),
            _ => Ok(false),
        }
    }

    async fn grant_native_asset_role(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
        granted_at: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::RoleMember {
            asset: asset.clone(),
            role: *role,
            account: *account,
        };
        self.set_cached_value(key, TokenValue::RoleMemberGrantedAt(granted_at))
            .await
    }

    async fn revoke_native_asset_role(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::RoleMember {
            asset: asset.clone(),
            role: *role,
            account: *account,
        };
        self.delete_cached_value(key).await
    }

    async fn get_native_asset_pause_state(
        &self,
        asset: &Hash,
    ) -> Result<PauseState, BlockchainError> {
        let key = TokenKey::PauseState(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::PauseState(value)) => Ok(value),
            _ => Ok(PauseState::default()),
        }
    }

    async fn set_native_asset_pause_state(
        &mut self,
        asset: &Hash,
        state: &PauseState,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::PauseState(asset.clone());
        self.set_cached_value(key, TokenValue::PauseState(state.clone()))
            .await
    }

    async fn get_native_asset_freeze_state(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<FreezeState, BlockchainError> {
        let key = TokenKey::FreezeState {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::FreezeState(value)) => Ok(value),
            _ => Ok(FreezeState::default()),
        }
    }

    async fn set_native_asset_freeze_state(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        state: &FreezeState,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::FreezeState {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::FreezeState(state.clone()))
            .await
    }

    async fn get_native_asset_escrow_counter(
        &self,
        asset: &Hash,
    ) -> Result<u64, BlockchainError> {
        let key = TokenKey::EscrowCounter(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::EscrowCounter(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_escrow_counter(
        &mut self,
        asset: &Hash,
        counter: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::EscrowCounter(asset.clone());
        self.set_cached_value(key, TokenValue::EscrowCounter(counter))
            .await
    }

    async fn get_native_asset_escrow(
        &self,
        asset: &Hash,
        escrow_id: u64,
    ) -> Result<Escrow, BlockchainError> {
        let key = TokenKey::Escrow {
            asset: asset.clone(),
            escrow_id,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Escrow(value)) => Ok(value),
            _ => Err(BlockchainError::Unknown),
        }
    }

    async fn set_native_asset_escrow(
        &mut self,
        asset: &Hash,
        escrow: &Escrow,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Escrow {
            asset: asset.clone(),
            escrow_id: escrow.id,
        };
        self.set_cached_value(key, TokenValue::Escrow(escrow.clone()))
            .await
    }

    async fn delete_native_asset_escrow(
        &mut self,
        asset: &Hash,
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Escrow {
            asset: asset.clone(),
            escrow_id,
        };
        self.delete_cached_value(key).await
    }

    async fn get_native_asset_permit_nonce(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        let key = TokenKey::PermitNonce {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::PermitNonce(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_permit_nonce(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        nonce: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::PermitNonce {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::PermitNonce(nonce))
            .await
    }

    async fn get_native_asset_delegation(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<Delegation, BlockchainError> {
        let key = TokenKey::Delegation {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Delegation(value)) => Ok(value),
            _ => Ok(Delegation::default()),
        }
    }

    async fn set_native_asset_delegation(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        delegation: &Delegation,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Delegation {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::Delegation(delegation.clone()))
            .await
    }

    async fn get_native_asset_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        let key = TokenKey::CheckpointCount {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::CheckpointCount(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::CheckpointCount {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::CheckpointCount(count))
            .await
    }

    async fn get_native_asset_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<Checkpoint, BlockchainError> {
        let key = TokenKey::Checkpoint {
            asset: asset.clone(),
            account: *account,
            index,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Checkpoint(value)) => Ok(value),
            _ => Err(BlockchainError::Unknown),
        }
    }

    async fn set_native_asset_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &Checkpoint,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::Checkpoint {
            asset: asset.clone(),
            account: *account,
            index,
        };
        self.set_cached_value(key, TokenValue::Checkpoint(checkpoint.clone()))
            .await
    }

    async fn get_native_asset_balance_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        let key = TokenKey::BalanceCheckpointCount {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::BalanceCheckpointCount(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_balance_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::BalanceCheckpointCount {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::BalanceCheckpointCount(count))
            .await
    }

    async fn get_native_asset_balance_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<BalanceCheckpoint, BlockchainError> {
        let key = TokenKey::BalanceCheckpoint {
            asset: asset.clone(),
            account: *account,
            index,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::BalanceCheckpoint(value)) => Ok(value),
            _ => Err(BlockchainError::Unknown),
        }
    }

    async fn set_native_asset_balance_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &BalanceCheckpoint,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::BalanceCheckpoint {
            asset: asset.clone(),
            account: *account,
            index,
        };
        self.set_cached_value(key, TokenValue::BalanceCheckpoint(checkpoint.clone()))
            .await
    }

    async fn get_native_asset_delegation_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        let key = TokenKey::DelegationCheckpointCount {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::DelegationCheckpointCount(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_delegation_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::DelegationCheckpointCount {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::DelegationCheckpointCount(count))
            .await
    }

    async fn get_native_asset_delegation_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<DelegationCheckpoint, BlockchainError> {
        let key = TokenKey::DelegationCheckpoint {
            asset: asset.clone(),
            account: *account,
            index,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::DelegationCheckpoint(value)) => Ok(value),
            _ => Err(BlockchainError::Unknown),
        }
    }

    async fn set_native_asset_delegation_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &DelegationCheckpoint,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::DelegationCheckpoint {
            asset: asset.clone(),
            account: *account,
            index,
        };
        self.set_cached_value(key, TokenValue::DelegationCheckpoint(checkpoint.clone()))
            .await
    }

    async fn get_native_asset_supply_checkpoint_count(
        &self,
        asset: &Hash,
    ) -> Result<u32, BlockchainError> {
        let key = TokenKey::SupplyCheckpointCount(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::SupplyCheckpointCount(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_supply_checkpoint_count(
        &mut self,
        asset: &Hash,
        count: u32,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::SupplyCheckpointCount(asset.clone());
        self.set_cached_value(key, TokenValue::SupplyCheckpointCount(count))
            .await
    }

    async fn get_native_asset_supply_checkpoint(
        &self,
        asset: &Hash,
        index: u32,
    ) -> Result<SupplyCheckpoint, BlockchainError> {
        let key = TokenKey::SupplyCheckpoint {
            asset: asset.clone(),
            index,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::SupplyCheckpoint(value)) => Ok(value),
            _ => Err(BlockchainError::Unknown),
        }
    }

    async fn set_native_asset_supply_checkpoint(
        &mut self,
        asset: &Hash,
        index: u32,
        checkpoint: &SupplyCheckpoint,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::SupplyCheckpoint {
            asset: asset.clone(),
            index,
        };
        self.set_cached_value(key, TokenValue::SupplyCheckpoint(checkpoint.clone()))
            .await
    }

    async fn get_native_asset_vote_power(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        let key = TokenKey::VotePower {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::VotePower(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_vote_power(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        votes: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::VotePower {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::VotePower(votes)).await
    }

    async fn get_native_asset_role_members(
        &self,
        asset: &Hash,
        role: &RoleId,
    ) -> Result<Vec<[u8; 32]>, BlockchainError> {
        let key = TokenKey::RoleMembers {
            asset: asset.clone(),
            role: *role,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::RoleMembers(value)) => Ok(value),
            _ => Ok(Vec::new()),
        }
    }

    async fn add_native_asset_role_member(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let mut members = self.get_native_asset_role_members(asset, role).await?;
        if !members.contains(account) {
            members.push(*account);
        }
        let key = TokenKey::RoleMembers {
            asset: asset.clone(),
            role: *role,
        };
        self.set_cached_value(key, TokenValue::RoleMembers(members))
            .await
    }

    async fn remove_native_asset_role_member(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let mut members = self.get_native_asset_role_members(asset, role).await?;
        members.retain(|member| member != account);
        let key = TokenKey::RoleMembers {
            asset: asset.clone(),
            role: *role,
        };
        self.set_cached_value(key, TokenValue::RoleMembers(members))
            .await
    }

    async fn get_native_asset_pending_admin(
        &self,
        asset: &Hash,
    ) -> Result<Option<[u8; 32]>, BlockchainError> {
        let key = TokenKey::PendingAdmin(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::PendingAdmin(value)) => Ok(value),
            _ => Ok(None),
        }
    }

    async fn set_native_asset_pending_admin(
        &mut self,
        asset: &Hash,
        admin: Option<&[u8; 32]>,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::PendingAdmin(asset.clone());
        self.set_cached_value(
            key,
            TokenValue::PendingAdmin(admin.map(|a| *a)),
        )
        .await
    }

    async fn get_native_asset_metadata_uri(
        &self,
        asset: &Hash,
    ) -> Result<Option<String>, BlockchainError> {
        let key = TokenKey::MetadataUri(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::MetadataUri(value)) => Ok(value),
            _ => Ok(None),
        }
    }

    async fn set_native_asset_metadata_uri(
        &mut self,
        asset: &Hash,
        uri: Option<&str>,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::MetadataUri(asset.clone());
        self.set_cached_value(key, TokenValue::MetadataUri(uri.map(|v| v.to_owned())))
            .await
    }

    async fn get_native_asset_lock_ids(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<Vec<u64>, BlockchainError> {
        let key = TokenKey::LockIds {
            asset: asset.clone(),
            account: *account,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::LockIds(value)) => Ok(value),
            _ => Ok(Vec::new()),
        }
    }

    async fn add_native_asset_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        let mut ids = self.get_native_asset_lock_ids(asset, account).await?;
        if !ids.contains(&lock_id) {
            ids.push(lock_id);
        }
        let key = TokenKey::LockIds {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::LockIds(ids)).await
    }

    async fn remove_native_asset_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        let mut ids = self.get_native_asset_lock_ids(asset, account).await?;
        ids.retain(|id| *id != lock_id);
        let key = TokenKey::LockIds {
            asset: asset.clone(),
            account: *account,
        };
        self.set_cached_value(key, TokenValue::LockIds(ids)).await
    }

    async fn get_native_asset_user_escrows(
        &self,
        asset: &Hash,
        user: &[u8; 32],
    ) -> Result<Vec<u64>, BlockchainError> {
        let key = TokenKey::UserEscrows {
            asset: asset.clone(),
            user: *user,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::UserEscrows(value)) => Ok(value),
            _ => Ok(Vec::new()),
        }
    }

    async fn add_native_asset_user_escrow(
        &mut self,
        asset: &Hash,
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        let mut escrows = self.get_native_asset_user_escrows(asset, user).await?;
        if !escrows.contains(&escrow_id) {
            escrows.push(escrow_id);
        }
        let key = TokenKey::UserEscrows {
            asset: asset.clone(),
            user: *user,
        };
        self.set_cached_value(key, TokenValue::UserEscrows(escrows))
            .await
    }

    async fn remove_native_asset_user_escrow(
        &mut self,
        asset: &Hash,
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        let mut escrows = self.get_native_asset_user_escrows(asset, user).await?;
        escrows.retain(|id| *id != escrow_id);
        let key = TokenKey::UserEscrows {
            asset: asset.clone(),
            user: *user,
        };
        self.set_cached_value(key, TokenValue::UserEscrows(escrows))
            .await
    }

    async fn get_native_asset_owner_agents(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, BlockchainError> {
        let key = TokenKey::OwnerAgents {
            asset: asset.clone(),
            owner: *owner,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::OwnerAgents(value)) => Ok(value),
            _ => Ok(Vec::new()),
        }
    }

    async fn add_native_asset_owner_agent(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let mut agents = self.get_native_asset_owner_agents(asset, owner).await?;
        if !agents.contains(agent) {
            agents.push(*agent);
        }
        let key = TokenKey::OwnerAgents {
            asset: asset.clone(),
            owner: *owner,
        };
        self.set_cached_value(key, TokenValue::OwnerAgents(agents))
            .await
    }

    async fn remove_native_asset_owner_agent(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let mut agents = self.get_native_asset_owner_agents(asset, owner).await?;
        agents.retain(|a| a != agent);
        let key = TokenKey::OwnerAgents {
            asset: asset.clone(),
            owner: *owner,
        };
        self.set_cached_value(key, TokenValue::OwnerAgents(agents))
            .await
    }

    async fn get_native_asset_role_member(
        &self,
        asset: &Hash,
        role: &RoleId,
        index: u32,
    ) -> Result<[u8; 32], BlockchainError> {
        let members = self.get_native_asset_role_members(asset, role).await?;
        members
            .get(index as usize)
            .copied()
            .ok_or(BlockchainError::Unknown)
    }

    async fn get_native_asset_agent_auth(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<AgentAuthorization, BlockchainError> {
        let key = TokenKey::AgentAuth {
            asset: asset.clone(),
            owner: *owner,
            agent: *agent,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::AgentAuth(value)) => Ok(value),
            _ => Err(BlockchainError::Unknown),
        }
    }

    async fn set_native_asset_agent_auth(
        &mut self,
        asset: &Hash,
        auth: &AgentAuthorization,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::AgentAuth {
            asset: asset.clone(),
            owner: auth.owner,
            agent: auth.agent,
        };
        self.set_cached_value(key, TokenValue::AgentAuth(auth.clone()))
            .await
    }

    async fn delete_native_asset_agent_auth(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::AgentAuth {
            asset: asset.clone(),
            owner: *owner,
            agent: *agent,
        };
        self.delete_cached_value(key).await
    }

    async fn has_native_asset_agent_auth(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        let key = TokenKey::AgentAuth {
            asset: asset.clone(),
            owner: *owner,
            agent: *agent,
        };
        Ok(self.get_cached_value(&key).await?.is_some())
    }

    async fn get_native_asset_admin_delay(
        &self,
        asset: &Hash,
    ) -> Result<AdminDelay, BlockchainError> {
        let key = TokenKey::AdminDelay(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::AdminDelay(value)) => Ok(value),
            _ => Ok(AdminDelay::default()),
        }
    }

    async fn set_native_asset_admin_delay(
        &mut self,
        asset: &Hash,
        delay: &AdminDelay,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::AdminDelay(asset.clone());
        self.set_cached_value(key, TokenValue::AdminDelay(delay.clone()))
            .await
    }

    async fn get_native_asset_timelock_min_delay(
        &self,
        asset: &Hash,
    ) -> Result<u64, BlockchainError> {
        let key = TokenKey::TimelockMinDelay(asset.clone());
        match self.get_cached_value(&key).await? {
            Some(TokenValue::TimelockMinDelay(value)) => Ok(value),
            _ => Ok(0),
        }
    }

    async fn set_native_asset_timelock_min_delay(
        &mut self,
        asset: &Hash,
        delay: u64,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::TimelockMinDelay(asset.clone());
        self.set_cached_value(key, TokenValue::TimelockMinDelay(delay))
            .await
    }

    async fn get_native_asset_timelock_operation(
        &self,
        asset: &Hash,
        operation_id: &[u8; 32],
    ) -> Result<Option<TimelockOperation>, BlockchainError> {
        let key = TokenKey::TimelockOperation {
            asset: asset.clone(),
            operation_id: *operation_id,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::TimelockOperation(value)) => Ok(Some(value)),
            Some(TokenValue::TimelockOperationOpt(value)) => Ok(value),
            _ => Ok(None),
        }
    }

    async fn set_native_asset_timelock_operation(
        &mut self,
        asset: &Hash,
        operation: &TimelockOperation,
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::TimelockOperation {
            asset: asset.clone(),
            operation_id: operation.id,
        };
        self.set_cached_value(key, TokenValue::TimelockOperation(operation.clone()))
            .await
    }

    async fn delete_native_asset_timelock_operation(
        &mut self,
        asset: &Hash,
        operation_id: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let key = TokenKey::TimelockOperation {
            asset: asset.clone(),
            operation_id: *operation_id,
        };
        self.delete_cached_value(key).await
    }

    async fn get_native_asset_delegators(
        &self,
        asset: &Hash,
        delegatee: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, BlockchainError> {
        let key = TokenKey::Delegators {
            asset: asset.clone(),
            delegatee: *delegatee,
        };
        match self.get_cached_value(&key).await? {
            Some(TokenValue::Delegators(value)) => Ok(value),
            _ => Ok(Vec::new()),
        }
    }

    async fn add_native_asset_delegator(
        &mut self,
        asset: &Hash,
        delegatee: &[u8; 32],
        delegator: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let mut delegators = self.get_native_asset_delegators(asset, delegatee).await?;
        if !delegators.contains(delegator) {
            delegators.push(*delegator);
        }
        let key = TokenKey::Delegators {
            asset: asset.clone(),
            delegatee: *delegatee,
        };
        self.set_cached_value(key, TokenValue::Delegators(delegators))
            .await
    }

    async fn remove_native_asset_delegator(
        &mut self,
        asset: &Hash,
        delegatee: &[u8; 32],
        delegator: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        let mut delegators = self.get_native_asset_delegators(asset, delegatee).await?;
        delegators.retain(|d| d != delegator);
        let key = TokenKey::Delegators {
            asset: asset.clone(),
            delegatee: *delegatee,
        };
        self.set_cached_value(key, TokenValue::Delegators(delegators))
            .await
    }

    async fn execute_batch(&mut self, batch: StorageWriteBatch) -> Result<(), BlockchainError> {
        if batch.is_empty() {
            return Ok(());
        }
        Err(BlockchainError::UnsupportedOperation)
    }
}
