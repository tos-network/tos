// Native NFT System - Storage Layer
// This module defines storage key prefixes and serialization for NFT types.
//
// Storage Key Structure:
// - Collection:        nft:col:<collection_id>
// - Token:             nft:tok:<collection_id><token_id>
// - Owner Balance:     nft:own:<collection_id><owner>
// - Operator Approval: nft:opr:<owner><collection_id><operator>
// - Mint Count:        nft:mnt:<collection_id><user>
// - Collection Nonce:  nft:nonce (global counter for ID generation)
// - TBA:               nft:tba:<collection_id><token_id>
// - Rental Listing:    nft:lst:<listing_id>
// - Active Rental:     nft:rnt:<collection_id><token_id>

use crate::crypto::{Hash, PublicKey};
use crate::serializer::{Reader, ReaderError, Serializer, Writer};

use super::error::NftError;
use super::types::*;

// ========================================
// Storage Key Prefixes
// ========================================

/// Storage key prefixes for NFT data
pub mod prefixes {
    /// Collection data prefix
    pub const COLLECTION: &[u8] = b"nft:col:";

    /// Token (NFT) data prefix
    pub const TOKEN: &[u8] = b"nft:tok:";

    /// Owner token balance prefix (for balance_of queries)
    pub const OWNER_BALANCE: &[u8] = b"nft:own:";

    /// Operator approval prefix (for approve_for_all)
    pub const OPERATOR_APPROVAL: &[u8] = b"nft:opr:";

    /// Mint count prefix (for tracking per-user mints)
    pub const MINT_COUNT: &[u8] = b"nft:mnt:";

    /// Global collection creation nonce
    pub const COLLECTION_NONCE: &[u8] = b"nft:nonce";

    /// Token Bound Account prefix
    pub const TBA: &[u8] = b"nft:tba:";

    /// Rental listing prefix
    pub const RENTAL_LISTING: &[u8] = b"nft:lst:";

    /// Active rental prefix
    pub const ACTIVE_RENTAL: &[u8] = b"nft:rnt:";
}

// ========================================
// Storage Key Generation Functions
// ========================================

/// Generate storage key for a collection
pub fn collection_key(id: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefixes::COLLECTION.len() + 32);
    key.extend_from_slice(prefixes::COLLECTION);
    key.extend_from_slice(id.as_bytes());
    key
}

/// Generate storage key for an NFT token
pub fn token_key(collection: &Hash, token_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefixes::TOKEN.len() + 32 + 8);
    key.extend_from_slice(prefixes::TOKEN);
    key.extend_from_slice(collection.as_bytes());
    key.extend_from_slice(&token_id.to_be_bytes());
    key
}

/// Generate storage key for owner balance in a collection
pub fn owner_balance_key(collection: &Hash, owner: &PublicKey) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefixes::OWNER_BALANCE.len() + 32 + 32);
    key.extend_from_slice(prefixes::OWNER_BALANCE);
    key.extend_from_slice(collection.as_bytes());
    key.extend_from_slice(owner.as_bytes());
    key
}

/// Generate storage key for operator approval
pub fn operator_approval_key(
    owner: &PublicKey,
    collection: &Hash,
    operator: &PublicKey,
) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefixes::OPERATOR_APPROVAL.len() + 32 + 32 + 32);
    key.extend_from_slice(prefixes::OPERATOR_APPROVAL);
    key.extend_from_slice(owner.as_bytes());
    key.extend_from_slice(collection.as_bytes());
    key.extend_from_slice(operator.as_bytes());
    key
}

/// Generate storage key for mint count
pub fn mint_count_key(collection: &Hash, user: &PublicKey) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefixes::MINT_COUNT.len() + 32 + 32);
    key.extend_from_slice(prefixes::MINT_COUNT);
    key.extend_from_slice(collection.as_bytes());
    key.extend_from_slice(user.as_bytes());
    key
}

/// Generate storage key for collection nonce
pub fn collection_nonce_key() -> Vec<u8> {
    prefixes::COLLECTION_NONCE.to_vec()
}

/// Generate storage key for Token Bound Account
pub fn tba_key(collection: &Hash, token_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefixes::TBA.len() + 32 + 8);
    key.extend_from_slice(prefixes::TBA);
    key.extend_from_slice(collection.as_bytes());
    key.extend_from_slice(&token_id.to_be_bytes());
    key
}

/// Generate storage key for rental listing
pub fn rental_listing_key(listing_id: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefixes::RENTAL_LISTING.len() + 32);
    key.extend_from_slice(prefixes::RENTAL_LISTING);
    key.extend_from_slice(listing_id.as_bytes());
    key
}

/// Generate storage key for active rental
pub fn active_rental_key(collection: &Hash, token_id: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefixes::ACTIVE_RENTAL.len() + 32 + 8);
    key.extend_from_slice(prefixes::ACTIVE_RENTAL);
    key.extend_from_slice(collection.as_bytes());
    key.extend_from_slice(&token_id.to_be_bytes());
    key
}

// ========================================
// Serializer Implementations
// ========================================

impl Serializer for AttributeValue {
    fn write(&self, writer: &mut Writer) {
        writer.write_u8(self.type_id());
        match self {
            AttributeValue::String(s) => {
                // Write length as u16 to support up to 256 bytes
                let bytes = s.as_bytes();
                writer.write_u16(bytes.len() as u16);
                writer.write_bytes(bytes);
            }
            AttributeValue::Number(n) => {
                // Write i64 as bytes
                writer.write_bytes(&n.to_be_bytes());
            }
            AttributeValue::Boolean(b) => {
                writer.write_bool(*b);
            }
            AttributeValue::Array(arr) => {
                writer.write_u8(arr.len() as u8);
                for item in arr {
                    item.write(writer);
                }
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let type_id = reader.read_u8()?;
        match type_id {
            0 => {
                // String
                let len = reader.read_u16()? as usize;
                if len > MAX_ATTRIBUTE_STRING_LENGTH {
                    return Err(ReaderError::InvalidSize);
                }
                let s = reader.read_string_with_size(len)?;
                Ok(AttributeValue::String(s))
            }
            1 => {
                // Number (i64)
                let bytes: [u8; 8] = reader.read_bytes(8)?;
                Ok(AttributeValue::Number(i64::from_be_bytes(bytes)))
            }
            2 => {
                // Boolean
                Ok(AttributeValue::Boolean(reader.read_bool()?))
            }
            3 => {
                // Array
                let len = reader.read_u8()? as usize;
                if len > MAX_ATTRIBUTE_ARRAY_LENGTH {
                    return Err(ReaderError::InvalidSize);
                }
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    arr.push(AttributeValue::read(reader)?);
                }
                Ok(AttributeValue::Array(arr))
            }
            _ => Err(ReaderError::InvalidValue),
        }
    }
}

impl Serializer for Royalty {
    fn write(&self, writer: &mut Writer) {
        self.recipient.write(writer);
        writer.write_u16(self.basis_points);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let recipient = PublicKey::read(reader)?;
        let basis_points = reader.read_u16()?;
        Ok(Royalty {
            recipient,
            basis_points,
        })
    }
}

impl Serializer for MintAuthority {
    fn write(&self, writer: &mut Writer) {
        writer.write_u8(self.type_id());
        match self {
            MintAuthority::CreatorOnly => {
                // No additional data
            }
            MintAuthority::Whitelist(addrs) => {
                writer.write_u8(addrs.len() as u8);
                for addr in addrs {
                    addr.write(writer);
                }
            }
            MintAuthority::WhitelistMerkle {
                root,
                max_per_address,
            } => {
                root.write(writer);
                writer.write_u64(max_per_address);
            }
            MintAuthority::Public {
                max_per_address,
                price,
                payment_recipient,
            } => {
                writer.write_u64(max_per_address);
                writer.write_u64(price);
                payment_recipient.write(writer);
            }
            MintAuthority::Contract(addr) => {
                addr.write(writer);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let type_id = reader.read_u8()?;
        match type_id {
            0 => Ok(MintAuthority::CreatorOnly),
            1 => {
                let len = reader.read_u8()? as usize;
                if len > MAX_WHITELIST_SIZE {
                    return Err(ReaderError::InvalidSize);
                }
                let mut addrs = Vec::with_capacity(len);
                for _ in 0..len {
                    addrs.push(PublicKey::read(reader)?);
                }
                Ok(MintAuthority::Whitelist(addrs))
            }
            2 => {
                let root = Hash::read(reader)?;
                let max_per_address = reader.read_u64()?;
                Ok(MintAuthority::WhitelistMerkle {
                    root,
                    max_per_address,
                })
            }
            3 => {
                let max_per_address = reader.read_u64()?;
                let price = reader.read_u64()?;
                let payment_recipient = PublicKey::read(reader)?;
                Ok(MintAuthority::Public {
                    max_per_address,
                    price,
                    payment_recipient,
                })
            }
            4 => {
                let addr = PublicKey::read(reader)?;
                Ok(MintAuthority::Contract(addr))
            }
            _ => Err(ReaderError::InvalidValue),
        }
    }
}

impl Serializer for NftCollection {
    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        writer.write_string(&self.name);

        // Symbol is short (max 8 bytes), use u8 length
        let symbol_bytes = self.symbol.as_bytes();
        writer.write_u8(symbol_bytes.len() as u8);
        writer.write_bytes(symbol_bytes);

        self.creator.write(writer);
        writer.write_u64(&self.total_supply);
        writer.write_u64(&self.next_token_id);

        // Optional max_supply
        match self.max_supply {
            Some(max) => {
                writer.write_bool(true);
                writer.write_u64(&max);
            }
            None => {
                writer.write_bool(false);
            }
        }

        // Base URI with u16 length
        let base_uri_bytes = self.base_uri.as_bytes();
        writer.write_u16(base_uri_bytes.len() as u16);
        writer.write_bytes(base_uri_bytes);

        self.mint_authority.write(writer);
        self.royalty.write(writer);

        // Optional freeze_authority
        match &self.freeze_authority {
            Some(auth) => {
                writer.write_bool(true);
                auth.write(writer);
            }
            None => {
                writer.write_bool(false);
            }
        }

        // Optional metadata_authority
        match &self.metadata_authority {
            Some(auth) => {
                writer.write_bool(true);
                auth.write(writer);
            }
            None => {
                writer.write_bool(false);
            }
        }

        writer.write_bool(self.is_paused);
        writer.write_u64(&self.created_at);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let id = Hash::read(reader)?;
        let name = reader.read_string()?;

        // Read symbol
        let symbol_len = reader.read_u8()? as usize;
        if symbol_len > MAX_SYMBOL_LENGTH {
            return Err(ReaderError::InvalidSize);
        }
        let symbol = reader.read_string_with_size(symbol_len)?;

        let creator = PublicKey::read(reader)?;
        let total_supply = reader.read_u64()?;
        let next_token_id = reader.read_u64()?;

        // Optional max_supply
        let max_supply = if reader.read_bool()? {
            Some(reader.read_u64()?)
        } else {
            None
        };

        // Base URI
        let base_uri_len = reader.read_u16()? as usize;
        if base_uri_len > MAX_BASE_URI_LENGTH {
            return Err(ReaderError::InvalidSize);
        }
        let base_uri = reader.read_string_with_size(base_uri_len)?;

        let mint_authority = MintAuthority::read(reader)?;
        let royalty = Royalty::read(reader)?;

        // Optional freeze_authority
        let freeze_authority = if reader.read_bool()? {
            Some(PublicKey::read(reader)?)
        } else {
            None
        };

        // Optional metadata_authority
        let metadata_authority = if reader.read_bool()? {
            Some(PublicKey::read(reader)?)
        } else {
            None
        };

        let is_paused = reader.read_bool()?;
        let created_at = reader.read_u64()?;

        Ok(NftCollection {
            id,
            name,
            symbol,
            creator,
            total_supply,
            next_token_id,
            max_supply,
            base_uri,
            mint_authority,
            royalty,
            freeze_authority,
            metadata_authority,
            is_paused,
            created_at,
        })
    }
}

impl Serializer for Nft {
    fn write(&self, writer: &mut Writer) {
        self.collection.write(writer);
        writer.write_u64(&self.token_id);
        self.owner.write(writer);

        // Metadata URI with u16 length
        let uri_bytes = self.metadata_uri.as_bytes();
        writer.write_u16(uri_bytes.len() as u16);
        writer.write_bytes(uri_bytes);

        // Attributes
        writer.write_u8(self.attributes.len() as u8);
        for (key, value) in &self.attributes {
            // Key with u8 length
            let key_bytes = key.as_bytes();
            writer.write_u8(key_bytes.len() as u8);
            writer.write_bytes(key_bytes);
            value.write(writer);
        }

        writer.write_u64(&self.created_at);
        self.creator.write(writer);

        // Optional royalty
        match &self.royalty {
            Some(r) => {
                writer.write_bool(true);
                r.write(writer);
            }
            None => {
                writer.write_bool(false);
            }
        }

        // Optional approved
        match &self.approved {
            Some(addr) => {
                writer.write_bool(true);
                addr.write(writer);
            }
            None => {
                writer.write_bool(false);
            }
        }

        writer.write_bool(self.is_frozen);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let collection = Hash::read(reader)?;
        let token_id = reader.read_u64()?;
        let owner = PublicKey::read(reader)?;

        // Metadata URI
        let uri_len = reader.read_u16()? as usize;
        if uri_len > MAX_METADATA_URI_LENGTH {
            return Err(ReaderError::InvalidSize);
        }
        let metadata_uri = reader.read_string_with_size(uri_len)?;

        // Attributes
        let attr_count = reader.read_u8()? as usize;
        if attr_count > MAX_ATTRIBUTES_COUNT {
            return Err(ReaderError::InvalidSize);
        }
        let mut attributes = Vec::with_capacity(attr_count);
        for _ in 0..attr_count {
            let key_len = reader.read_u8()? as usize;
            if key_len > MAX_ATTRIBUTE_KEY_LENGTH {
                return Err(ReaderError::InvalidSize);
            }
            let key = reader.read_string_with_size(key_len)?;
            let value = AttributeValue::read(reader)?;
            attributes.push((key, value));
        }

        let created_at = reader.read_u64()?;
        let creator = PublicKey::read(reader)?;

        // Optional royalty
        let royalty = if reader.read_bool()? {
            Some(Royalty::read(reader)?)
        } else {
            None
        };

        // Optional approved
        let approved = if reader.read_bool()? {
            Some(PublicKey::read(reader)?)
        } else {
            None
        };

        let is_frozen = reader.read_bool()?;

        Ok(Nft {
            collection,
            token_id,
            owner,
            metadata_uri,
            attributes,
            created_at,
            creator,
            royalty,
            approved,
            is_frozen,
        })
    }
}

impl Serializer for RentalListingStatus {
    fn write(&self, writer: &mut Writer) {
        let id = match self {
            RentalListingStatus::Active => 0u8,
            RentalListingStatus::Cancelled => 1,
            RentalListingStatus::Accepted => 2,
        };
        writer.write_u8(id);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        match reader.read_u8()? {
            0 => Ok(RentalListingStatus::Active),
            1 => Ok(RentalListingStatus::Cancelled),
            2 => Ok(RentalListingStatus::Accepted),
            _ => Err(ReaderError::InvalidValue),
        }
    }
}

impl Serializer for RentalListing {
    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.collection.write(writer);
        writer.write_u64(&self.token_id);
        self.owner.write(writer);
        writer.write_u64(&self.duration);
        writer.write_u64(&self.rent_fee);
        self.payment_token.write(writer);

        // Optional allowed_renter
        match &self.allowed_renter {
            Some(renter) => {
                writer.write_bool(true);
                renter.write(writer);
            }
            None => {
                writer.write_bool(false);
            }
        }

        self.status.write(writer);
        writer.write_u64(&self.created_at);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let id = Hash::read(reader)?;
        let collection = Hash::read(reader)?;
        let token_id = reader.read_u64()?;
        let owner = PublicKey::read(reader)?;
        let duration = reader.read_u64()?;
        let rent_fee = reader.read_u64()?;
        let payment_token = Hash::read(reader)?;

        let allowed_renter = if reader.read_bool()? {
            Some(PublicKey::read(reader)?)
        } else {
            None
        };

        let status = RentalListingStatus::read(reader)?;
        let created_at = reader.read_u64()?;

        Ok(RentalListing {
            id,
            collection,
            token_id,
            owner,
            duration,
            rent_fee,
            payment_token,
            allowed_renter,
            status,
            created_at,
        })
    }
}

impl Serializer for RentalStatus {
    fn write(&self, writer: &mut Writer) {
        let id = match self {
            RentalStatus::Active => 0u8,
            RentalStatus::Expired => 1,
            RentalStatus::Terminated => 2,
        };
        writer.write_u8(id);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        match reader.read_u8()? {
            0 => Ok(RentalStatus::Active),
            1 => Ok(RentalStatus::Expired),
            2 => Ok(RentalStatus::Terminated),
            _ => Err(ReaderError::InvalidValue),
        }
    }
}

impl Serializer for NftRental {
    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.collection.write(writer);
        writer.write_u64(&self.token_id);
        self.owner.write(writer);
        self.renter.write(writer);
        writer.write_u64(&self.expires_at);
        writer.write_u64(&self.rent_fee);
        self.payment_token.write(writer);
        self.status.write(writer);
        writer.write_u64(&self.created_at);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let id = Hash::read(reader)?;
        let collection = Hash::read(reader)?;
        let token_id = reader.read_u64()?;
        let owner = PublicKey::read(reader)?;
        let renter = PublicKey::read(reader)?;
        let expires_at = reader.read_u64()?;
        let rent_fee = reader.read_u64()?;
        let payment_token = Hash::read(reader)?;
        let status = RentalStatus::read(reader)?;
        let created_at = reader.read_u64()?;

        Ok(NftRental {
            id,
            collection,
            token_id,
            owner,
            renter,
            expires_at,
            rent_fee,
            payment_token,
            status,
            created_at,
        })
    }
}

impl Serializer for TokenBoundAccount {
    fn write(&self, writer: &mut Writer) {
        // Write NFT tuple (collection, token_id)
        self.nft.0.write(writer);
        writer.write_u64(&self.nft.1);
        self.account.write(writer);
        writer.write_u64(&self.created_at);
        writer.write_bool(self.is_active);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let collection = Hash::read(reader)?;
        let token_id = reader.read_u64()?;
        let account = crate::crypto::Address::read(reader)?;
        let created_at = reader.read_u64()?;
        let is_active = reader.read_bool()?;

        Ok(TokenBoundAccount {
            nft: (collection, token_id),
            account,
            created_at,
            is_active,
        })
    }
}

// ========================================
// Helper Functions for Storage Operations
// ========================================

/// Encode a u64 value for storage
pub fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

/// Decode a u64 value from storage bytes
pub fn decode_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != 8 {
        return None;
    }
    let arr: [u8; 8] = bytes.try_into().ok()?;
    Some(u64::from_be_bytes(arr))
}

/// Encode a boolean for storage (as single byte)
pub fn encode_bool(value: bool) -> [u8; 1] {
    [if value { 1 } else { 0 }]
}

/// Decode a boolean from storage bytes
pub fn decode_bool(bytes: &[u8]) -> Option<bool> {
    if bytes.is_empty() {
        return None;
    }
    match bytes[0] {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// Convert NftError to error code for syscall return
impl From<NftError> for u64 {
    fn from(err: NftError) -> u64 {
        err.code()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_crypto::curve25519_dalek::{traits::Identity, RistrettoPoint};

    fn test_public_key() -> PublicKey {
        PublicKey::new(RistrettoPoint::identity().compress())
    }

    fn test_hash() -> Hash {
        Hash::new([1u8; 32])
    }

    #[test]
    fn test_storage_key_generation() {
        let collection_id = test_hash();
        let owner = test_public_key();

        // Collection key
        let key = collection_key(&collection_id);
        assert!(key.starts_with(prefixes::COLLECTION));
        assert_eq!(key.len(), prefixes::COLLECTION.len() + 32);

        // Token key
        let key = token_key(&collection_id, 1);
        assert!(key.starts_with(prefixes::TOKEN));
        assert_eq!(key.len(), prefixes::TOKEN.len() + 32 + 8);

        // Owner balance key
        let key = owner_balance_key(&collection_id, &owner);
        assert!(key.starts_with(prefixes::OWNER_BALANCE));
        assert_eq!(key.len(), prefixes::OWNER_BALANCE.len() + 32 + 32);
    }

    #[test]
    fn test_attribute_value_serialization() {
        // String
        let attr = AttributeValue::String("test value".to_string());
        let bytes = attr.to_bytes();
        let decoded = AttributeValue::from_bytes(&bytes).expect("decode failed");
        assert_eq!(attr, decoded);

        // Number
        let attr = AttributeValue::Number(-12345);
        let bytes = attr.to_bytes();
        let decoded = AttributeValue::from_bytes(&bytes).expect("decode failed");
        assert_eq!(attr, decoded);

        // Boolean
        let attr = AttributeValue::Boolean(true);
        let bytes = attr.to_bytes();
        let decoded = AttributeValue::from_bytes(&bytes).expect("decode failed");
        assert_eq!(attr, decoded);

        // Array
        let attr = AttributeValue::Array(vec![
            AttributeValue::String("a".to_string()),
            AttributeValue::Number(42),
            AttributeValue::Boolean(false),
        ]);
        let bytes = attr.to_bytes();
        let decoded = AttributeValue::from_bytes(&bytes).expect("decode failed");
        assert_eq!(attr, decoded);
    }

    #[test]
    fn test_royalty_serialization() {
        let royalty = Royalty::new(test_public_key(), 500);
        let bytes = royalty.to_bytes();
        let decoded = Royalty::from_bytes(&bytes).expect("decode failed");
        assert_eq!(royalty, decoded);
    }

    #[test]
    fn test_mint_authority_serialization() {
        // CreatorOnly
        let auth = MintAuthority::CreatorOnly;
        let bytes = auth.to_bytes();
        let decoded = MintAuthority::from_bytes(&bytes).expect("decode failed");
        assert_eq!(auth, decoded);

        // Whitelist
        let auth = MintAuthority::Whitelist(vec![test_public_key()]);
        let bytes = auth.to_bytes();
        let decoded = MintAuthority::from_bytes(&bytes).expect("decode failed");
        assert_eq!(auth, decoded);

        // WhitelistMerkle
        let auth = MintAuthority::WhitelistMerkle {
            root: test_hash(),
            max_per_address: 5,
        };
        let bytes = auth.to_bytes();
        let decoded = MintAuthority::from_bytes(&bytes).expect("decode failed");
        assert_eq!(auth, decoded);

        // Public
        let auth = MintAuthority::Public {
            max_per_address: 10,
            price: 1000,
            payment_recipient: test_public_key(),
        };
        let bytes = auth.to_bytes();
        let decoded = MintAuthority::from_bytes(&bytes).expect("decode failed");
        assert_eq!(auth, decoded);

        // Contract
        let auth = MintAuthority::Contract(test_public_key());
        let bytes = auth.to_bytes();
        let decoded = MintAuthority::from_bytes(&bytes).expect("decode failed");
        assert_eq!(auth, decoded);
    }

    #[test]
    fn test_nft_collection_serialization() {
        let collection = NftCollection {
            id: test_hash(),
            name: "Test Collection".to_string(),
            symbol: "TEST".to_string(),
            creator: test_public_key(),
            total_supply: 100,
            next_token_id: 101,
            max_supply: Some(1000),
            base_uri: "https://example.com/".to_string(),
            mint_authority: MintAuthority::CreatorOnly,
            royalty: Royalty::new(test_public_key(), 250),
            freeze_authority: Some(test_public_key()),
            metadata_authority: None,
            is_paused: false,
            created_at: 12345,
        };

        let bytes = collection.to_bytes();
        let decoded = NftCollection::from_bytes(&bytes).expect("decode failed");

        assert_eq!(collection.id, decoded.id);
        assert_eq!(collection.name, decoded.name);
        assert_eq!(collection.symbol, decoded.symbol);
        assert_eq!(collection.total_supply, decoded.total_supply);
        assert_eq!(collection.max_supply, decoded.max_supply);
        assert_eq!(collection.is_paused, decoded.is_paused);
    }

    #[test]
    fn test_nft_serialization() {
        let nft = Nft {
            collection: test_hash(),
            token_id: 42,
            owner: test_public_key(),
            metadata_uri: "https://example.com/42.json".to_string(),
            attributes: vec![
                (
                    "rarity".to_string(),
                    AttributeValue::String("rare".to_string()),
                ),
                ("power".to_string(), AttributeValue::Number(100)),
            ],
            created_at: 12345,
            creator: test_public_key(),
            royalty: Some(Royalty::new(test_public_key(), 500)),
            approved: None,
            is_frozen: false,
        };

        let bytes = nft.to_bytes();
        let decoded = Nft::from_bytes(&bytes).expect("decode failed");

        assert_eq!(nft.collection, decoded.collection);
        assert_eq!(nft.token_id, decoded.token_id);
        assert_eq!(nft.metadata_uri, decoded.metadata_uri);
        assert_eq!(nft.attributes.len(), decoded.attributes.len());
        assert_eq!(nft.is_frozen, decoded.is_frozen);
    }

    #[test]
    fn test_rental_listing_serialization() {
        let listing = RentalListing {
            id: test_hash(),
            collection: test_hash(),
            token_id: 1,
            owner: test_public_key(),
            duration: 86400,
            rent_fee: 1000,
            payment_token: Hash::zero(),
            allowed_renter: Some(test_public_key()),
            status: RentalListingStatus::Active,
            created_at: 12345,
        };

        let bytes = listing.to_bytes();
        let decoded = RentalListing::from_bytes(&bytes).expect("decode failed");

        assert_eq!(listing.id, decoded.id);
        assert_eq!(listing.duration, decoded.duration);
        assert_eq!(listing.rent_fee, decoded.rent_fee);
        assert_eq!(listing.status, decoded.status);
    }

    #[test]
    fn test_nft_rental_serialization() {
        let rental = NftRental {
            id: test_hash(),
            collection: test_hash(),
            token_id: 1,
            owner: test_public_key(),
            renter: test_public_key(),
            expires_at: 100000,
            rent_fee: 1000,
            payment_token: Hash::zero(),
            status: RentalStatus::Active,
            created_at: 12345,
        };

        let bytes = rental.to_bytes();
        let decoded = NftRental::from_bytes(&bytes).expect("decode failed");

        assert_eq!(rental.id, decoded.id);
        assert_eq!(rental.expires_at, decoded.expires_at);
        assert_eq!(rental.status, decoded.status);
    }

    #[test]
    fn test_u64_encoding() {
        let value = 12345678u64;
        let encoded = encode_u64(value);
        let decoded = decode_u64(&encoded).expect("decode failed");
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_bool_encoding() {
        assert_eq!(decode_bool(&encode_bool(true)), Some(true));
        assert_eq!(decode_bool(&encode_bool(false)), Some(false));
        assert_eq!(decode_bool(&[]), None);
        assert_eq!(decode_bool(&[2]), None);
    }
}
