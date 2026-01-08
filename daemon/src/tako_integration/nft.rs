// NFT Adapter: TOS NftStorage → TAKO NftProvider
//
// This module bridges TOS's NftStorage with TAKO's NftProvider trait,
// enabling smart contracts to access the native NFT system via syscalls.
//
// Unlike the referral adapter which uses async/sync conversion,
// the NFT storage in tos_common is synchronous, making integration simpler.

use tos_common::crypto::{Hash, PublicKey};
use tos_common::nft::operations::{
    burn, create_collection, freeze, is_frozen, mint, thaw, transfer, CreateCollectionParams,
    MintParams, NftStorage, RuntimeContext,
};
use tos_common::nft::{MintAuthority, Nft, NftCollection, NftError, NftResult};
use tos_common::serializer::Serializer;
// TAKO's NftProvider trait (aliased to avoid conflict)
use tos_program_runtime::storage::{NftCollectionData, NftData, NftProvider as TakoNftProvider};
use tos_tbpf::error::EbpfError;

/// Adapter that wraps TOS's NftStorage to implement TAKO's NftProvider
///
/// # Architecture
///
/// ```text
/// TAKO syscall (e.g., nft_mint)
///     ↓
/// InvokeContext::nft_mint()
///     ↓
/// TosNftAdapter::mint() [TakoNftProvider]
///     ↓
/// tos_common::nft::operations::mint() [business logic]
///     ↓
/// NftStorage trait methods [RocksDB]
/// ```
///
/// # Synchronous Operation
///
/// Unlike the referral adapter, NFT storage is synchronous, so no
/// async/sync conversion is needed.
pub struct TosNftAdapter<'a, S: NftStorage> {
    /// TOS NFT storage provider (mutable for write operations)
    storage: &'a mut S,
}

impl<'a, S: NftStorage> TosNftAdapter<'a, S> {
    /// Create a new NFT adapter
    ///
    /// # Arguments
    ///
    /// * `storage` - TOS NFT storage implementing NftStorage trait
    pub fn new(storage: &'a mut S) -> Self {
        Self { storage }
    }

    /// Convert [u8; 32] bytes to TOS Hash
    fn bytes_to_hash(bytes: &[u8; 32]) -> Hash {
        Hash::new(*bytes)
    }

    /// Convert TOS Hash to [u8; 32] bytes
    fn hash_to_bytes(hash: &Hash) -> [u8; 32] {
        *hash.as_bytes()
    }

    /// Convert [u8; 32] bytes to TOS PublicKey
    fn bytes_to_pubkey(bytes: &[u8; 32]) -> Result<PublicKey, EbpfError> {
        PublicKey::from_bytes(bytes).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid public key bytes",
            )))
        })
    }

    /// Convert TOS PublicKey to [u8; 32] bytes
    fn pubkey_to_bytes(pubkey: &PublicKey) -> [u8; 32] {
        *pubkey.as_bytes()
    }

    /// Convert NftError to EbpfError
    fn convert_error(err: NftError) -> EbpfError {
        EbpfError::SyscallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("NFT error: {}", err),
        )))
    }

    /// Convert NftCollection to NftCollectionData
    fn collection_to_data(collection: &NftCollection) -> NftCollectionData {
        // Extract max_per_address from MintAuthority
        let max_per_address = match &collection.mint_authority {
            MintAuthority::Public {
                max_per_address, ..
            } => *max_per_address,
            MintAuthority::WhitelistMerkle {
                max_per_address, ..
            } => *max_per_address,
            _ => 0, // CreatorOnly and Whitelist don't have per-address limits
        };

        NftCollectionData {
            id: Self::hash_to_bytes(&collection.id),
            creator: Self::pubkey_to_bytes(&collection.creator),
            name: collection.name.as_bytes().to_vec(),
            symbol: collection.symbol.as_bytes().to_vec(),
            base_uri: collection.base_uri.as_bytes().to_vec(),
            max_supply: collection.max_supply.unwrap_or(0),
            total_supply: collection.total_supply,
            next_token_id: collection.next_token_id,
            royalty_recipient: Self::pubkey_to_bytes(&collection.royalty.recipient),
            royalty_bps: collection.royalty.basis_points,
            minting_paused: collection.is_paused,
            max_per_address,
            created_at: collection.created_at,
        }
    }

    /// Convert Nft to NftData
    fn nft_to_data(nft: &tos_common::nft::Nft) -> NftData {
        NftData {
            collection: Self::hash_to_bytes(&nft.collection),
            token_id: nft.token_id,
            owner: Self::pubkey_to_bytes(&nft.owner),
            approved: nft.approved.as_ref().map(Self::pubkey_to_bytes),
            token_uri: nft.metadata_uri.as_bytes().to_vec(),
            minted_at: nft.created_at,
        }
    }
}

impl<'a, S: NftStorage> TakoNftProvider for TosNftAdapter<'a, S> {
    fn create_collection(
        &mut self,
        creator: &[u8; 32],
        name: &[u8],
        symbol: &[u8],
        base_uri: &[u8],
        max_supply: u64,
        royalty_recipient: &[u8; 32],
        royalty_bps: u16,
        max_per_address: u64,
        block_height: u64,
    ) -> Result<[u8; 32], EbpfError> {
        let creator_pk = Self::bytes_to_pubkey(creator)?;
        let royalty_pk = Self::bytes_to_pubkey(royalty_recipient)?;

        let name_str = String::from_utf8(name.to_vec()).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in collection name",
            )))
        })?;

        let symbol_str = String::from_utf8(symbol.to_vec()).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in collection symbol",
            )))
        })?;

        let base_uri_str = String::from_utf8(base_uri.to_vec()).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in base URI",
            )))
        })?;

        let ctx = RuntimeContext::new(creator_pk, block_height);

        // Convert max_supply: 0 means unlimited in TAKO, None in TOS
        let max_supply_opt = if max_supply == 0 {
            None
        } else {
            Some(max_supply)
        };

        // Construct mint authority with max_per_address
        // Use Public minting with zero price (free mint from contract)
        let mint_authority = MintAuthority::Public {
            max_per_address,
            price: 0,
            payment_recipient: royalty_pk.clone(),
        };

        let params = CreateCollectionParams {
            name: name_str,
            symbol: symbol_str,
            base_uri: base_uri_str,
            max_supply: max_supply_opt,
            royalty_recipient: royalty_pk,
            royalty_basis_points: royalty_bps,
            mint_authority,
            freeze_authority: None,
            metadata_authority: None,
        };

        let collection_id =
            create_collection(self.storage, &ctx, params).map_err(Self::convert_error)?;

        Ok(Self::hash_to_bytes(&collection_id))
    }

    fn get_collection(
        &self,
        collection: &[u8; 32],
    ) -> Result<Option<NftCollectionData>, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let collection_opt = self.storage.get_collection(&hash);
        Ok(collection_opt.map(|c| Self::collection_to_data(&c)))
    }

    fn collection_exists(&self, collection: &[u8; 32]) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        Ok(self.storage.collection_exists(&hash))
    }

    fn set_minting_paused(
        &mut self,
        collection: &[u8; 32],
        caller: &[u8; 32],
        paused: bool,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        let mut collection_data = self
            .storage
            .get_collection(&hash)
            .ok_or_else(|| Self::convert_error(NftError::CollectionNotFound))?;

        // Only creator can pause/unpause
        if collection_data.creator != caller_pk {
            return Err(Self::convert_error(NftError::NotCreator));
        }

        collection_data.is_paused = paused;
        self.storage
            .set_collection(&collection_data)
            .map_err(Self::convert_error)
    }

    fn mint(
        &mut self,
        collection: &[u8; 32],
        to: &[u8; 32],
        token_uri: &[u8],
        caller: &[u8; 32],
        block_height: u64,
    ) -> Result<u64, EbpfError> {
        let collection_hash = Self::bytes_to_hash(collection);
        let to_pk = Self::bytes_to_pubkey(to)?;
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        let uri_str = String::from_utf8(token_uri.to_vec()).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in token URI",
            )))
        })?;

        let ctx = RuntimeContext::new(caller_pk, block_height);
        let params = MintParams::new(collection_hash, to_pk).with_uri(uri_str);

        mint(self.storage, &ctx, params).map_err(Self::convert_error)
    }

    fn batch_mint(
        &mut self,
        collection: &[u8; 32],
        recipients: &[[u8; 32]],
        uris: &[&[u8]],
        caller: &[u8; 32],
        block_height: u64,
    ) -> Result<Vec<u64>, EbpfError> {
        // Validate inputs
        if recipients.is_empty() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Empty recipients list",
            ))));
        }

        if recipients.len() != uris.len() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Recipients and URIs count mismatch",
            ))));
        }

        const MAX_BATCH_SIZE: usize = 100;
        if recipients.len() > MAX_BATCH_SIZE {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Batch size {} exceeds maximum {}",
                    recipients.len(),
                    MAX_BATCH_SIZE
                ),
            ))));
        }

        let collection_hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let ctx = RuntimeContext::new(caller_pk, block_height);

        let mut token_ids = Vec::with_capacity(recipients.len());

        for (i, (recipient, uri)) in recipients.iter().zip(uris.iter()).enumerate() {
            let to_pk = Self::bytes_to_pubkey(recipient).map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid recipient public key at index {}: {}", i, e),
                )))
            })?;

            let uri_str = String::from_utf8(uri.to_vec()).map_err(|_| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid UTF-8 in token URI at index {}", i),
                )))
            })?;

            let params = MintParams::new(collection_hash.clone(), to_pk).with_uri(uri_str);
            let token_id = mint(self.storage, &ctx, params).map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Mint failed at index {}: {}", i, e),
                )))
            })?;

            token_ids.push(token_id);
        }

        Ok(token_ids)
    }

    fn burn(
        &mut self,
        collection: &[u8; 32],
        token_id: u64,
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let collection_hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        // RuntimeContext needs block_height, but burn doesn't use it
        // We use 0 as placeholder since burn doesn't need block height
        let ctx = RuntimeContext::new(caller_pk, 0);

        burn(self.storage, &ctx, &collection_hash, token_id).map_err(Self::convert_error)
    }

    fn transfer(
        &mut self,
        collection: &[u8; 32],
        token_id: u64,
        _from: &[u8; 32],
        to: &[u8; 32],
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let collection_hash = Self::bytes_to_hash(collection);
        // Note: `from` is not used - transfer validates current owner internally
        let to_pk = Self::bytes_to_pubkey(to)?;
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        // RuntimeContext needs block_height, but transfer doesn't use it for basic transfer
        let ctx = RuntimeContext::new(caller_pk, 0);

        // Use the transfer function from operations module
        // Note: transfer validates ownership and permissions internally using ctx.caller
        transfer(self.storage, &ctx, &collection_hash, token_id, &to_pk)
            .map_err(Self::convert_error)
    }

    fn batch_transfer(
        &mut self,
        transfers: &[([u8; 32], u64, [u8; 32])],
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Validate inputs
        if transfers.is_empty() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Empty transfers list",
            ))));
        }

        const MAX_BATCH_SIZE: usize = 100;
        if transfers.len() > MAX_BATCH_SIZE {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Batch size {} exceeds maximum {}",
                    transfers.len(),
                    MAX_BATCH_SIZE
                ),
            ))));
        }

        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let ctx = RuntimeContext::new(caller_pk, 0);

        for (i, (collection, token_id, to)) in transfers.iter().enumerate() {
            let collection_hash = Self::bytes_to_hash(collection);
            let to_pk = Self::bytes_to_pubkey(to).map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid recipient public key at index {}: {}", i, e),
                )))
            })?;

            transfer(self.storage, &ctx, &collection_hash, *token_id, &to_pk).map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Transfer failed at index {}: {}", i, e),
                )))
            })?;
        }

        Ok(())
    }

    fn batch_burn(
        &mut self,
        burns: &[([u8; 32], u64)],
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Validate inputs
        if burns.is_empty() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Empty burns list",
            ))));
        }

        const MAX_BATCH_SIZE: usize = 100;
        if burns.len() > MAX_BATCH_SIZE {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Batch size {} exceeds maximum {}",
                    burns.len(),
                    MAX_BATCH_SIZE
                ),
            ))));
        }

        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let ctx = RuntimeContext::new(caller_pk, 0);

        for (i, (collection, token_id)) in burns.iter().enumerate() {
            let collection_hash = Self::bytes_to_hash(collection);

            burn(self.storage, &ctx, &collection_hash, *token_id).map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Burn failed at index {}: {}", i, e),
                )))
            })?;
        }

        Ok(())
    }

    fn freeze(
        &mut self,
        collection: &[u8; 32],
        token_id: u64,
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let collection_hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let ctx = RuntimeContext::new(caller_pk, 0);

        freeze(self.storage, &ctx, &collection_hash, token_id).map_err(|e| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Freeze failed: {}", e),
            )))
        })
    }

    fn thaw(
        &mut self,
        collection: &[u8; 32],
        token_id: u64,
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let collection_hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let ctx = RuntimeContext::new(caller_pk, 0);

        thaw(self.storage, &ctx, &collection_hash, token_id).map_err(|e| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Thaw failed: {}", e),
            )))
        })
    }

    fn is_frozen(&self, collection: &[u8; 32], token_id: u64) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        is_frozen(self.storage, &hash, token_id).map_err(|e| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("is_frozen query failed: {}", e),
            )))
        })
    }

    fn batch_freeze(
        &mut self,
        tokens: &[([u8; 32], u64)],
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Validate inputs
        if tokens.is_empty() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Empty tokens list",
            ))));
        }

        const MAX_BATCH_SIZE: usize = 100;
        if tokens.len() > MAX_BATCH_SIZE {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Batch size {} exceeds maximum {}",
                    tokens.len(),
                    MAX_BATCH_SIZE
                ),
            ))));
        }

        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let ctx = RuntimeContext::new(caller_pk, 0);

        for (i, (collection, token_id)) in tokens.iter().enumerate() {
            let collection_hash = Self::bytes_to_hash(collection);

            freeze(self.storage, &ctx, &collection_hash, *token_id).map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Freeze failed at index {}: {}", i, e),
                )))
            })?;
        }

        Ok(())
    }

    fn batch_thaw(
        &mut self,
        tokens: &[([u8; 32], u64)],
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        // Validate inputs
        if tokens.is_empty() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Empty tokens list",
            ))));
        }

        const MAX_BATCH_SIZE: usize = 100;
        if tokens.len() > MAX_BATCH_SIZE {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Batch size {} exceeds maximum {}",
                    tokens.len(),
                    MAX_BATCH_SIZE
                ),
            ))));
        }

        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let ctx = RuntimeContext::new(caller_pk, 0);

        for (i, (collection, token_id)) in tokens.iter().enumerate() {
            let collection_hash = Self::bytes_to_hash(collection);

            thaw(self.storage, &ctx, &collection_hash, *token_id).map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Thaw failed at index {}: {}", i, e),
                )))
            })?;
        }

        Ok(())
    }

    fn get_nft(&self, collection: &[u8; 32], token_id: u64) -> Result<Option<NftData>, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let nft_opt = self.storage.get_nft(&hash, token_id);
        Ok(nft_opt.map(|n| Self::nft_to_data(&n)))
    }

    fn nft_exists(&self, collection: &[u8; 32], token_id: u64) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        Ok(self.storage.nft_exists(&hash, token_id))
    }

    fn owner_of(
        &self,
        collection: &[u8; 32],
        token_id: u64,
    ) -> Result<Option<[u8; 32]>, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let nft_opt = self.storage.get_nft(&hash, token_id);
        Ok(nft_opt.map(|n| Self::pubkey_to_bytes(&n.owner)))
    }

    fn balance_of(&self, collection: &[u8; 32], owner: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let owner_pk = Self::bytes_to_pubkey(owner)?;
        Ok(self.storage.get_balance(&hash, &owner_pk))
    }

    fn token_uri(
        &self,
        collection: &[u8; 32],
        token_id: u64,
    ) -> Result<Option<Vec<u8>>, EbpfError> {
        let hash = Self::bytes_to_hash(collection);

        // First get the NFT
        let nft = match self.storage.get_nft(&hash, token_id) {
            Some(n) => n,
            None => return Ok(None),
        };

        // If NFT has its own metadata_uri, return it
        if !nft.metadata_uri.is_empty() {
            return Ok(Some(nft.metadata_uri.as_bytes().to_vec()));
        }

        // Otherwise, try to construct from collection's base_uri
        let collection_data = match self.storage.get_collection(&hash) {
            Some(c) => c,
            None => return Ok(None),
        };

        if collection_data.base_uri.is_empty() {
            Ok(Some(Vec::new()))
        } else {
            // Combine base_uri + token_id
            let uri = format!("{}{}", collection_data.base_uri, token_id);
            Ok(Some(uri.into_bytes()))
        }
    }

    fn approve(
        &mut self,
        collection: &[u8; 32],
        token_id: u64,
        operator: Option<&[u8; 32]>,
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        let mut nft = self
            .storage
            .get_nft(&hash, token_id)
            .ok_or_else(|| Self::convert_error(NftError::TokenNotFound))?;

        // Only owner can approve
        if nft.owner != caller_pk {
            return Err(Self::convert_error(NftError::NotOwner));
        }

        // Set or clear approval
        nft.approved = match operator {
            Some(op) => Some(Self::bytes_to_pubkey(op)?),
            None => None,
        };

        self.storage.set_nft(&nft).map_err(Self::convert_error)
    }

    fn get_approved(
        &self,
        collection: &[u8; 32],
        token_id: u64,
    ) -> Result<Option<[u8; 32]>, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let nft_opt = self.storage.get_nft(&hash, token_id);
        Ok(nft_opt.and_then(|n| n.approved.map(|a| Self::pubkey_to_bytes(&a))))
    }

    fn set_approval_for_all(
        &mut self,
        collection: &[u8; 32],
        operator: &[u8; 32],
        approved: bool,
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let operator_pk = Self::bytes_to_pubkey(operator)?;

        // Caller cannot approve themselves
        if caller_pk == operator_pk {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Cannot approve self as operator",
            ))));
        }

        self.storage
            .set_approval_for_all(&caller_pk, &hash, &operator_pk, approved)
            .map_err(Self::convert_error)
    }

    fn is_approved_for_all(
        &self,
        collection: &[u8; 32],
        owner: &[u8; 32],
        operator: &[u8; 32],
    ) -> Result<bool, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let owner_pk = Self::bytes_to_pubkey(owner)?;
        let operator_pk = Self::bytes_to_pubkey(operator)?;

        Ok(self
            .storage
            .is_approved_for_all(&owner_pk, &hash, &operator_pk))
    }

    fn get_total_supply(&self, collection: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(collection);

        match self.storage.get_collection(&hash) {
            Some(col) => Ok(col.total_supply),
            None => Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Collection not found",
            )))),
        }
    }

    fn get_mint_count(&self, collection: &[u8; 32], user: &[u8; 32]) -> Result<u64, EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let user_pk = Self::bytes_to_pubkey(user)?;

        Ok(self.storage.get_mint_count(&hash, &user_pk))
    }

    fn set_token_uri(
        &mut self,
        collection: &[u8; 32],
        token_id: u64,
        new_uri: &[u8],
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        // 1. Get the collection and check metadata_authority
        let nft_collection = self.storage.get_collection(&hash).ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Collection not found",
            )))
        })?;

        // 2. Check if caller is metadata_authority
        let metadata_authority = nft_collection.metadata_authority.as_ref().ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Collection has no metadata_authority (immutable URIs)",
            )))
        })?;

        if caller_pk != *metadata_authority {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Caller is not metadata_authority",
            ))));
        }

        // 3. Get the NFT
        let mut nft = self.storage.get_nft(&hash, token_id).ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Token not found",
            )))
        })?;

        // 4. Update the URI
        let new_uri_str = String::from_utf8(new_uri.to_vec()).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in URI",
            )))
        })?;

        nft.metadata_uri = new_uri_str;

        // 5. Save the updated NFT
        self.storage.set_nft(&nft).map_err(Self::convert_error)
    }

    fn update_collection(
        &mut self,
        collection: &[u8; 32],
        caller: &[u8; 32],
        new_base_uri: Option<&[u8]>,
        new_royalty_recipient: Option<&[u8; 32]>,
        new_royalty_bps: u16,
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        // 1. Get the collection
        let mut nft_collection = self.storage.get_collection(&hash).ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Collection not found",
            )))
        })?;

        // 2. Check if caller is the creator
        if caller_pk != nft_collection.creator {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Caller is not the collection creator",
            ))));
        }

        // 3. Update base_uri if provided
        if let Some(uri_bytes) = new_base_uri {
            let new_uri = String::from_utf8(uri_bytes.to_vec()).map_err(|_| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid UTF-8 in base_uri",
                )))
            })?;
            nft_collection.base_uri = new_uri;
        }

        // 4. Update royalty if provided
        if let Some(recipient_bytes) = new_royalty_recipient {
            let recipient_pk = Self::bytes_to_pubkey(recipient_bytes)?;

            // Validate royalty_bps
            if new_royalty_bps > 10000 {
                return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Royalty basis points exceeds 10000 (100%)",
                ))));
            }

            nft_collection.royalty = tos_common::nft::Royalty {
                recipient: recipient_pk,
                basis_points: new_royalty_bps,
            };
        }

        // 5. Save the updated collection
        self.storage
            .set_collection(&nft_collection)
            .map_err(Self::convert_error)
    }

    fn transfer_collection_ownership(
        &mut self,
        collection: &[u8; 32],
        caller: &[u8; 32],
        new_owner: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;
        let new_owner_pk = Self::bytes_to_pubkey(new_owner)?;

        // 1. Get the collection
        let mut nft_collection = self.storage.get_collection(&hash).ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Collection not found",
            )))
        })?;

        // 2. Check if caller is the current creator
        if caller_pk != nft_collection.creator {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Caller is not the collection creator",
            ))));
        }

        // 3. Transfer ownership
        nft_collection.creator = new_owner_pk;

        // 4. Save the updated collection
        self.storage
            .set_collection(&nft_collection)
            .map_err(Self::convert_error)
    }

    fn update_attribute(
        &mut self,
        collection: &[u8; 32],
        token_id: u64,
        key: &[u8],
        value: &[u8],
        value_type: u8,
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        // 1. Get the collection and check metadata_authority
        let nft_collection = self.storage.get_collection(&hash).ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Collection not found",
            )))
        })?;

        // 2. Check if caller is metadata_authority
        let metadata_authority = nft_collection.metadata_authority.as_ref().ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Collection has no metadata_authority (immutable attributes)",
            )))
        })?;

        if caller_pk != *metadata_authority {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Caller is not metadata_authority",
            ))));
        }

        // 3. Get the NFT
        let mut nft = self.storage.get_nft(&hash, token_id).ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Token not found",
            )))
        })?;

        // 4. Parse the key
        let key_str = String::from_utf8(key.to_vec()).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in attribute key",
            )))
        })?;

        // 5. Parse the value based on value_type
        let attr_value = match value_type {
            0 => {
                // String
                let s = String::from_utf8(value.to_vec()).map_err(|_| {
                    EbpfError::SyscallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Invalid UTF-8 in attribute value",
                    )))
                })?;
                tos_common::nft::AttributeValue::String(s)
            }
            1 => {
                // Number (i64 as 8 bytes LE)
                if value.len() < 8 {
                    return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Number attribute requires 8 bytes",
                    ))));
                }
                let num = i64::from_le_bytes([
                    value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
                ]);
                tos_common::nft::AttributeValue::Number(num)
            }
            2 => {
                // Boolean
                if value.is_empty() {
                    return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Boolean attribute requires at least 1 byte",
                    ))));
                }
                tos_common::nft::AttributeValue::Boolean(value[0] != 0)
            }
            3 => {
                // Array - not supported via syscall (would require recursive parsing)
                return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Array attributes are not supported via syscall",
                ))));
            }
            _ => {
                return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Unknown attribute value type",
                ))));
            }
        };

        // 6. Update or add the attribute
        let mut found = false;
        for (k, v) in nft.attributes.iter_mut() {
            if k == &key_str {
                *v = attr_value.clone();
                found = true;
                break;
            }
        }
        if !found {
            // Check max attributes limit (e.g., 64)
            const MAX_ATTRIBUTES: usize = 64;
            if nft.attributes.len() >= MAX_ATTRIBUTES {
                return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Maximum attributes limit reached",
                ))));
            }
            nft.attributes.push((key_str, attr_value));
        }

        // 7. Save the updated NFT
        self.storage.set_nft(&nft).map_err(Self::convert_error)
    }

    fn remove_attribute(
        &mut self,
        collection: &[u8; 32],
        token_id: u64,
        key: &[u8],
        caller: &[u8; 32],
    ) -> Result<(), EbpfError> {
        let hash = Self::bytes_to_hash(collection);
        let caller_pk = Self::bytes_to_pubkey(caller)?;

        // 1. Get the collection and check metadata_authority
        let nft_collection = self.storage.get_collection(&hash).ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Collection not found",
            )))
        })?;

        // 2. Check if caller is metadata_authority
        let metadata_authority = nft_collection.metadata_authority.as_ref().ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Collection has no metadata_authority (immutable attributes)",
            )))
        })?;

        if caller_pk != *metadata_authority {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Caller is not metadata_authority",
            ))));
        }

        // 3. Get the NFT
        let mut nft = self.storage.get_nft(&hash, token_id).ok_or_else(|| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Token not found",
            )))
        })?;

        // 4. Parse the key
        let key_str = String::from_utf8(key.to_vec()).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in attribute key",
            )))
        })?;

        // 5. Find and remove the attribute
        let original_len = nft.attributes.len();
        nft.attributes.retain(|(k, _)| k != &key_str);

        if nft.attributes.len() == original_len {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Attribute key not found",
            ))));
        }

        // 6. Save the updated NFT
        self.storage.set_nft(&nft).map_err(Self::convert_error)
    }
}

/// No-operation NFT storage - used as placeholder when no NFT storage is available
///
/// This struct implements NftStorage with no-op implementations that return
/// empty results. It's used when calling execute functions without an NFT provider.
pub struct NoOpNftStorage;

impl NftStorage for NoOpNftStorage {
    fn get_collection(&self, _id: &Hash) -> Option<NftCollection> {
        None
    }

    fn set_collection(&mut self, _collection: &NftCollection) -> NftResult<()> {
        Err(NftError::StorageError)
    }

    fn collection_exists(&self, _id: &Hash) -> bool {
        false
    }

    fn get_nft(&self, _collection: &Hash, _token_id: u64) -> Option<Nft> {
        None
    }

    fn set_nft(&mut self, _nft: &Nft) -> NftResult<()> {
        Err(NftError::StorageError)
    }

    fn delete_nft(&mut self, _collection: &Hash, _token_id: u64) -> NftResult<()> {
        Err(NftError::StorageError)
    }

    fn nft_exists(&self, _collection: &Hash, _token_id: u64) -> bool {
        false
    }

    fn get_balance(&self, _collection: &Hash, _owner: &PublicKey) -> u64 {
        0
    }

    fn increment_balance(&mut self, _collection: &Hash, _owner: &PublicKey) -> NftResult<u64> {
        Err(NftError::StorageError)
    }

    fn decrement_balance(&mut self, _collection: &Hash, _owner: &PublicKey) -> NftResult<u64> {
        Err(NftError::StorageError)
    }

    fn is_approved_for_all(
        &self,
        _owner: &PublicKey,
        _collection: &Hash,
        _operator: &PublicKey,
    ) -> bool {
        false
    }

    fn set_approval_for_all(
        &mut self,
        _owner: &PublicKey,
        _collection: &Hash,
        _operator: &PublicKey,
        _approved: bool,
    ) -> NftResult<()> {
        Err(NftError::StorageError)
    }

    fn get_mint_count(&self, _collection: &Hash, _user: &PublicKey) -> u64 {
        0
    }

    fn increment_mint_count(&mut self, _collection: &Hash, _user: &PublicKey) -> NftResult<u64> {
        Err(NftError::StorageError)
    }

    fn get_and_increment_collection_nonce(&mut self) -> NftResult<u64> {
        Err(NftError::StorageError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::nft::{Nft, NftCollection, NftResult};

    /// Mock NFT storage for testing
    struct MockNftStorage {
        collections: std::collections::HashMap<Hash, NftCollection>,
        nfts: std::collections::HashMap<(Hash, u64), Nft>,
        balances: std::collections::HashMap<(Hash, PublicKey), u64>,
        approvals: std::collections::HashMap<(PublicKey, Hash, PublicKey), bool>,
        mint_counts: std::collections::HashMap<(Hash, PublicKey), u64>,
        collection_nonce: u64,
    }

    impl Default for MockNftStorage {
        fn default() -> Self {
            Self {
                collections: std::collections::HashMap::new(),
                nfts: std::collections::HashMap::new(),
                balances: std::collections::HashMap::new(),
                approvals: std::collections::HashMap::new(),
                mint_counts: std::collections::HashMap::new(),
                collection_nonce: 0,
            }
        }
    }

    impl NftStorage for MockNftStorage {
        fn get_collection(&self, id: &Hash) -> Option<NftCollection> {
            self.collections.get(id).cloned()
        }

        fn set_collection(&mut self, collection: &NftCollection) -> NftResult<()> {
            self.collections
                .insert(collection.id.clone(), collection.clone());
            Ok(())
        }

        fn collection_exists(&self, id: &Hash) -> bool {
            self.collections.contains_key(id)
        }

        fn get_nft(&self, collection: &Hash, token_id: u64) -> Option<Nft> {
            self.nfts.get(&(collection.clone(), token_id)).cloned()
        }

        fn set_nft(&mut self, nft: &Nft) -> NftResult<()> {
            self.nfts
                .insert((nft.collection.clone(), nft.token_id), nft.clone());
            Ok(())
        }

        fn delete_nft(&mut self, collection: &Hash, token_id: u64) -> NftResult<()> {
            self.nfts.remove(&(collection.clone(), token_id));
            Ok(())
        }

        fn nft_exists(&self, collection: &Hash, token_id: u64) -> bool {
            self.nfts.contains_key(&(collection.clone(), token_id))
        }

        fn get_balance(&self, collection: &Hash, owner: &PublicKey) -> u64 {
            self.balances
                .get(&(collection.clone(), owner.clone()))
                .copied()
                .unwrap_or(0)
        }

        fn increment_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64> {
            let balance = self
                .balances
                .entry((collection.clone(), owner.clone()))
                .or_insert(0);
            *balance = balance.checked_add(1).ok_or(NftError::Overflow)?;
            Ok(*balance)
        }

        fn decrement_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64> {
            let balance = self
                .balances
                .entry((collection.clone(), owner.clone()))
                .or_insert(0);
            *balance = balance.checked_sub(1).ok_or(NftError::Overflow)?;
            Ok(*balance)
        }

        fn is_approved_for_all(
            &self,
            owner: &PublicKey,
            collection: &Hash,
            operator: &PublicKey,
        ) -> bool {
            self.approvals
                .get(&(owner.clone(), collection.clone(), operator.clone()))
                .copied()
                .unwrap_or(false)
        }

        fn set_approval_for_all(
            &mut self,
            owner: &PublicKey,
            collection: &Hash,
            operator: &PublicKey,
            approved: bool,
        ) -> NftResult<()> {
            self.approvals.insert(
                (owner.clone(), collection.clone(), operator.clone()),
                approved,
            );
            Ok(())
        }

        fn get_mint_count(&self, collection: &Hash, user: &PublicKey) -> u64 {
            self.mint_counts
                .get(&(collection.clone(), user.clone()))
                .copied()
                .unwrap_or(0)
        }

        fn increment_mint_count(&mut self, collection: &Hash, user: &PublicKey) -> NftResult<u64> {
            let count = self
                .mint_counts
                .entry((collection.clone(), user.clone()))
                .or_insert(0);
            *count = count.checked_add(1).ok_or(NftError::Overflow)?;
            Ok(*count)
        }

        fn get_and_increment_collection_nonce(&mut self) -> NftResult<u64> {
            let nonce = self.collection_nonce;
            self.collection_nonce = self
                .collection_nonce
                .checked_add(1)
                .ok_or(NftError::Overflow)?;
            Ok(nonce)
        }
    }

    #[test]
    fn test_collection_exists() {
        let mut storage = MockNftStorage::default();
        let adapter = TosNftAdapter::new(&mut storage);

        let collection = [1u8; 32];
        assert!(!adapter.collection_exists(&collection).unwrap());
    }

    #[test]
    fn test_get_collection_none() {
        let mut storage = MockNftStorage::default();
        let adapter = TosNftAdapter::new(&mut storage);

        let collection = [1u8; 32];
        assert!(adapter.get_collection(&collection).unwrap().is_none());
    }

    #[test]
    fn test_nft_exists() {
        let mut storage = MockNftStorage::default();
        let adapter = TosNftAdapter::new(&mut storage);

        let collection = [1u8; 32];
        assert!(!adapter.nft_exists(&collection, 1).unwrap());
    }

    #[test]
    fn test_balance_of() {
        let mut storage = MockNftStorage::default();
        let adapter = TosNftAdapter::new(&mut storage);

        let collection = [1u8; 32];
        let owner = [2u8; 32];
        assert_eq!(adapter.balance_of(&collection, &owner).unwrap(), 0);
    }

    #[test]
    fn test_owner_of_none() {
        let mut storage = MockNftStorage::default();
        let adapter = TosNftAdapter::new(&mut storage);

        let collection = [1u8; 32];
        assert!(adapter.owner_of(&collection, 1).unwrap().is_none());
    }

    #[test]
    fn test_is_approved_for_all() {
        let mut storage = MockNftStorage::default();
        let adapter = TosNftAdapter::new(&mut storage);

        let collection = [1u8; 32];
        let owner = [2u8; 32];
        let operator = [3u8; 32];
        assert!(!adapter
            .is_approved_for_all(&collection, &owner, &operator)
            .unwrap());
    }

    #[test]
    fn test_set_approval_for_all() {
        let mut storage = MockNftStorage::default();

        {
            let mut adapter = TosNftAdapter::new(&mut storage);

            let collection = [1u8; 32];
            let owner = [2u8; 32];
            let operator = [3u8; 32];

            // Set approval
            adapter
                .set_approval_for_all(&collection, &operator, true, &owner)
                .unwrap();
        }

        // Check approval persisted
        let adapter = TosNftAdapter::new(&mut storage);
        let collection = [1u8; 32];
        let owner = [2u8; 32];
        let operator = [3u8; 32];
        assert!(adapter
            .is_approved_for_all(&collection, &owner, &operator)
            .unwrap());
    }

    #[test]
    fn test_cannot_approve_self() {
        let mut storage = MockNftStorage::default();
        let mut adapter = TosNftAdapter::new(&mut storage);

        let collection = [1u8; 32];
        let owner = [2u8; 32];

        // Try to approve self - should fail
        let result = adapter.set_approval_for_all(&collection, &owner, true, &owner);
        assert!(result.is_err());
    }
}
