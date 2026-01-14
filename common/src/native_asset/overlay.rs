use crate::crypto::Hash;
use crate::native_asset::{
    AdminDelay, AgentAuthorization, Allowance, BalanceCheckpoint, Checkpoint, Delegation,
    DelegationCheckpoint, Escrow, FreezeState, NativeAssetData, PauseState, RoleConfig, RoleId,
    SupplyCheckpoint, TimelockOperation, TokenLock,
};
use std::collections::HashMap;

/// Key types for overlay storage
///
/// Each variant represents a unique storage key in the native asset system.
/// Keys are designed to be hashable and comparable for use in HashMap.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NativeAssetKey {
    // ===== Asset-level keys =====
    /// Asset metadata (name, symbol, decimals, etc.)
    Asset(Hash),
    /// Total supply of an asset
    Supply(Hash),
    /// Pause state for an asset
    PauseState(Hash),
    /// Escrow ID counter for an asset
    EscrowCounter(Hash),
    /// Pending admin transfer for an asset
    PendingAdmin(Hash),
    /// Supply checkpoint count for an asset
    SupplyCheckpointCount(Hash),
    /// Admin delay configuration
    AdminDelay(Hash),
    /// Timelock minimum delay
    TimelockMinDelay(Hash),
    /// Metadata URI for an asset
    MetadataUri(Hash),

    // ===== Account-level keys =====
    /// Balance for account on asset
    Balance { asset: Hash, account: [u8; 32] },
    /// Allowance from owner to spender
    Allowance {
        asset: Hash,
        owner: [u8; 32],
        spender: [u8; 32],
    },
    /// Token lock by ID
    Lock {
        asset: Hash,
        account: [u8; 32],
        lock_id: u64,
    },
    /// Lock count for account
    LockCount { asset: Hash, account: [u8; 32] },
    /// Total locked balance for account
    LockedBalance { asset: Hash, account: [u8; 32] },
    /// Next lock ID for account
    NextLockId { asset: Hash, account: [u8; 32] },
    /// List of lock IDs for account
    LockIds { asset: Hash, account: [u8; 32] },
    /// Freeze state for account
    FreezeState { asset: Hash, account: [u8; 32] },
    /// Permit nonce for account
    PermitNonce { asset: Hash, account: [u8; 32] },
    /// Vote delegation for account
    Delegation { asset: Hash, account: [u8; 32] },
    /// Stored vote power for account (O(1) lookup)
    VotePower { asset: Hash, account: [u8; 32] },
    /// Vote checkpoint count for account
    CheckpointCount { asset: Hash, account: [u8; 32] },
    /// Balance checkpoint count for account
    BalanceCheckpointCount { asset: Hash, account: [u8; 32] },
    /// Delegation checkpoint count for account
    DelegationCheckpointCount { asset: Hash, account: [u8; 32] },

    // ===== Indexed keys (with numeric index) =====
    /// Vote checkpoint at index
    Checkpoint {
        asset: Hash,
        account: [u8; 32],
        index: u32,
    },
    /// Balance checkpoint at index
    BalanceCheckpoint {
        asset: Hash,
        account: [u8; 32],
        index: u32,
    },
    /// Delegation checkpoint at index
    DelegationCheckpoint {
        asset: Hash,
        account: [u8; 32],
        index: u32,
    },
    /// Supply checkpoint at index
    SupplyCheckpoint { asset: Hash, index: u32 },

    // ===== Role keys =====
    /// Role configuration
    RoleConfig { asset: Hash, role: RoleId },
    /// Role membership (account has role)
    RoleMember {
        asset: Hash,
        role: RoleId,
        account: [u8; 32],
    },
    /// List of accounts with role
    RoleMembers { asset: Hash, role: RoleId },

    // ===== Escrow keys =====
    /// Escrow by ID
    Escrow { asset: Hash, escrow_id: u64 },
    /// List of escrow IDs for user
    UserEscrows { asset: Hash, user: [u8; 32] },

    // ===== Agent keys =====
    /// Agent authorization
    AgentAuth {
        asset: Hash,
        owner: [u8; 32],
        agent: [u8; 32],
    },
    /// List of agents for owner
    OwnerAgents { asset: Hash, owner: [u8; 32] },

    // ===== Delegation index (reverse mapping) =====
    /// List of delegators for a delegatee
    Delegators { asset: Hash, delegatee: [u8; 32] },

    // ===== Timelock =====
    /// Timelock operation by ID
    TimelockOperation { asset: Hash, operation_id: [u8; 32] },
}

/// Value types for overlay storage
///
/// Each variant wraps the underlying data type stored at a key.
/// The `Deleted` variant marks a key as deleted (tombstone).
#[derive(Debug, Clone)]
pub enum NativeAssetValue {
    // ===== Primitive values =====
    Asset(NativeAssetData),
    Balance(u64),
    Supply(u64),
    VotePower(u64),
    LockedBalance(u64),
    NextLockId(u64),
    LockCount(u32),
    CheckpointCount(u32),
    BalanceCheckpointCount(u32),
    DelegationCheckpointCount(u32),
    SupplyCheckpointCount(u32),
    PermitNonce(u64),
    EscrowCounter(u64),
    TimelockMinDelay(u64),
    /// Granted timestamp for role membership (0 = not member)
    RoleMemberGrantedAt(u64),

    // ===== Complex values =====
    Allowance(Allowance),
    Lock(TokenLock),
    FreezeState(FreezeState),
    PauseState(PauseState),
    Delegation(Delegation),
    Checkpoint(Checkpoint),
    BalanceCheckpoint(BalanceCheckpoint),
    DelegationCheckpoint(DelegationCheckpoint),
    SupplyCheckpoint(SupplyCheckpoint),
    RoleConfig(RoleConfig),
    Escrow(Escrow),
    AgentAuth(AgentAuthorization),
    AdminDelay(AdminDelay),
    TimelockOperation(TimelockOperation),

    // ===== Optional values =====
    PendingAdmin(Option<[u8; 32]>),
    MetadataUri(Option<String>),
    TimelockOperationOpt(Option<TimelockOperation>),

    // ===== Index values (lists) =====
    LockIds(Vec<u64>),
    UserEscrows(Vec<u64>),
    OwnerAgents(Vec<[u8; 32]>),
    RoleMembers(Vec<[u8; 32]>),
    Delegators(Vec<[u8; 32]>),

    // ===== Tombstone =====
    /// Marks a key as deleted
    Deleted,
}

/// Overlay cache for native asset operations
///
/// Provides an in-memory cache that accumulates writes during contract execution.
/// On success, changes are preserved. On failure, they are dropped.
#[derive(Debug, Clone, Default)]
pub struct NativeAssetOverlay {
    /// Changes to be applied (key → value)
    pub changes: HashMap<NativeAssetKey, NativeAssetValue>,
}

impl NativeAssetOverlay {
    /// Create a new empty overlay
    pub fn new() -> Self {
        Self {
            changes: HashMap::new(),
        }
    }

    /// Check if the overlay is empty
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Get the number of changes in the overlay
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Get a value from the overlay (returns None if not in overlay)
    pub fn get(&self, key: &NativeAssetKey) -> Option<&NativeAssetValue> {
        self.changes.get(key)
    }

    /// Set a value in the overlay
    pub fn set(&mut self, key: NativeAssetKey, value: NativeAssetValue) {
        self.changes.insert(key, value);
    }

    /// Mark a key as deleted in the overlay
    pub fn delete(&mut self, key: NativeAssetKey) {
        self.changes.insert(key, NativeAssetValue::Deleted);
    }

    /// Check if a key is marked as deleted
    pub fn is_deleted(&self, key: &NativeAssetKey) -> bool {
        matches!(self.get(key), Some(NativeAssetValue::Deleted))
    }

    /// Clear all changes from the overlay
    pub fn clear(&mut self) {
        self.changes.clear();
    }

    /// Merge another overlay into this one (other's changes take precedence)
    pub fn merge(&mut self, other: NativeAssetOverlay) {
        for (key, value) in other.changes {
            self.changes.insert(key, value);
        }
    }
}
