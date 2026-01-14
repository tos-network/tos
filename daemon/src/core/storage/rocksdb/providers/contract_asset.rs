//! RocksDB Contract Asset Provider Implementation
//!
//! Implements storage operations for contract assets (ERC20-like tokens).

use crate::core::{
    error::BlockchainError,
    storage::{
        providers::contract_asset::{
            build_contract_asset_admin_delay_key, build_contract_asset_agent_auth_key,
            build_contract_asset_allowance_key, build_contract_asset_balance_checkpoint_count_key,
            build_contract_asset_balance_checkpoint_key, build_contract_asset_balance_key,
            build_contract_asset_checkpoint_count_key, build_contract_asset_checkpoint_key,
            build_contract_asset_delegation_checkpoint_count_key,
            build_contract_asset_delegation_checkpoint_key, build_contract_asset_delegation_key,
            build_contract_asset_delegators_key, build_contract_asset_escrow_counter_key,
            build_contract_asset_escrow_key, build_contract_asset_freeze_key,
            build_contract_asset_key, build_contract_asset_lock_count_key,
            build_contract_asset_lock_index_key, build_contract_asset_lock_key,
            build_contract_asset_lock_next_id_key, build_contract_asset_locked_balance_key,
            build_contract_asset_metadata_key, build_contract_asset_owner_agents_key,
            build_contract_asset_pause_key, build_contract_asset_pending_admin_key,
            build_contract_asset_permit_nonce_key, build_contract_asset_role_config_key,
            build_contract_asset_role_member_key, build_contract_asset_role_members_key,
            build_contract_asset_supply_checkpoint_count_key,
            build_contract_asset_supply_checkpoint_key, build_contract_asset_supply_key,
            build_contract_asset_timelock_min_delay_key,
            build_contract_asset_timelock_operation_key, build_contract_asset_user_escrows_key,
            build_contract_asset_vote_power_key, BatchOperation, StorageWriteBatch,
        },
        ContractAssetProvider, RocksStorage,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    contract_asset::{
        AdminDelay, AgentAuthorization, Allowance, BalanceCheckpoint, Checkpoint,
        ContractAssetData, Delegation, DelegationCheckpoint, Escrow, FreezeState, PauseState,
        RoleConfig, RoleId, SupplyCheckpoint, TimelockOperation, TokenLock,
    },
    crypto::Hash,
};

use super::super::Column;

#[async_trait(?Send)]
impl ContractAssetProvider for RocksStorage {
    // ===== Asset Data Operations =====

    async fn has_contract_asset(&self, asset: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has contract asset {}", asset);
        }
        let key = build_contract_asset_key(asset);
        self.contains_data(Column::ContractAssets, &key)
    }

    async fn get_contract_asset(&self, asset: &Hash) -> Result<ContractAssetData, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset {}", asset);
        }
        let key = build_contract_asset_key(asset);
        self.load_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset(
        &mut self,
        asset: &Hash,
        data: &ContractAssetData,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set contract asset {}", asset);
        }
        let key = build_contract_asset_key(asset);
        self.insert_into_disk(Column::ContractAssets, &key, data)
    }

    async fn get_contract_asset_supply(&self, asset: &Hash) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset supply {}", asset);
        }
        let key = build_contract_asset_supply_key(asset);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_supply(
        &mut self,
        asset: &Hash,
        supply: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set contract asset supply {} = {}", asset, supply);
        }
        let key = build_contract_asset_supply_key(asset);
        self.insert_into_disk(Column::ContractAssets, &key, &supply)
    }

    // ===== Balance Operations =====

    async fn get_contract_asset_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset balance {} for {:?}", asset, account);
        }
        let key = build_contract_asset_balance_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_balance(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        balance: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset balance {} for {:?} = {}",
                asset,
                account,
                balance
            );
        }
        let key = build_contract_asset_balance_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &balance)
    }

    async fn has_contract_asset_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has contract asset balance {} for {:?}", asset, account);
        }
        let key = build_contract_asset_balance_key(asset, account);
        self.contains_data(Column::ContractAssets, &key)
    }

    // ===== Allowance Operations =====

    async fn get_contract_asset_allowance(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<Allowance, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset allowance {} owner {:?} spender {:?}",
                asset,
                owner,
                spender
            );
        }
        let key = build_contract_asset_allowance_key(asset, owner, spender);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_contract_asset_allowance(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
        allowance: &Allowance,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset allowance {} owner {:?} spender {:?}",
                asset,
                owner,
                spender
            );
        }
        let key = build_contract_asset_allowance_key(asset, owner, spender);
        self.insert_into_disk(Column::ContractAssets, &key, allowance)
    }

    async fn delete_contract_asset_allowance(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete contract asset allowance {} owner {:?} spender {:?}",
                asset,
                owner,
                spender
            );
        }
        let key = build_contract_asset_allowance_key(asset, owner, spender);
        self.remove_from_disk(Column::ContractAssets, &key)
    }

    // ===== Timelock Operations =====

    async fn get_contract_asset_lock(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<TokenLock, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset lock {} account {:?} id {}",
                asset,
                account,
                lock_id
            );
        }
        let key = build_contract_asset_lock_key(asset, account, lock_id);
        self.load_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_lock(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock: &TokenLock,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset lock {} account {:?} id {}",
                asset,
                account,
                lock.id
            );
        }
        let key = build_contract_asset_lock_key(asset, account, lock.id);
        self.insert_into_disk(Column::ContractAssets, &key, lock)
    }

    async fn delete_contract_asset_lock(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete contract asset lock {} account {:?} id {}",
                asset,
                account,
                lock_id
            );
        }
        let key = build_contract_asset_lock_key(asset, account, lock_id);
        self.remove_from_disk(Column::ContractAssets, &key)
    }

    async fn get_contract_asset_lock_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset lock count {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_lock_count_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_lock_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset lock count {} account {:?} = {}",
                asset,
                account,
                count
            );
        }
        let key = build_contract_asset_lock_count_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &count)
    }

    async fn get_contract_asset_next_lock_id(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset next lock id {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_lock_next_id_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_next_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        next_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset next lock id {} account {:?} = {}",
                asset,
                account,
                next_id
            );
        }
        let key = build_contract_asset_lock_next_id_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &next_id)
    }

    async fn get_contract_asset_locked_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset locked balance {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_locked_balance_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_locked_balance(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        locked: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset locked balance {} account {:?} = {}",
                asset,
                account,
                locked
            );
        }
        let key = build_contract_asset_locked_balance_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &locked)
    }

    // ===== Role Operations =====

    async fn get_contract_asset_role_config(
        &self,
        asset: &Hash,
        role: &RoleId,
    ) -> Result<RoleConfig, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset role config {} role {:?}", asset, role);
        }
        let key = build_contract_asset_role_config_key(asset, role);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_contract_asset_role_config(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        config: &RoleConfig,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set contract asset role config {} role {:?}", asset, role);
        }
        let key = build_contract_asset_role_config_key(asset, role);
        self.insert_into_disk(Column::ContractAssets, &key, config)
    }

    async fn has_contract_asset_role(
        &self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "has contract asset role {} role {:?} account {:?}",
                asset,
                role,
                account
            );
        }
        let key = build_contract_asset_role_member_key(asset, role, account);
        self.contains_data(Column::ContractAssets, &key)
    }

    async fn grant_contract_asset_role(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
        granted_at: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "grant contract asset role {} role {:?} account {:?} at {}",
                asset,
                role,
                account,
                granted_at
            );
        }
        let key = build_contract_asset_role_member_key(asset, role, account);
        self.insert_into_disk(Column::ContractAssets, &key, &granted_at)
    }

    async fn revoke_contract_asset_role(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "revoke contract asset role {} role {:?} account {:?}",
                asset,
                role,
                account
            );
        }
        let key = build_contract_asset_role_member_key(asset, role, account);
        self.remove_from_disk(Column::ContractAssets, &key)
    }

    // ===== Pause/Freeze Operations =====

    async fn get_contract_asset_pause_state(
        &self,
        asset: &Hash,
    ) -> Result<PauseState, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset pause state {}", asset);
        }
        let key = build_contract_asset_pause_key(asset);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_contract_asset_pause_state(
        &mut self,
        asset: &Hash,
        state: &PauseState,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set contract asset pause state {}", asset);
        }
        let key = build_contract_asset_pause_key(asset);
        self.insert_into_disk(Column::ContractAssets, &key, state)
    }

    async fn get_contract_asset_freeze_state(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<FreezeState, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset freeze state {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_freeze_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_contract_asset_freeze_state(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        state: &FreezeState,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset freeze state {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_freeze_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, state)
    }

    // ===== Escrow Operations =====

    async fn get_contract_asset_escrow_counter(
        &self,
        asset: &Hash,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset escrow counter {}", asset);
        }
        let key = build_contract_asset_escrow_counter_key(asset);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_escrow_counter(
        &mut self,
        asset: &Hash,
        counter: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set contract asset escrow counter {} = {}", asset, counter);
        }
        let key = build_contract_asset_escrow_counter_key(asset);
        self.insert_into_disk(Column::ContractAssets, &key, &counter)
    }

    async fn get_contract_asset_escrow(
        &self,
        asset: &Hash,
        escrow_id: u64,
    ) -> Result<Escrow, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset escrow {} id {}", asset, escrow_id);
        }
        let key = build_contract_asset_escrow_key(asset, escrow_id);
        self.load_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_escrow(
        &mut self,
        asset: &Hash,
        escrow: &Escrow,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set contract asset escrow {} id {}", asset, escrow.id);
        }
        let key = build_contract_asset_escrow_key(asset, escrow.id);
        self.insert_into_disk(Column::ContractAssets, &key, escrow)
    }

    async fn delete_contract_asset_escrow(
        &mut self,
        asset: &Hash,
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete contract asset escrow {} id {}", asset, escrow_id);
        }
        let key = build_contract_asset_escrow_key(asset, escrow_id);
        self.remove_from_disk(Column::ContractAssets, &key)
    }

    // ===== Permit Operations =====

    async fn get_contract_asset_permit_nonce(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset permit nonce {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_permit_nonce_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_permit_nonce(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        nonce: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset permit nonce {} account {:?} = {}",
                asset,
                account,
                nonce
            );
        }
        let key = build_contract_asset_permit_nonce_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &nonce)
    }

    // ===== Governance Operations =====

    async fn get_contract_asset_delegation(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<Delegation, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset delegation {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_delegation_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn set_contract_asset_delegation(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        delegation: &Delegation,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset delegation {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_delegation_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, delegation)
    }

    async fn get_contract_asset_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset checkpoint count {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_checkpoint_count_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset checkpoint count {} account {:?} = {}",
                asset,
                account,
                count
            );
        }
        let key = build_contract_asset_checkpoint_count_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &count)
    }

    async fn get_contract_asset_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<Checkpoint, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset checkpoint {} account {:?} index {}",
                asset,
                account,
                index
            );
        }
        let key = build_contract_asset_checkpoint_key(asset, account, index);
        self.load_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &Checkpoint,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset checkpoint {} account {:?} index {}",
                asset,
                account,
                index
            );
        }
        let key = build_contract_asset_checkpoint_key(asset, account, index);
        self.insert_into_disk(Column::ContractAssets, &key, checkpoint)
    }

    // ===== Agent Operations =====

    async fn get_contract_asset_agent_auth(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<AgentAuthorization, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset agent auth {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_contract_asset_agent_auth_key(asset, owner, agent);
        self.load_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_agent_auth(
        &mut self,
        asset: &Hash,
        auth: &AgentAuthorization,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset agent auth {} owner {:?} agent {:?}",
                asset,
                auth.owner,
                auth.agent
            );
        }
        let key = build_contract_asset_agent_auth_key(asset, &auth.owner, &auth.agent);
        self.insert_into_disk(Column::ContractAssets, &key, auth)
    }

    async fn delete_contract_asset_agent_auth(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete contract asset agent auth {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_contract_asset_agent_auth_key(asset, owner, agent);
        self.remove_from_disk(Column::ContractAssets, &key)
    }

    async fn has_contract_asset_agent_auth(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "has contract asset agent auth {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_contract_asset_agent_auth_key(asset, owner, agent);
        self.contains_data(Column::ContractAssets, &key)
    }

    // ===== Metadata Operations =====

    async fn get_contract_asset_metadata_uri(
        &self,
        asset: &Hash,
    ) -> Result<Option<String>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset metadata uri {}", asset);
        }
        let key = build_contract_asset_metadata_key(asset);
        self.load_optional_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_metadata_uri(
        &mut self,
        asset: &Hash,
        uri: Option<&str>,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set contract asset metadata uri {} = {:?}", asset, uri);
        }
        let key = build_contract_asset_metadata_key(asset);
        match uri {
            Some(u) => self.insert_into_disk(Column::ContractAssets, &key, &u.to_string()),
            None => self.remove_from_disk(Column::ContractAssets, &key),
        }
    }

    // ===== Lock Index Operations =====

    async fn get_contract_asset_lock_ids(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<Vec<u64>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset lock ids {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_lock_index_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn add_contract_asset_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add contract asset lock id {} account {:?} lock_id {}",
                asset,
                account,
                lock_id
            );
        }
        let key = build_contract_asset_lock_index_key(asset, account);
        let mut ids: Vec<u64> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();

        // Prevent duplicates
        if !ids.contains(&lock_id) {
            ids.push(lock_id);
            self.insert_into_disk(Column::ContractAssets, &key, &ids)?;
        }
        Ok(())
    }

    async fn remove_contract_asset_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove contract asset lock id {} account {:?} lock_id {}",
                asset,
                account,
                lock_id
            );
        }
        let key = build_contract_asset_lock_index_key(asset, account);
        let mut ids: Vec<u64> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();

        if let Some(pos) = ids.iter().position(|&id| id == lock_id) {
            ids.swap_remove(pos);
            if ids.is_empty() {
                self.remove_from_disk(Column::ContractAssets, &key)?;
            } else {
                self.insert_into_disk(Column::ContractAssets, &key, &ids)?;
            }
        }
        Ok(())
    }

    // ===== User Escrow Index Operations =====

    async fn get_contract_asset_user_escrows(
        &self,
        asset: &Hash,
        user: &[u8; 32],
    ) -> Result<Vec<u64>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset user escrows {} user {:?}", asset, user);
        }
        let key = build_contract_asset_user_escrows_key(asset, user);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn add_contract_asset_user_escrow(
        &mut self,
        asset: &Hash,
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add contract asset user escrow {} user {:?} escrow_id {}",
                asset,
                user,
                escrow_id
            );
        }
        let key = build_contract_asset_user_escrows_key(asset, user);
        let mut ids: Vec<u64> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();

        // Prevent duplicates
        if !ids.contains(&escrow_id) {
            ids.push(escrow_id);
            self.insert_into_disk(Column::ContractAssets, &key, &ids)?;
        }
        Ok(())
    }

    async fn remove_contract_asset_user_escrow(
        &mut self,
        asset: &Hash,
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove contract asset user escrow {} user {:?} escrow_id {}",
                asset,
                user,
                escrow_id
            );
        }
        let key = build_contract_asset_user_escrows_key(asset, user);
        let mut ids: Vec<u64> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();

        if let Some(pos) = ids.iter().position(|&id| id == escrow_id) {
            ids.swap_remove(pos);
            if ids.is_empty() {
                self.remove_from_disk(Column::ContractAssets, &key)?;
            } else {
                self.insert_into_disk(Column::ContractAssets, &key, &ids)?;
            }
        }
        Ok(())
    }

    // ===== Owner Agents Index Operations =====

    async fn get_contract_asset_owner_agents(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset owner agents {} owner {:?}",
                asset,
                owner
            );
        }
        let key = build_contract_asset_owner_agents_key(asset, owner);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn add_contract_asset_owner_agent(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add contract asset owner agent {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_contract_asset_owner_agents_key(asset, owner);
        let mut agents: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();

        // Prevent duplicates
        if !agents.contains(agent) {
            agents.push(*agent);
            self.insert_into_disk(Column::ContractAssets, &key, &agents)?;
        }
        Ok(())
    }

    async fn remove_contract_asset_owner_agent(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove contract asset owner agent {} owner {:?} agent {:?}",
                asset,
                owner,
                agent
            );
        }
        let key = build_contract_asset_owner_agents_key(asset, owner);
        let mut agents: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();

        if let Some(pos) = agents.iter().position(|a| a == agent) {
            agents.swap_remove(pos);
            if agents.is_empty() {
                self.remove_from_disk(Column::ContractAssets, &key)?;
            } else {
                self.insert_into_disk(Column::ContractAssets, &key, &agents)?;
            }
        }
        Ok(())
    }

    // ===== Role Members Index Operations =====

    async fn get_contract_asset_role_members(
        &self,
        asset: &Hash,
        role: &RoleId,
    ) -> Result<Vec<[u8; 32]>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset role members {} role {:?}", asset, role);
        }
        let key = build_contract_asset_role_members_key(asset, role);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn get_contract_asset_role_member(
        &self,
        asset: &Hash,
        role: &RoleId,
        index: u32,
    ) -> Result<[u8; 32], BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset role member {} role {:?} index {}",
                asset,
                role,
                index
            );
        }
        let members = self.get_contract_asset_role_members(asset, role).await?;
        members
            .get(index as usize)
            .copied()
            .ok_or(BlockchainError::Unknown)
    }

    async fn add_contract_asset_role_member(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add contract asset role member {} role {:?} account {:?}",
                asset,
                role,
                account
            );
        }
        let key = build_contract_asset_role_members_key(asset, role);
        let mut members: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();

        // Prevent duplicates
        if !members.contains(account) {
            members.push(*account);
            self.insert_into_disk(Column::ContractAssets, &key, &members)?;
        }
        Ok(())
    }

    async fn remove_contract_asset_role_member(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove contract asset role member {} role {:?} account {:?}",
                asset,
                role,
                account
            );
        }
        let key = build_contract_asset_role_members_key(asset, role);
        let mut members: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();

        if let Some(pos) = members.iter().position(|m| m == account) {
            members.swap_remove(pos);
            if members.is_empty() {
                self.remove_from_disk(Column::ContractAssets, &key)?;
            } else {
                self.insert_into_disk(Column::ContractAssets, &key, &members)?;
            }
        }
        Ok(())
    }

    // ===== Admin Proposal Operations =====

    async fn get_contract_asset_pending_admin(
        &self,
        asset: &Hash,
    ) -> Result<Option<[u8; 32]>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset pending admin {}", asset);
        }
        let key = build_contract_asset_pending_admin_key(asset);
        self.load_optional_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_pending_admin(
        &mut self,
        asset: &Hash,
        admin: Option<&[u8; 32]>,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set contract asset pending admin {} = {:?}", asset, admin);
        }
        let key = build_contract_asset_pending_admin_key(asset);
        match admin {
            Some(a) => self.insert_into_disk(Column::ContractAssets, &key, a),
            None => self.remove_from_disk(Column::ContractAssets, &key),
        }
    }

    // ===== Balance Checkpoint Operations =====

    async fn get_contract_asset_balance_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset balance checkpoint count {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_balance_checkpoint_count_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_balance_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset balance checkpoint count {} account {:?} = {}",
                asset,
                account,
                count
            );
        }
        let key = build_contract_asset_balance_checkpoint_count_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &count)
    }

    async fn get_contract_asset_balance_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<BalanceCheckpoint, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset balance checkpoint {} account {:?} index {}",
                asset,
                account,
                index
            );
        }
        let key = build_contract_asset_balance_checkpoint_key(asset, account, index);
        self.load_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_balance_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &BalanceCheckpoint,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset balance checkpoint {} account {:?} index {}",
                asset,
                account,
                index
            );
        }
        let key = build_contract_asset_balance_checkpoint_key(asset, account, index);
        self.insert_into_disk(Column::ContractAssets, &key, checkpoint)
    }

    // ===== Delegation Checkpoint Operations =====

    async fn get_contract_asset_delegation_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset delegation checkpoint count {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_delegation_checkpoint_count_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_delegation_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset delegation checkpoint count {} account {:?} = {}",
                asset,
                account,
                count
            );
        }
        let key = build_contract_asset_delegation_checkpoint_count_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &count)
    }

    async fn get_contract_asset_delegation_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<DelegationCheckpoint, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset delegation checkpoint {} account {:?} index {}",
                asset,
                account,
                index
            );
        }
        let key = build_contract_asset_delegation_checkpoint_key(asset, account, index);
        self.load_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_delegation_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &DelegationCheckpoint,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset delegation checkpoint {} account {:?} index {}",
                asset,
                account,
                index
            );
        }
        let key = build_contract_asset_delegation_checkpoint_key(asset, account, index);
        self.insert_into_disk(Column::ContractAssets, &key, checkpoint)
    }

    // ===== Supply Checkpoint Operations =====

    async fn get_contract_asset_supply_checkpoint_count(
        &self,
        asset: &Hash,
    ) -> Result<u32, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset supply checkpoint count {}", asset);
        }
        let key = build_contract_asset_supply_checkpoint_count_key(asset);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|opt| opt.unwrap_or(0))
    }

    async fn set_contract_asset_supply_checkpoint_count(
        &mut self,
        asset: &Hash,
        count: u32,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset supply checkpoint count {} = {}",
                asset,
                count
            );
        }
        let key = build_contract_asset_supply_checkpoint_count_key(asset);
        self.insert_into_disk(Column::ContractAssets, &key, &count)
    }

    async fn get_contract_asset_supply_checkpoint(
        &self,
        asset: &Hash,
        index: u32,
    ) -> Result<SupplyCheckpoint, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset supply checkpoint {} index {}",
                asset,
                index
            );
        }
        let key = build_contract_asset_supply_checkpoint_key(asset, index);
        self.load_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_supply_checkpoint(
        &mut self,
        asset: &Hash,
        index: u32,
        checkpoint: &SupplyCheckpoint,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset supply checkpoint {} index {}",
                asset,
                index
            );
        }
        let key = build_contract_asset_supply_checkpoint_key(asset, index);
        self.insert_into_disk(Column::ContractAssets, &key, checkpoint)
    }

    // ===== Admin Delay Operations =====

    async fn get_contract_asset_admin_delay(
        &self,
        asset: &Hash,
    ) -> Result<AdminDelay, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset admin delay {}", asset);
        }
        let key = build_contract_asset_admin_delay_key(asset);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|opt| opt.unwrap_or_default())
    }

    async fn set_contract_asset_admin_delay(
        &mut self,
        asset: &Hash,
        delay: &AdminDelay,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset admin delay {} = {:?}",
                asset,
                delay.delay
            );
        }
        let key = build_contract_asset_admin_delay_key(asset);
        self.insert_into_disk(Column::ContractAssets, &key, delay)
    }

    // ===== Timelock Operations =====

    async fn get_contract_asset_timelock_min_delay(
        &self,
        asset: &Hash,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get contract asset timelock min delay {}", asset);
        }
        let key = build_contract_asset_timelock_min_delay_key(asset);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|opt| opt.unwrap_or(0))
    }

    async fn set_contract_asset_timelock_min_delay(
        &mut self,
        asset: &Hash,
        delay: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset timelock min delay {} = {}",
                asset,
                delay
            );
        }
        let key = build_contract_asset_timelock_min_delay_key(asset);
        self.insert_into_disk(Column::ContractAssets, &key, &delay)
    }

    async fn get_contract_asset_timelock_operation(
        &self,
        asset: &Hash,
        operation_id: &[u8; 32],
    ) -> Result<Option<TimelockOperation>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset timelock operation {} id {:?}",
                asset,
                operation_id
            );
        }
        let key = build_contract_asset_timelock_operation_key(asset, operation_id);
        self.load_optional_from_disk(Column::ContractAssets, &key)
    }

    async fn set_contract_asset_timelock_operation(
        &mut self,
        asset: &Hash,
        operation: &TimelockOperation,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset timelock operation {} id {:?}",
                asset,
                operation.id
            );
        }
        let key = build_contract_asset_timelock_operation_key(asset, &operation.id);
        self.insert_into_disk(Column::ContractAssets, &key, operation)
    }

    async fn delete_contract_asset_timelock_operation(
        &mut self,
        asset: &Hash,
        operation_id: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete contract asset timelock operation {} id {:?}",
                asset,
                operation_id
            );
        }
        let key = build_contract_asset_timelock_operation_key(asset, operation_id);
        self.remove_from_disk(Column::ContractAssets, &key)
    }

    // ===== Vote Power Operations =====

    async fn get_contract_asset_vote_power(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset vote power {} account {:?}",
                asset,
                account
            );
        }
        let key = build_contract_asset_vote_power_key(asset, account);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or(0))
    }

    async fn set_contract_asset_vote_power(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        votes: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract asset vote power {} account {:?} = {}",
                asset,
                account,
                votes
            );
        }
        let key = build_contract_asset_vote_power_key(asset, account);
        self.insert_into_disk(Column::ContractAssets, &key, &votes)
    }

    // ===== Delegators Index Operations =====

    async fn get_contract_asset_delegators(
        &self,
        asset: &Hash,
        delegatee: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract asset delegators {} delegatee {:?}",
                asset,
                delegatee
            );
        }
        let key = build_contract_asset_delegators_key(asset, delegatee);
        self.load_optional_from_disk(Column::ContractAssets, &key)
            .map(|v| v.unwrap_or_default())
    }

    async fn add_contract_asset_delegator(
        &mut self,
        asset: &Hash,
        delegatee: &[u8; 32],
        delegator: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add contract asset delegator {} delegatee {:?} delegator {:?}",
                asset,
                delegatee,
                delegator
            );
        }
        let key = build_contract_asset_delegators_key(asset, delegatee);
        let mut delegators: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();
        delegators.sort_unstable();

        // Use binary search for O(log n) lookup instead of O(n) contains()
        // Maintains sorted order for deterministic iteration
        match delegators.binary_search(delegator) {
            Ok(_) => {
                // Already exists, no-op
            }
            Err(insert_pos) => {
                // Not found, insert at sorted position
                delegators.insert(insert_pos, *delegator);
                self.insert_into_disk(Column::ContractAssets, &key, &delegators)?;
            }
        }
        Ok(())
    }

    async fn remove_contract_asset_delegator(
        &mut self,
        asset: &Hash,
        delegatee: &[u8; 32],
        delegator: &[u8; 32],
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "remove contract asset delegator {} delegatee {:?} delegator {:?}",
                asset,
                delegatee,
                delegator
            );
        }
        let key = build_contract_asset_delegators_key(asset, delegatee);
        let mut delegators: Vec<[u8; 32]> = self
            .load_optional_from_disk(Column::ContractAssets, &key)?
            .unwrap_or_default();
        delegators.sort_unstable();

        // Use binary search for O(log n) lookup instead of O(n) position()
        if let Ok(index) = delegators.binary_search(delegator) {
            delegators.remove(index);
            if delegators.is_empty() {
                self.remove_from_disk(Column::ContractAssets, &key)?;
            } else {
                self.insert_into_disk(Column::ContractAssets, &key, &delegators)?;
            }
        }
        Ok(())
    }

    // ===== Atomic Batch Operations =====

    async fn execute_batch(&mut self, batch: StorageWriteBatch) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("execute_batch with {} operations", batch.len());
        }

        if batch.is_empty() {
            return Ok(());
        }

        // Convert BatchOperation to the format expected by write_batch
        let operations: Vec<_> = batch
            .operations
            .iter()
            .map(|op| match op {
                BatchOperation::Put { cf, key, value } => {
                    (*cf, key.as_slice(), Some(value.as_slice()))
                }
                BatchOperation::Delete { cf, key } => (*cf, key.as_slice(), None),
            })
            .collect();

        self.write_batch(operations)
    }
}
