//! Utility Functions
//!
//! Helper functions for integration testing

use std::time::Duration;
use tokio::time::sleep;

/// Wait for a condition with timeout
///
/// Polls the condition function until it returns true or timeout is reached
pub async fn wait_for_condition<F>(
    mut condition: F,
    timeout_secs: u64,
    poll_interval_ms: u64,
) -> bool
where
    F: FnMut() -> bool,
{
    let timeout_duration = Duration::from_secs(timeout_secs);
    let poll_duration = Duration::from_millis(poll_interval_ms);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        if condition() {
            return true;
        }
        sleep(poll_duration).await;
    }

    false
}

/// Wait for an async condition with timeout
pub async fn wait_for_async_condition<F, Fut>(
    mut condition: F,
    timeout_secs: u64,
    poll_interval_ms: u64,
) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let timeout_duration = Duration::from_secs(timeout_secs);
    let poll_duration = Duration::from_millis(poll_interval_ms);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        if condition().await {
            return true;
        }
        sleep(poll_duration).await;
    }

    false
}

/// Format bytes in human-readable form
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_idx])
}

/// Format duration in human-readable form
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();

    if secs < 60 {
        format!("{:.2}s", duration.as_secs_f64())
    } else if secs < 3600 {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        format!("{}m {}s", mins, remaining_secs)
    } else {
        let hours = secs / 3600;
        let remaining_mins = (secs % 3600) / 60;
        format!("{}h {}m", hours, remaining_mins)
    }
}

/// Generate random alphanumeric string
pub fn random_string(length: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();

    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wait_for_condition_success() {
        let mut counter = 0;

        let result = wait_for_condition(
            || {
                counter += 1;
                counter >= 3
            },
            5,
            100,
        )
        .await;

        assert!(result);
        assert!(counter >= 3);
    }

    #[tokio::test]
    async fn test_wait_for_condition_timeout() {
        let result = wait_for_condition(|| false, 1, 100).await;

        assert!(!result);
    }

    #[tokio::test]
    async fn test_wait_for_async_condition_success() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let counter = Arc::new(Mutex::new(0));
        let counter_clone = counter.clone();

        let result = wait_for_async_condition(
            move || {
                let counter = counter_clone.clone();
                async move {
                    let mut c = counter.lock().await;
                    *c += 1;
                    *c >= 3
                }
            },
            5,
            100,
        )
        .await;

        assert!(result);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(512), "512.00 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1_048_576), "1.00 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.00 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30.00s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3665)), "1h 1m");
    }

    #[test]
    fn test_random_string() {
        let s1 = random_string(10);
        let s2 = random_string(10);

        assert_eq!(s1.len(), 10);
        assert_eq!(s2.len(), 10);
        assert_ne!(s1, s2); // Extremely unlikely to be equal
    }
}
