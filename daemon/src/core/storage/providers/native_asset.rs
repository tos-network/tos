//! Native Asset Provider
//!
//! Provides storage operations for native assets (ERC20-like tokens).

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    crypto::Hash,
    native_asset::{
        AgentAuthorization, Allowance, BalanceCheckpoint, Checkpoint, Delegation,
        DelegationCheckpoint, Escrow, FreezeState, NativeAssetData, PauseState, RoleConfig, RoleId,
        TokenLock,
    },
};

// ===== Native Asset Provider Trait =====

#[async_trait]
pub trait NativeAssetProvider {
    // ===== Asset Data Operations =====

    /// Check if a native asset exists
    async fn has_native_asset(&self, asset: &Hash) -> Result<bool, BlockchainError>;

    /// Get native asset data
    async fn get_native_asset(&self, asset: &Hash) -> Result<NativeAssetData, BlockchainError>;

    /// Store native asset data
    async fn set_native_asset(
        &mut self,
        asset: &Hash,
        data: &NativeAssetData,
    ) -> Result<(), BlockchainError>;

    /// Get total supply for asset
    async fn get_native_asset_supply(&self, asset: &Hash) -> Result<u64, BlockchainError>;

    /// Set total supply for asset
    async fn set_native_asset_supply(
        &mut self,
        asset: &Hash,
        supply: u64,
    ) -> Result<(), BlockchainError>;

    // ===== Balance Operations =====

    /// Get balance for account and asset
    async fn get_native_asset_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError>;

    /// Set balance for account and asset
    async fn set_native_asset_balance(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        balance: u64,
    ) -> Result<(), BlockchainError>;

    /// Check if account has any balance for asset
    async fn has_native_asset_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<bool, BlockchainError>;

    // ===== Allowance Operations =====

    /// Get allowance for owner-spender pair
    async fn get_native_asset_allowance(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<Allowance, BlockchainError>;

    /// Set allowance for owner-spender pair
    async fn set_native_asset_allowance(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
        allowance: &Allowance,
    ) -> Result<(), BlockchainError>;

    /// Delete allowance for owner-spender pair
    async fn delete_native_asset_allowance(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        spender: &[u8; 32],
    ) -> Result<(), BlockchainError>;

    // ===== Timelock Operations =====

    /// Get lock by ID
    async fn get_native_asset_lock(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<TokenLock, BlockchainError>;

    /// Set lock data
    async fn set_native_asset_lock(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock: &TokenLock,
    ) -> Result<(), BlockchainError>;

    /// Delete lock
    async fn delete_native_asset_lock(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError>;

    /// Get lock count for account
    async fn get_native_asset_lock_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError>;

    /// Set lock count for account
    async fn set_native_asset_lock_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError>;

    /// Get next lock ID for account
    async fn get_native_asset_next_lock_id(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError>;

    /// Set next lock ID for account
    async fn set_native_asset_next_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        next_id: u64,
    ) -> Result<(), BlockchainError>;

    /// Get total locked balance for account
    async fn get_native_asset_locked_balance(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError>;

    /// Set total locked balance for account
    async fn set_native_asset_locked_balance(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        locked: u64,
    ) -> Result<(), BlockchainError>;

    // ===== Role Operations =====

    /// Get role configuration
    async fn get_native_asset_role_config(
        &self,
        asset: &Hash,
        role: &RoleId,
    ) -> Result<RoleConfig, BlockchainError>;

    /// Set role configuration
    async fn set_native_asset_role_config(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        config: &RoleConfig,
    ) -> Result<(), BlockchainError>;

    /// Check if account has role
    async fn has_native_asset_role(
        &self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<bool, BlockchainError>;

    /// Grant role to account
    async fn grant_native_asset_role(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
        granted_at: u64,
    ) -> Result<(), BlockchainError>;

    /// Revoke role from account
    async fn revoke_native_asset_role(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError>;

    // ===== Pause/Freeze Operations =====

    /// Get pause state for asset
    async fn get_native_asset_pause_state(
        &self,
        asset: &Hash,
    ) -> Result<PauseState, BlockchainError>;

    /// Set pause state for asset
    async fn set_native_asset_pause_state(
        &mut self,
        asset: &Hash,
        state: &PauseState,
    ) -> Result<(), BlockchainError>;

    /// Get freeze state for account
    async fn get_native_asset_freeze_state(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<FreezeState, BlockchainError>;

    /// Set freeze state for account
    async fn set_native_asset_freeze_state(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        state: &FreezeState,
    ) -> Result<(), BlockchainError>;

    // ===== Escrow Operations =====

    /// Get escrow counter for asset
    async fn get_native_asset_escrow_counter(&self, asset: &Hash) -> Result<u64, BlockchainError>;

    /// Set escrow counter for asset
    async fn set_native_asset_escrow_counter(
        &mut self,
        asset: &Hash,
        counter: u64,
    ) -> Result<(), BlockchainError>;

    /// Get escrow by ID
    async fn get_native_asset_escrow(
        &self,
        asset: &Hash,
        escrow_id: u64,
    ) -> Result<Escrow, BlockchainError>;

    /// Set escrow data
    async fn set_native_asset_escrow(
        &mut self,
        asset: &Hash,
        escrow: &Escrow,
    ) -> Result<(), BlockchainError>;

    /// Delete escrow
    async fn delete_native_asset_escrow(
        &mut self,
        asset: &Hash,
        escrow_id: u64,
    ) -> Result<(), BlockchainError>;

    // ===== Permit Operations =====

    /// Get permit nonce for account
    async fn get_native_asset_permit_nonce(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u64, BlockchainError>;

    /// Set permit nonce for account
    async fn set_native_asset_permit_nonce(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        nonce: u64,
    ) -> Result<(), BlockchainError>;

    // ===== Governance Operations =====

    /// Get delegation for account
    async fn get_native_asset_delegation(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<Delegation, BlockchainError>;

    /// Set delegation for account
    async fn set_native_asset_delegation(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        delegation: &Delegation,
    ) -> Result<(), BlockchainError>;

    /// Get checkpoint count for account
    async fn get_native_asset_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError>;

    /// Set checkpoint count for account
    async fn set_native_asset_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError>;

    /// Get checkpoint by index
    async fn get_native_asset_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<Checkpoint, BlockchainError>;

    /// Set checkpoint
    async fn set_native_asset_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &Checkpoint,
    ) -> Result<(), BlockchainError>;

    // ===== Agent Operations =====

    /// Get agent authorization
    async fn get_native_asset_agent_auth(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<AgentAuthorization, BlockchainError>;

    /// Set agent authorization
    async fn set_native_asset_agent_auth(
        &mut self,
        asset: &Hash,
        auth: &AgentAuthorization,
    ) -> Result<(), BlockchainError>;

    /// Delete agent authorization
    async fn delete_native_asset_agent_auth(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError>;

    /// Check if agent is authorized
    async fn has_native_asset_agent_auth(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<bool, BlockchainError>;

    // ===== Metadata Operations =====

    /// Get metadata URI for asset
    async fn get_native_asset_metadata_uri(
        &self,
        asset: &Hash,
    ) -> Result<Option<String>, BlockchainError>;

    /// Set metadata URI for asset
    async fn set_native_asset_metadata_uri(
        &mut self,
        asset: &Hash,
        uri: Option<&str>,
    ) -> Result<(), BlockchainError>;

    // ===== Lock Index Operations =====

    /// Get list of lock IDs for an account
    async fn get_native_asset_lock_ids(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<Vec<u64>, BlockchainError>;

    /// Add lock ID to account's lock index
    async fn add_native_asset_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError>;

    /// Remove lock ID from account's lock index
    async fn remove_native_asset_lock_id(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        lock_id: u64,
    ) -> Result<(), BlockchainError>;

    // ===== User Escrow Index Operations =====

    /// Get list of escrow IDs for a user
    async fn get_native_asset_user_escrows(
        &self,
        asset: &Hash,
        user: &[u8; 32],
    ) -> Result<Vec<u64>, BlockchainError>;

    /// Add escrow ID to user's escrow index
    async fn add_native_asset_user_escrow(
        &mut self,
        asset: &Hash,
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), BlockchainError>;

    /// Remove escrow ID from user's escrow index
    async fn remove_native_asset_user_escrow(
        &mut self,
        asset: &Hash,
        user: &[u8; 32],
        escrow_id: u64,
    ) -> Result<(), BlockchainError>;

    // ===== Owner Agents Index Operations =====

    /// Get list of agents for an owner
    async fn get_native_asset_owner_agents(
        &self,
        asset: &Hash,
        owner: &[u8; 32],
    ) -> Result<Vec<[u8; 32]>, BlockchainError>;

    /// Add agent to owner's agents index
    async fn add_native_asset_owner_agent(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError>;

    /// Remove agent from owner's agents index
    async fn remove_native_asset_owner_agent(
        &mut self,
        asset: &Hash,
        owner: &[u8; 32],
        agent: &[u8; 32],
    ) -> Result<(), BlockchainError>;

    // ===== Role Members Index Operations =====

    /// Get list of members for a role
    async fn get_native_asset_role_members(
        &self,
        asset: &Hash,
        role: &RoleId,
    ) -> Result<Vec<[u8; 32]>, BlockchainError>;

    /// Get role member by index
    async fn get_native_asset_role_member(
        &self,
        asset: &Hash,
        role: &RoleId,
        index: u32,
    ) -> Result<[u8; 32], BlockchainError>;

    /// Add member to role members index
    async fn add_native_asset_role_member(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError>;

    /// Remove member from role members index
    async fn remove_native_asset_role_member(
        &mut self,
        asset: &Hash,
        role: &RoleId,
        account: &[u8; 32],
    ) -> Result<(), BlockchainError>;

    // ===== Admin Proposal Operations =====

    /// Get pending admin for asset
    async fn get_native_asset_pending_admin(
        &self,
        asset: &Hash,
    ) -> Result<Option<[u8; 32]>, BlockchainError>;

    /// Set pending admin for asset
    async fn set_native_asset_pending_admin(
        &mut self,
        asset: &Hash,
        admin: Option<&[u8; 32]>,
    ) -> Result<(), BlockchainError>;

    // ===== Balance Checkpoint Operations =====

    /// Get balance checkpoint count for an account
    async fn get_native_asset_balance_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError>;

    /// Set balance checkpoint count for an account
    async fn set_native_asset_balance_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError>;

    /// Get balance checkpoint at index
    async fn get_native_asset_balance_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<BalanceCheckpoint, BlockchainError>;

    /// Set balance checkpoint at index
    async fn set_native_asset_balance_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &BalanceCheckpoint,
    ) -> Result<(), BlockchainError>;

    // ===== Delegation Checkpoint Operations =====

    /// Get delegation checkpoint count for an account
    async fn get_native_asset_delegation_checkpoint_count(
        &self,
        asset: &Hash,
        account: &[u8; 32],
    ) -> Result<u32, BlockchainError>;

    /// Set delegation checkpoint count for an account
    async fn set_native_asset_delegation_checkpoint_count(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        count: u32,
    ) -> Result<(), BlockchainError>;

    /// Get delegation checkpoint at index
    async fn get_native_asset_delegation_checkpoint(
        &self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
    ) -> Result<DelegationCheckpoint, BlockchainError>;

    /// Set delegation checkpoint at index
    async fn set_native_asset_delegation_checkpoint(
        &mut self,
        asset: &Hash,
        account: &[u8; 32],
        index: u32,
        checkpoint: &DelegationCheckpoint,
    ) -> Result<(), BlockchainError>;
}

// ===== Storage Key Builders =====

/// Build storage key for native asset data
pub fn build_native_asset_key(asset: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key
}

/// Build storage key for native asset balance
pub fn build_native_asset_balance_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_BALANCE_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for native asset allowance
pub fn build_native_asset_allowance_key(
    asset: &Hash,
    owner: &[u8; 32],
    spender: &[u8; 32],
) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_ALLOWANCE_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(owner);
    key.extend_from_slice(spender);
    key
}

/// Build storage key for native asset supply
pub fn build_native_asset_supply_key(asset: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_SUPPLY_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key
}

/// Build storage key for token lock
pub fn build_native_asset_lock_key(asset: &Hash, account: &[u8; 32], lock_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32 + 8);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_LOCK_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key.extend_from_slice(&lock_id.to_be_bytes());
    key
}

/// Build storage key for lock count
pub fn build_native_asset_lock_count_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_LOCK_COUNT_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for next lock ID
pub fn build_native_asset_lock_next_id_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_LOCK_NEXT_ID_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for locked balance
pub fn build_native_asset_locked_balance_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_LOCKED_BALANCE_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for role config
pub fn build_native_asset_role_config_key(asset: &Hash, role: &RoleId) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_ROLE_CONFIG_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(role);
    key
}

/// Build storage key for role member
pub fn build_native_asset_role_member_key(
    asset: &Hash,
    role: &RoleId,
    account: &[u8; 32],
) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_ROLE_MEMBER_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(role);
    key.extend_from_slice(account);
    key
}

/// Build storage key for pause state
pub fn build_native_asset_pause_key(asset: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_PAUSE_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key
}

/// Build storage key for freeze state
pub fn build_native_asset_freeze_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_FREEZE_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for escrow counter
pub fn build_native_asset_escrow_counter_key(asset: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_ESCROW_COUNTER_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key
}

/// Build storage key for escrow data
pub fn build_native_asset_escrow_key(asset: &Hash, escrow_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 8);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_ESCROW_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(&escrow_id.to_be_bytes());
    key
}

/// Build storage key for permit nonce
pub fn build_native_asset_permit_nonce_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_PERMIT_NONCE_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for delegation
pub fn build_native_asset_delegation_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_DELEGATION_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for checkpoint
pub fn build_native_asset_checkpoint_key(asset: &Hash, account: &[u8; 32], index: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32 + 4);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_CHECKPOINT_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key.extend_from_slice(&index.to_be_bytes());
    key
}

/// Build storage key for checkpoint count
pub fn build_native_asset_checkpoint_count_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_CHECKPOINT_COUNT_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for agent authorization
pub fn build_native_asset_agent_auth_key(
    asset: &Hash,
    owner: &[u8; 32],
    agent: &[u8; 32],
) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_AGENT_AUTH_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(owner);
    key.extend_from_slice(agent);
    key
}

/// Build storage key for metadata URI
pub fn build_native_asset_metadata_key(asset: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_METADATA_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key
}

/// Build storage key for lock index (list of lock IDs per account)
pub fn build_native_asset_lock_index_key(asset: &Hash, account: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_LOCK_INDEX_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for user escrows index
pub fn build_native_asset_user_escrows_key(asset: &Hash, user: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_USER_ESCROWS_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(user);
    key
}

/// Build storage key for owner agents index
pub fn build_native_asset_owner_agents_key(asset: &Hash, owner: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_OWNER_AGENTS_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(owner);
    key
}

/// Build storage key for role members index
pub fn build_native_asset_role_members_key(asset: &Hash, role: &RoleId) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_ROLE_MEMBERS_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(role);
    key
}

/// Build storage key for pending admin
pub fn build_native_asset_pending_admin_key(asset: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_PENDING_ADMIN_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key
}

/// Build storage key for balance checkpoint
pub fn build_native_asset_balance_checkpoint_key(
    asset: &Hash,
    account: &[u8; 32],
    index: u32,
) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32 + 4);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_BALANCE_CHECKPOINT_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key.extend_from_slice(&index.to_be_bytes());
    key
}

/// Build storage key for balance checkpoint count
pub fn build_native_asset_balance_checkpoint_count_key(
    asset: &Hash,
    account: &[u8; 32],
) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_BALANCE_CHECKPOINT_COUNT_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}

/// Build storage key for delegation checkpoint
pub fn build_native_asset_delegation_checkpoint_key(
    asset: &Hash,
    account: &[u8; 32],
    index: u32,
) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32 + 4);
    key.extend_from_slice(tos_common::native_asset::NATIVE_ASSET_DELEGATION_CHECKPOINT_PREFIX);
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key.extend_from_slice(&index.to_be_bytes());
    key
}

/// Build storage key for delegation checkpoint count
pub fn build_native_asset_delegation_checkpoint_count_key(
    asset: &Hash,
    account: &[u8; 32],
) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 32);
    key.extend_from_slice(
        tos_common::native_asset::NATIVE_ASSET_DELEGATION_CHECKPOINT_COUNT_PREFIX,
    );
    key.extend_from_slice(asset.as_bytes());
    key.extend_from_slice(account);
    key
}
