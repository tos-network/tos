// A simple module to define the time types used in the project
//
// IMPORTANT SECURITY NOTE:
// The functions in this module use SystemTime::now() which is NON-DETERMINISTIC
// and should NEVER be used for consensus-critical operations.
//
// SAFE USAGE:
// - Logging timestamps
// - Metrics collection
// - Cache TTL management
// - Network admission control (with generous time buffers)
//
// UNSAFE USAGE (NEVER DO THIS):
// - Block validation that affects consensus outcome
// - Transaction ordering
// - Difficulty adjustment calculations
// - Any computation that must be deterministic across all nodes
//
// For consensus operations, always use block timestamps from the blockchain itself.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Millis timestamps used to determine it using its type
pub type TimestampMillis = u64;

// Seconds timestamps used to determine it using its type
pub type TimestampSeconds = u64;

#[inline]
pub fn get_current_time() -> Duration {
    let start = SystemTime::now();

    start
        .duration_since(UNIX_EPOCH)
        .expect("Incorrect time returned from get_current_time")
}

// Return timestamp in seconds
// SAFETY: Non-consensus operation - uses system time
// Only use for logging, metrics, or admission control (not deterministic consensus)
pub fn get_current_time_in_seconds() -> TimestampSeconds {
    get_current_time().as_secs()
}

// Return timestamp in milliseconds
// SAFETY: Non-consensus operation - uses system time
// Only use for logging, metrics, or admission control (not deterministic consensus)
// We cast it to u64 as we have plenty of time before it overflows (year 584,942,417 AD)
pub fn get_current_time_in_millis() -> TimestampMillis {
    get_current_time().as_millis() as TimestampMillis
}
