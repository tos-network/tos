use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::{BuildHasher, BuildHasherDefault, Hasher},
};

use anyhow::{Context as AnyContext, Result};

// A hasher for `TypeId`s that takes advantage of its known characteristics.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpHasher(u64);

impl Hasher for NoOpHasher {
    fn write(&mut self, _bytes: &[u8]) {
        // SAFETY: This hasher is intended ONLY for TypeId, which should call write_u64().
        // TypeId is a u64 internally, so the standard Hasher::hash() implementation
        // calls write_u64() directly, never calling this write() method.
        //
        // In debug builds, panic early to surface misuse during development.
        // In release builds, use no-op to avoid crashes in production (hash will be incorrect,
        // but this is acceptable as a graceful degradation if HashMap is accidentally misused).
        #[cfg(debug_assertions)]
        panic!("NoOpHasher::write called; this hasher only supports write_u64 for TypeId.");
        #[cfg(not(debug_assertions))]
        {
            // no-op in release
        }
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[derive(Clone, Default)]
pub struct NoOpBuildHasher;

impl BuildHasher for NoOpBuildHasher {
    type Hasher = NoOpHasher;

    fn build_hasher(&self) -> Self::Hasher {
        NoOpHasher::default()
    }
}

pub struct Context {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>, BuildHasherDefault<NoOpHasher>>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            values: HashMap::default(),
        }
    }

    pub fn store<T: Send + Sync + 'static>(&mut self, data: T) {
        self.values.insert(TypeId::of::<T>(), Box::new(data));
    }

    pub fn remove<T: 'static>(&mut self) {
        self.values.remove(&TypeId::of::<T>());
    }

    pub fn has<T: 'static>(&self) -> bool {
        self.values.contains_key(&TypeId::of::<T>())
    }

    pub fn get_optional<T: 'static>(&self) -> Option<&T> {
        self.values
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref())
    }

    pub fn get<T: 'static>(&self) -> Result<&T> {
        self.get_optional().context("Requested type not found")
    }

    pub fn get_copy<T: 'static + Copy>(&self) -> Result<T> {
        self.get().copied()
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
