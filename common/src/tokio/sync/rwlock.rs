use log::{debug, error, log, Level};
use std::{
    future::Future,
    ops::{Deref, DerefMut},
    panic::Location,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{
        RwLock as InnerRwLock, RwLockReadGuard as InnerRwLockReadGuard,
        RwLockWriteGuard as InnerRwLockWriteGuard,
    },
    time::timeout,
};

// Simple wrapper around RwLock
// to panic on a failed lock and print all actual lock locations
pub struct RwLock<T: ?Sized> {
    init_location: &'static Location<'static>,
    active_write_location: Arc<StdMutex<Option<(&'static Location<'static>, Instant)>>>,
    active_read_locations: Arc<StdMutex<Vec<(&'static Location<'static>, Instant)>>>,
    read_guards: Arc<AtomicU64>,
    inner: InnerRwLock<T>,
}

impl<T: ?Sized> RwLock<T> {
    #[track_caller]
    pub fn new(value: T) -> Self
    where
        T: Sized,
    {
        Self {
            init_location: Location::caller(),
            active_write_location: Arc::new(StdMutex::new(None)),
            active_read_locations: Arc::new(StdMutex::new(Vec::new())),
            read_guards: Arc::new(AtomicU64::new(0)),
            inner: InnerRwLock::new(value),
        }
    }

    fn show_locations(&self, location: &Location, write: bool) {
        let mut msg = String::new();
        {
            let location = match self.active_write_location.lock() {
                Ok(guard) => guard,
                Err(err) => {
                    if log::log_enabled!(log::Level::Warn) {
                        error!("RwLock active write location lock poisoned");
                    }
                    err.into_inner()
                }
            };
            if let Some((location, start)) = location.as_ref() {
                msg.push_str(&format!(
                    "\n- write guard at: {} since {:?}",
                    location,
                    start.elapsed()
                ));
            } else {
                msg.push_str("\n- no active write location");
            }
        }

        {
            let locations = match self.active_read_locations.lock() {
                Ok(guard) => guard,
                Err(err) => {
                    if log::log_enabled!(log::Level::Warn) {
                        error!("RwLock active read locations lock poisoned");
                    }
                    err.into_inner()
                }
            };
            for (i, (location, start)) in locations.iter().enumerate() {
                msg.push_str(&format!(
                    "\n- read guard #{} at: {} since {:?}",
                    i,
                    location,
                    start.elapsed()
                ));
            }
        }

        let guards = self.read_guards.load(Ordering::SeqCst);
        if log::log_enabled!(log::Level::Error) {
            error!(
                "RwLock {} (write = {}) (active guards = {}) timed out at {}: {}",
                self.init_location, write, guards, location, msg
            );
        }
    }

    #[track_caller]
    pub fn read(&self) -> impl Future<Output = RwLockReadGuard<'_, T>> {
        let location = Location::caller();
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "RwLock {} trying to read at {}",
                self.init_location, location
            );
        }

        async move {
            let guard = match timeout(Duration::from_secs(10), self.inner.read()).await {
                Ok(guard) => guard,
                Err(_) => {
                    self.show_locations(location, false);
                    self.inner.read().await
                }
            };

            if log::log_enabled!(log::Level::Debug) {
                log!(
                    Level::Debug,
                    "RwLock {} read guard acquired at {}",
                    self.init_location,
                    location
                );
            }

            let mut locations = match self.active_read_locations.lock() {
                Ok(guard) => guard,
                Err(err) => {
                    if log::log_enabled!(log::Level::Warn) {
                        error!("RwLock active read locations lock poisoned");
                    }
                    err.into_inner()
                }
            };
            locations.push((location, Instant::now()));

            self.read_guards.fetch_add(1, Ordering::SeqCst);

            RwLockReadGuard {
                init_location: self.init_location,
                inner: Some(guard),
                locations: self.active_read_locations.clone(),
                location,
                read_guards: self.read_guards.clone(),
            }
        }
    }

    #[track_caller]
    pub fn write(&self) -> impl Future<Output = RwLockWriteGuard<'_, T>> {
        let location = Location::caller();
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "RwLock {} trying to write at {}",
                self.init_location, location
            );
        }

        async move {
            let guard = match timeout(Duration::from_secs(10), self.inner.write()).await {
                Ok(guard) => guard,
                Err(_) => {
                    self.show_locations(location, true);
                    self.inner.write().await
                }
            };

            if log::log_enabled!(log::Level::Debug) {
                log!(
                    Level::Debug,
                    "RwLock {} write guard acquired at {}",
                    self.init_location,
                    location
                );
            }

            let mut active_location = match self.active_write_location.lock() {
                Ok(guard) => guard,
                Err(err) => {
                    if log::log_enabled!(log::Level::Warn) {
                        error!("RwLock active write location lock poisoned");
                    }
                    err.into_inner()
                }
            };
            *active_location = Some((location, Instant::now()));
            RwLockWriteGuard {
                init_location: self.init_location,
                inner: Some(guard),
                active_location: self.active_write_location.clone(),
            }
        }
    }
}

#[derive(Debug)]
pub struct RwLockReadGuard<'a, T: ?Sized> {
    init_location: &'static Location<'static>,
    inner: Option<InnerRwLockReadGuard<'a, T>>,
    locations: Arc<StdMutex<Vec<(&'static Location<'static>, Instant)>>>,
    location: &'static Location<'static>,
    read_guards: Arc<AtomicU64>,
}

impl<'a, T: ?Sized> Drop for RwLockReadGuard<'a, T> {
    fn drop(&mut self) {
        if let Some(guard) = self.inner.take() {
            drop(guard);
        } else if log::log_enabled!(log::Level::Warn) {
            error!("RwLock read guard dropped without inner guard");
        }

        // We don't use a HashSet in case of multi threading where we would lock at same location
        let mut locations = match self.locations.lock() {
            Ok(guard) => guard,
            Err(err) => {
                if log::log_enabled!(log::Level::Warn) {
                    error!("RwLock read locations lock poisoned");
                }
                err.into_inner()
            }
        };

        // Only remove if we find the location - don't remove index 0 as fallback
        // which would corrupt tracking by removing the wrong entry
        let lifetime = if let Some(index) = locations.iter().position(|(v, _)| *v == self.location)
        {
            let (_, lifetime) = locations.remove(index);
            lifetime
        } else {
            if log::log_enabled!(log::Level::Error) {
                error!(
                    "RwLock read guard location missing at {}, cannot remove from tracking",
                    self.location
                );
            }
            Instant::now()
        };
        let guards = self.read_guards.fetch_sub(1, Ordering::SeqCst);
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Dropping {} RwLockReadGuard at {} after {:?} (guards = {})",
                self.init_location,
                self.location,
                lifetime.elapsed(),
                guards
            );
        }
    }
}

impl<'a, T: ?Sized> Deref for RwLockReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self.inner.as_ref() {
            Some(inner) => inner,
            None => {
                if log::log_enabled!(log::Level::Error) {
                    error!("RwLock read guard used after drop");
                }
                panic!("RwLock read guard used after drop")
            }
        }
    }
}

#[derive(Debug)]
pub struct RwLockWriteGuard<'a, T: ?Sized> {
    init_location: &'static Location<'static>,
    inner: Option<InnerRwLockWriteGuard<'a, T>>,
    active_location: Arc<StdMutex<Option<(&'static Location<'static>, Instant)>>>,
}

impl<'a, T: ?Sized> Drop for RwLockWriteGuard<'a, T> {
    fn drop(&mut self) {
        if let Some(guard) = self.inner.take() {
            drop(guard);
        } else if log::log_enabled!(log::Level::Warn) {
            error!("RwLock write guard dropped without inner guard");
        }

        let (active_location, lifetime) = match self.active_location.lock() {
            Ok(mut guard) => guard.take().unwrap_or((self.init_location, Instant::now())),
            Err(err) => {
                if log::log_enabled!(log::Level::Warn) {
                    error!("RwLock write location lock poisoned");
                }
                err.into_inner()
                    .take()
                    .unwrap_or((self.init_location, Instant::now()))
            }
        };

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Dropping {} RwLockWriteGuard at {} after {:?}",
                self.init_location,
                active_location,
                lifetime.elapsed()
            );
        }
    }
}

impl<'a, T: ?Sized> Deref for RwLockWriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self.inner.as_ref() {
            Some(inner) => inner,
            None => {
                if log::log_enabled!(log::Level::Error) {
                    error!("RwLock write guard used after drop");
                }
                panic!("RwLock write guard used after drop")
            }
        }
    }
}

impl<'a, T: ?Sized> DerefMut for RwLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self.inner.as_mut() {
            Some(inner) => inner,
            None => {
                if log::log_enabled!(log::Level::Error) {
                    error!("RwLock write guard used after drop");
                }
                panic!("RwLock write guard used after drop")
            }
        }
    }
}

// impl<T: ?Sized> AsRef<InnerRwLock<T>> for RwLock<T> {
//     fn as_ref(&self) -> &InnerRwLock<T> {
//         &self.inner
//     }
// }

// impl<T: ?Sized> Deref for RwLock<T> {
//     type Target = InnerRwLock<T>;

//     fn deref(&self) -> &Self::Target {
//         &self.inner
//     }
// }

// impl<T: ?Sized> DerefMut for RwLock<T> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.inner
//     }
// }

impl<T: ?Sized> std::fmt::Debug for RwLock<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rwlock() {
        let lock = RwLock::new(42);
        {
            let read_guard = lock.read().await;
            {
                let locations = lock.active_read_locations.lock().unwrap();
                assert_eq!(locations.len(), 1);
            }
            assert_eq!(*read_guard, 42);
        }

        {
            let locations = lock.active_read_locations.lock().unwrap();
            assert!(locations.is_empty());
        }

        {
            let mut write_guard = lock.write().await;
            {
                let location = lock.active_write_location.lock().unwrap();
                assert!(location.is_some());
            }

            *write_guard += 1;
        }
        {
            let read_guard = lock.read().await;
            assert_eq!(*read_guard, 43);
        }
    }
}
