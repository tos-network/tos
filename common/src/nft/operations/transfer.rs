// NFT Transfer Operations
// This module contains transfer and safe_transfer operation logic.

use crate::crypto::{Hash, PublicKey};
use crate::nft::{NftError, NftResult, MAX_BATCH_SIZE};

use super::validation::{validate_recipient, validate_safe_transfer_data, validate_token_id};
use super::{check_nft_permission, NftStorage, RuntimeContext};

// ========================================
// Transfer Operation
// ========================================

/// Transfer an NFT to a new owner
///
/// # Parameters
/// - `storage`: Storage backend
/// - `ctx`: Runtime context (caller, block height)
/// - `collection`: Collection ID
/// - `token_id`: Token ID
/// - `to`: New owner address
///
/// # Returns
/// - `Ok(())`: Success
/// - `Err(NftError)`: Error code
pub fn transfer<S: NftStorage>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection: &Hash,
    token_id: u64,
    to: &PublicKey,
) -> NftResult<()> {
    // Step 1: Input validation
    validate_token_id(token_id)?;
    validate_recipient(to)?;

    // Step 2: Get NFT
    let mut nft = storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)?;

    // Step 3: Business rules check
    // 3.1 Self-transfer not allowed
    if nft.owner == *to {
        return Err(NftError::SelfTransfer);
    }

    // 3.2 Check frozen status
    if nft.is_frozen {
        return Err(NftError::TokenFrozen);
    }

    // 3.3 Check collection paused (optional, depends on business requirements)
    let collection_data = storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)?;
    if collection_data.is_paused {
        return Err(NftError::CollectionPaused);
    }

    // 3.4 Check rental status
    if storage.has_active_rental(collection, token_id) {
        return Err(NftError::RentalActive);
    }

    // Step 4: Permission check
    check_nft_permission(storage, &nft, &ctx.caller)?;

    // Step 5: Execute transfer
    let from = nft.owner.clone();

    // 5.1 Update NFT owner
    nft.owner = to.clone();

    // 5.2 Security: Clear single-token approval
    nft.approved = None;

    // 5.3 Store updated NFT
    storage.set_nft(&nft)?;

    // 5.4 Update balances
    storage.decrement_balance(collection, &from)?;
    storage.increment_balance(collection, to)?;

    Ok(())
}

// ========================================
// Safe Transfer Operation
// ========================================

/// Result of calling on_nft_received hook
pub enum ReceiverHookResult {
    /// Receiver accepted the NFT
    Accepted,
    /// Receiver rejected the NFT
    Rejected,
    /// Receiver is not a contract (EOA)
    NotContract,
    /// Receiver contract doesn't implement the hook
    NotImplemented,
}

/// Trait for checking if an address is a contract and calling hooks
pub trait ContractChecker {
    /// Check if address is a contract
    fn is_contract(&self, address: &PublicKey) -> bool;

    /// Call on_nft_received hook on receiver contract
    /// Returns the hook result
    fn call_on_nft_received(
        &self,
        contract: &PublicKey,
        operator: &PublicKey,
        from: &PublicKey,
        collection: &Hash,
        token_id: u64,
        data: &[u8],
    ) -> ReceiverHookResult;
}

/// Safe transfer an NFT with receiver hook callback
///
/// # Parameters
/// - `storage`: Storage backend
/// - `contract_checker`: Contract checking interface
/// - `ctx`: Runtime context
/// - `collection`: Collection ID
/// - `token_id`: Token ID
/// - `to`: New owner address
/// - `data`: Extra data to pass to receiver hook
///
/// # Returns
/// - `Ok(())`: Success
/// - `Err(NftError)`: Error code
pub fn safe_transfer<S: NftStorage, C: ContractChecker>(
    storage: &mut S,
    contract_checker: &C,
    ctx: &RuntimeContext,
    collection: &Hash,
    token_id: u64,
    to: &PublicKey,
    data: &[u8],
) -> NftResult<()> {
    // Step 1: Input validation
    validate_token_id(token_id)?;
    validate_recipient(to)?;
    validate_safe_transfer_data(data)?;

    // Step 2: Get collection and check paused
    let collection_data = storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)?;
    if collection_data.is_paused {
        return Err(NftError::CollectionPaused);
    }

    // Step 3: Get NFT
    let mut nft = storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)?;

    // Step 4: Check frozen status
    if nft.is_frozen {
        return Err(NftError::TokenFrozen);
    }

    // Step 5: Permission check
    check_nft_permission(storage, &nft, &ctx.caller)?;

    // Step 6: Check self-transfer
    if nft.owner == *to {
        return Err(NftError::SelfTransfer);
    }

    // Step 7: Check rental status
    if storage.has_active_rental(collection, token_id) {
        return Err(NftError::RentalActive);
    }

    // Step 8: Update state BEFORE calling external contract (CEI pattern)
    // This prevents reentrancy attacks
    let from = nft.owner.clone();
    nft.owner = to.clone();
    nft.approved = None;

    // Step 9: Store NFT
    storage.set_nft(&nft)?;

    // Step 10: Update balances
    storage.decrement_balance(collection, &from)?;
    storage.increment_balance(collection, to)?;

    // Step 11: Call receiver hook if target is a contract
    // Note: If hook fails, runtime should rollback all state changes
    if contract_checker.is_contract(to) {
        let hook_result = contract_checker.call_on_nft_received(
            to,
            &ctx.caller,
            &from,
            collection,
            token_id,
            data,
        );

        match hook_result {
            ReceiverHookResult::Accepted => {}
            ReceiverHookResult::Rejected => {
                return Err(NftError::ReceiverRejected);
            }
            ReceiverHookResult::NotImplemented => {
                return Err(NftError::ReceiverNotImplemented);
            }
            ReceiverHookResult::NotContract => {
                // This shouldn't happen, but treat as success
            }
        }
    }

    Ok(())
}

// ========================================
// Batch Transfer Operation
// ========================================

/// Batch transfer multiple NFTs
///
/// # Parameters
/// - `storage`: Storage backend
/// - `ctx`: Runtime context
/// - `collection`: Collection ID
/// - `transfers`: List of (token_id, to) tuples
///
/// # Returns
/// - `Ok(())`: All transfers succeeded
/// - `Err(NftError)`: First error encountered
pub fn batch_transfer<S: NftStorage>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection: &Hash,
    transfers: &[(u64, PublicKey)],
) -> NftResult<()> {
    // Validate batch size
    if transfers.is_empty() {
        return Err(NftError::BatchEmpty);
    }
    if transfers.len() > MAX_BATCH_SIZE {
        return Err(NftError::BatchSizeExceeded);
    }

    // Check for duplicates
    let mut seen = std::collections::HashSet::new();
    for (token_id, _) in transfers {
        if !seen.insert(*token_id) {
            return Err(NftError::DuplicateToken);
        }
    }

    // Execute all transfers
    for (token_id, to) in transfers {
        transfer(storage, ctx, collection, *token_id, to)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::collection::{create_collection, CreateCollectionParams};
    use super::super::mint::{mint, MintParams};
    use super::*;
    use crate::nft::{MintAuthority, Nft, NftCollection};
    use std::collections::HashMap;

    // Mock storage
    struct MockStorage {
        collections: HashMap<Hash, NftCollection>,
        nfts: HashMap<(Hash, u64), Nft>,
        balances: HashMap<(Hash, PublicKey), u64>,
        approvals: HashMap<(PublicKey, Hash, PublicKey), bool>,
        mint_counts: HashMap<(Hash, PublicKey), u64>,
        nonce: u64,
        active_rentals: std::collections::HashSet<(Hash, u64)>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                collections: HashMap::new(),
                nfts: HashMap::new(),
                balances: HashMap::new(),
                approvals: HashMap::new(),
                mint_counts: HashMap::new(),
                nonce: 0,
                active_rentals: std::collections::HashSet::new(),
            }
        }

        fn add_rental(&mut self, collection: Hash, token_id: u64) {
            self.active_rentals.insert((collection, token_id));
        }
    }

    impl NftStorage for MockStorage {
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
            *self
                .balances
                .get(&(collection.clone(), owner.clone()))
                .unwrap_or(&0)
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
            *self
                .approvals
                .get(&(owner.clone(), collection.clone(), operator.clone()))
                .unwrap_or(&false)
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
            *self
                .mint_counts
                .get(&(collection.clone(), user.clone()))
                .unwrap_or(&0)
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
            let current = self.nonce;
            self.nonce = self.nonce.checked_add(1).ok_or(NftError::Overflow)?;
            Ok(current)
        }

        fn has_active_rental(&self, collection: &Hash, token_id: u64) -> bool {
            self.active_rentals
                .contains(&(collection.clone(), token_id))
        }
    }

    // Mock contract checker
    struct MockContractChecker {
        contracts: std::collections::HashSet<PublicKey>,
        accepting: std::collections::HashSet<PublicKey>,
    }

    impl MockContractChecker {
        fn new() -> Self {
            Self {
                contracts: std::collections::HashSet::new(),
                accepting: std::collections::HashSet::new(),
            }
        }

        fn add_contract(&mut self, addr: PublicKey, accepts: bool) {
            self.contracts.insert(addr.clone());
            if accepts {
                self.accepting.insert(addr);
            }
        }
    }

    impl ContractChecker for MockContractChecker {
        fn is_contract(&self, address: &PublicKey) -> bool {
            self.contracts.contains(address)
        }

        fn call_on_nft_received(
            &self,
            contract: &PublicKey,
            _operator: &PublicKey,
            _from: &PublicKey,
            _collection: &Hash,
            _token_id: u64,
            _data: &[u8],
        ) -> ReceiverHookResult {
            if !self.contracts.contains(contract) {
                ReceiverHookResult::NotContract
            } else if self.accepting.contains(contract) {
                ReceiverHookResult::Accepted
            } else {
                ReceiverHookResult::Rejected
            }
        }
    }

    fn test_public_key() -> PublicKey {
        PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[10u8; 32])
                .expect("valid"),
        )
    }

    fn test_public_key_2() -> PublicKey {
        PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[1u8; 32])
                .expect("valid"),
        )
    }

    fn setup_test() -> (MockStorage, Hash, u64, PublicKey) {
        let mut storage = MockStorage::new();
        let creator = test_public_key();

        // Create collection
        let ctx = RuntimeContext::new(creator.clone(), 100);
        let params = CreateCollectionParams {
            name: "Test".to_string(),
            symbol: "TEST".to_string(),
            base_uri: "".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: Some(creator.clone()),
            metadata_authority: None,
        };
        let collection_id = create_collection(&mut storage, &ctx, params).unwrap();

        // Mint NFT
        let token_id = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), creator.clone()),
        )
        .unwrap();

        (storage, collection_id, token_id, creator)
    }

    #[test]
    fn test_transfer_success() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let recipient = test_public_key_2();

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let result = transfer(&mut storage, &ctx, &collection_id, token_id, &recipient);
        assert!(result.is_ok());

        // Verify ownership changed
        let nft = storage.get_nft(&collection_id, token_id).unwrap();
        assert_eq!(nft.owner, recipient);
        assert!(nft.approved.is_none());

        // Verify balances
        assert_eq!(storage.get_balance(&collection_id, &owner), 0);
        assert_eq!(storage.get_balance(&collection_id, &recipient), 1);
    }

    #[test]
    fn test_transfer_by_approved() {
        let (mut storage, collection_id, token_id, _owner) = setup_test();
        let operator = test_public_key_2();
        let recipient = PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[2u8; 32])
                .expect("valid"),
        );

        // Approve operator
        let mut nft = storage.get_nft(&collection_id, token_id).unwrap();
        nft.approved = Some(operator.clone());
        storage.set_nft(&nft).unwrap();

        // Transfer by operator
        let ctx = RuntimeContext::new(operator, 100);
        let result = transfer(&mut storage, &ctx, &collection_id, token_id, &recipient);
        assert!(result.is_ok());

        let nft = storage.get_nft(&collection_id, token_id).unwrap();
        assert_eq!(nft.owner, recipient);
    }

    #[test]
    fn test_transfer_by_global_operator() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let operator = test_public_key_2();
        let recipient = PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[2u8; 32])
                .expect("valid"),
        );

        // Set global approval
        storage
            .set_approval_for_all(&owner, &collection_id, &operator, true)
            .unwrap();

        // Transfer by operator
        let ctx = RuntimeContext::new(operator.clone(), 100);
        let result = transfer(&mut storage, &ctx, &collection_id, token_id, &recipient);
        assert!(result.is_ok());
    }

    #[test]
    fn test_transfer_not_authorized() {
        let (mut storage, collection_id, token_id, _owner) = setup_test();
        let other = test_public_key_2();

        let ctx = RuntimeContext::new(other.clone(), 100);
        let result = transfer(&mut storage, &ctx, &collection_id, token_id, &other);
        assert_eq!(result, Err(NftError::NotApproved));
    }

    #[test]
    fn test_transfer_self_transfer_fails() {
        let (mut storage, collection_id, token_id, owner) = setup_test();

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let result = transfer(&mut storage, &ctx, &collection_id, token_id, &owner);
        assert_eq!(result, Err(NftError::SelfTransfer));
    }

    #[test]
    fn test_transfer_frozen_fails() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let recipient = test_public_key_2();

        // Freeze token
        let mut nft = storage.get_nft(&collection_id, token_id).unwrap();
        nft.is_frozen = true;
        storage.set_nft(&nft).unwrap();

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let result = transfer(&mut storage, &ctx, &collection_id, token_id, &recipient);
        assert_eq!(result, Err(NftError::TokenFrozen));
    }

    #[test]
    fn test_transfer_rented_fails() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let recipient = test_public_key_2();

        // Add active rental
        storage.add_rental(collection_id.clone(), token_id);

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let result = transfer(&mut storage, &ctx, &collection_id, token_id, &recipient);
        assert_eq!(result, Err(NftError::RentalActive));
    }

    #[test]
    fn test_transfer_clears_approval() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let operator = test_public_key_2();
        let recipient = PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[2u8; 32])
                .expect("valid"),
        );

        // Set approval
        let mut nft = storage.get_nft(&collection_id, token_id).unwrap();
        nft.approved = Some(operator);
        storage.set_nft(&nft).unwrap();

        // Transfer
        let ctx = RuntimeContext::new(owner.clone(), 100);
        transfer(&mut storage, &ctx, &collection_id, token_id, &recipient).unwrap();

        // Approval should be cleared
        let nft = storage.get_nft(&collection_id, token_id).unwrap();
        assert!(nft.approved.is_none());
    }

    #[test]
    fn test_safe_transfer_to_eoa() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let recipient = test_public_key_2();
        let contract_checker = MockContractChecker::new();

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let result = safe_transfer(
            &mut storage,
            &contract_checker,
            &ctx,
            &collection_id,
            token_id,
            &recipient,
            &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_transfer_to_accepting_contract() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let contract = test_public_key_2();
        let mut contract_checker = MockContractChecker::new();
        contract_checker.add_contract(contract.clone(), true);

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let result = safe_transfer(
            &mut storage,
            &contract_checker,
            &ctx,
            &collection_id,
            token_id,
            &contract,
            b"test data",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_transfer_to_rejecting_contract() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let contract = test_public_key_2();
        let mut contract_checker = MockContractChecker::new();
        contract_checker.add_contract(contract.clone(), false); // Rejects

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let result = safe_transfer(
            &mut storage,
            &contract_checker,
            &ctx,
            &collection_id,
            token_id,
            &contract,
            &[],
        );
        assert_eq!(result, Err(NftError::ReceiverRejected));
    }

    #[test]
    fn test_batch_transfer_success() {
        let (mut storage, collection_id, _, owner) = setup_test();
        let recipient = test_public_key_2();

        // Mint more NFTs
        let ctx = RuntimeContext::new(owner.clone(), 100);
        let id2 = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), owner.clone()),
        )
        .unwrap();
        let id3 = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), owner.clone()),
        )
        .unwrap();

        // Batch transfer
        let transfers = vec![
            (1, recipient.clone()),
            (id2, recipient.clone()),
            (id3, recipient.clone()),
        ];
        let result = batch_transfer(&mut storage, &ctx, &collection_id, &transfers);
        assert!(result.is_ok());

        // All transferred
        assert_eq!(storage.get_balance(&collection_id, &owner), 0);
        assert_eq!(storage.get_balance(&collection_id, &recipient), 3);
    }

    #[test]
    fn test_batch_transfer_duplicate_fails() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let recipient = test_public_key_2();

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let transfers = vec![(token_id, recipient.clone()), (token_id, recipient.clone())]; // Duplicate
        let result = batch_transfer(&mut storage, &ctx, &collection_id, &transfers);
        assert_eq!(result, Err(NftError::DuplicateToken));
    }
}
