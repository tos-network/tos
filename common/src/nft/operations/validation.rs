// NFT Input Validation Helpers
// This module provides validation functions for NFT operation inputs.

use crate::crypto::{Hash, PublicKey};
use crate::nft::{
    AttributeValue, MintAuthority, NftError, NftResult, MAX_ATTRIBUTES_COUNT,
    MAX_ATTRIBUTE_KEY_LENGTH, MAX_BASE_URI_LENGTH, MAX_METADATA_URI_LENGTH, MAX_NAME_LENGTH,
    MAX_ROYALTY_BASIS_POINTS, MAX_SYMBOL_LENGTH,
};
use tos_crypto::curve25519_dalek::{traits::Identity, RistrettoPoint};

// ========================================
// Identity Key Helpers
// ========================================

/// Get the identity (zero) public key
pub fn identity_public_key() -> PublicKey {
    PublicKey::new(RistrettoPoint::identity().compress())
}

/// Check if a public key is the identity (zero) key
pub fn is_identity_key(key: &PublicKey) -> bool {
    *key.as_bytes() == *RistrettoPoint::identity().compress().as_bytes()
}

// ========================================
// Collection Validation
// ========================================

/// Validate collection name
pub fn validate_name(name: &str) -> NftResult<()> {
    if name.is_empty() {
        return Err(NftError::InvalidAmount);
    }
    if name.len() > MAX_NAME_LENGTH {
        return Err(NftError::NameTooLong);
    }
    Ok(())
}

/// Validate collection symbol
pub fn validate_symbol(symbol: &str) -> NftResult<()> {
    if symbol.is_empty() {
        return Err(NftError::InvalidAmount);
    }
    if symbol.len() > MAX_SYMBOL_LENGTH {
        return Err(NftError::SymbolTooLong);
    }
    if !symbol
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(NftError::SymbolInvalidChar);
    }
    Ok(())
}

/// Validate base URI
pub fn validate_base_uri(uri: &str) -> NftResult<()> {
    if uri.len() > MAX_BASE_URI_LENGTH {
        return Err(NftError::UriTooLong);
    }
    Ok(())
}

/// Validate metadata URI
pub fn validate_metadata_uri(uri: &str) -> NftResult<()> {
    if uri.len() > MAX_METADATA_URI_LENGTH {
        return Err(NftError::UriTooLong);
    }
    Ok(())
}

/// Validate royalty configuration
pub fn validate_royalty(basis_points: u16, recipient: &PublicKey) -> NftResult<()> {
    if basis_points > MAX_ROYALTY_BASIS_POINTS {
        return Err(NftError::RoyaltyTooHigh);
    }
    // If royalty > 0, recipient must be valid (non-zero)
    if basis_points > 0 && is_identity_key(recipient) {
        return Err(NftError::InvalidAmount);
    }
    Ok(())
}

// ========================================
// NFT Validation
// ========================================

/// Validate token ID (must be non-zero)
pub fn validate_token_id(token_id: u64) -> NftResult<()> {
    if token_id == 0 {
        return Err(NftError::InvalidTokenId);
    }
    Ok(())
}

/// Validate recipient address (must be non-zero)
pub fn validate_recipient(recipient: &PublicKey) -> NftResult<()> {
    if is_identity_key(recipient) {
        return Err(NftError::InvalidAmount);
    }
    Ok(())
}

/// Validate collection ID (must be non-zero)
pub fn validate_collection_id(collection: &Hash) -> NftResult<()> {
    if *collection == Hash::zero() {
        return Err(NftError::CollectionNotFound);
    }
    Ok(())
}

/// Validate attributes list
pub fn validate_attributes(attributes: &[(String, AttributeValue)]) -> NftResult<()> {
    if attributes.len() > MAX_ATTRIBUTES_COUNT {
        return Err(NftError::TooManyAttributes);
    }

    for (key, value) in attributes {
        if key.len() > MAX_ATTRIBUTE_KEY_LENGTH {
            return Err(NftError::AttributeKeyTooLong);
        }
        value.validate()?;
    }

    Ok(())
}

// ========================================
// Mint Authority Validation
// ========================================

/// Check if caller has mint authority for a collection
pub fn check_mint_authority(
    mint_authority: &MintAuthority,
    caller: &PublicKey,
    creator: &PublicKey,
    to: &PublicKey,
    current_mint_count: u64,
) -> NftResult<()> {
    match mint_authority {
        MintAuthority::CreatorOnly => {
            if caller != creator {
                return Err(NftError::NotCreator);
            }
        }
        MintAuthority::Whitelist(addrs) => {
            if !addrs.contains(caller) {
                return Err(NftError::NotMintAuthority);
            }
        }
        MintAuthority::WhitelistMerkle {
            max_per_address, ..
        } => {
            // Security: to must equal caller to prevent front-running
            if to != caller {
                return Err(NftError::Unauthorized);
            }
            // Note: Merkle proof verification is done separately
            // Check mint quota
            if current_mint_count >= *max_per_address {
                return Err(NftError::MintLimitExceeded);
            }
        }
        MintAuthority::Public {
            max_per_address, ..
        } => {
            // Check mint quota (0 = unlimited)
            if *max_per_address > 0 && current_mint_count >= *max_per_address {
                return Err(NftError::MintLimitExceeded);
            }
            // Note: Payment is handled separately
        }
        MintAuthority::Contract(contract_addr) => {
            if caller != contract_addr {
                return Err(NftError::NotMintAuthority);
            }
        }
    }

    Ok(())
}

// ========================================
// Data Length Validation
// ========================================

/// Maximum data length for safe_transfer (4KB)
pub const MAX_SAFE_TRANSFER_DATA_LENGTH: usize = 4096;

/// Validate safe_transfer data length
pub fn validate_safe_transfer_data(data: &[u8]) -> NftResult<()> {
    if data.len() > MAX_SAFE_TRANSFER_DATA_LENGTH {
        return Err(NftError::DataTooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name() {
        assert!(validate_name("Test Collection").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name(&"x".repeat(MAX_NAME_LENGTH)).is_ok());
        assert!(validate_name(&"x".repeat(MAX_NAME_LENGTH + 1)).is_err());
    }

    #[test]
    fn test_validate_symbol() {
        assert!(validate_symbol("TEST").is_ok());
        assert!(validate_symbol("TEST123").is_ok());
        assert!(validate_symbol("").is_err());
        assert!(validate_symbol("test").is_err()); // lowercase
        assert!(validate_symbol("TEST!").is_err()); // special char
        assert!(validate_symbol(&"X".repeat(MAX_SYMBOL_LENGTH)).is_ok());
        assert!(validate_symbol(&"X".repeat(MAX_SYMBOL_LENGTH + 1)).is_err());
    }

    #[test]
    fn test_validate_royalty() {
        // Use a non-identity key for valid recipient
        let valid_recipient = PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[10u8; 32])
                .expect("valid"),
        );
        let zero_recipient = identity_public_key();

        // Zero royalty with zero recipient is OK
        assert!(validate_royalty(0, &zero_recipient).is_ok());

        // Max royalty with valid (non-zero) recipient is OK
        assert!(validate_royalty(MAX_ROYALTY_BASIS_POINTS, &valid_recipient).is_ok());

        // Over max is not OK
        assert!(validate_royalty(MAX_ROYALTY_BASIS_POINTS + 1, &valid_recipient).is_err());

        // Non-zero royalty with zero recipient is not OK
        assert!(validate_royalty(100, &zero_recipient).is_err());
    }

    #[test]
    fn test_validate_token_id() {
        assert!(validate_token_id(1).is_ok());
        assert!(validate_token_id(u64::MAX).is_ok());
        assert!(validate_token_id(0).is_err());
    }

    #[test]
    fn test_validate_attributes() {
        // Empty is OK
        assert!(validate_attributes(&[]).is_ok());

        // Valid attributes
        let attrs = vec![
            (
                "key1".to_string(),
                AttributeValue::String("value".to_string()),
            ),
            ("key2".to_string(), AttributeValue::Number(42)),
        ];
        assert!(validate_attributes(&attrs).is_ok());

        // Too many attributes
        let many_attrs: Vec<_> = (0..MAX_ATTRIBUTES_COUNT + 1)
            .map(|i| (format!("k{}", i), AttributeValue::Number(i as i64)))
            .collect();
        assert!(validate_attributes(&many_attrs).is_err());

        // Key too long
        let long_key_attrs = vec![(
            "x".repeat(MAX_ATTRIBUTE_KEY_LENGTH + 1),
            AttributeValue::Number(1),
        )];
        assert!(validate_attributes(&long_key_attrs).is_err());
    }

    #[test]
    fn test_validate_safe_transfer_data() {
        assert!(validate_safe_transfer_data(&[]).is_ok());
        assert!(validate_safe_transfer_data(&[0u8; MAX_SAFE_TRANSFER_DATA_LENGTH]).is_ok());
        assert!(validate_safe_transfer_data(&[0u8; MAX_SAFE_TRANSFER_DATA_LENGTH + 1]).is_err());
    }
}
