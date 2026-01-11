//! RocksDB Native Asset Provider Implementation
//!
//! Implements storage operations for native assets (ERC20-like tokens).

use crate::core::{
    error::BlockchainError,
    storage::{
        providers::native_asset::{
            build_native_asset_agent_auth_key, build_native_asset_allowance_key,
            build_native_asset_balance_key, build_native_asset_checkpoint_count_key,
            build_native_asset_checkpoint_key, build_native_asset_delegation_key,
            build_native_asset_escrow_counter_key, build_native_asset_escrow_key,
            build_native_asset_freeze_key, build_native_asset_key,
            build_native_asset_lock_count_key, build_native_asset_lock_index_key,
            build_native_asset_lock_key, build_native_asset_lock_next_id_key,
            build_native_asset_locked_balance_key, build_native_asset_metadata_key,
            build_native_asset_owner_agents_key, build_native_asset_pause_key,
            build_native_asset_pending_admin_key, build_native_asset_permit_nonce_key,
            build_native_asset_role_config_key, build_native_asset_role_member_key,
            build_native_asset_role_members_key, build_native_asset_supply_key,
            build_native_asset_user_escrows_key,
        },
        NativeAssetProvider, RocksStorage,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    crypto::Hash,
    native_asset::{
        AgentAuthorization, Allowance, Checkpoint, Delegation, Escrow, FreezeState,
        NativeAssetData, PauseState, RoleConfig, RoleId, TokenLock,
    },
};

use super::super::Column;

#[async_trait]
impl NativeAssetProvider for RocksStorage {
    // ===== Asset Data Operations =====

    async fn has_native_asset(&self, asset: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has native asset {}", asset);
        }
        let key = build_native_asset_key(asset);
        self.contains_data(Column::NativeAssets, &key)
    }

    async fn get_native_asset(&self, asset: &Hash) -> Result<NativeAssetData, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset {}", asset);
        }
        let key = build_native_asset_key(asset);
        self.load_from_disk(Column::NativeAssets, &key)
    }

    async fn set_native_asset(
        &mut self,
        asset: &Hash,
        data: &NativeAssetData,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set native asset {}", asset);
        }
        let key = build_native_asset_key(asset);
        self.insert_into_disk(Column::NativeAssets, &key, data)
    }

    async fn get_native_asset_supply(&self, asset: &Hash) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset supply {}", asset);
        }
        let key = build_native_asset_supply_key(asset);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_native_asset_supply(
        &mut self,
        asset: &Hash,
        supply: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set native asset supply {} = {}", asset, supply);
        }
        let key = build_native_asset_supply_key(asset);
        self.insert_into_disk(Column::NativeAssets, &key, &supply)
    }

    // ===== Balance Operations =====

    async fn get_native_asset_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset balance {} for {:?}", asset, account);
        }
        let key = build_native_asset_balance_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_native_asset_balance(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        balance: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset balance {} for {:?} = {}",
                asset,
                account,
                balance
            );
        }
        let key = build_native_asset_balance_key(asset, account);
        self.insert_into_disk(Column::NativeAssets, &key, &balance)
    }

    async fn has_native_asset_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has native asset balance {} for {:?}", asset, account);
        }
        let key = build_native_asset_balance_key(asset, account);
        self.contains_data(Column::NativeAssets, &key)
    }

    // ===== Allowance Operations =====

    async fn get_native_asset_allowance(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<Allowance, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset allowance {} owner {:?} spender {:?}",
                asset,
                owner,
                spender
            );
        }
        let key = build_native_asset_allowance_key(asset, owner, spender);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_native_asset_allowance(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
        allowance: &Allowance,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset allowance {} owner {:?} spender {:?}",
                asset,
                owner,
                spender
            );
        }
        let key = build_native_asset_allowance_key(asset, owner, spender);
        self.insert_into_disk(Column::NativeAssets, &key, allowance)
    }

    async fn delete_native_asset_allowance(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete native asset allowance {} owner {:?} spender {:?}",
                asset,
                owner,
                spender
            );
        }
        let key = build_native_asset_allowance_key(asset, owner, spender);
        self.remove_from_disk(Column::NativeAssets, &key)
    }

    // ===== Timelock Operations =====

    async fn get_native_asset_lock(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<TokenLock, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset lock {} account {:?} id {}",
                asset,
                account,
                lock_id
            );
        }
        let key = build_native_asset_lock_key(asset, account, lock_id);
        self.load_from_disk(Column::NativeAssets, &key)
    }

    async fn set_native_asset_lock(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock: &TokenLock,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset lock {} account {:?} id {}",
                asset,
                account,
                lock.id
            );
        }
        let key = build_native_asset_lock_key(asset, account, lock.id);
        self.insert_into_disk(Column::NativeAssets, &key, lock)
    }

    async fn delete_native_asset_lock(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete native asset lock {} account {:?} id {}",
                asset,
                account,
                lock_id
            );
        }
        let key = build_native_asset_lock_key(asset, account, lock_id);
        self.remove_from_disk(Column::NativeAssets, &key)
    }

    async fn get_native_asset_lock_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset lock count {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_lock_count_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_native_asset_lock_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset lock count {} account {:?} = {}",
                asset,
                account,
                count
            );
        }
        let key = build_native_asset_lock_count_key(asset, account);
        self.insert_into_disk(Column::NativeAssets, &key, &count)
    }

    async fn get_native_asset_next_lock_id(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset next lock id {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_lock_next_id_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_native_asset_next_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        next_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset next lock id {} account {:?} = {}",
                asset,
                account,
                next_id
            );
        }
        let key = build_native_asset_lock_next_id_key(asset, account);
        self.insert_into_disk(Column::NativeAssets, &key, &next_id)
    }

    async fn get_native_asset_locked_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset locked balance {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_locked_balance_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_native_asset_locked_balance(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        locked: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset locked balance {} account {:?} = {}",
                asset,
                account,
                locked
            );
        }
        let key = build_native_asset_locked_balance_key(asset, account);
        self.insert_into_disk(Column::NativeAssets, &key, &locked)
    }

    // ===== Role Operations =====

    async fn get_native_asset_role_config(
        &self,
        asset: &Hash,
        role: &RoleId,
    ) -> Result<RoleConfig, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset role config {} role {:?}", asset, role);
        }
        let key = build_native_asset_role_config_key(asset, role);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_native_asset_role_config(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        config: &RoleConfig,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set native asset role config {} role {:?}", asset, role);
        }
        let key = build_native_asset_role_config_key(asset, role);
        self.insert_into_disk(Column::NativeAssets, &key, config)
    }

    async fn has_native_asset_role(
        &self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "has native asset role {} role {:?} account {:?}",
                asset,
                role,
                account
            );
        }
        let key = build_native_asset_role_member_key(asset, role, account);
        self.contains_data(Column::NativeAssets, &key)
    }

    async fn grant_native_asset_role(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
        granted_at: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "grant native asset role {} role {:?} account {:?} at {}",
                asset,
                role,
                account,
                granted_at
            );
        }
        let key = build_native_asset_role_member_key(asset, role, account);
        self.insert_into_disk(Column::NativeAssets, &key, &granted_at)
    }

    async fn revoke_native_asset_role(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "revoke native asset role {} role {:?} account {:?}",
                asset,
                role,
                account
            );
        }
        let key = build_native_asset_role_member_key(asset, role, account);
        self.remove_from_disk(Column::NativeAssets, &key)
    }

    // ===== Pause/Freeze Operations =====

    async fn get_native_asset_pause_state(
        &self,
        asset: &Hash,
    ) -> Result<PauseState, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset pause state {}", asset);
        }
        let key = build_native_asset_pause_key(asset);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_native_asset_pause_state(
        &mut self,
        asset: &Hash,
        state: &PauseState,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set native asset pause state {}", asset);
        }
        let key = build_native_asset_pause_key(asset);
        self.insert_into_disk(Column::NativeAssets, &key, state)
    }

    async fn get_native_asset_freeze_state(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<FreezeState, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset freeze state {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_freeze_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_native_asset_freeze_state(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        state: &FreezeState,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset freeze state {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_freeze_key(asset, account);
        self.insert_into_disk(Column::NativeAssets, &key, state)
    }

    // ===== Escrow Operations =====

    async fn get_native_asset_escrow_counter(&self, asset: &Hash) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset escrow counter {}", asset);
        }
        let key = build_native_asset_escrow_counter_key(asset);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_native_asset_escrow_counter(
        &mut self,
        asset: &Hash,
        counter: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set native asset escrow counter {} = {}", asset, counter);
        }
        let key = build_native_asset_escrow_counter_key(asset);
        self.insert_into_disk(Column::NativeAssets, &key, &counter)
    }

    async fn get_native_asset_escrow(
        &self,
        asset: &Hash,
        escrow_id: u64,
    ) -> Result<Escrow, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset escrow {} id {}", asset, escrow_id);
        }
        let key = build_native_asset_escrow_key(asset, escrow_id);
        self.load_from_disk(Column::NativeAssets, &key)
    }

    async fn set_native_asset_escrow(
        &mut self,
        asset: &Hash,
        escrow: &Escrow,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set native asset escrow {} id {}", asset, escrow.id);
        }
        let key = build_native_asset_escrow_key(asset, escrow.id);
        self.insert_into_disk(Column::NativeAssets, &key, escrow)
    }

    async fn delete_native_asset_escrow(
        &mut self,
        asset: &Hash,
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete native asset escrow {} id {}", asset, escrow_id);
        }
        let key = build_native_asset_escrow_key(asset, escrow_id);
        self.remove_from_disk(Column::NativeAssets, &key)
    }

    // ===== Permit Operations =====

    async fn get_native_asset_permit_nonce(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset permit nonce {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_permit_nonce_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_native_asset_permit_nonce(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        nonce: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset permit nonce {} account {:?} = {}",
                asset,
                account,
                nonce
            );
        }
        let key = build_native_asset_permit_nonce_key(asset, account);
        self.insert_into_disk(Column::NativeAssets, &key, &nonce)
    }

    // ===== Governance Operations =====

    async fn get_native_asset_delegation(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<Delegation, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset delegation {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_delegation_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_native_asset_delegation(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        delegation: &Delegation,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset delegation {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_delegation_key(asset, account);
        self.insert_into_disk(Column::NativeAssets, &key, delegation)
    }

    async fn get_native_asset_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset checkpoint count {} account {:?}",
                asset,
                account
            );
        }
        let key = build_native_asset_checkpoint_count_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_native_asset_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset checkpoint count {} account {:?} = {}",
                asset,
                account,
                count
            );
        }
        let key = build_native_asset_checkpoint_count_key(asset, account);
        self.insert_into_disk(Column::NativeAssets, &key, &count)
    }

    async fn get_native_asset_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<Checkpoint, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset checkpoint {} account {:?} index {}",
                asset,
                account,
                index
            );
        }
        let key = build_native_asset_checkpoint_key(asset, account, index);
        self.load_from_disk(Column::NativeAssets, &key)
    }

    async fn set_native_asset_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &Checkpoint,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset checkpoint {} account {:?} index {}",
                asset,
                account,
                index
            );
        }
        let key = build_native_asset_checkpoint_key(asset, account, index);
        self.insert_into_disk(Column::NativeAssets, &key, checkpoint)
    }

    // ===== Agent Operations =====

    async fn get_native_asset_agent_auth(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<AgentAuthorization, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset agent auth {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_native_asset_agent_auth_key(asset, owner, agent);
        self.load_from_disk(Column::NativeAssets, &key)
    }

    async fn set_native_asset_agent_auth(
        &mut self,
        asset: &Hash,
        auth: &AgentAuthorization,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set native asset agent auth {} owner {:?} agent {:?}",
                asset,
                auth.owner,
                auth.agent
            );
        }
        let key = build_native_asset_agent_auth_key(asset, &auth.owner, &auth.agent);
        self.insert_into_disk(Column::NativeAssets, &key, auth)
    }

    async fn delete_native_asset_agent_auth(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete native asset agent auth {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_native_asset_agent_auth_key(asset, owner, agent);
        self.remove_from_disk(Column::NativeAssets, &key)
    }

    async fn has_native_asset_agent_auth(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "has native asset agent auth {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_native_asset_agent_auth_key(asset, owner, agent);
        self.contains_data(Column::NativeAssets, &key)
    }

    // ===== Metadata Operations =====

    async fn get_native_asset_metadata_uri(
        &self,
        asset: &Hash,
    ) -> Result<Option<String>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset metadata uri {}", asset);
        }
        let key = build_native_asset_metadata_key(asset);
        self.load_optional_from_disk(Column::NativeAssets, &key)
    }

    async fn set_native_asset_metadata_uri(
        &mut self,
        asset: &Hash,
        uri: Option<&str>,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set native asset metadata uri {} = {:?}", asset, uri);
        }
        let key = build_native_asset_metadata_key(asset);
        match uri {
            Some(u) => self.insert_into_disk(Column::NativeAssets, &key, &u.to_string()),
            None => self.remove_from_disk(Column::NativeAssets, &key),
        }
    }

    // ===== Lock Index Operations =====

    async fn get_native_asset_lock_ids(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<Vec<u64>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset lock ids {} account {:?}", asset, account);
        }
        let key = build_native_asset_lock_index_key(asset, account);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn add_native_asset_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add native asset lock id {} account {:?} lock_id {}",
                asset,
                account,
                lock_id
            );
        }
        let key = build_native_asset_lock_index_key(asset, account);
        let mut ids: Vec<u64> = self
            .load_optional_from_disk(Column::NativeAssets, &key)?
            .unwrap_or_default();

        // Prevent duplicates
        if !ids.contains(&lock_id) {
            ids.push(lock_id);
            self.insert_into_disk(Column::NativeAssets, &key, &ids)?;
        }
        Ok(())
    }

    async fn remove_native_asset_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove native asset lock id {} account {:?} lock_id {}",
                asset,
                account,
                lock_id
            );
        }
        let key = build_native_asset_lock_index_key(asset, account);
        let mut ids: Vec<u64> = self
            .load_optional_from_disk(Column::NativeAssets, &key)?
            .unwrap_or_default();

        if let Some(pos) = ids.iter().position(|&id| id == lock_id) {
            ids.swap_remove(pos);
            if ids.is_empty() {
                self.remove_from_disk(Column::NativeAssets, &key)?;
            } else {
                self.insert_into_disk(Column::NativeAssets, &key, &ids)?;
            }
        }
        Ok(())
    }

    // ===== User Escrow Index Operations =====

    async fn get_native_asset_user_escrows(
        &self,
        asset: &Hash,
        user: &[u8; 32],
    ) -> Result<Vec<u64>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset user escrows {} user {:?}", asset, user);
        }
        let key = build_native_asset_user_escrows_key(asset, user);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn add_native_asset_user_escrow(
        &mut self,
        asset: &Hash,
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add native asset user escrow {} user {:?} escrow_id {}",
                asset,
                user,
                escrow_id
            );
        }
        let key = build_native_asset_user_escrows_key(asset, user);
        let mut ids: Vec<u64> = self
            .load_optional_from_disk(Column::NativeAssets, &key)?
            .unwrap_or_default();

        // Prevent duplicates
        if !ids.contains(&escrow_id) {
            ids.push(escrow_id);
            self.insert_into_disk(Column::NativeAssets, &key, &ids)?;
        }
        Ok(())
    }

    async fn remove_native_asset_user_escrow(
        &mut self,
        asset: &Hash,
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove native asset user escrow {} user {:?} escrow_id {}",
                asset,
                user,
                escrow_id
            );
        }
        let key = build_native_asset_user_escrows_key(asset, user);
        let mut ids: Vec<u64> = self
            .load_optional_from_disk(Column::NativeAssets, &key)?
            .unwrap_or_default();

        if let Some(pos) = ids.iter().position(|&id| id == escrow_id) {
            ids.swap_remove(pos);
            if ids.is_empty() {
                self.remove_from_disk(Column::NativeAssets, &key)?;
            } else {
                self.insert_into_disk(Column::NativeAssets, &key, &ids)?;
            }
        }
        Ok(())
    }

    // ===== Owner Agents Index Operations =====

    async fn get_native_asset_owner_agents(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset owner agents {} owner {:?}", asset, owner);
        }
        let key = build_native_asset_owner_agents_key(asset, owner);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn add_native_asset_owner_agent(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add native asset owner agent {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_native_asset_owner_agents_key(asset, owner);
        let mut agents: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::NativeAssets, &key)?
            .unwrap_or_default();

        // Prevent duplicates
        if !agents.contains(agent) {
            agents.push(*agent);
            self.insert_into_disk(Column::NativeAssets, &key, &agents)?;
        }
        Ok(())
    }

    async fn remove_native_asset_owner_agent(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove native asset owner agent {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_native_asset_owner_agents_key(asset, owner);
        let mut agents: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::NativeAssets, &key)?
            .unwrap_or_default();

        if let Some(pos) = agents.iter().position(|a| a == agent) {
            agents.swap_remove(pos);
            if agents.is_empty() {
                self.remove_from_disk(Column::NativeAssets, &key)?;
            } else {
                self.insert_into_disk(Column::NativeAssets, &key, &agents)?;
            }
        }
        Ok(())
    }

    // ===== Role Members Index Operations =====

    async fn get_native_asset_role_members(
        &self,
        asset: &Hash,
        role: &RoleId,
    ) -> Result<Vec<[u8; 32]>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset role members {} role {:?}", asset, role);
        }
        let key = build_native_asset_role_members_key(asset, role);
        self.load_optional_from_disk(Column::NativeAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn get_native_asset_role_member(
        &self,
        asset: &Hash,
        role: &RoleId,
        index: u32,
    ) -> Result<[u8; 32], BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get native asset role member {} role {:?} index {}",
                asset,
                role,
                index
            );
        }
        let members = self.get_native_asset_role_members(asset, role).await?;
        members
            .get(index as usize)
            .copied()
            .ok_or(BlockchainError::Unknown)
    }

    async fn add_native_asset_role_member(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add native asset role member {} role {:?} account {:?}",
                asset,
                role,
                account
            );
        }
        let key = build_native_asset_role_members_key(asset, role);
        let mut members: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::NativeAssets, &key)?
            .unwrap_or_default();

        // Prevent duplicates
        if !members.contains(account) {
            members.push(*account);
            self.insert_into_disk(Column::NativeAssets, &key, &members)?;
        }
        Ok(())
    }

    async fn remove_native_asset_role_member(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove native asset role member {} role {:?} account {:?}",
                asset,
                role,
                account
            );
        }
        let key = build_native_asset_role_members_key(asset, role);
        let mut members: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::NativeAssets, &key)?
            .unwrap_or_default();

        if let Some(pos) = members.iter().position(|m| m == account) {
            members.swap_remove(pos);
            if members.is_empty() {
                self.remove_from_disk(Column::NativeAssets, &key)?;
            } else {
                self.insert_into_disk(Column::NativeAssets, &key, &members)?;
            }
        }
        Ok(())
    }

    // ===== Admin Proposal Operations =====

    async fn get_native_asset_pending_admin(
        &self,
        asset: &Hash,
    ) -> Result<Option<[u8; 32]>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get native asset pending admin {}", asset);
        }
        let key = build_native_asset_pending_admin_key(asset);
        self.load_optional_from_disk(Column::NativeAssets, &key)
    }

    async fn set_native_asset_pending_admin(
        &mut self,
        asset: &Hash,
        admin: Option<&[u8; 32]>,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set native asset pending admin {} = {:?}", asset, admin);
        }
        let key = build_native_asset_pending_admin_key(asset);
        match admin {
            Some(a) => self.insert_into_disk(Column::NativeAssets, &key, a),
            None => self.remove_from_disk(Column::NativeAssets, &key),
        }
    }
}
