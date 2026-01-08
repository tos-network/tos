// Native NFT System - Core Types
// This module defines all data structures for NFT operations.

use crate::crypto::{Address, Hash, PublicKey};
use serde::{Deserialize, Serialize};
use tos_crypto::curve25519_dalek::{traits::Identity, RistrettoPoint};

use super::error::NftError;

/// Create an identity (zero) public key for representing "no key" scenarios
fn identity_public_key() -> PublicKey {
    PublicKey::new(RistrettoPoint::identity().compress())
}

/// Check if a public key is the identity (zero) key
#[allow(dead_code)]
fn is_identity_key(key: &PublicKey) -> bool {
    *key.as_bytes() == *RistrettoPoint::identity().compress().as_bytes()
}

// ========================================
// Protocol Constants
// ========================================

/// Maximum collection name length (bytes)
pub const MAX_NAME_LENGTH: usize = 64;

/// Maximum symbol length (bytes)
pub const MAX_SYMBOL_LENGTH: usize = 8;

/// Maximum metadata URI length (bytes)
pub const MAX_METADATA_URI_LENGTH: usize = 512;

/// Maximum base URI length (bytes)
pub const MAX_BASE_URI_LENGTH: usize = 256;

/// Maximum attributes per NFT
pub const MAX_ATTRIBUTES_COUNT: usize = 32;

/// Maximum attribute key length (bytes)
pub const MAX_ATTRIBUTE_KEY_LENGTH: usize = 32;

/// Maximum attribute string value length (bytes)
pub const MAX_ATTRIBUTE_STRING_LENGTH: usize = 256;

/// Maximum array elements in attribute
pub const MAX_ATTRIBUTE_ARRAY_LENGTH: usize = 16;

/// Maximum batch operation size
pub const MAX_BATCH_SIZE: usize = 100;

/// Maximum whitelist size (use Merkle tree for larger lists)
pub const MAX_WHITELIST_SIZE: usize = 100;

/// Maximum royalty basis points (50% = 5000)
pub const MAX_ROYALTY_BASIS_POINTS: u16 = 5000;

/// Maximum rental duration in blocks (~3 months at 1 block/sec)
pub const MAX_RENTAL_DURATION: u64 = 2_628_000;

// ========================================
// Attribute Value
// ========================================

/// Attribute value types for NFT metadata
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    /// String value (max 256 bytes)
    String(String),

    /// Numeric value (i64)
    Number(i64),

    /// Boolean value
    Boolean(bool),

    /// Array of values (max 16 elements, no nested arrays)
    Array(Vec<AttributeValue>),
}

impl AttributeValue {
    /// Validate the attribute value
    pub fn validate(&self) -> Result<(), NftError> {
        match self {
            AttributeValue::String(s) => {
                if s.len() > MAX_ATTRIBUTE_STRING_LENGTH {
                    return Err(NftError::AttributeValueTooLong);
                }
            }
            AttributeValue::Number(_) => {}
            AttributeValue::Boolean(_) => {}
            AttributeValue::Array(arr) => {
                if arr.len() > MAX_ATTRIBUTE_ARRAY_LENGTH {
                    return Err(NftError::ArrayTooLong);
                }
                for item in arr {
                    // Nested arrays are not allowed
                    if matches!(item, AttributeValue::Array(_)) {
                        return Err(NftError::NestedArray);
                    }
                    item.validate()?;
                }
            }
        }
        Ok(())
    }

    /// Get type identifier for serialization
    pub fn type_id(&self) -> u8 {
        match self {
            AttributeValue::String(_) => 0,
            AttributeValue::Number(_) => 1,
            AttributeValue::Boolean(_) => 2,
            AttributeValue::Array(_) => 3,
        }
    }
}

// ========================================
// Royalty Configuration
// ========================================

/// Royalty configuration for NFT sales
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Royalty {
    /// Recipient address for royalty payments
    pub recipient: PublicKey,

    /// Royalty percentage in basis points (100 = 1%, max 5000 = 50%)
    pub basis_points: u16,
}

impl Royalty {
    /// Create a new royalty configuration
    pub fn new(recipient: PublicKey, basis_points: u16) -> Self {
        Self {
            recipient,
            basis_points,
        }
    }

    /// Create zero royalty (no royalty)
    pub fn zero() -> Self {
        Self {
            recipient: identity_public_key(),
            basis_points: 0,
        }
    }

    /// Validate the royalty configuration
    pub fn validate(&self) -> Result<(), NftError> {
        if self.basis_points > MAX_ROYALTY_BASIS_POINTS {
            return Err(NftError::RoyaltyTooHigh);
        }
        // If royalty is set, recipient must be valid
        // (For basis_points == 0, recipient can be zero address)
        Ok(())
    }

    /// Calculate royalty amount using checked arithmetic
    pub fn calculate(&self, price: u64) -> Result<u64, NftError> {
        if self.basis_points == 0 {
            return Ok(0);
        }
        price
            .checked_mul(self.basis_points as u64)
            .ok_or(NftError::Overflow)?
            .checked_div(10000)
            .ok_or(NftError::Overflow)
    }
}

impl Default for Royalty {
    fn default() -> Self {
        Self::zero()
    }
}

// ========================================
// Mint Authority
// ========================================

/// Mint authority configuration
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum MintAuthority {
    /// Only creator can mint
    #[default]
    CreatorOnly,

    /// Whitelist of addresses (max 100)
    Whitelist(Vec<PublicKey>),

    /// Merkle tree whitelist (for large-scale whitelists)
    WhitelistMerkle {
        /// Merkle root hash
        root: Hash,
        /// Maximum mints per address
        max_per_address: u64,
    },

    /// Public minting
    Public {
        /// Maximum mints per address (0 = unlimited)
        max_per_address: u64,
        /// Mint price (0 = free)
        price: u64,
        /// Payment recipient address
        payment_recipient: PublicKey,
    },

    /// Contract-controlled minting
    Contract(PublicKey),
}

impl MintAuthority {
    /// Validate the mint authority configuration
    pub fn validate(&self) -> Result<(), NftError> {
        match self {
            MintAuthority::CreatorOnly => Ok(()),
            MintAuthority::Whitelist(addrs) => {
                if addrs.is_empty() {
                    return Err(NftError::InvalidAmount);
                }
                if addrs.len() > MAX_WHITELIST_SIZE {
                    return Err(NftError::WhitelistTooLarge);
                }
                Ok(())
            }
            MintAuthority::WhitelistMerkle {
                root,
                max_per_address,
            } => {
                if *root == Hash::zero() {
                    return Err(NftError::InvalidAmount);
                }
                if *max_per_address == 0 {
                    return Err(NftError::InvalidAmount);
                }
                Ok(())
            }
            MintAuthority::Public {
                price,
                payment_recipient,
                ..
            } => {
                // If price > 0, must have valid recipient to prevent fund loss
                if *price > 0 && *payment_recipient == identity_public_key() {
                    return Err(NftError::InvalidAmount);
                }
                Ok(())
            }
            MintAuthority::Contract(addr) => {
                if *addr == identity_public_key() {
                    return Err(NftError::InvalidAmount);
                }
                Ok(())
            }
        }
    }

    /// Get type identifier
    pub fn type_id(&self) -> u8 {
        match self {
            MintAuthority::CreatorOnly => 0,
            MintAuthority::Whitelist(_) => 1,
            MintAuthority::WhitelistMerkle { .. } => 2,
            MintAuthority::Public { .. } => 3,
            MintAuthority::Contract(_) => 4,
        }
    }
}

// ========================================
// NFT Collection
// ========================================

/// NFT Collection definition
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NftCollection {
    /// Collection ID (system-generated at creation)
    pub id: Hash,

    /// Collection name (max 64 bytes)
    pub name: String,

    /// Symbol (max 8 bytes, uppercase ASCII)
    pub symbol: String,

    /// Creator address
    pub creator: PublicKey,

    /// Current total supply
    pub total_supply: u64,

    /// Next token ID (starts from 1)
    pub next_token_id: u64,

    /// Maximum supply (None = unlimited)
    pub max_supply: Option<u64>,

    /// Base URI for token metadata (max 256 bytes)
    pub base_uri: String,

    /// Mint authority configuration
    pub mint_authority: MintAuthority,

    /// Default royalty configuration
    pub royalty: Royalty,

    /// Freeze authority (None = cannot freeze)
    pub freeze_authority: Option<PublicKey>,

    /// Metadata update authority (None = immutable)
    pub metadata_authority: Option<PublicKey>,

    /// Whether the collection is paused
    pub is_paused: bool,

    /// Creation block height
    pub created_at: u64,
}

impl NftCollection {
    /// Validate the collection configuration
    pub fn validate(&self) -> Result<(), NftError> {
        // Name validation
        if self.name.is_empty() {
            return Err(NftError::InvalidAmount);
        }
        if self.name.len() > MAX_NAME_LENGTH {
            return Err(NftError::NameTooLong);
        }

        // Symbol validation
        if self.symbol.is_empty() {
            return Err(NftError::InvalidAmount);
        }
        if self.symbol.len() > MAX_SYMBOL_LENGTH {
            return Err(NftError::SymbolTooLong);
        }
        if !self
            .symbol
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return Err(NftError::SymbolInvalidChar);
        }

        // Base URI validation
        if self.base_uri.len() > MAX_BASE_URI_LENGTH {
            return Err(NftError::UriTooLong);
        }

        // Creator validation
        if self.creator == identity_public_key() {
            return Err(NftError::InvalidAmount);
        }

        // Mint authority validation
        self.mint_authority.validate()?;

        // Royalty validation
        self.royalty.validate()?;

        Ok(())
    }

    /// Check if more tokens can be minted
    pub fn can_mint(&self, count: u64) -> Result<(), NftError> {
        if self.is_paused {
            return Err(NftError::CollectionPaused);
        }

        if let Some(max) = self.max_supply {
            let new_supply = self
                .total_supply
                .checked_add(count)
                .ok_or(NftError::Overflow)?;
            if new_supply > max {
                return Err(NftError::MaxSupplyReached);
            }
        }

        Ok(())
    }

    /// Get next token ID and increment
    pub fn allocate_token_id(&mut self) -> Result<u64, NftError> {
        let token_id = self.next_token_id;
        self.next_token_id = self
            .next_token_id
            .checked_add(1)
            .ok_or(NftError::Overflow)?;
        Ok(token_id)
    }
}

// ========================================
// NFT Token
// ========================================

/// NFT Token definition
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Nft {
    /// Collection ID
    pub collection: Hash,

    /// Token ID (starts from 1, 0 is invalid)
    pub token_id: u64,

    /// Owner address
    pub owner: PublicKey,

    /// Metadata URI
    pub metadata_uri: String,

    /// On-chain attributes
    pub attributes: Vec<(String, AttributeValue)>,

    /// Creation block height
    pub created_at: u64,

    /// Creator (minter) address
    pub creator: PublicKey,

    /// Token-specific royalty (overrides collection default)
    pub royalty: Option<Royalty>,

    /// Single token approval (auto-cleared on transfer/burn)
    pub approved: Option<PublicKey>,

    /// Whether the token is frozen
    pub is_frozen: bool,
}

impl Nft {
    /// Validate the NFT data
    pub fn validate(&self) -> Result<(), NftError> {
        // Token ID validation
        if self.token_id == 0 {
            return Err(NftError::InvalidTokenId);
        }

        // Collection ID validation
        if self.collection == Hash::zero() {
            return Err(NftError::CollectionNotFound);
        }

        // Owner validation
        if self.owner == identity_public_key() {
            return Err(NftError::InvalidAmount);
        }

        // URI validation
        if self.metadata_uri.len() > MAX_METADATA_URI_LENGTH {
            return Err(NftError::UriTooLong);
        }

        // Attributes validation
        if self.attributes.len() > MAX_ATTRIBUTES_COUNT {
            return Err(NftError::TooManyAttributes);
        }
        for (key, value) in &self.attributes {
            if key.len() > MAX_ATTRIBUTE_KEY_LENGTH {
                return Err(NftError::AttributeKeyTooLong);
            }
            value.validate()?;
        }

        // Token royalty validation
        if let Some(ref royalty) = self.royalty {
            royalty.validate()?;
        }

        Ok(())
    }

    /// Check if the address can operate on this NFT
    pub fn can_operate(&self, operator: &PublicKey) -> bool {
        // Owner can always operate
        if self.owner == *operator {
            return true;
        }

        // Single token approval
        if self.approved.as_ref() == Some(operator) {
            return true;
        }

        // Global operator approval needs storage lookup
        // This method only checks basic permissions
        false
    }

    /// Check if the NFT can be transferred
    pub fn can_transfer(&self) -> Result<(), NftError> {
        if self.is_frozen {
            return Err(NftError::TokenFrozen);
        }
        Ok(())
    }

    /// Clear approval (called after transfer/burn)
    pub fn clear_approval(&mut self) {
        self.approved = None;
    }
}

// ========================================
// Operator Approval
// ========================================

/// Global operator approval for a collection
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperatorApproval {
    /// Owner address
    pub owner: PublicKey,

    /// Collection ID
    pub collection: Hash,

    /// Operator address
    pub operator: PublicKey,

    /// Whether approved
    pub approved: bool,
}

impl OperatorApproval {
    /// Create new operator approval
    pub fn new(owner: PublicKey, collection: Hash, operator: PublicKey, approved: bool) -> Self {
        Self {
            owner,
            collection,
            operator,
            approved,
        }
    }
}

// ========================================
// Token Bound Account (TBA)
// ========================================

/// Token Bound Account - allows NFTs to own assets
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenBoundAccount {
    /// Bound NFT (collection ID, token ID)
    pub nft: (Hash, u64),

    /// Derived account address
    pub account: Address,

    /// Creation block height
    pub created_at: u64,

    /// Whether active
    pub is_active: bool,
}

// ========================================
// Rental System Types
// ========================================

/// Rental listing status
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RentalListingStatus {
    /// Listing is active and can be accepted
    Active,
    /// Listing has been cancelled
    Cancelled,
    /// Listing has been accepted (rental created)
    Accepted,
}

/// Rental listing - first step of two-step rental flow
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RentalListing {
    /// Listing ID
    pub id: Hash,

    /// Collection ID
    pub collection: Hash,

    /// Token ID
    pub token_id: u64,

    /// Owner (lister) address
    pub owner: PublicKey,

    /// Rental duration in blocks
    pub duration: u64,

    /// Rental fee
    pub rent_fee: u64,

    /// Payment token (Hash::zero() = native token)
    pub payment_token: Hash,

    /// Allowed renter (None = anyone)
    pub allowed_renter: Option<PublicKey>,

    /// Listing status
    pub status: RentalListingStatus,

    /// Creation block height
    pub created_at: u64,
}

/// Rental status
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RentalStatus {
    /// Rental is active
    Active,
    /// Rental has expired
    Expired,
    /// Rental was terminated early
    Terminated,
}

/// Active rental record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NftRental {
    /// Rental ID
    pub id: Hash,

    /// Collection ID
    pub collection: Hash,

    /// Token ID
    pub token_id: u64,

    /// Owner address
    pub owner: PublicKey,

    /// Renter address
    pub renter: PublicKey,

    /// Expiration block height
    pub expires_at: u64,

    /// Rental fee paid
    pub rent_fee: u64,

    /// Payment token used
    pub payment_token: Hash,

    /// Rental status
    pub status: RentalStatus,

    /// Creation block height
    pub created_at: u64,
}

impl NftRental {
    /// Check if rental is expired at given block height
    pub fn is_expired(&self, current_height: u64) -> bool {
        current_height >= self.expires_at
    }

    /// Check if rental is active (not expired and status is Active)
    pub fn is_active(&self, current_height: u64) -> bool {
        self.status == RentalStatus::Active && !self.is_expired(current_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attribute_value_validation() {
        // Valid string
        let attr = AttributeValue::String("test".to_string());
        assert!(attr.validate().is_ok());

        // String too long
        let long_string = "x".repeat(MAX_ATTRIBUTE_STRING_LENGTH + 1);
        let attr = AttributeValue::String(long_string);
        assert_eq!(attr.validate(), Err(NftError::AttributeValueTooLong));

        // Valid array
        let attr = AttributeValue::Array(vec![
            AttributeValue::String("a".to_string()),
            AttributeValue::Number(42),
        ]);
        assert!(attr.validate().is_ok());

        // Nested array (not allowed)
        let attr = AttributeValue::Array(vec![AttributeValue::Array(vec![])]);
        assert_eq!(attr.validate(), Err(NftError::NestedArray));

        // Array too long
        let long_array = vec![AttributeValue::Number(1); MAX_ATTRIBUTE_ARRAY_LENGTH + 1];
        let attr = AttributeValue::Array(long_array);
        assert_eq!(attr.validate(), Err(NftError::ArrayTooLong));
    }

    #[test]
    fn test_royalty_validation() {
        // Valid royalty
        let royalty = Royalty::new(identity_public_key(), 500); // 5%
        assert!(royalty.validate().is_ok());

        // Royalty too high
        let royalty = Royalty::new(identity_public_key(), 6000); // 60%
        assert_eq!(royalty.validate(), Err(NftError::RoyaltyTooHigh));

        // Zero royalty is valid
        let royalty = Royalty::zero();
        assert!(royalty.validate().is_ok());
    }

    #[test]
    fn test_royalty_calculation() {
        let royalty = Royalty::new(identity_public_key(), 500); // 5%

        // 5% of 1000 = 50
        assert_eq!(royalty.calculate(1000), Ok(50));

        // 5% of 100 = 5
        assert_eq!(royalty.calculate(100), Ok(5));

        // Zero royalty
        let zero_royalty = Royalty::zero();
        assert_eq!(zero_royalty.calculate(1000), Ok(0));
    }

    #[test]
    fn test_mint_authority_validation() {
        // CreatorOnly is always valid
        let auth = MintAuthority::CreatorOnly;
        assert!(auth.validate().is_ok());

        // Empty whitelist is invalid
        let auth = MintAuthority::Whitelist(vec![]);
        assert_eq!(auth.validate(), Err(NftError::InvalidAmount));

        // Whitelist too large
        let addrs = vec![identity_public_key(); MAX_WHITELIST_SIZE + 1];
        let auth = MintAuthority::Whitelist(addrs);
        assert_eq!(auth.validate(), Err(NftError::WhitelistTooLarge));
    }

    #[test]
    fn test_rental_expiration() {
        let rental = NftRental {
            id: Hash::zero(),
            collection: Hash::zero(),
            token_id: 1,
            owner: identity_public_key(),
            renter: identity_public_key(),
            expires_at: 100,
            rent_fee: 0,
            payment_token: Hash::zero(),
            status: RentalStatus::Active,
            created_at: 0,
        };

        // Not expired at height 50
        assert!(!rental.is_expired(50));
        assert!(rental.is_active(50));

        // Expired at height 100
        assert!(rental.is_expired(100));
        assert!(!rental.is_active(100));

        // Expired at height 150
        assert!(rental.is_expired(150));
        assert!(!rental.is_active(150));
    }
}
