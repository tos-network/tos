use std::collections::HashMap;
use std::sync::Mutex;

use crate::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    nft::{Nft, NftCollection, NftError, NftRental, NftResult, RentalListing, TokenBoundAccount},
    versioned_type::VersionedState,
};

use super::operations::NftStorage;

pub trait NftStorageProvider: Sync {
    fn get_collection(
        &self,
        id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, NftCollection)>, anyhow::Error>;

    fn get_token(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Nft)>, anyhow::Error>;

    fn get_owner_balance(
        &self,
        collection: &Hash,
        owner: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error>;

    fn get_operator_approval(
        &self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, bool)>, anyhow::Error>;

    fn get_mint_count(
        &self,
        collection: &Hash,
        user: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error>;

    fn get_collection_nonce(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error>;

    fn get_tba(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, TokenBoundAccount)>, anyhow::Error>;

    fn get_rental_listing(
        &self,
        listing_id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, RentalListing)>, anyhow::Error>;

    fn get_active_rental(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, NftRental)>, anyhow::Error>;
}

#[derive(Debug, Clone)]
pub struct NftCache {
    pub collections: HashMap<Hash, (VersionedState, Option<NftCollection>)>,
    pub tokens: HashMap<(Hash, u64), (VersionedState, Option<Nft>)>,
    pub owner_balances: HashMap<(Hash, PublicKey), (VersionedState, u64)>,
    pub operator_approvals: HashMap<(PublicKey, Hash, PublicKey), (VersionedState, bool)>,
    pub mint_counts: HashMap<(Hash, PublicKey), (VersionedState, u64)>,
    pub collection_nonce: Option<(VersionedState, u64)>,
    pub tbas: HashMap<(Hash, u64), (VersionedState, Option<TokenBoundAccount>)>,
    pub rental_listings: HashMap<Hash, (VersionedState, Option<RentalListing>)>,
    pub active_rentals: HashMap<(Hash, u64), (VersionedState, Option<NftRental>)>,
}

impl Default for NftCache {
    fn default() -> Self {
        Self::new()
    }
}

impl NftCache {
    pub fn new() -> Self {
        Self {
            collections: HashMap::new(),
            tokens: HashMap::new(),
            owner_balances: HashMap::new(),
            operator_approvals: HashMap::new(),
            mint_counts: HashMap::new(),
            collection_nonce: None,
            tbas: HashMap::new(),
            rental_listings: HashMap::new(),
            active_rentals: HashMap::new(),
        }
    }
}

pub struct NftCacheStorage<'a> {
    provider: &'a (dyn NftStorageProvider + Sync),
    topoheight: TopoHeight,
    cache: Mutex<&'a mut NftCache>,
}

impl<'a> NftCacheStorage<'a> {
    pub fn new(
        provider: &'a (dyn NftStorageProvider + Sync),
        topoheight: TopoHeight,
        cache: &'a mut NftCache,
    ) -> Self {
        Self {
            provider,
            topoheight,
            cache: Mutex::new(cache),
        }
    }

    fn with_cache<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut NftCache) -> R,
    {
        let mut cache_ref = match self.cache.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        f(*cache_ref)
    }

    fn map_err(err: anyhow::Error) -> NftError {
        let _ = err;
        NftError::StorageError
    }

    fn get_cached_collection(&self, id: &Hash) -> Option<Option<NftCollection>> {
        self.with_cache(|cache| cache.collections.get(id).map(|(_, value)| value.clone()))
    }

    fn get_cached_token(&self, key: &(Hash, u64)) -> Option<Option<Nft>> {
        self.with_cache(|cache| cache.tokens.get(key).map(|(_, value)| value.clone()))
    }

    fn get_cached_balance(&self, key: &(Hash, PublicKey)) -> Option<u64> {
        self.with_cache(|cache| cache.owner_balances.get(key).map(|(_, value)| *value))
    }

    fn get_cached_approval(&self, key: &(PublicKey, Hash, PublicKey)) -> Option<bool> {
        self.with_cache(|cache| cache.operator_approvals.get(key).map(|(_, value)| *value))
    }

    fn get_cached_mint_count(&self, key: &(Hash, PublicKey)) -> Option<u64> {
        self.with_cache(|cache| cache.mint_counts.get(key).map(|(_, value)| *value))
    }

    fn get_cached_tba(&self, key: &(Hash, u64)) -> Option<Option<TokenBoundAccount>> {
        self.with_cache(|cache| cache.tbas.get(key).map(|(_, value)| value.clone()))
    }

    fn get_cached_active_rental(&self, key: &(Hash, u64)) -> Option<Option<NftRental>> {
        self.with_cache(|cache| {
            cache
                .active_rentals
                .get(key)
                .map(|(_, value)| value.clone())
        })
    }
}

impl NftStorage for NftCacheStorage<'_> {
    fn get_collection(&self, id: &Hash) -> Option<NftCollection> {
        if let Some(value) = self.get_cached_collection(id) {
            return value;
        }

        let fetched = self
            .provider
            .get_collection(id, self.topoheight)
            .ok()
            .flatten()?;
        let (topo, collection) = fetched;
        self.with_cache(|cache| {
            cache.collections.insert(
                id.clone(),
                (VersionedState::FetchedAt(topo), Some(collection.clone())),
            );
        });
        Some(collection)
    }

    fn set_collection(&mut self, collection: &NftCollection) -> NftResult<()> {
        let id = collection.id.clone();
        let existing_state =
            self.with_cache(|cache| cache.collections.get(&id).map(|(state, _)| *state));
        let state = match existing_state {
            Some(mut state) => {
                state.mark_updated();
                state
            }
            None => match self
                .provider
                .get_collection(&id, self.topoheight)
                .map_err(Self::map_err)?
            {
                Some((topo, _)) => VersionedState::Updated(topo),
                None => VersionedState::New,
            },
        };

        self.with_cache(|cache| {
            cache
                .collections
                .insert(id, (state, Some(collection.clone())));
        });
        Ok(())
    }

    fn collection_exists(&self, id: &Hash) -> bool {
        self.get_collection(id).is_some()
    }

    fn get_nft(&self, collection: &Hash, token_id: u64) -> Option<Nft> {
        let key = (collection.clone(), token_id);
        if let Some(value) = self.get_cached_token(&key) {
            return value;
        }

        let fetched = self
            .provider
            .get_token(collection, token_id, self.topoheight)
            .ok()
            .flatten()?;
        let (topo, nft) = fetched;
        self.with_cache(|cache| {
            cache
                .tokens
                .insert(key, (VersionedState::FetchedAt(topo), Some(nft.clone())));
        });
        Some(nft)
    }

    fn set_nft(&mut self, nft: &Nft) -> NftResult<()> {
        let key = (nft.collection.clone(), nft.token_id);
        let existing_state =
            self.with_cache(|cache| cache.tokens.get(&key).map(|(state, _)| *state));
        let state = match existing_state {
            Some(mut state) => {
                state.mark_updated();
                state
            }
            None => match self
                .provider
                .get_token(&nft.collection, nft.token_id, self.topoheight)
                .map_err(Self::map_err)?
            {
                Some((topo, _)) => VersionedState::Updated(topo),
                None => VersionedState::New,
            },
        };

        self.with_cache(|cache| {
            cache.tokens.insert(key, (state, Some(nft.clone())));
        });
        Ok(())
    }

    fn delete_nft(&mut self, collection: &Hash, token_id: u64) -> NftResult<()> {
        let key = (collection.clone(), token_id);
        let cached_state = self.with_cache(|cache| cache.tokens.get(&key).map(|(state, _)| *state));
        if let Some(state) = cached_state {
            if state.is_new() {
                self.with_cache(|cache| {
                    cache.tokens.remove(&key);
                });
                return Ok(());
            }
            let mut updated_state = state;
            updated_state.mark_updated();
            self.with_cache(|cache| {
                cache.tokens.insert(key, (updated_state, None));
            });
            return Ok(());
        }

        if let Some((topo, _)) = self
            .provider
            .get_token(collection, token_id, self.topoheight)
            .map_err(Self::map_err)?
        {
            self.with_cache(|cache| {
                cache
                    .tokens
                    .insert(key, (VersionedState::Updated(topo), None));
            });
        }

        Ok(())
    }

    fn nft_exists(&self, collection: &Hash, token_id: u64) -> bool {
        self.get_nft(collection, token_id).is_some()
    }

    fn get_balance(&self, collection: &Hash, owner: &PublicKey) -> u64 {
        let key = (collection.clone(), owner.clone());
        if let Some(value) = self.get_cached_balance(&key) {
            return value;
        }

        let fetched = self
            .provider
            .get_owner_balance(collection, owner, self.topoheight)
            .ok()
            .flatten();

        if let Some((topo, balance)) = fetched {
            self.with_cache(|cache| {
                cache
                    .owner_balances
                    .insert(key, (VersionedState::FetchedAt(topo), balance));
            });
            return balance;
        }

        0
    }

    fn increment_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64> {
        let key = (collection.clone(), owner.clone());
        let cached = self.with_cache(|cache| cache.owner_balances.get(&key).cloned());
        let (state, current) = if let Some((state, value)) = cached {
            (state, value)
        } else if let Some((topo, value)) = self
            .provider
            .get_owner_balance(collection, owner, self.topoheight)
            .map_err(Self::map_err)?
        {
            (VersionedState::FetchedAt(topo), value)
        } else {
            (VersionedState::New, 0)
        };

        let mut new_state = state;
        if !new_state.is_new() {
            new_state.mark_updated();
        }
        let new_balance = current.checked_add(1).ok_or(NftError::Overflow)?;
        self.with_cache(|cache| {
            cache.owner_balances.insert(key, (new_state, new_balance));
        });
        Ok(new_balance)
    }

    fn decrement_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64> {
        let key = (collection.clone(), owner.clone());
        let cached = self.with_cache(|cache| cache.owner_balances.get(&key).cloned());
        let (state, current) = if let Some((state, value)) = cached {
            (state, value)
        } else if let Some((topo, value)) = self
            .provider
            .get_owner_balance(collection, owner, self.topoheight)
            .map_err(Self::map_err)?
        {
            (VersionedState::FetchedAt(topo), value)
        } else {
            (VersionedState::New, 0)
        };

        let mut new_state = state;
        if !new_state.is_new() {
            new_state.mark_updated();
        }
        let new_balance = current.checked_sub(1).ok_or(NftError::Overflow)?;
        self.with_cache(|cache| {
            cache.owner_balances.insert(key, (new_state, new_balance));
        });
        Ok(new_balance)
    }

    fn is_approved_for_all(
        &self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
    ) -> bool {
        let key = (owner.clone(), collection.clone(), operator.clone());
        if let Some(value) = self.get_cached_approval(&key) {
            return value;
        }

        let fetched = self
            .provider
            .get_operator_approval(owner, collection, operator, self.topoheight)
            .ok()
            .flatten();
        if let Some((topo, approved)) = fetched {
            self.with_cache(|cache| {
                cache
                    .operator_approvals
                    .insert(key, (VersionedState::FetchedAt(topo), approved));
            });
            return approved;
        }

        false
    }

    fn set_approval_for_all(
        &mut self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        approved: bool,
    ) -> NftResult<()> {
        let key = (owner.clone(), collection.clone(), operator.clone());
        let existing_state =
            self.with_cache(|cache| cache.operator_approvals.get(&key).map(|(state, _)| *state));
        let state = match existing_state {
            Some(mut state) => {
                state.mark_updated();
                state
            }
            None => match self
                .provider
                .get_operator_approval(owner, collection, operator, self.topoheight)
                .map_err(Self::map_err)?
            {
                Some((topo, _)) => VersionedState::Updated(topo),
                None => VersionedState::New,
            },
        };

        self.with_cache(|cache| {
            cache.operator_approvals.insert(key, (state, approved));
        });
        Ok(())
    }

    fn get_mint_count(&self, collection: &Hash, user: &PublicKey) -> u64 {
        let key = (collection.clone(), user.clone());
        if let Some(value) = self.get_cached_mint_count(&key) {
            return value;
        }

        let fetched = self
            .provider
            .get_mint_count(collection, user, self.topoheight)
            .ok()
            .flatten();
        if let Some((topo, count)) = fetched {
            self.with_cache(|cache| {
                cache
                    .mint_counts
                    .insert(key, (VersionedState::FetchedAt(topo), count));
            });
            return count;
        }

        0
    }

    fn increment_mint_count(&mut self, collection: &Hash, user: &PublicKey) -> NftResult<u64> {
        let key = (collection.clone(), user.clone());
        let cached = self.with_cache(|cache| cache.mint_counts.get(&key).cloned());
        let (state, current) = if let Some((state, value)) = cached {
            (state, value)
        } else if let Some((topo, value)) = self
            .provider
            .get_mint_count(collection, user, self.topoheight)
            .map_err(Self::map_err)?
        {
            (VersionedState::FetchedAt(topo), value)
        } else {
            (VersionedState::New, 0)
        };

        let mut new_state = state;
        if !new_state.is_new() {
            new_state.mark_updated();
        }
        let new_count = current.checked_add(1).ok_or(NftError::Overflow)?;
        self.with_cache(|cache| {
            cache.mint_counts.insert(key, (new_state, new_count));
        });
        Ok(new_count)
    }

    fn get_and_increment_collection_nonce(&mut self) -> NftResult<u64> {
        let cached = self.with_cache(|cache| cache.collection_nonce);
        let (state, current) = if let Some((state, value)) = cached {
            (state, value)
        } else if let Some((topo, value)) = self
            .provider
            .get_collection_nonce(self.topoheight)
            .map_err(Self::map_err)?
        {
            (VersionedState::FetchedAt(topo), value)
        } else {
            (VersionedState::New, 0)
        };

        let next = current.checked_add(1).ok_or(NftError::Overflow)?;
        let mut new_state = state;
        if !new_state.is_new() {
            new_state.mark_updated();
        }
        self.with_cache(|cache| {
            cache.collection_nonce = Some((new_state, next));
        });
        Ok(current)
    }

    fn has_active_rental(&self, collection: &Hash, token_id: u64) -> bool {
        let key = (collection.clone(), token_id);
        if let Some(value) = self.get_cached_active_rental(&key) {
            return value
                .as_ref()
                .map(|rental| rental.is_active(self.topoheight))
                .unwrap_or(false);
        }

        let fetched = self
            .provider
            .get_active_rental(collection, token_id, self.topoheight)
            .ok()
            .flatten();
        if let Some((topo, rental)) = fetched {
            let active = rental.is_active(self.topoheight);
            self.with_cache(|cache| {
                cache
                    .active_rentals
                    .insert(key, (VersionedState::FetchedAt(topo), Some(rental)));
            });
            return active;
        }

        false
    }

    fn tba_has_assets(&self, collection: &Hash, token_id: u64) -> bool {
        let key = (collection.clone(), token_id);
        if let Some(value) = self.get_cached_tba(&key) {
            return value.map(|tba| tba.is_active).unwrap_or(false);
        }

        let fetched = self
            .provider
            .get_tba(collection, token_id, self.topoheight)
            .ok()
            .flatten();
        if let Some((topo, tba)) = fetched {
            let active = tba.is_active;
            self.with_cache(|cache| {
                cache
                    .tbas
                    .insert(key, (VersionedState::FetchedAt(topo), Some(tba)));
            });
            return active;
        }

        false
    }

    fn remove_tba(&mut self, collection: &Hash, token_id: u64) -> NftResult<()> {
        let key = (collection.clone(), token_id);
        let cached_state = self.with_cache(|cache| cache.tbas.get(&key).map(|(state, _)| *state));
        if let Some(state) = cached_state {
            if state.is_new() {
                self.with_cache(|cache| {
                    cache.tbas.remove(&key);
                });
                return Ok(());
            }
            let mut updated_state = state;
            updated_state.mark_updated();
            self.with_cache(|cache| {
                cache.tbas.insert(key, (updated_state, None));
            });
            return Ok(());
        }

        if let Some((topo, _)) = self
            .provider
            .get_tba(collection, token_id, self.topoheight)
            .map_err(Self::map_err)?
        {
            self.with_cache(|cache| {
                cache
                    .tbas
                    .insert(key, (VersionedState::Updated(topo), None));
            });
        }

        Ok(())
    }
}
