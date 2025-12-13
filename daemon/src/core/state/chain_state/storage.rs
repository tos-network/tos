use std::ops::{Deref, DerefMut};

use crate::core::storage::Storage;

pub enum StorageReference<'a, S: Storage> {
    Mutable(&'a mut S),
    Immutable(&'a S),
}

impl<'a, S: Storage> StorageReference<'a, S> {
    /// Try to get a mutable reference to the storage.
    /// Returns an error if the storage is immutable.
    pub fn try_as_mut(&mut self) -> Result<&mut S, &'static str> {
        match self {
            Self::Mutable(s) => Ok(*s),
            Self::Immutable(_) => Err("Cannot mutably borrow immutable storage"),
        }
    }
}

impl<'a, S: Storage> AsRef<S> for StorageReference<'a, S> {
    fn as_ref(&self) -> &S {
        match self {
            Self::Mutable(s) => *s,
            Self::Immutable(s) => s,
        }
    }
}

impl<'a, S: Storage> AsMut<S> for StorageReference<'a, S> {
    fn as_mut(&mut self) -> &mut S {
        match self {
            Self::Mutable(s) => *s,
            #[allow(clippy::panic)]
            Self::Immutable(_) => {
                eprintln!("fatal: Cannot mutably borrow immutable storage");
                std::process::abort()
            }
        }
    }
}

impl<'a, S: Storage> Deref for StorageReference<'a, S> {
    type Target = S;

    fn deref(&self) -> &S {
        self.as_ref()
    }
}

impl<'a, S: Storage> DerefMut for StorageReference<'a, S> {
    fn deref_mut(&mut self) -> &mut S {
        match self {
            Self::Mutable(s) => *s,
            #[allow(clippy::panic)]
            Self::Immutable(_) => {
                eprintln!("fatal: Cannot mutably borrow immutable storage");
                std::process::abort()
            }
        }
    }
}
