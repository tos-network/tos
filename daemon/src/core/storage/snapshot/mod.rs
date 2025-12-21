mod bytes_view;
mod changes;
mod iterator_mode;

use std::{
    collections::{HashMap, HashSet},
    error::Error as StdError,
    hash::Hash,
};

use anyhow::Context;
use bytes::Bytes;
use itertools::Either;
use tos_common::serializer::Serializer;

use crate::core::{error::BlockchainError, storage::StorageCache};

pub use bytes_view::*;
pub use changes::Changes;
pub use iterator_mode::*;

/// Represents the state of an entry in the snapshot.
/// This provides a clearer API than using nested Option<Option<T>>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryState<T> {
    /// The entry has been added/modified in our snapshot
    Stored(T),
    /// The entry has been deleted in our snapshot
    Deleted,
    /// The entry is not present in our snapshot, must fallback on disk
    Absent,
}

impl<T> EntryState<T> {
    /// Returns true if the entry is stored
    pub fn is_stored(&self) -> bool {
        matches!(self, EntryState::Stored(_))
    }

    /// Returns true if the entry is deleted
    pub fn is_deleted(&self) -> bool {
        matches!(self, EntryState::Deleted)
    }

    /// Returns true if the entry is absent
    pub fn is_absent(&self) -> bool {
        matches!(self, EntryState::Absent)
    }

    /// Maps the stored value using the provided function
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> EntryState<U> {
        match self {
            EntryState::Stored(v) => EntryState::Stored(f(v)),
            EntryState::Deleted => EntryState::Deleted,
            EntryState::Absent => EntryState::Absent,
        }
    }

    /// Returns the stored value if present, otherwise None
    pub fn stored(self) -> Option<T> {
        match self {
            EntryState::Stored(v) => Some(v),
            _ => None,
        }
    }
}

/// Snapshot is a transactional batch of changes that can be committed or rolled back.
/// It holds a set of changes per column/tree and a cache state.
///
/// The snapshot is generic over the column type C which must implement Hash + Eq.
/// Changes are stored as Bytes to avoid serialization/deserialization until needed.
#[derive(Debug)]
pub struct Snapshot<C: Hash + Eq> {
    /// Pending changes organized by column
    pub trees: HashMap<C, Changes>,
    /// Snapshot of the application-level cache at the time snapshot was created
    pub cache: StorageCache,
}

impl<C: Hash + Eq + Clone> Snapshot<C> {
    /// Create a deep clone of the snapshot including the cache.
    /// This is used when we need an independent copy that can be modified.
    pub fn clone_mut(&mut self) -> Self {
        Self {
            trees: self.trees.clone(),
            cache: self.cache.clone_mut(),
        }
    }
}

impl<C: Hash + Eq + Clone> Clone for Snapshot<C> {
    fn clone(&self) -> Self {
        Self {
            trees: self.trees.clone(),
            // Don't clone the cache, just create a new empty one
            cache: StorageCache::default(),
        }
    }
}

impl<C: Hash + Eq> Snapshot<C> {
    /// Create a new snapshot with a clone of the current cache state.
    ///
    /// The cache is deep-cloned using `clone_mut()` so modifications to the
    /// snapshot's cache don't affect the original storage cache.
    pub fn new(cache: StorageCache) -> Self {
        Self {
            trees: HashMap::new(),
            cache,
        }
    }

    /// Get immutable access to the snapshot's cache
    pub fn cache(&self) -> &StorageCache {
        &self.cache
    }

    /// Get mutable access to the snapshot's cache
    pub fn cache_mut(&mut self) -> &mut StorageCache {
        &mut self.cache
    }

    /// Consume the snapshot and return the cache
    pub fn into_cache(self) -> StorageCache {
        self.cache
    }

    /// Consume the snapshot and return both the trees and the cache
    pub fn into_parts(self) -> (HashMap<C, Changes>, StorageCache) {
        (self.trees, self.cache)
    }

    /// Remove a key from our snapshot.
    /// Returns the previous value state.
    pub fn delete<K: Into<Bytes>>(&mut self, column: C, key: K) -> EntryState<Bytes> {
        self.trees
            .entry(column)
            .or_insert_with(Changes::default)
            .remove(key)
    }

    /// Count entries based on our snapshot state and the provided iterator
    /// for remaining entries on disk.
    pub fn count_entries<I: AsRef<[u8]>, E: StdError + Send + Sync + 'static>(
        &self,
        column: C,
        iterator: impl Iterator<Item = Result<(I, I), E>>,
    ) -> usize {
        let changes = self.trees.get(&column);
        let has_stored = changes.map_or(false, |changes| {
            changes.writes.values().any(|value| value.is_some())
        });
        let mut disk_keys = if has_stored {
            Some(HashSet::new())
        } else {
            None
        };
        let mut count = 0usize;

        for res in iterator {
            match res {
                Ok((k, _)) => {
                    if let Some(keys) = disk_keys.as_mut() {
                        keys.insert(Bytes::copy_from_slice(k.as_ref()));
                    }

                    let is_deleted = changes.map_or(false, |changes| {
                        changes
                            .writes
                            .get(k.as_ref())
                            .map_or(false, |v| v.is_none())
                    });

                    if !is_deleted {
                        count += 1;
                    }
                }
                Err(_) => {
                    // Preserve previous behavior: treat iterator errors as entries.
                    count += 1;
                }
            }
        }

        if has_stored {
            if let (Some(changes), Some(keys)) = (changes, disk_keys.as_ref()) {
                for (k, v) in changes.writes.iter() {
                    if v.is_some() && !keys.contains(k) {
                        count += 1;
                    }
                }
            }
        }

        count
    }

    /// Check if snapshot is empty based on our snapshot state and the provided
    /// iterator for remaining entries on disk.
    pub fn is_empty<I: AsRef<[u8]>, E: StdError + Send + Sync + 'static>(
        &self,
        column: C,
        iterator: impl Iterator<Item = Result<(I, I), E>>,
    ) -> bool {
        let changes = self.trees.get(&column);

        if let Some(batch) = changes.as_ref() {
            let any = batch.writes.iter().find(|(_, v)| v.is_some());

            if any.is_some() {
                return false;
            }
        }

        let next = iterator
            .map(|res| {
                let (k, _) = res?;

                let is_deleted = changes.map_or(false, |changes| {
                    changes
                        .writes
                        .get(k.as_ref())
                        .map_or(false, |v| v.is_none())
                });

                let v = if is_deleted { None } else { Some(()) };

                Ok::<_, E>(v)
            })
            .filter_map(Result::transpose)
            .next();

        next.is_none()
    }

    /// Insert a key-value pair into our snapshot.
    /// Returns the previous value state.
    pub fn put<K: Into<Bytes>, V: Into<Bytes>>(
        &mut self,
        column: C,
        key: K,
        value: V,
    ) -> EntryState<Bytes> {
        self.trees
            .entry(column)
            .or_insert_with(Changes::default)
            .insert(key, value)
    }

    /// Get a value from our snapshot.
    /// Returns EntryState indicating whether the value is stored, deleted, or absent.
    pub fn get<'a, K: AsRef<[u8]>>(&'a self, column: C, key: K) -> EntryState<&'a Bytes> {
        match self.trees.get(&column) {
            Some(batch) => match batch.writes.get(key.as_ref()) {
                Some(Some(v)) => EntryState::Stored(v),
                Some(None) => EntryState::Deleted,
                None => EntryState::Absent,
            },
            None => EntryState::Absent,
        }
    }

    /// Get the size of a value from our snapshot.
    pub fn get_size<K: AsRef<[u8]>>(&self, column: C, key: K) -> EntryState<usize> {
        match self.trees.get(&column) {
            Some(batch) => match batch.writes.get(key.as_ref()) {
                Some(Some(v)) => EntryState::Stored(v.len()),
                Some(None) => EntryState::Deleted,
                None => EntryState::Absent,
            },
            None => EntryState::Absent,
        }
    }

    /// Check if a key is present in our snapshot.
    /// Returns None if key wasn't overwritten yet.
    pub fn contains<K: AsRef<[u8]>>(&self, column: C, key: K) -> Option<bool> {
        let batch = self.trees.get(&column)?;
        batch.contains(key)
    }

    /// Check if a key is present in our snapshot, defaulting to false.
    pub fn contains_key<K: AsRef<[u8]>>(&self, column: C, key: K) -> bool {
        self.contains(column, key).unwrap_or(false)
    }

    /// Lazy iterator over raw keys and values as BytesView.
    /// Note that this iterator is not allocating or copying any data from it!
    pub fn lazy_iter_raw<
        'a,
        I: AsRef<[u8]> + Into<BytesView<'a>> + 'a,
        E: StdError + Send + Sync + 'static,
    >(
        &'a self,
        column: C,
        mode: IteratorMode,
        iterator: impl Iterator<Item = Result<(I, I), E>> + 'a,
    ) -> impl Iterator<Item = Result<(BytesView<'a>, BytesView<'a>), BlockchainError>> + 'a {
        match self.trees.get(&column) {
            Some(tree) => {
                let disk_iter = iterator
                    .map(|res| {
                        let (key, value) = res.context("Internal error in snapshot iterator")?;

                        // Snapshot doesn't contain the key,
                        // We can use the one from disk
                        let k = key.as_ref();
                        if !tree.writes.contains_key(k) {
                            let k = key.into();
                            let v = value.into();
                            Ok(Some((k, v)))
                        } else {
                            Ok(None)
                        }
                    })
                    .filter_map(Result::transpose);

                let mem_iter: Box<
                    dyn Iterator<Item = (BytesView<'a>, BytesView<'a>)> + Send + Sync,
                > = match mode {
                    IteratorMode::Start => Box::new(
                        tree.writes
                            .iter()
                            .filter_map(|(k, v)| v.as_ref().map(|v| (k.into(), v.into()))),
                    ),
                    IteratorMode::End => Box::new(
                        tree.writes
                            .iter()
                            .rev()
                            .filter_map(|(k, v)| v.as_ref().map(|v| (k.into(), v.into()))),
                    ),
                    IteratorMode::WithPrefix(prefix, direction) => {
                        let prefix = prefix.to_vec();
                        let iter = match direction {
                            Direction::Forward => Either::Left(tree.writes.iter()),
                            Direction::Reverse => Either::Right(tree.writes.iter().rev()),
                        };
                        Box::new(iter.filter_map(move |(k, v)| {
                            if let Some(v) = v {
                                if k.starts_with(&prefix) {
                                    return Some((k.into(), v.into()));
                                }
                            }
                            None
                        }))
                    }
                    IteratorMode::From(start, direction) => {
                        let start = Bytes::from(start.to_vec());
                        let iter = match direction {
                            Direction::Forward => Either::Left(tree.writes.range(start..)),
                            Direction::Reverse => Either::Right(tree.writes.range(..=start).rev()),
                        };
                        Box::new(iter.filter_map(|(k, v)| v.as_ref().map(|v| (k.into(), v.into()))))
                    }
                    IteratorMode::Range {
                        lower_bound,
                        upper_bound,
                        direction,
                    } => {
                        let lower = Bytes::from(lower_bound.to_vec());
                        let upper = Bytes::from(upper_bound.to_vec());
                        let iter = match direction {
                            Direction::Forward => Either::Left(tree.writes.range(lower..upper)),
                            Direction::Reverse => {
                                Either::Right(tree.writes.range(lower..upper).rev())
                            }
                        };
                        Box::new(iter.filter_map(|(k, v)| v.as_ref().map(|v| (k.into(), v.into()))))
                    }
                };

                let mem_iter = mem_iter.map(|(k, v)| Ok((k, v)));

                Either::Left(disk_iter.chain(mem_iter))
            }
            None => {
                let disk_iter = iterator.map(|res| {
                    let (key, value) = res.context("Internal error in snapshot iterator")?;
                    let k = key.into();
                    let v = value.into();
                    Ok((k, v))
                });

                Either::Right(disk_iter)
            }
        }
    }

    /// Lazy iterator over keys and values.
    /// Both are parsed from bytes.
    /// It will fallback on the disk iterator if the key is not present in the batch.
    /// Note that this iterator is lazy and is not allocating or copying any data from it!
    #[inline]
    pub fn lazy_iter<
        'a,
        K,
        V,
        I: AsRef<[u8]> + Into<BytesView<'a>> + 'a,
        E: StdError + Send + Sync + 'static,
    >(
        &'a self,
        column: C,
        mode: IteratorMode,
        iterator: impl Iterator<Item = Result<(I, I), E>> + 'a,
    ) -> impl Iterator<Item = Result<(K, V), BlockchainError>> + 'a
    where
        K: Serializer + 'a,
        V: Serializer + 'a,
    {
        self.lazy_iter_raw::<I, E>(column, mode, iterator)
            .map(|res| {
                let (k_bytes, v_bytes) = res?;

                let k = K::from_bytes(k_bytes.as_ref())
                    .context("Failed to deserialize key in snapshot iterator")?;
                let v = V::from_bytes(v_bytes.as_ref())
                    .context("Failed to deserialize value in snapshot iterator")?;

                Ok((k, v))
            })
    }

    /// Similar to `lazy_iter` but only for keys.
    /// Note that this iterator is lazy and is not allocating or copying any data from it!
    #[inline]
    pub fn lazy_iter_keys<
        'a,
        K,
        I: AsRef<[u8]> + Into<BytesView<'a>> + 'a,
        E: StdError + Send + Sync + 'static,
    >(
        &'a self,
        column: C,
        mode: IteratorMode,
        iterator: impl Iterator<Item = Result<(I, I), E>> + 'a,
    ) -> impl Iterator<Item = Result<K, BlockchainError>> + 'a
    where
        K: Serializer + 'a,
    {
        self.lazy_iter::<K, (), I, E>(column, mode, iterator)
            .map(|res| res.map(|(k, _)| k))
    }

    /// Iterator over keys and values that collects all data into a Vec.
    /// Both are parsed from bytes.
    /// It will fallback on the disk iterator if the key is not present in the batch.
    /// NOTE: this iterator will copy and allocate the data from the iterators
    /// to prevent borrowing current snapshot.
    pub fn iter_owned<
        'a,
        K,
        V,
        I: AsRef<[u8]> + Into<BytesView<'a>> + 'a,
        E: StdError + Send + Sync + 'static,
    >(
        &self,
        column: C,
        mode: IteratorMode,
        iterator: impl Iterator<Item = Result<(I, I), E>> + 'a,
    ) -> impl Iterator<Item = Result<(K, V), BlockchainError>> + 'a
    where
        K: Serializer + 'a,
        V: Serializer + 'a,
    {
        match self.trees.get(&column) {
            Some(tree) => {
                let disk_iter = iterator
                    .map(|res| {
                        let (key, value) = res.context("Internal error in snapshot iterator")?;

                        // Snapshot doesn't contain the key,
                        // We can use the one from disk
                        let k = key.as_ref();
                        if !tree.writes.contains_key(k) {
                            Ok(Some((
                                K::from_bytes(key.as_ref())?,
                                V::from_bytes(value.as_ref())?,
                            )))
                        } else {
                            Ok(None)
                        }
                    })
                    .filter_map(Result::transpose);

                let mem_iter: Box<dyn Iterator<Item = (&Bytes, &Bytes)> + Send + Sync> = match mode
                {
                    IteratorMode::Start => Box::new(
                        tree.writes
                            .iter()
                            .filter_map(|(k, v)| v.as_ref().map(|v| (k, v))),
                    ),
                    IteratorMode::End => Box::new(
                        tree.writes
                            .iter()
                            .rev()
                            .filter_map(|(k, v)| v.as_ref().map(|v| (k, v))),
                    ),
                    IteratorMode::WithPrefix(prefix, direction) => {
                        let prefix = prefix.to_vec();
                        let iter = match direction {
                            Direction::Forward => Either::Left(tree.writes.iter()),
                            Direction::Reverse => Either::Right(tree.writes.iter().rev()),
                        };
                        Box::new(iter.filter_map(move |(k, v)| {
                            if let Some(v) = v {
                                if k.starts_with(&prefix) {
                                    return Some((k, v));
                                }
                            }
                            None
                        }))
                    }
                    IteratorMode::From(start, direction) => {
                        let start = Bytes::from(start.to_vec());
                        let iter = match direction {
                            Direction::Forward => Either::Left(tree.writes.range(start..)),
                            Direction::Reverse => Either::Right(tree.writes.range(..=start).rev()),
                        };
                        Box::new(iter.filter_map(|(k, v)| v.as_ref().map(|v| (k, v))))
                    }
                    IteratorMode::Range {
                        lower_bound,
                        upper_bound,
                        direction,
                    } => {
                        let lower = Bytes::from(lower_bound.to_vec());
                        let upper = Bytes::from(upper_bound.to_vec());
                        let iter = match direction {
                            Direction::Forward => Either::Left(tree.writes.range(lower..upper)),
                            Direction::Reverse => {
                                Either::Right(tree.writes.range(lower..upper).rev())
                            }
                        };
                        Box::new(iter.filter_map(|(k, v)| v.as_ref().map(|v| (k, v))))
                    }
                };

                let mem_iter = mem_iter.map(|(k, v)| Ok((K::from_bytes(k)?, V::from_bytes(v)?)));

                Either::Left(disk_iter.chain(mem_iter).collect::<Vec<_>>().into_iter())
            }
            None => {
                let disk_iter = iterator.map(|res| {
                    let (key, value) = res.context("Internal error in snapshot iterator")?;
                    Ok((K::from_bytes(key.as_ref())?, V::from_bytes(value.as_ref())?))
                });

                Either::Right(disk_iter)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::StorageCache;
    use bytes::Bytes;

    #[test]
    fn test_entry_state_methods() {
        let stored: EntryState<i32> = EntryState::Stored(42);
        assert!(stored.is_stored());
        assert!(!stored.is_deleted());
        assert!(!stored.is_absent());

        let deleted: EntryState<i32> = EntryState::Deleted;
        assert!(!deleted.is_stored());
        assert!(deleted.is_deleted());
        assert!(!deleted.is_absent());

        let absent: EntryState<i32> = EntryState::Absent;
        assert!(!absent.is_stored());
        assert!(!absent.is_deleted());
        assert!(absent.is_absent());
    }

    #[test]
    fn test_entry_state_map() {
        let stored: EntryState<i32> = EntryState::Stored(42);
        let mapped = stored.map(|v| v * 2);
        assert_eq!(mapped, EntryState::Stored(84));

        let deleted: EntryState<i32> = EntryState::Deleted;
        let mapped = deleted.map(|v| v * 2);
        assert_eq!(mapped, EntryState::Deleted);
    }

    #[test]
    fn test_entry_state_stored() {
        let stored: EntryState<i32> = EntryState::Stored(42);
        assert_eq!(stored.stored(), Some(42));

        let deleted: EntryState<i32> = EntryState::Deleted;
        assert_eq!(deleted.stored(), None);

        let absent: EntryState<i32> = EntryState::Absent;
        assert_eq!(absent.stored(), None);
    }

    #[test]
    fn test_snapshot_put_get() {
        let cache = StorageCache::default();
        let mut snapshot: Snapshot<&str> = Snapshot::new(cache);

        // Initially absent
        assert!(snapshot.get("col", b"key1").is_absent());

        // After put, it should be stored
        let prev = snapshot.put("col", "key1", "value1");
        assert!(prev.is_absent());
        assert!(snapshot.get("col", b"key1").is_stored());

        // Put again should return previous value
        let prev = snapshot.put("col", "key1", "value2");
        assert!(prev.is_stored());
    }

    #[test]
    fn test_snapshot_delete() {
        let cache = StorageCache::default();
        let mut snapshot: Snapshot<&str> = Snapshot::new(cache);

        // Put then delete
        snapshot.put("col", "key1", "value1");
        let prev = snapshot.delete("col", "key1");
        assert!(prev.is_stored());

        // After delete, get should return Deleted
        assert!(snapshot.get("col", b"key1").is_deleted());

        // Delete non-existent key
        let prev = snapshot.delete("col", "key2");
        assert!(prev.is_absent());
    }

    #[test]
    fn test_snapshot_contains() {
        let cache = StorageCache::default();
        let mut snapshot: Snapshot<&str> = Snapshot::new(cache);

        // Not in snapshot yet
        assert_eq!(snapshot.contains("col", b"key1"), None);
        assert!(!snapshot.contains_key("col", b"key1"));

        // After put
        snapshot.put("col", "key1", "value1");
        assert_eq!(snapshot.contains("col", b"key1"), Some(true));
        assert!(snapshot.contains_key("col", b"key1"));

        // After delete
        snapshot.delete("col", "key1");
        assert_eq!(snapshot.contains("col", b"key1"), Some(false));
        assert!(!snapshot.contains_key("col", b"key1"));
    }

    #[test]
    fn test_snapshot_clone_mut() {
        let cache = StorageCache::default();
        let mut snapshot: Snapshot<&str> = Snapshot::new(cache);
        snapshot.put("col", "key1", "value1");

        let mut cloned = snapshot.clone_mut();
        cloned.put("col", "key2", "value2");

        // Original should not have key2
        assert!(snapshot.get("col", b"key2").is_absent());

        // Cloned should have both keys
        assert!(cloned.get("col", b"key1").is_stored());
        assert!(cloned.get("col", b"key2").is_stored());
    }

    #[test]
    fn test_snapshot_count_entries_snapshot_only() {
        let cache = StorageCache::default();
        let mut snapshot: Snapshot<&str> = Snapshot::new(cache);
        snapshot.put("col", "key1", "value1");

        let iterator = std::iter::empty::<Result<(Bytes, Bytes), std::io::Error>>();
        assert_eq!(snapshot.count_entries("col", iterator), 1);
    }
}
