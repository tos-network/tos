// NFT Operations Module
// This module contains the core business logic for NFT operations.
//
// The operations are designed to be runtime-agnostic:
// - Storage operations are abstracted via traits
// - Runtime functions (caller, block height) are passed as parameters
// - This allows testing and reuse across different runtime environments

mod burn;
mod collection;
mod freeze;
mod mint;
mod query;
mod transfer;
mod validation;

pub use burn::*;
pub use collection::*;
pub use freeze::*;
pub use mint::*;
pub use query::*;
pub use transfer::*;
pub use validation::*;

use crate::crypto::{Hash, PublicKey};
use crate::nft::{Nft, NftCollection, NftError, NftResult};

// ========================================
// Storage Trait (for dependency injection)
// ========================================

/// Abstract storage interface for NFT operations
/// Runtime implementations provide concrete storage backends
pub trait NftStorage {
    // Collection operations
    fn get_collection(&self, id: &Hash) -> Option<NftCollection>;
    fn set_collection(&mut self, collection: &NftCollection) -> NftResult<()>;
    fn collection_exists(&self, id: &Hash) -> bool;

    // Token operations
    fn get_nft(&self, collection: &Hash, token_id: u64) -> Option<Nft>;
    fn set_nft(&mut self, nft: &Nft) -> NftResult<()>;
    fn delete_nft(&mut self, collection: &Hash, token_id: u64) -> NftResult<()>;
    fn nft_exists(&self, collection: &Hash, token_id: u64) -> bool;

    // Balance operations
    fn get_balance(&self, collection: &Hash, owner: &PublicKey) -> u64;
    fn increment_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64>;
    fn decrement_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64>;

    // Operator approval operations
    fn is_approved_for_all(
        &self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
    ) -> bool;
    fn set_approval_for_all(
        &mut self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        approved: bool,
    ) -> NftResult<()>;

    // Mint count operations
    fn get_mint_count(&self, collection: &Hash, user: &PublicKey) -> u64;
    fn increment_mint_count(&mut self, collection: &Hash, user: &PublicKey) -> NftResult<u64>;

    // Collection nonce (for ID generation)
    fn get_and_increment_collection_nonce(&mut self) -> NftResult<u64>;

    // Rental check (optional, for Phase 9 integration)
    fn has_active_rental(&self, collection: &Hash, token_id: u64) -> bool {
        // Default: no rental system
        let _ = (collection, token_id);
        false
    }

    // TBA check (optional, for Phase 9 integration)
    fn tba_has_assets(&self, collection: &Hash, token_id: u64) -> bool {
        // Default: no TBA system
        let _ = (collection, token_id);
        false
    }

    fn remove_tba(&mut self, collection: &Hash, token_id: u64) -> NftResult<()> {
        // Default: no TBA system
        let _ = (collection, token_id);
        Ok(())
    }
}

// ========================================
// Runtime Context
// ========================================

/// Runtime context providing caller and block information
pub struct RuntimeContext {
    /// Current caller (transaction signer)
    pub caller: PublicKey,
    /// Current block height
    pub block_height: u64,
}

impl RuntimeContext {
    /// Create a new runtime context
    pub fn new(caller: PublicKey, block_height: u64) -> Self {
        Self {
            caller,
            block_height,
        }
    }
}

// ========================================
// Permission Checking Utilities
// ========================================

/// Check if the caller has permission to operate on an NFT
/// Returns Ok(()) if authorized, Err with appropriate error code otherwise
pub fn check_nft_permission<S: NftStorage + ?Sized>(
    storage: &S,
    nft: &Nft,
    caller: &PublicKey,
) -> NftResult<()> {
    // Owner always has permission
    if nft.owner == *caller {
        return Ok(());
    }

    // Check single token approval
    if nft.approved.as_ref() == Some(caller) {
        return Ok(());
    }

    // Check global operator approval
    if storage.is_approved_for_all(&nft.owner, &nft.collection, caller) {
        return Ok(());
    }

    Err(NftError::NotApproved)
}
