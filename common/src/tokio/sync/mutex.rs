use log::{debug, error, log, Level};
use std::{
    future::Future,
    ops::{Deref, DerefMut},
    panic::Location,
    sync::Mutex as StdMutex,
    time::Duration,
};
use tokio::{
    sync::{Mutex as InnerMutex, MutexGuard},
    time::timeout,
};

pub struct Mutex<T: ?Sized> {
    init_location: &'static Location<'static>,
    last_location: StdMutex<Option<&'static Location<'static>>>,
    inner: InnerMutex<T>,
}

impl<T: ?Sized> Mutex<T> {
    #[track_caller]
    pub fn new(t: T) -> Self
    where
        T: Sized,
    {
        Self {
            init_location: Location::caller(),
            last_location: StdMutex::new(None),
            inner: InnerMutex::new(t),
        }
    }

    #[track_caller]
    pub fn lock(&self) -> impl Future<Output = MutexGuard<'_, T>> {
        let location = Location::caller();
        if log::log_enabled!(log::Level::Debug) {
            debug!("Mutex at {} locking at {}", self.init_location, location);
        }

        async move {
            let guard = match timeout(Duration::from_secs(10), self.inner.lock()).await {
                Ok(guard) => guard,
                Err(_) => {
                    // Build error message in a scope to ensure MutexGuard is dropped before await
                    {
                        let last = match self.last_location.lock() {
                            Ok(guard) => guard,
                            Err(err) => {
                                if log::log_enabled!(log::Level::Warn) {
                                    error!("Mutex last location lock poisoned");
                                }
                                err.into_inner()
                            }
                        };
                        let mut msg = format!(
                            "Mutex at {} failed locking at {}.",
                            self.init_location, location
                        );
                        if let Some(last) = *last {
                            msg.push_str(&format!("\n- Last successful lock at: {last}"));
                        }

                        if log::log_enabled!(log::Level::Error) {
                            error!("{}", msg);
                        }
                    } // MutexGuard dropped here before await
                    self.inner.lock().await
                }
            };

            if log::log_enabled!(log::Level::Debug) {
                log!(
                    Level::Debug,
                    "Mutex {} write guard acquired at {}",
                    self.init_location,
                    location
                );
            }
            let mut last = match self.last_location.lock() {
                Ok(guard) => guard,
                Err(err) => {
                    if log::log_enabled!(log::Level::Warn) {
                        error!("Mutex last location lock poisoned");
                    }
                    err.into_inner()
                }
            };
            *last = Some(location);
            guard
        }
    }
}

impl<T: ?Sized> AsRef<InnerMutex<T>> for Mutex<T> {
    fn as_ref(&self) -> &InnerMutex<T> {
        &self.inner
    }
}

impl<T: ?Sized> Deref for Mutex<T> {
    type Target = InnerMutex<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ?Sized> DerefMut for Mutex<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: ?Sized> std::fmt::Debug for Mutex<T>
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
    async fn test_mutex() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock().await;
        {
            let location = mutex.last_location.lock().unwrap();
            assert!(location.is_some());
        }
        assert_eq!(*guard, 42);
    }
}
