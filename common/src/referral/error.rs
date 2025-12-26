// Referral system error types

use thiserror::Error;

/// Errors that can occur in the referral system
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ReferralError {
    /// User has already bound a referrer
    #[error("User has already bound a referrer")]
    AlreadyBound,

    /// Referrer address not found on chain
    #[error("Referrer not found")]
    ReferrerNotFound,

    /// Attempted to set self as referrer
    #[error("Cannot set self as referrer")]
    SelfReferral,

    /// Detected circular reference in referral chain
    #[error("Circular reference detected in referral chain")]
    CircularReference,

    /// Requested level exceeds maximum allowed
    #[error("Requested {requested} levels exceeds maximum {max}")]
    LevelsTooDeep { max: u8, requested: u8 },

    /// Number of ratios does not match number of levels
    #[error("Number of ratios ({ratios}) does not match levels ({levels})")]
    RatiosMismatch { levels: u8, ratios: usize },

    /// Total reward ratio exceeds 100%
    #[error("Total reward ratio {total} exceeds 10000 (100%)")]
    RatiosTooHigh { total: u32 },

    /// Insufficient balance for distribution
    #[error("Insufficient balance for distribution: need {needed}, have {available}")]
    InsufficientBalance { needed: u64, available: u64 },

    /// No uplines found for user
    #[error("User has no uplines")]
    NoUplines,

    /// User not found in referral system
    #[error("User not found in referral system")]
    UserNotFound,

    /// Pagination offset exceeds total count
    #[error("Offset {offset} exceeds total count {total}")]
    InvalidOffset { offset: u32, total: u32 },

    /// Page size exceeds maximum allowed
    #[error("Page size {requested} exceeds maximum {max}")]
    PageSizeTooLarge { max: u32, requested: u32 },

    /// Internal storage error
    #[error("Internal storage error: {0}")]
    StorageError(String),

    /// Binding is not allowed at this time (e.g., during specific block processing)
    #[error("Referrer binding not allowed at this time")]
    BindingNotAllowed,

    /// Rate limit exceeded for binding operations
    #[error("Rate limit exceeded: minimum interval between bindings is {min_interval} seconds")]
    RateLimitExceeded { min_interval: u64 },

    /// Maximum direct referrals reached for the referrer
    #[error("Referrer has reached maximum direct referrals limit of {max}")]
    MaxDirectReferralsReached { max: u32 },
}

/// Result type for referral operations
pub type ReferralResult<T> = Result<T, ReferralError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ReferralError::AlreadyBound;
        assert_eq!(err.to_string(), "User has already bound a referrer");

        let err = ReferralError::LevelsTooDeep {
            max: 100,
            requested: 150,
        };
        assert_eq!(err.to_string(), "Requested 150 levels exceeds maximum 100");

        let err = ReferralError::RatiosTooHigh { total: 12000 };
        assert_eq!(
            err.to_string(),
            "Total reward ratio 12000 exceeds 10000 (100%)"
        );
    }
}
