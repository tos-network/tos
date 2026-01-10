//! Native Asset Error Codes
//!
//! Range: 0x0200 - 0x02FF
//! Format: ASSET_ERROR_<CATEGORY>_<SPECIFIC>

use std::fmt;

// ===== General Errors (0x0200 - 0x020F) =====

pub const ASSET_ERROR_NOT_FOUND: u64 = 0x0200;
pub const ASSET_ERROR_ALREADY_EXISTS: u64 = 0x0201;
pub const ASSET_ERROR_ZERO_AMOUNT: u64 = 0x0202;
pub const ASSET_ERROR_ZERO_ADDRESS: u64 = 0x0203;
pub const ASSET_ERROR_SELF_OPERATION: u64 = 0x0204;
pub const ASSET_ERROR_PAUSED: u64 = 0x0205;
pub const ASSET_ERROR_NOT_PAUSED: u64 = 0x0206;
pub const ASSET_ERROR_ACCOUNT_FROZEN: u64 = 0x0207;
pub const ASSET_ERROR_NOT_FROZEN: u64 = 0x0208;
pub const ASSET_ERROR_OVERFLOW: u64 = 0x0209;
pub const ASSET_ERROR_UNDERFLOW: u64 = 0x020A;

// ===== Balance Errors (0x0210 - 0x021F) =====

pub const ASSET_ERROR_INSUFFICIENT_BALANCE: u64 = 0x0210;
pub const ASSET_ERROR_MAX_SUPPLY_EXCEEDED: u64 = 0x0211;
pub const ASSET_ERROR_LOCKED_BALANCE: u64 = 0x0212;

// ===== Authorization Errors (0x0220 - 0x022F) =====

pub const ASSET_ERROR_NOT_AUTHORIZED: u64 = 0x0220;
pub const ASSET_ERROR_INSUFFICIENT_ALLOWANCE: u64 = 0x0221;
pub const ASSET_ERROR_OWNER_REQUIRED: u64 = 0x0222;
pub const ASSET_ERROR_SPENDER_REQUIRED: u64 = 0x0223;

// ===== Validation Errors (0x0230 - 0x023F) =====

pub const ASSET_ERROR_NAME_EMPTY: u64 = 0x0230;
pub const ASSET_ERROR_NAME_TOO_LONG: u64 = 0x0231;
pub const ASSET_ERROR_SYMBOL_EMPTY: u64 = 0x0232;
pub const ASSET_ERROR_SYMBOL_TOO_LONG: u64 = 0x0233;
pub const ASSET_ERROR_SYMBOL_INVALID: u64 = 0x0234;
pub const ASSET_ERROR_DECIMALS_TOO_HIGH: u64 = 0x0235;
pub const ASSET_ERROR_INVALID_PARAMS: u64 = 0x0236;
pub const ASSET_ERROR_PARAMS_TOO_LARGE: u64 = 0x0237;

// ===== Timelock Errors (0x0240 - 0x024F) =====

pub const ASSET_ERROR_LOCK_NOT_FOUND: u64 = 0x0240;
pub const ASSET_ERROR_LOCK_NOT_EXPIRED: u64 = 0x0241;
pub const ASSET_ERROR_LOCK_ALREADY_EXPIRED: u64 = 0x0242;
pub const ASSET_ERROR_MAX_LOCKS_EXCEEDED: u64 = 0x0243;
pub const ASSET_ERROR_LOCK_DURATION_TOO_SHORT: u64 = 0x0244;
pub const ASSET_ERROR_LOCK_DURATION_TOO_LONG: u64 = 0x0245;
pub const ASSET_ERROR_LOCK_AMOUNT_ZERO: u64 = 0x0246;
pub const ASSET_ERROR_INVALID_LOCK: u64 = 0x0247;
pub const ASSET_ERROR_LOCK_NOT_TRANSFERABLE: u64 = 0x0248;
pub const ASSET_ERROR_INVALID_AMOUNT: u64 = 0x0249;

// ===== Escrow Errors (0x0250 - 0x025F) =====

pub const ASSET_ERROR_ESCROW_NOT_FOUND: u64 = 0x0250;
pub const ASSET_ERROR_ESCROW_ALREADY_RELEASED: u64 = 0x0251;
pub const ASSET_ERROR_ESCROW_ALREADY_CANCELLED: u64 = 0x0252;
pub const ASSET_ERROR_ESCROW_CONDITION_NOT_MET: u64 = 0x0253;
pub const ASSET_ERROR_ESCROW_EXPIRED: u64 = 0x0254;
pub const ASSET_ERROR_ESCROW_NOT_EXPIRED: u64 = 0x0255;
pub const ASSET_ERROR_NOT_ESCROW_PARTICIPANT: u64 = 0x0256;
pub const ASSET_ERROR_ALREADY_APPROVED: u64 = 0x0257;
pub const ASSET_ERROR_NOT_AN_APPROVER: u64 = 0x0258;
pub const ASSET_ERROR_SELF_ESCROW: u64 = 0x0259;
pub const ASSET_ERROR_METADATA_TOO_LARGE: u64 = 0x025A;
pub const ASSET_ERROR_ESCROW_DISPUTED: u64 = 0x025B;

// ===== Governance Errors (0x0260 - 0x026F) =====

pub const ASSET_ERROR_NO_VOTING_POWER: u64 = 0x0260;
pub const ASSET_ERROR_ALREADY_DELEGATED: u64 = 0x0261;
pub const ASSET_ERROR_SELF_DELEGATION: u64 = 0x0262;
pub const ASSET_ERROR_CHECKPOINT_NOT_FOUND: u64 = 0x0263;
pub const ASSET_ERROR_FUTURE_LOOKUP: u64 = 0x0264;

// ===== Permit Errors (0x0270 - 0x027F) =====

pub const ASSET_ERROR_PERMIT_EXPIRED: u64 = 0x0270;
pub const ASSET_ERROR_INVALID_NONCE: u64 = 0x0271;
pub const ASSET_ERROR_INVALID_SIGNATURE: u64 = 0x0272;
pub const ASSET_ERROR_INVALID_DEADLINE: u64 = 0x0273;

// ===== AGI/Agent Errors (0x0280 - 0x028F) =====

pub const ASSET_ERROR_AGENT_NOT_FOUND: u64 = 0x0280;
pub const ASSET_ERROR_AGENT_ALREADY_EXISTS: u64 = 0x0281;
pub const ASSET_ERROR_SPENDING_LIMIT_EXCEEDED: u64 = 0x0282;
pub const ASSET_ERROR_RECIPIENT_NOT_ALLOWED: u64 = 0x0283;
pub const ASSET_ERROR_AUTH_EXPIRED: u64 = 0x0284;
pub const ASSET_ERROR_CANNOT_DELEGATE: u64 = 0x0285;
pub const ASSET_ERROR_MAX_AGENTS_EXCEEDED: u64 = 0x0286;

// ===== Role Errors (0x0290 - 0x029F) =====

pub const ASSET_ERROR_ROLE_NOT_FOUND: u64 = 0x0290;
pub const ASSET_ERROR_ROLE_NOT_HELD: u64 = 0x0291;
pub const ASSET_ERROR_ROLE_ALREADY_HELD: u64 = 0x0292;
pub const ASSET_ERROR_NOT_ROLE_ADMIN: u64 = 0x0293;
pub const ASSET_ERROR_MAX_ROLES_EXCEEDED: u64 = 0x0294;
pub const ASSET_ERROR_ALREADY_PAUSED: u64 = 0x0295;
pub const ASSET_ERROR_ALREADY_FROZEN: u64 = 0x0296;
pub const ASSET_ERROR_CANNOT_REVOKE_LAST_ADMIN: u64 = 0x0299;

/// Native Asset Error enum for internal use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeAssetError {
    // General
    NotFound,
    AlreadyExists,
    ZeroAmount,
    ZeroAddress,
    SelfOperation,
    Paused,
    NotPaused,
    AccountFrozen,
    NotFrozen,
    Overflow,
    Underflow,

    // Balance
    InsufficientBalance,
    MaxSupplyExceeded,
    LockedBalance,

    // Authorization
    NotAuthorized,
    InsufficientAllowance,
    OwnerRequired,
    SpenderRequired,

    // Validation
    NameEmpty,
    NameTooLong,
    SymbolEmpty,
    SymbolTooLong,
    SymbolInvalid,
    DecimalsTooHigh,
    InvalidParams,
    ParamsTooLarge,

    // Timelock
    LockNotFound,
    LockNotExpired,
    LockAlreadyExpired,
    MaxLocksExceeded,
    LockDurationTooShort,
    LockDurationTooLong,
    LockAmountZero,
    InvalidLock,
    LockNotTransferable,
    InvalidAmount,

    // Escrow
    EscrowNotFound,
    EscrowAlreadyReleased,
    EscrowAlreadyCancelled,
    EscrowConditionNotMet,
    EscrowExpired,
    EscrowNotExpired,
    NotEscrowParticipant,
    AlreadyApproved,
    NotAnApprover,
    SelfEscrow,
    MetadataTooLarge,
    EscrowDisputed,

    // Governance
    NoVotingPower,
    AlreadyDelegated,
    SelfDelegation,
    CheckpointNotFound,
    FutureLookup,

    // Permit
    PermitExpired,
    InvalidNonce,
    InvalidSignature,
    InvalidDeadline,

    // AGI/Agent
    AgentNotFound,
    AgentAlreadyExists,
    SpendingLimitExceeded,
    RecipientNotAllowed,
    AuthExpired,
    CannotDelegate,
    MaxAgentsExceeded,

    // Role
    RoleNotFound,
    RoleNotHeld,
    RoleAlreadyHeld,
    NotRoleAdmin,
    MaxRolesExceeded,
    AlreadyPaused,
    AlreadyFrozen,
    CannotRevokeLastAdmin,
}

impl NativeAssetError {
    /// Convert error to u64 error code
    pub fn to_code(self) -> u64 {
        match self {
            // General
            Self::NotFound => ASSET_ERROR_NOT_FOUND,
            Self::AlreadyExists => ASSET_ERROR_ALREADY_EXISTS,
            Self::ZeroAmount => ASSET_ERROR_ZERO_AMOUNT,
            Self::ZeroAddress => ASSET_ERROR_ZERO_ADDRESS,
            Self::SelfOperation => ASSET_ERROR_SELF_OPERATION,
            Self::Paused => ASSET_ERROR_PAUSED,
            Self::NotPaused => ASSET_ERROR_NOT_PAUSED,
            Self::AccountFrozen => ASSET_ERROR_ACCOUNT_FROZEN,
            Self::NotFrozen => ASSET_ERROR_NOT_FROZEN,
            Self::Overflow => ASSET_ERROR_OVERFLOW,
            Self::Underflow => ASSET_ERROR_UNDERFLOW,

            // Balance
            Self::InsufficientBalance => ASSET_ERROR_INSUFFICIENT_BALANCE,
            Self::MaxSupplyExceeded => ASSET_ERROR_MAX_SUPPLY_EXCEEDED,
            Self::LockedBalance => ASSET_ERROR_LOCKED_BALANCE,

            // Authorization
            Self::NotAuthorized => ASSET_ERROR_NOT_AUTHORIZED,
            Self::InsufficientAllowance => ASSET_ERROR_INSUFFICIENT_ALLOWANCE,
            Self::OwnerRequired => ASSET_ERROR_OWNER_REQUIRED,
            Self::SpenderRequired => ASSET_ERROR_SPENDER_REQUIRED,

            // Validation
            Self::NameEmpty => ASSET_ERROR_NAME_EMPTY,
            Self::NameTooLong => ASSET_ERROR_NAME_TOO_LONG,
            Self::SymbolEmpty => ASSET_ERROR_SYMBOL_EMPTY,
            Self::SymbolTooLong => ASSET_ERROR_SYMBOL_TOO_LONG,
            Self::SymbolInvalid => ASSET_ERROR_SYMBOL_INVALID,
            Self::DecimalsTooHigh => ASSET_ERROR_DECIMALS_TOO_HIGH,
            Self::InvalidParams => ASSET_ERROR_INVALID_PARAMS,
            Self::ParamsTooLarge => ASSET_ERROR_PARAMS_TOO_LARGE,

            // Timelock
            Self::LockNotFound => ASSET_ERROR_LOCK_NOT_FOUND,
            Self::LockNotExpired => ASSET_ERROR_LOCK_NOT_EXPIRED,
            Self::LockAlreadyExpired => ASSET_ERROR_LOCK_ALREADY_EXPIRED,
            Self::MaxLocksExceeded => ASSET_ERROR_MAX_LOCKS_EXCEEDED,
            Self::LockDurationTooShort => ASSET_ERROR_LOCK_DURATION_TOO_SHORT,
            Self::LockDurationTooLong => ASSET_ERROR_LOCK_DURATION_TOO_LONG,
            Self::LockAmountZero => ASSET_ERROR_LOCK_AMOUNT_ZERO,
            Self::InvalidLock => ASSET_ERROR_INVALID_LOCK,
            Self::LockNotTransferable => ASSET_ERROR_LOCK_NOT_TRANSFERABLE,
            Self::InvalidAmount => ASSET_ERROR_INVALID_AMOUNT,

            // Escrow
            Self::EscrowNotFound => ASSET_ERROR_ESCROW_NOT_FOUND,
            Self::EscrowAlreadyReleased => ASSET_ERROR_ESCROW_ALREADY_RELEASED,
            Self::EscrowAlreadyCancelled => ASSET_ERROR_ESCROW_ALREADY_CANCELLED,
            Self::EscrowConditionNotMet => ASSET_ERROR_ESCROW_CONDITION_NOT_MET,
            Self::EscrowExpired => ASSET_ERROR_ESCROW_EXPIRED,
            Self::EscrowNotExpired => ASSET_ERROR_ESCROW_NOT_EXPIRED,
            Self::NotEscrowParticipant => ASSET_ERROR_NOT_ESCROW_PARTICIPANT,
            Self::AlreadyApproved => ASSET_ERROR_ALREADY_APPROVED,
            Self::NotAnApprover => ASSET_ERROR_NOT_AN_APPROVER,
            Self::SelfEscrow => ASSET_ERROR_SELF_ESCROW,
            Self::MetadataTooLarge => ASSET_ERROR_METADATA_TOO_LARGE,
            Self::EscrowDisputed => ASSET_ERROR_ESCROW_DISPUTED,

            // Governance
            Self::NoVotingPower => ASSET_ERROR_NO_VOTING_POWER,
            Self::AlreadyDelegated => ASSET_ERROR_ALREADY_DELEGATED,
            Self::SelfDelegation => ASSET_ERROR_SELF_DELEGATION,
            Self::CheckpointNotFound => ASSET_ERROR_CHECKPOINT_NOT_FOUND,
            Self::FutureLookup => ASSET_ERROR_FUTURE_LOOKUP,

            // Permit
            Self::PermitExpired => ASSET_ERROR_PERMIT_EXPIRED,
            Self::InvalidNonce => ASSET_ERROR_INVALID_NONCE,
            Self::InvalidSignature => ASSET_ERROR_INVALID_SIGNATURE,
            Self::InvalidDeadline => ASSET_ERROR_INVALID_DEADLINE,

            // AGI/Agent
            Self::AgentNotFound => ASSET_ERROR_AGENT_NOT_FOUND,
            Self::AgentAlreadyExists => ASSET_ERROR_AGENT_ALREADY_EXISTS,
            Self::SpendingLimitExceeded => ASSET_ERROR_SPENDING_LIMIT_EXCEEDED,
            Self::RecipientNotAllowed => ASSET_ERROR_RECIPIENT_NOT_ALLOWED,
            Self::AuthExpired => ASSET_ERROR_AUTH_EXPIRED,
            Self::CannotDelegate => ASSET_ERROR_CANNOT_DELEGATE,
            Self::MaxAgentsExceeded => ASSET_ERROR_MAX_AGENTS_EXCEEDED,

            // Role
            Self::RoleNotFound => ASSET_ERROR_ROLE_NOT_FOUND,
            Self::RoleNotHeld => ASSET_ERROR_ROLE_NOT_HELD,
            Self::RoleAlreadyHeld => ASSET_ERROR_ROLE_ALREADY_HELD,
            Self::NotRoleAdmin => ASSET_ERROR_NOT_ROLE_ADMIN,
            Self::MaxRolesExceeded => ASSET_ERROR_MAX_ROLES_EXCEEDED,
            Self::AlreadyPaused => ASSET_ERROR_ALREADY_PAUSED,
            Self::AlreadyFrozen => ASSET_ERROR_ALREADY_FROZEN,
            Self::CannotRevokeLastAdmin => ASSET_ERROR_CANNOT_REVOKE_LAST_ADMIN,
        }
    }

    /// Create error from u64 error code
    pub fn from_code(code: u64) -> Option<Self> {
        match code {
            ASSET_ERROR_NOT_FOUND => Some(Self::NotFound),
            ASSET_ERROR_ALREADY_EXISTS => Some(Self::AlreadyExists),
            ASSET_ERROR_ZERO_AMOUNT => Some(Self::ZeroAmount),
            ASSET_ERROR_ZERO_ADDRESS => Some(Self::ZeroAddress),
            ASSET_ERROR_SELF_OPERATION => Some(Self::SelfOperation),
            ASSET_ERROR_PAUSED => Some(Self::Paused),
            ASSET_ERROR_NOT_PAUSED => Some(Self::NotPaused),
            ASSET_ERROR_ACCOUNT_FROZEN => Some(Self::AccountFrozen),
            ASSET_ERROR_NOT_FROZEN => Some(Self::NotFrozen),
            ASSET_ERROR_OVERFLOW => Some(Self::Overflow),
            ASSET_ERROR_UNDERFLOW => Some(Self::Underflow),
            ASSET_ERROR_INSUFFICIENT_BALANCE => Some(Self::InsufficientBalance),
            ASSET_ERROR_MAX_SUPPLY_EXCEEDED => Some(Self::MaxSupplyExceeded),
            ASSET_ERROR_LOCKED_BALANCE => Some(Self::LockedBalance),
            ASSET_ERROR_NOT_AUTHORIZED => Some(Self::NotAuthorized),
            ASSET_ERROR_INSUFFICIENT_ALLOWANCE => Some(Self::InsufficientAllowance),
            ASSET_ERROR_LOCK_NOT_FOUND => Some(Self::LockNotFound),
            ASSET_ERROR_LOCK_NOT_EXPIRED => Some(Self::LockNotExpired),
            ASSET_ERROR_MAX_LOCKS_EXCEEDED => Some(Self::MaxLocksExceeded),
            ASSET_ERROR_ESCROW_NOT_FOUND => Some(Self::EscrowNotFound),
            ASSET_ERROR_PERMIT_EXPIRED => Some(Self::PermitExpired),
            ASSET_ERROR_AGENT_NOT_FOUND => Some(Self::AgentNotFound),
            ASSET_ERROR_SPENDING_LIMIT_EXCEEDED => Some(Self::SpendingLimitExceeded),
            ASSET_ERROR_ROLE_NOT_FOUND => Some(Self::RoleNotFound),
            ASSET_ERROR_ROLE_NOT_HELD => Some(Self::RoleNotHeld),
            ASSET_ERROR_CANNOT_REVOKE_LAST_ADMIN => Some(Self::CannotRevokeLastAdmin),
            _ => None,
        }
    }
}

impl fmt::Display for NativeAssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "Asset not found"),
            Self::AlreadyExists => write!(f, "Asset already exists"),
            Self::ZeroAmount => write!(f, "Amount cannot be zero"),
            Self::ZeroAddress => write!(f, "Address cannot be zero"),
            Self::SelfOperation => write!(f, "Self operation not allowed"),
            Self::Paused => write!(f, "Asset is paused"),
            Self::NotPaused => write!(f, "Asset is not paused"),
            Self::AccountFrozen => write!(f, "Account is frozen"),
            Self::NotFrozen => write!(f, "Account is not frozen"),
            Self::Overflow => write!(f, "Arithmetic overflow"),
            Self::Underflow => write!(f, "Arithmetic underflow"),
            Self::InsufficientBalance => write!(f, "Insufficient balance"),
            Self::MaxSupplyExceeded => write!(f, "Max supply exceeded"),
            Self::LockedBalance => write!(f, "Balance is locked"),
            Self::NotAuthorized => write!(f, "Not authorized"),
            Self::InsufficientAllowance => write!(f, "Insufficient allowance"),
            Self::OwnerRequired => write!(f, "Owner required"),
            Self::SpenderRequired => write!(f, "Spender required"),
            Self::NameEmpty => write!(f, "Name cannot be empty"),
            Self::NameTooLong => write!(f, "Name too long"),
            Self::SymbolEmpty => write!(f, "Symbol cannot be empty"),
            Self::SymbolTooLong => write!(f, "Symbol too long"),
            Self::SymbolInvalid => write!(f, "Symbol invalid"),
            Self::DecimalsTooHigh => write!(f, "Decimals too high"),
            Self::InvalidParams => write!(f, "Invalid parameters"),
            Self::ParamsTooLarge => write!(f, "Parameters too large"),
            Self::LockNotFound => write!(f, "Lock not found"),
            Self::LockNotExpired => write!(f, "Lock not expired"),
            Self::LockAlreadyExpired => write!(f, "Lock already expired"),
            Self::MaxLocksExceeded => write!(f, "Max locks exceeded"),
            Self::LockDurationTooShort => write!(f, "Lock duration too short"),
            Self::LockDurationTooLong => write!(f, "Lock duration too long"),
            Self::LockAmountZero => write!(f, "Lock amount cannot be zero"),
            Self::InvalidLock => write!(f, "Invalid lock"),
            Self::LockNotTransferable => write!(f, "Lock is not transferable"),
            Self::InvalidAmount => write!(f, "Invalid amount"),
            Self::EscrowNotFound => write!(f, "Escrow not found"),
            Self::EscrowAlreadyReleased => write!(f, "Escrow already released"),
            Self::EscrowAlreadyCancelled => write!(f, "Escrow already cancelled"),
            Self::EscrowConditionNotMet => write!(f, "Escrow condition not met"),
            Self::EscrowExpired => write!(f, "Escrow expired"),
            Self::EscrowNotExpired => write!(f, "Escrow not expired"),
            Self::NotEscrowParticipant => write!(f, "Not escrow participant"),
            Self::AlreadyApproved => write!(f, "Already approved"),
            Self::NotAnApprover => write!(f, "Not an approver"),
            Self::SelfEscrow => write!(f, "Self escrow not allowed"),
            Self::MetadataTooLarge => write!(f, "Metadata too large"),
            Self::EscrowDisputed => write!(f, "Escrow is disputed"),
            Self::NoVotingPower => write!(f, "No voting power"),
            Self::AlreadyDelegated => write!(f, "Already delegated"),
            Self::SelfDelegation => write!(f, "Self delegation not allowed"),
            Self::CheckpointNotFound => write!(f, "Checkpoint not found"),
            Self::FutureLookup => write!(f, "Future lookup not allowed"),
            Self::PermitExpired => write!(f, "Permit expired"),
            Self::InvalidNonce => write!(f, "Invalid nonce"),
            Self::InvalidSignature => write!(f, "Invalid signature"),
            Self::InvalidDeadline => write!(f, "Invalid deadline"),
            Self::AgentNotFound => write!(f, "Agent not found"),
            Self::AgentAlreadyExists => write!(f, "Agent already exists"),
            Self::SpendingLimitExceeded => write!(f, "Spending limit exceeded"),
            Self::RecipientNotAllowed => write!(f, "Recipient not allowed"),
            Self::AuthExpired => write!(f, "Authorization expired"),
            Self::CannotDelegate => write!(f, "Cannot delegate"),
            Self::MaxAgentsExceeded => write!(f, "Max agents exceeded"),
            Self::RoleNotFound => write!(f, "Role not found"),
            Self::RoleNotHeld => write!(f, "Role not held"),
            Self::RoleAlreadyHeld => write!(f, "Role already held"),
            Self::NotRoleAdmin => write!(f, "Not role admin"),
            Self::MaxRolesExceeded => write!(f, "Max roles exceeded"),
            Self::AlreadyPaused => write!(f, "Already paused"),
            Self::AlreadyFrozen => write!(f, "Already frozen"),
            Self::CannotRevokeLastAdmin => write!(f, "Cannot revoke last admin"),
        }
    }
}

impl std::error::Error for NativeAssetError {}
