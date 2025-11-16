// File: testing-framework/src/orchestrator/clock.rs
//
// Clock Abstraction - V3.0 Deterministic Infrastructure
//
// This module implements the Clock trait to enable deterministic time control
// in tests while maintaining production compatibility with real system time.

use std::future::Future;
use std::pin::Pin;
use tokio::time::{self, Duration, Instant};

/// Clock abstraction trait - all business code depends on this trait
///
/// This trait provides time-related operations (reading current time and sleeping)
/// while allowing complete control over time progression in tests. By injecting
/// a Clock implementation, business code becomes testable without relying on
/// real wall-clock time.
///
/// # Examples
///
/// ## Production Usage (SystemClock)
///
/// ```rust
/// use std::sync::Arc;
/// use tokio::time::Duration;
/// use tos_testing_framework::orchestrator::clock::{Clock, SystemClock};
///
/// #[tokio::main]
/// async fn main() {
///     let clock: Arc<dyn Clock> = Arc::new(SystemClock);
///     let start = clock.now();
///     clock.sleep(Duration::from_millis(100)).await;
///     let elapsed = clock.now() - start;
///     assert!(elapsed >= Duration::from_millis(100));
/// }
/// ```
///
/// ## Test Usage (PausedClock)
///
/// ```rust
/// use std::sync::Arc;
/// use tokio::time::Duration;
/// use tos_testing_framework::orchestrator::clock::{Clock, PausedClock};
///
/// #[tokio::test(start_paused = true)]
/// async fn test_with_paused_time() {
///     let clock = Arc::new(PausedClock::new());
///     let start = clock.now();
///
///     // Manually advance time by 1 hour
///     clock.advance(Duration::from_secs(3600)).await;
///
///     let elapsed = clock.now() - start;
///     assert_eq!(elapsed, Duration::from_secs(3600));
/// }
/// ```
pub trait Clock: Send + Sync {
    /// Returns the current instant in time
    ///
    /// In production (SystemClock), this returns the real wall-clock time.
    /// In tests (PausedClock), this returns the simulated time which can be
    /// manually advanced for deterministic testing.
    fn now(&self) -> Instant;

    /// Sleeps for the specified duration
    ///
    /// In production (SystemClock), this actually waits for the duration.
    /// In tests (PausedClock), this cooperates with tokio::time::pause() to
    /// enable instant time advancement without real delays.
    fn sleep(&self, d: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// System real-time clock (production environment)
///
/// This implementation uses tokio's actual time functions and should be used
/// in production deployments. It provides real wall-clock time with no ability
/// to manipulate time progression.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use tokio::time::Duration;
/// use tos_testing_framework::orchestrator::clock::{Clock, SystemClock};
///
/// async fn production_code(clock: Arc<dyn Clock>) {
///     let start = clock.now();
///     // Real work happens here...
///     clock.sleep(Duration::from_secs(1)).await;
///     let duration = clock.now() - start;
///     println!("Elapsed: {:?}", duration);
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let clock: Arc<dyn Clock> = Arc::new(SystemClock);
///     production_code(clock).await;
/// }
/// ```
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        time::Instant::now()
    }

    fn sleep(&self, d: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(time::sleep(d))
    }
}

/// Paused clock (test environment)
///
/// This implementation works with tokio's `time::pause()` mechanism to enable
/// deterministic control over time progression. Time only advances when explicitly
/// told to via the `advance()` method, making tests fast and reproducible.
///
/// # Important Notes
///
/// 1. Must use `#[tokio::test(start_paused = true)]` attribute for automatic pause
/// 2. Alternatively, call `time::pause()` manually before creating PausedClock
/// 3. All time advancement is explicit via `advance()` - no real waiting occurs
/// 4. `sleep()` returns immediately in paused mode (controlled by tokio runtime)
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use tokio::time::Duration;
/// use tos_testing_framework::orchestrator::clock::{Clock, PausedClock};
///
/// #[tokio::test(start_paused = true)]
/// async fn test_timeout_logic() {
///     let clock = Arc::new(PausedClock::new());
///
///     // Simulate timeout after 10 seconds
///     let timeout = Duration::from_secs(10);
///     let start = clock.now();
///
///     // Advance time by 5 seconds - not timed out yet
///     clock.advance(Duration::from_secs(5)).await;
///     assert!(clock.now() - start < timeout);
///
///     // Advance time by another 6 seconds - now timed out
///     clock.advance(Duration::from_secs(6)).await;
///     assert!(clock.now() - start > timeout);
/// }
/// ```
///
/// # Testing Block Timestamps
///
/// ```rust
/// use std::sync::Arc;
/// use tokio::time::Duration;
/// use tos_testing_framework::orchestrator::clock::{Clock, PausedClock};
///
/// #[tokio::test(start_paused = true)]
/// async fn test_block_timestamp_validation() {
///     let clock = Arc::new(PausedClock::new());
///
///     // Create block at T=0
///     let block_time = clock.now();
///
///     // Advance to T+600s (10 minutes)
///     clock.advance(Duration::from_secs(600)).await;
///
///     // Verify block is not too old (within 1 hour)
///     let age = clock.now() - block_time;
///     assert!(age < Duration::from_secs(3600));
/// }
/// ```
pub struct PausedClock;

impl PausedClock {
    /// Creates a new PausedClock and pauses tokio time
    ///
    /// This constructor calls `time::pause()` to enable deterministic time control.
    /// If you're using `#[tokio::test(start_paused = true)]`, time is already paused,
    /// so this is just a convenience.
    ///
    /// # Examples
    ///
    /// ## With test attribute (recommended)
    ///
    /// ```rust
    /// #[tokio::test(start_paused = true)]  // Time already paused
    /// async fn test_example() {
    ///     let clock = PausedClock::new();
    ///     // ... test code ...
    /// }
    /// ```
    ///
    /// ## Manual pause
    ///
    /// ```rust
    /// #[tokio::test]
    /// async fn test_manual() {
    ///     let clock = PausedClock::new();  // Calls time::pause() internally
    ///     // ... test code ...
    /// }
    /// ```
    pub fn new() -> Self {
        time::pause();
        Self
    }

    /// Manually advance time by the specified duration
    ///
    /// This is the key method for controlling time in tests. It advances the
    /// tokio runtime's internal clock without any real delay. Any pending
    /// `sleep()` futures that expire during this advancement will be woken up.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::sync::Arc;
    /// use tokio::time::Duration;
    /// use tos_testing_framework::orchestrator::clock::{Clock, PausedClock};
    ///
    /// #[tokio::test(start_paused = true)]
    /// async fn test_time_advancement() {
    ///     let clock = Arc::new(PausedClock::new());
    ///     let start = clock.now();
    ///
    ///     // Advance by 1 hour
    ///     clock.advance(Duration::from_secs(3600)).await;
    ///
    ///     assert_eq!(clock.now() - start, Duration::from_secs(3600));
    /// }
    /// ```
    ///
    /// ## Testing with concurrent sleeps
    ///
    /// ```rust
    /// use std::sync::Arc;
    /// use tokio::time::Duration;
    /// use tos_testing_framework::orchestrator::clock::{Clock, PausedClock};
    ///
    /// #[tokio::test(start_paused = true)]
    /// async fn test_concurrent_timeouts() {
    ///     let clock = Arc::new(PausedClock::new());
    ///
    ///     // Spawn two tasks with different timeouts
    ///     let clock1 = clock.clone();
    ///     let task1 = tokio::spawn(async move {
    ///         clock1.sleep(Duration::from_secs(5)).await;
    ///         "task1 done"
    ///     });
    ///
    ///     let clock2 = clock.clone();
    ///     let task2 = tokio::spawn(async move {
    ///         clock2.sleep(Duration::from_secs(10)).await;
    ///         "task2 done"
    ///     });
    ///
    ///     // Advance by 6 seconds - task1 completes, task2 still waiting
    ///     clock.advance(Duration::from_secs(6)).await;
    ///     assert_eq!(task1.await.unwrap(), "task1 done");
    ///
    ///     // Advance by 5 more seconds - task2 completes
    ///     clock.advance(Duration::from_secs(5)).await;
    ///     assert_eq!(task2.await.unwrap(), "task2 done");
    /// }
    /// ```
    pub async fn advance(&self, d: Duration) {
        time::advance(d).await
    }
}

impl Clock for PausedClock {
    fn now(&self) -> Instant {
        // Returns the current simulated time (paused, only advances via advance())
        time::Instant::now()
    }

    fn sleep(&self, d: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        // In paused mode, tokio::time::sleep does not actually wait.
        // It cooperates with the paused runtime and returns instantly
        // unless time is advanced past the sleep duration.
        Box::pin(time::sleep(d))
    }
}

impl Default for PausedClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_paused_clock_advancement() {
        let clock = Arc::new(PausedClock::new());
        let start = clock.now();

        // Advance by 1 second
        clock.advance(Duration::from_secs(1)).await;
        assert_eq!(clock.now() - start, Duration::from_secs(1));

        // Advance by another 2 seconds
        clock.advance(Duration::from_secs(2)).await;
        assert_eq!(clock.now() - start, Duration::from_secs(3));
    }

    #[tokio::test]
    async fn test_paused_clock_sleep() {
        let clock = Arc::new(PausedClock::new());
        let start = clock.now();

        // Create a sleep future
        let sleep_task = {
            let clock_clone = clock.clone();
            tokio::spawn(async move {
                clock_clone.sleep(Duration::from_millis(100)).await;
            })
        };

        // Give the sleep task a moment to register
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Manually advance past the sleep duration
        clock.advance(Duration::from_millis(150)).await;

        // Sleep should complete
        sleep_task.await.unwrap();

        let elapsed = clock.now() - start;
        assert!(elapsed >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_system_clock() {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let start = clock.now();

        // Real sleep for 10ms
        clock.sleep(Duration::from_millis(10)).await;

        let elapsed = clock.now() - start;
        // Should have actually waited
        assert!(elapsed >= Duration::from_millis(10));
    }

    // ============================================================================
    // Additional Clock Tests for V3.0 Coverage
    // ============================================================================

    #[tokio::test]
    async fn test_paused_clock_zero_advancement() {
        let clock = Arc::new(PausedClock::new());
        let start = clock.now();

        // Advance by zero duration
        clock.advance(Duration::from_secs(0)).await;

        assert_eq!(clock.now() - start, Duration::from_secs(0));
    }

    #[tokio::test]
    async fn test_paused_clock_large_advancement() {
        let clock = Arc::new(PausedClock::new());
        let start = clock.now();

        // Advance by a very large duration (1 year)
        let one_year = Duration::from_secs(365 * 24 * 3600);
        clock.advance(one_year).await;

        assert_eq!(clock.now() - start, one_year);
    }

    #[tokio::test]
    async fn test_paused_clock_multiple_advancements() {
        let clock = Arc::new(PausedClock::new());
        let start = clock.now();

        // Advance multiple times in small increments
        for i in 1..=10 {
            clock.advance(Duration::from_secs(1)).await;
            let elapsed = clock.now() - start;
            assert_eq!(elapsed, Duration::from_secs(i));
        }
    }

    #[tokio::test]
    async fn test_paused_clock_concurrent_reads() {
        let clock = Arc::new(PausedClock::new());

        // Spawn multiple tasks reading time concurrently
        let mut handles = vec![];
        for _ in 0..10 {
            let clock_clone = clock.clone();
            handles.push(tokio::spawn(async move {
                let _instant = clock_clone.now();
            }));
        }

        // All should complete without issues
        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_paused_clock_sleep_expires_correctly() {
        let clock = Arc::new(PausedClock::new());

        let sleep_duration = Duration::from_secs(5);
        let clock_clone = clock.clone();

        let sleep_task = tokio::spawn(async move {
            clock_clone.sleep(sleep_duration).await;
            42
        });

        // Give task time to register
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Advance past the sleep duration
        clock.advance(sleep_duration + Duration::from_secs(1)).await;

        // Task should complete
        let result = sleep_task.await.unwrap();
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn test_paused_clock_multiple_concurrent_sleeps() {
        let clock = Arc::new(PausedClock::new());

        // Create multiple sleep tasks with different durations
        let clock1 = clock.clone();
        let task1 = tokio::spawn(async move {
            clock1.sleep(Duration::from_secs(1)).await;
            1
        });

        let clock2 = clock.clone();
        let task2 = tokio::spawn(async move {
            clock2.sleep(Duration::from_secs(3)).await;
            2
        });

        let clock3 = clock.clone();
        let task3 = tokio::spawn(async move {
            clock3.sleep(Duration::from_secs(5)).await;
            3
        });

        // Give tasks time to register (need more time for tokio runtime)
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Advance by 2 seconds - task1 should complete
        clock.advance(Duration::from_secs(2)).await;
        tokio::task::yield_now().await; // Allow task to finish

        // Advance by 2 more seconds - task2 should complete
        clock.advance(Duration::from_secs(2)).await;
        tokio::task::yield_now().await;

        // Advance by 2 more seconds - task3 should complete
        clock.advance(Duration::from_secs(2)).await;
        tokio::task::yield_now().await;

        // Verify results
        assert_eq!(task1.await.unwrap(), 1);
        assert_eq!(task2.await.unwrap(), 2);
        assert_eq!(task3.await.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_paused_clock_default_trait() {
        let clock = PausedClock::default();
        let start = clock.now();

        clock.advance(Duration::from_secs(1)).await;
        assert_eq!(clock.now() - start, Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_system_clock_now_monotonic() {
        let clock = SystemClock;

        let time1 = clock.now();
        let time2 = clock.now();
        let time3 = clock.now();

        // Time should be monotonically increasing (or equal)
        assert!(time2 >= time1);
        assert!(time3 >= time2);
    }

    #[tokio::test]
    async fn test_system_clock_sleep_accuracy() {
        let clock = Arc::new(SystemClock);
        let start = clock.now();

        // Sleep for 50ms
        clock.sleep(Duration::from_millis(50)).await;

        let elapsed = clock.now() - start;
        // Should be at least 50ms, but allow some overhead
        assert!(elapsed >= Duration::from_millis(50));
        assert!(elapsed < Duration::from_millis(100)); // Reasonable upper bound
    }

    #[tokio::test]
    async fn test_clock_trait_object_usage() {
        // Test that we can use Clock as a trait object
        let clocks: Vec<Arc<dyn Clock>> = vec![Arc::new(SystemClock), Arc::new(PausedClock::new())];

        for clock in clocks {
            let _instant = clock.now();
            // Clock trait object works correctly
        }
    }

    #[tokio::test]
    async fn test_paused_clock_precise_timing() {
        let clock = Arc::new(PausedClock::new());
        let start = clock.now();

        // Test precise microsecond advancement
        clock.advance(Duration::from_micros(100)).await;
        assert_eq!(clock.now() - start, Duration::from_micros(100));

        // Test precise nanosecond advancement
        clock.advance(Duration::from_nanos(500)).await;
        assert_eq!(
            clock.now() - start,
            Duration::from_micros(100) + Duration::from_nanos(500)
        );
    }

    #[tokio::test]
    async fn test_paused_clock_instant_comparison() {
        let clock = Arc::new(PausedClock::new());

        let instant1 = clock.now();
        clock.advance(Duration::from_secs(5)).await;
        let instant2 = clock.now();

        // instant2 should be later than instant1
        assert!(instant2 > instant1);
        assert_eq!(instant2 - instant1, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_paused_clock_duration_arithmetic() {
        let clock = Arc::new(PausedClock::new());
        let start = clock.now();

        // Advance by various durations
        clock.advance(Duration::from_secs(10)).await;
        clock.advance(Duration::from_millis(500)).await;
        clock.advance(Duration::from_micros(250)).await;

        let total_expected =
            Duration::from_secs(10) + Duration::from_millis(500) + Duration::from_micros(250);

        let elapsed = clock.now() - start;
        assert_eq!(elapsed, total_expected);
    }

    #[tokio::test]
    async fn test_paused_clock_shared_across_tasks() {
        let clock = Arc::new(PausedClock::new());

        // Create multiple tasks using the same clock
        let clock1 = clock.clone();
        let clock2 = clock.clone();

        let start1 = clock1.now();
        let start2 = clock2.now();

        // Advance time
        clock.advance(Duration::from_secs(10)).await;

        // Both clones should see the same advanced time
        let elapsed1 = clock1.now() - start1;
        let elapsed2 = clock2.now() - start2;
        assert_eq!(elapsed1, elapsed2);
        assert_eq!(elapsed1, Duration::from_secs(10));
    }
}
