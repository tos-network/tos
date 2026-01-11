//! Native Asset Constants
//!
//! Defines limits, prefixes, and configuration constants.

// ===== Asset Limits =====

/// Maximum length of asset name (bytes)
pub const MAX_NAME_LENGTH: usize = 64;

/// Maximum length of asset symbol/ticker (bytes)
pub const MAX_SYMBOL_LENGTH: usize = 12;

/// Maximum decimals for an asset
pub const MAX_DECIMALS: u8 = 18;

/// Maximum metadata URI length (bytes)
pub const MAX_METADATA_URI_LENGTH: usize = 512;

// ===== Timelock Limits =====

/// Maximum lock duration in seconds (10 years)
pub const MAX_LOCK_DURATION: u64 = 10 * 365 * 24 * 60 * 60;

/// Maximum number of locks per account per asset
pub const MAX_LOCKS_PER_ACCOUNT: usize = 32;

// ===== Escrow Limits =====

/// Maximum escrow participants
pub const MAX_ESCROW_PARTICIPANTS: usize = 10;

/// Maximum escrow approvers
pub const MAX_ESCROW_APPROVERS: usize = 5;

/// Maximum escrow metadata size (bytes)
pub const MAX_ESCROW_METADATA_SIZE: usize = 1024;

/// Maximum escrow params serialized size
pub const MAX_ESCROW_PARAMS_SIZE: usize = 4096;

// ===== AGI Limits =====

/// Maximum agents per owner
pub const MAX_AGENTS_PER_OWNER: usize = 32;

/// Maximum allowed recipients per agent
pub const MAX_ALLOWED_RECIPIENTS: usize = 10;

// ===== Role Limits =====

/// Maximum roles per asset
pub const MAX_ROLES_PER_ASSET: usize = 64;

/// Maximum role members per role
pub const MAX_ROLE_MEMBERS: usize = 256;

// ===== Storage Key Prefixes (4-byte) =====

/// Native asset data prefix
pub const NATIVE_ASSET_PREFIX: &[u8] = b"nasd";

/// Native asset balance prefix
pub const NATIVE_ASSET_BALANCE_PREFIX: &[u8] = b"nasb";

/// Native asset allowance prefix
pub const NATIVE_ASSET_ALLOWANCE_PREFIX: &[u8] = b"nasa";

/// Native asset total supply prefix
pub const NATIVE_ASSET_SUPPLY_PREFIX: &[u8] = b"nass";

/// Lock data prefix (nalD = native asset lock Data)
pub const NATIVE_ASSET_LOCK_PREFIX: &[u8] = b"nalD";

/// Lock count prefix (nalC = native asset lock Count)
pub const NATIVE_ASSET_LOCK_COUNT_PREFIX: &[u8] = b"nalC";

/// Next lock ID prefix (nalN = native asset lock Next)
pub const NATIVE_ASSET_LOCK_NEXT_ID_PREFIX: &[u8] = b"nalN";

/// Locked balance prefix (nalB = native asset lock Balance)
pub const NATIVE_ASSET_LOCKED_BALANCE_PREFIX: &[u8] = b"nalB";

/// Role config prefix
pub const NATIVE_ASSET_ROLE_CONFIG_PREFIX: &[u8] = b"narc";

/// Role member prefix
pub const NATIVE_ASSET_ROLE_MEMBER_PREFIX: &[u8] = b"narm";

/// Account roles prefix
pub const NATIVE_ASSET_ACCOUNT_ROLES_PREFIX: &[u8] = b"naar";

/// Pause state prefix
pub const NATIVE_ASSET_PAUSE_PREFIX: &[u8] = b"naps";

/// Freeze state prefix
pub const NATIVE_ASSET_FREEZE_PREFIX: &[u8] = b"nafz";

/// Metadata prefix
pub const NATIVE_ASSET_METADATA_PREFIX: &[u8] = b"namd";

/// Escrow counter prefix
pub const NATIVE_ASSET_ESCROW_COUNTER_PREFIX: &[u8] = b"naec";

/// Escrow data prefix
pub const NATIVE_ASSET_ESCROW_PREFIX: &[u8] = b"naes";

/// User escrows prefix
pub const NATIVE_ASSET_USER_ESCROWS_PREFIX: &[u8] = b"naue";

/// Permit nonce prefix
pub const NATIVE_ASSET_PERMIT_NONCE_PREFIX: &[u8] = b"napn";

/// Delegation prefix
pub const NATIVE_ASSET_DELEGATION_PREFIX: &[u8] = b"nadl";

/// Checkpoint prefix
pub const NATIVE_ASSET_CHECKPOINT_PREFIX: &[u8] = b"nack";

/// Checkpoint count prefix
pub const NATIVE_ASSET_CHECKPOINT_COUNT_PREFIX: &[u8] = b"nacc";

/// Agent authorization prefix
pub const NATIVE_ASSET_AGENT_AUTH_PREFIX: &[u8] = b"naag";

/// Lock index prefix (list of lock IDs per account)
pub const NATIVE_ASSET_LOCK_INDEX_PREFIX: &[u8] = b"nali";

/// Owner agents prefix (list of agents per owner)
pub const NATIVE_ASSET_OWNER_AGENTS_PREFIX: &[u8] = b"naoa";

/// Role members index prefix (list of members per role)
pub const NATIVE_ASSET_ROLE_MEMBERS_PREFIX: &[u8] = b"nari";

/// Pending admin prefix (for admin proposal workflow)
pub const NATIVE_ASSET_PENDING_ADMIN_PREFIX: &[u8] = b"napa";

/// Balance checkpoint prefix (for historical balance queries)
pub const NATIVE_ASSET_BALANCE_CHECKPOINT_PREFIX: &[u8] = b"nabc";

/// Balance checkpoint count prefix
pub const NATIVE_ASSET_BALANCE_CHECKPOINT_COUNT_PREFIX: &[u8] = b"nabC";

/// Delegation checkpoint prefix (for historical delegation queries)
pub const NATIVE_ASSET_DELEGATION_CHECKPOINT_PREFIX: &[u8] = b"nadc";

/// Delegation checkpoint count prefix
pub const NATIVE_ASSET_DELEGATION_CHECKPOINT_COUNT_PREFIX: &[u8] = b"nadC";
