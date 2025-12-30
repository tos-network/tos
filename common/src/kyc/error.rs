// KYC Error types
// Defines all error conditions for KYC operations

use std::fmt;

/// KYC verification and operation errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KycError {
    /// Invalid KYC level (not cumulative)
    /// Valid levels: 0, 7, 31, 63, 255, 2047, 8191, 16383, 32767
    InvalidLevel(u16),

    /// Committee not found
    CommitteeNotFound,

    /// Committee not active (suspended or dissolved)
    CommitteeNotActive,

    /// Level exceeds committee's maximum allowed level
    LevelExceedsCommitteeMax { requested: u16, max_allowed: u16 },

    /// Insufficient approvals for operation
    InsufficientApprovals { required: u8, provided: u8 },

    /// Duplicate approver in approval list
    DuplicateApprover,

    /// Invalid approver (not active member or is Observer)
    InvalidApprover,

    /// Approval timestamp expired (older than 24 hours)
    ApprovalExpired,

    /// Invalid cryptographic signature
    InvalidSignature,

    /// Global committee already exists (can only bootstrap once)
    GlobalCommitteeAlreadyExists,

    /// Not authorized to perform this operation
    NotAuthorized,

    /// KYC record not found for account
    KycNotFound,

    /// KYC record already exists for account
    KycAlreadyExists,

    /// Cannot downgrade KYC level
    CannotDowngradeLevel { current: u16, requested: u16 },

    /// Parent committee required for this operation
    ParentCommitteeRequired,

    /// Source and destination committee mismatch in transfer
    CommitteeMismatch,

    /// Emergency suspension has expired (24-hour timeout)
    EmergencySuspensionExpired,

    /// Rate limit exceeded
    RateLimitExceeded { limit: u32, current: u32 },

    /// Member not found in committee
    MemberNotFound,

    /// Invalid threshold configuration
    /// Governance threshold must be >= 2/3 of active members
    InvalidThreshold,

    /// Invalid KYC threshold
    /// KYC threshold must be >= 1
    InvalidKycThreshold,

    /// KYC status does not allow this operation
    InvalidStatus(String),

    /// KYC has expired
    KycExpired,

    /// KYC is revoked
    KycRevoked,

    /// KYC is suspended
    KycSuspended,

    /// Account address mismatch
    AccountMismatch,

    /// Invalid region for operation
    InvalidRegion,

    /// Transfer requires approval from both committees
    TransferIncomplete,

    /// Appeal already pending
    AppealAlreadyPending,

    /// Invalid data hash
    InvalidDataHash,

    /// Committee has insufficient active members
    InsufficientMembers { required: usize, active: usize },

    /// Operation not allowed for this tier
    TierNotAllowed { tier: u8, operation: String },

    /// Timestamp validation failed
    InvalidTimestamp,

    /// Serialization/deserialization error
    SerializationError(String),
}

impl fmt::Display for KycError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KycError::InvalidLevel(level) => {
                write!(f, "Invalid KYC level: {}. Valid levels are: 0, 7, 31, 63, 255, 2047, 8191, 16383, 32767", level)
            }
            KycError::CommitteeNotFound => write!(f, "Committee not found"),
            KycError::CommitteeNotActive => write!(f, "Committee is not active"),
            KycError::LevelExceedsCommitteeMax {
                requested,
                max_allowed,
            } => {
                write!(
                    f,
                    "Requested level {} exceeds committee maximum {}",
                    requested, max_allowed
                )
            }
            KycError::InsufficientApprovals { required, provided } => {
                write!(
                    f,
                    "Insufficient approvals: required {}, provided {}",
                    required, provided
                )
            }
            KycError::DuplicateApprover => write!(f, "Duplicate approver in approval list"),
            KycError::InvalidApprover => {
                write!(f, "Invalid approver: not active member or is Observer")
            }
            KycError::ApprovalExpired => write!(f, "Approval has expired (older than 24 hours)"),
            KycError::InvalidSignature => write!(f, "Invalid cryptographic signature"),
            KycError::GlobalCommitteeAlreadyExists => {
                write!(f, "Global committee already exists")
            }
            KycError::NotAuthorized => write!(f, "Not authorized for this operation"),
            KycError::KycNotFound => write!(f, "KYC record not found"),
            KycError::KycAlreadyExists => write!(f, "KYC record already exists"),
            KycError::CannotDowngradeLevel { current, requested } => {
                write!(
                    f,
                    "Cannot downgrade level from {} to {}",
                    current, requested
                )
            }
            KycError::ParentCommitteeRequired => {
                write!(f, "Parent committee approval required")
            }
            KycError::CommitteeMismatch => write!(f, "Committee mismatch in operation"),
            KycError::EmergencySuspensionExpired => {
                write!(f, "Emergency suspension has expired")
            }
            KycError::RateLimitExceeded { limit, current } => {
                write!(
                    f,
                    "Rate limit exceeded: limit {}, current {}",
                    limit, current
                )
            }
            KycError::MemberNotFound => write!(f, "Member not found in committee"),
            KycError::InvalidThreshold => {
                write!(f, "Invalid threshold: must be >= 2/3 of active members")
            }
            KycError::InvalidKycThreshold => {
                write!(f, "Invalid KYC threshold: must be >= 1")
            }
            KycError::InvalidStatus(msg) => write!(f, "Invalid status: {}", msg),
            KycError::KycExpired => write!(f, "KYC has expired"),
            KycError::KycRevoked => write!(f, "KYC has been revoked"),
            KycError::KycSuspended => write!(f, "KYC is suspended"),
            KycError::AccountMismatch => write!(f, "Account address mismatch"),
            KycError::InvalidRegion => write!(f, "Invalid region for operation"),
            KycError::TransferIncomplete => {
                write!(f, "Transfer requires approval from both committees")
            }
            KycError::AppealAlreadyPending => write!(f, "Appeal already pending"),
            KycError::InvalidDataHash => write!(f, "Invalid data hash"),
            KycError::InsufficientMembers { required, active } => {
                write!(
                    f,
                    "Insufficient active members: required {}, active {}",
                    required, active
                )
            }
            KycError::TierNotAllowed { tier, operation } => {
                write!(f, "Tier {} not allowed for operation: {}", tier, operation)
            }
            KycError::InvalidTimestamp => write!(f, "Invalid timestamp"),
            KycError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for KycError {}

/// Result type for KYC operations
pub type KycResult<T> = Result<T, KycError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        assert!(KycError::InvalidLevel(100).to_string().contains("100"));
        assert!(KycError::CommitteeNotFound
            .to_string()
            .contains("not found"));
        assert!(KycError::InsufficientApprovals {
            required: 3,
            provided: 1
        }
        .to_string()
        .contains("3"));
    }
}
