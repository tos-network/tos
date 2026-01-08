// Native NFT System - Error Codes
// This module defines all error codes for NFT operations.
//
// Error Code Ranges:
// - 0: Success
// - 1-99: Collection errors
// - 100-199: Token errors
// - 200-299: Permission errors
// - 300-399: Input validation errors
// - 400-499: Merkle verification errors
// - 500-599: Operation errors
// - 600-699: safe_transfer errors
// - 900-999: System errors
// - 2000-2099: Token Bound Account (TBA) errors
// - 2100-2109: Rental listing errors
// - 2110-2119: Rental state errors

use thiserror::Error;

/// NFT operation result type
pub type NftResult<T> = Result<T, NftError>;

/// NFT error type with numeric code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[repr(u64)]
pub enum NftError {
    // ========================================
    // Collection errors (1-99)
    // ========================================
    #[error("Collection not found")]
    CollectionNotFound = 1,

    #[error("Collection already exists")]
    CollectionAlreadyExists = 2,

    #[error("Max supply reached")]
    MaxSupplyReached = 3,

    #[error("Collection is paused")]
    CollectionPaused = 4,

    // ========================================
    // Token errors (100-199)
    // ========================================
    #[error("Token not found")]
    TokenNotFound = 100,

    #[error("Token already exists")]
    TokenAlreadyExists = 101,

    #[error("Token is frozen")]
    TokenFrozen = 102,

    #[error("Token is not frozen")]
    TokenNotFrozen = 103,

    // ========================================
    // Permission errors (200-299)
    // ========================================
    #[error("Not the owner")]
    NotOwner = 200,

    #[error("Not approved")]
    NotApproved = 201,

    #[error("Not the creator")]
    NotCreator = 202,

    #[error("Not mint authority")]
    NotMintAuthority = 203,

    #[error("Not freeze authority")]
    NotFreezeAuthority = 204,

    #[error("Not metadata authority")]
    NotMetadataAuthority = 205,

    #[error("Unauthorized")]
    Unauthorized = 206,

    // ========================================
    // Input validation errors (300-399)
    // ========================================
    #[error("Name too long")]
    NameTooLong = 300,

    #[error("Symbol too long")]
    SymbolTooLong = 301,

    #[error("Invalid symbol character")]
    SymbolInvalidChar = 302,

    #[error("URI too long")]
    UriTooLong = 303,

    #[error("Too many attributes")]
    TooManyAttributes = 304,

    #[error("Attribute key too long")]
    AttributeKeyTooLong = 305,

    #[error("Attribute value too long")]
    AttributeValueTooLong = 306,

    #[error("Array too long")]
    ArrayTooLong = 307,

    #[error("Nested array not allowed")]
    NestedArray = 308,

    #[error("Batch size exceeded")]
    BatchSizeExceeded = 309,

    #[error("Whitelist too large")]
    WhitelistTooLarge = 310,

    #[error("Royalty too high")]
    RoyaltyTooHigh = 311,

    #[error("Invalid amount")]
    InvalidAmount = 312,

    #[error("Invalid token ID")]
    InvalidTokenId = 313,

    #[error("Batch is empty")]
    BatchEmpty = 314,

    #[error("Duplicate token in batch")]
    DuplicateToken = 315,

    // ========================================
    // Merkle verification errors (400-499)
    // ========================================
    #[error("Invalid Merkle proof")]
    InvalidMerkleProof = 400,

    #[error("Mint limit exceeded")]
    MintLimitExceeded = 401,

    // ========================================
    // Operation errors (500-599)
    // ========================================
    #[error("Self approval not allowed")]
    SelfApproval = 500,

    #[error("Self transfer not allowed")]
    SelfTransfer = 501,

    #[error("Cannot burn frozen token")]
    CannotBurnFrozen = 502,

    #[error("Rental is active")]
    RentalActive = 503,

    // ========================================
    // safe_transfer errors (600-699)
    // ========================================
    #[error("Receiver rejected the NFT")]
    ReceiverRejected = 600,

    #[error("Receiver hook not implemented")]
    ReceiverNotImplemented = 601,

    #[error("Data too long")]
    DataTooLong = 602,

    #[error("Not authorized")]
    NotAuthorized = 603,

    // ========================================
    // System errors (900-999)
    // ========================================
    #[error("Arithmetic overflow")]
    Overflow = 900,

    #[error("Storage error")]
    StorageError = 901,

    #[error("Encoding error")]
    EncodingError = 902,

    #[error("Internal error")]
    Internal = 999,

    // ========================================
    // Token Bound Account errors (2000-2099)
    // ========================================
    #[error("TBA already exists")]
    TbaAlreadyExists = 2000,

    #[error("TBA not found")]
    TbaNotFound = 2001,

    #[error("Not TBA owner")]
    TbaNotOwner = 2002,

    #[error("TBA is inactive")]
    TbaInactive = 2003,

    #[error("TBA has assets")]
    TbaHasAssets = 2004,

    // ========================================
    // Rental listing errors (2100-2109)
    // ========================================
    #[error("Rental listing not found")]
    ListingNotFound = 2100,

    #[error("Rental listing already exists")]
    ListingExists = 2101,

    #[error("Listing not available for you")]
    ListingNotForYou = 2102,

    #[error("Not listing owner")]
    NotListingOwner = 2103,

    // ========================================
    // Rental state errors (2110-2119)
    // ========================================
    #[error("Rental not found")]
    RentalNotFound = 2110,

    #[error("Already rented")]
    AlreadyRented = 2111,

    #[error("Not rental owner")]
    NotRentalOwner = 2112,

    #[error("Not renter")]
    NotRenter = 2113,

    #[error("Rental not expired")]
    RentalNotExpired = 2114,

    #[error("Rental expired")]
    RentalExpired = 2115,

    #[error("Invalid duration")]
    InvalidDuration = 2116,

    #[error("Insufficient payment")]
    InsufficientPayment = 2117,

    #[error("Cannot rent to self")]
    SelfRent = 2118,
}

impl NftError {
    /// Get the numeric error code
    #[inline]
    pub fn code(&self) -> u64 {
        *self as u64
    }

    /// Create error from numeric code
    pub fn from_code(code: u64) -> Option<Self> {
        match code {
            1 => Some(Self::CollectionNotFound),
            2 => Some(Self::CollectionAlreadyExists),
            3 => Some(Self::MaxSupplyReached),
            4 => Some(Self::CollectionPaused),
            100 => Some(Self::TokenNotFound),
            101 => Some(Self::TokenAlreadyExists),
            102 => Some(Self::TokenFrozen),
            103 => Some(Self::TokenNotFrozen),
            200 => Some(Self::NotOwner),
            201 => Some(Self::NotApproved),
            202 => Some(Self::NotCreator),
            203 => Some(Self::NotMintAuthority),
            204 => Some(Self::NotFreezeAuthority),
            205 => Some(Self::NotMetadataAuthority),
            206 => Some(Self::Unauthorized),
            300 => Some(Self::NameTooLong),
            301 => Some(Self::SymbolTooLong),
            302 => Some(Self::SymbolInvalidChar),
            303 => Some(Self::UriTooLong),
            304 => Some(Self::TooManyAttributes),
            305 => Some(Self::AttributeKeyTooLong),
            306 => Some(Self::AttributeValueTooLong),
            307 => Some(Self::ArrayTooLong),
            308 => Some(Self::NestedArray),
            309 => Some(Self::BatchSizeExceeded),
            310 => Some(Self::WhitelistTooLarge),
            311 => Some(Self::RoyaltyTooHigh),
            312 => Some(Self::InvalidAmount),
            313 => Some(Self::InvalidTokenId),
            314 => Some(Self::BatchEmpty),
            315 => Some(Self::DuplicateToken),
            400 => Some(Self::InvalidMerkleProof),
            401 => Some(Self::MintLimitExceeded),
            500 => Some(Self::SelfApproval),
            501 => Some(Self::SelfTransfer),
            502 => Some(Self::CannotBurnFrozen),
            503 => Some(Self::RentalActive),
            600 => Some(Self::ReceiverRejected),
            601 => Some(Self::ReceiverNotImplemented),
            602 => Some(Self::DataTooLong),
            603 => Some(Self::NotAuthorized),
            900 => Some(Self::Overflow),
            901 => Some(Self::StorageError),
            902 => Some(Self::EncodingError),
            999 => Some(Self::Internal),
            2000 => Some(Self::TbaAlreadyExists),
            2001 => Some(Self::TbaNotFound),
            2002 => Some(Self::TbaNotOwner),
            2003 => Some(Self::TbaInactive),
            2004 => Some(Self::TbaHasAssets),
            2100 => Some(Self::ListingNotFound),
            2101 => Some(Self::ListingExists),
            2102 => Some(Self::ListingNotForYou),
            2103 => Some(Self::NotListingOwner),
            2110 => Some(Self::RentalNotFound),
            2111 => Some(Self::AlreadyRented),
            2112 => Some(Self::NotRentalOwner),
            2113 => Some(Self::NotRenter),
            2114 => Some(Self::RentalNotExpired),
            2115 => Some(Self::RentalExpired),
            2116 => Some(Self::InvalidDuration),
            2117 => Some(Self::InsufficientPayment),
            2118 => Some(Self::SelfRent),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes_unique() {
        // Verify all error codes are unique
        let codes = [
            NftError::CollectionNotFound,
            NftError::CollectionAlreadyExists,
            NftError::MaxSupplyReached,
            NftError::CollectionPaused,
            NftError::TokenNotFound,
            NftError::TokenAlreadyExists,
            NftError::TokenFrozen,
            NftError::TokenNotFrozen,
            NftError::NotOwner,
            NftError::NotApproved,
            NftError::NotCreator,
            NftError::NotMintAuthority,
            NftError::NotFreezeAuthority,
            NftError::NotMetadataAuthority,
            NftError::Unauthorized,
            NftError::NameTooLong,
            NftError::SymbolTooLong,
            NftError::SymbolInvalidChar,
            NftError::UriTooLong,
            NftError::TooManyAttributes,
            NftError::AttributeKeyTooLong,
            NftError::AttributeValueTooLong,
            NftError::ArrayTooLong,
            NftError::NestedArray,
            NftError::BatchSizeExceeded,
            NftError::WhitelistTooLarge,
            NftError::RoyaltyTooHigh,
            NftError::InvalidAmount,
            NftError::InvalidTokenId,
            NftError::BatchEmpty,
            NftError::DuplicateToken,
            NftError::InvalidMerkleProof,
            NftError::MintLimitExceeded,
            NftError::SelfApproval,
            NftError::SelfTransfer,
            NftError::CannotBurnFrozen,
            NftError::RentalActive,
            NftError::ReceiverRejected,
            NftError::ReceiverNotImplemented,
            NftError::DataTooLong,
            NftError::NotAuthorized,
            NftError::Overflow,
            NftError::StorageError,
            NftError::EncodingError,
            NftError::Internal,
            NftError::TbaAlreadyExists,
            NftError::TbaNotFound,
            NftError::TbaNotOwner,
            NftError::TbaInactive,
            NftError::TbaHasAssets,
            NftError::ListingNotFound,
            NftError::ListingExists,
            NftError::ListingNotForYou,
            NftError::NotListingOwner,
            NftError::RentalNotFound,
            NftError::AlreadyRented,
            NftError::NotRentalOwner,
            NftError::NotRenter,
            NftError::RentalNotExpired,
            NftError::RentalExpired,
            NftError::InvalidDuration,
            NftError::InsufficientPayment,
            NftError::SelfRent,
        ];

        let mut seen = std::collections::HashSet::new();
        for err in codes {
            let code = err.code();
            assert!(
                seen.insert(code),
                "Duplicate error code: {} for {:?}",
                code,
                err
            );
        }
    }

    #[test]
    fn test_error_code_roundtrip() {
        let err = NftError::TokenNotFound;
        let code = err.code();
        let recovered = NftError::from_code(code);
        assert_eq!(recovered, Some(err));
    }

    #[test]
    fn test_unknown_error_code() {
        assert_eq!(NftError::from_code(9999), None);
    }
}
