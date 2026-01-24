// Tests for concurrency limits, packet size validation, and timeout enforcement.
// Validates rate limiting behavior using MockRateLimiter.

#[cfg(test)]
mod tests {
    use super::super::mock::*;

    // =========================================================================
    // Test 1: Concurrent requests limited to PEER_OBJECTS_CONCURRENCY (64)
    // =========================================================================
    #[test]
    fn test_concurrent_requests_limited_to_64() {
        let mut limiter = MockRateLimiter::new();

        assert_eq!(limiter.max_concurrent, PEER_OBJECTS_CONCURRENCY);
        assert_eq!(limiter.max_concurrent, 64);

        // Acquire all 64 slots
        for i in 0..PEER_OBJECTS_CONCURRENCY {
            assert!(
                limiter.try_acquire(),
                "Should acquire slot {} of {}",
                i + 1,
                PEER_OBJECTS_CONCURRENCY
            );
        }

        assert_eq!(limiter.concurrent_requests, 64);
    }

    // =========================================================================
    // Test 2: Request beyond concurrency limit is blocked
    // =========================================================================
    #[test]
    fn test_request_beyond_concurrency_limit_blocked() {
        let mut limiter = MockRateLimiter::new();

        // Fill all slots
        for _ in 0..PEER_OBJECTS_CONCURRENCY {
            limiter.try_acquire();
        }

        // The 65th request should fail
        assert!(!limiter.try_acquire());
        assert_eq!(limiter.concurrent_requests, 64);

        // Multiple additional attempts should all fail
        assert!(!limiter.try_acquire());
        assert!(!limiter.try_acquire());
        assert_eq!(limiter.concurrent_requests, 64);
    }

    // =========================================================================
    // Test 3: Completed request frees concurrency slot
    // =========================================================================
    #[test]
    fn test_completed_request_frees_slot() {
        let mut limiter = MockRateLimiter::new();

        // Fill all slots
        for _ in 0..PEER_OBJECTS_CONCURRENCY {
            limiter.try_acquire();
        }
        assert!(!limiter.try_acquire()); // Full

        // Release one slot
        limiter.release();
        assert_eq!(limiter.concurrent_requests, 63);

        // Now one more can be acquired
        assert!(limiter.try_acquire());
        assert_eq!(limiter.concurrent_requests, 64);

        // Full again
        assert!(!limiter.try_acquire());
    }

    // =========================================================================
    // Test 4: Packet size within limit (< 5MB) accepted
    // =========================================================================
    #[test]
    fn test_packet_size_within_limit_accepted() {
        let limiter = MockRateLimiter::new();

        assert!(limiter.validate_packet_size(1).is_ok());
        assert!(limiter.validate_packet_size(1024).is_ok());
        assert!(limiter.validate_packet_size(1024 * 1024).is_ok()); // 1 MB
        assert!(limiter.validate_packet_size(4 * 1024 * 1024).is_ok()); // 4 MB
        assert!(limiter
            .validate_packet_size(PEER_MAX_PACKET_SIZE - 1)
            .is_ok());
    }

    // =========================================================================
    // Test 5: Packet size at exactly limit accepted
    // =========================================================================
    #[test]
    fn test_packet_size_at_exactly_limit_accepted() {
        let limiter = MockRateLimiter::new();

        assert_eq!(PEER_MAX_PACKET_SIZE, 5 * 1024 * 1024);
        assert!(limiter.validate_packet_size(PEER_MAX_PACKET_SIZE).is_ok());
    }

    // =========================================================================
    // Test 6: Packet size exceeding limit (> 5MB) rejected
    // =========================================================================
    #[test]
    fn test_packet_size_exceeding_limit_rejected() {
        let limiter = MockRateLimiter::new();

        let result = limiter.validate_packet_size(PEER_MAX_PACKET_SIZE + 1);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Packet size exceeds maximum");

        // Much larger sizes also rejected
        assert!(limiter.validate_packet_size(10 * 1024 * 1024).is_err());
        assert!(limiter.validate_packet_size(u32::MAX).is_err());
    }

    // =========================================================================
    // Test 7: Zero-size packet rejected
    // =========================================================================
    #[test]
    fn test_zero_size_packet_rejected() {
        let limiter = MockRateLimiter::new();

        let result = limiter.validate_packet_size(0);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Packet size is zero");
    }

    // =========================================================================
    // Test 8: Object request timeout at 15 seconds
    // =========================================================================
    #[test]
    fn test_object_request_timeout_15_seconds() {
        let mut limiter = MockRateLimiter::new();
        let request_id = 100u64;
        let start_ms = 5000u64;
        let deadline = start_ms + PEER_TIMEOUT_REQUEST_OBJECT;

        assert_eq!(PEER_TIMEOUT_REQUEST_OBJECT, 15_000);

        limiter.register_timeout(request_id, deadline);

        // Not timed out before deadline
        assert!(!limiter.is_timed_out(request_id, start_ms));
        assert!(!limiter.is_timed_out(request_id, start_ms + 14_999));

        // Timed out at and after deadline
        assert!(limiter.is_timed_out(request_id, deadline));
        assert!(limiter.is_timed_out(request_id, deadline + 1000));
    }

    // =========================================================================
    // Test 9: Bootstrap step timeout at 60 seconds
    // =========================================================================
    #[test]
    fn test_bootstrap_step_timeout_60_seconds() {
        let mut limiter = MockRateLimiter::new();
        let request_id = 200u64;
        let start_ms = 1000u64;
        let deadline = start_ms + PEER_TIMEOUT_BOOTSTRAP_STEP;

        assert_eq!(PEER_TIMEOUT_BOOTSTRAP_STEP, 60_000);

        limiter.register_timeout(request_id, deadline);

        // Not timed out before 60s
        assert!(!limiter.is_timed_out(request_id, start_ms));
        assert!(!limiter.is_timed_out(request_id, start_ms + 59_999));

        // Timed out at 60s
        assert!(limiter.is_timed_out(request_id, deadline));
    }

    // =========================================================================
    // Test 10: Connection init timeout at 5 seconds
    // =========================================================================
    #[test]
    fn test_connection_init_timeout_5_seconds() {
        let mut limiter = MockRateLimiter::new();
        let request_id = 300u64;
        let start_ms = 0u64;
        let deadline = start_ms + PEER_TIMEOUT_INIT_CONNECTION;

        assert_eq!(PEER_TIMEOUT_INIT_CONNECTION, 5_000);

        limiter.register_timeout(request_id, deadline);

        assert!(!limiter.is_timed_out(request_id, 4_999));
        assert!(limiter.is_timed_out(request_id, 5_000));
        assert!(limiter.is_timed_out(request_id, 5_001));
    }

    // =========================================================================
    // Test 11: Outgoing connection timeout at 30 seconds
    // =========================================================================
    #[test]
    fn test_outgoing_connection_timeout_30_seconds() {
        let mut limiter = MockRateLimiter::new();
        let request_id = 400u64;
        let start_ms = 2000u64;
        let deadline = start_ms + PEER_TIMEOUT_INIT_OUTGOING_CONNECTION;

        assert_eq!(PEER_TIMEOUT_INIT_OUTGOING_CONNECTION, 30_000);

        limiter.register_timeout(request_id, deadline);

        assert!(!limiter.is_timed_out(request_id, start_ms + 29_999));
        assert!(limiter.is_timed_out(request_id, start_ms + 30_000));
    }

    // =========================================================================
    // Test 12: Send timeout at 3 seconds (PEER_SEND_BYTES_TIMEOUT)
    // =========================================================================
    #[test]
    fn test_send_timeout_3_seconds() {
        let mut limiter = MockRateLimiter::new();
        let request_id = 500u64;
        let start_ms = 10_000u64;
        let deadline = start_ms + PEER_SEND_BYTES_TIMEOUT;

        assert_eq!(PEER_SEND_BYTES_TIMEOUT, 3_000);

        limiter.register_timeout(request_id, deadline);

        assert!(!limiter.is_timed_out(request_id, start_ms + 2_999));
        assert!(limiter.is_timed_out(request_id, start_ms + 3_000));
        assert!(limiter.is_timed_out(request_id, start_ms + 3_001));
    }

    // =========================================================================
    // Test 13: Multiple timeouts tracked independently
    // =========================================================================
    #[test]
    fn test_multiple_timeouts_tracked_independently() {
        let mut limiter = MockRateLimiter::new();

        // Register different timeouts for different requests
        let req_a = 1u64;
        let req_b = 2u64;
        let req_c = 3u64;

        let base_time = 1000u64;
        limiter.register_timeout(req_a, base_time + PEER_SEND_BYTES_TIMEOUT); // 4000
        limiter.register_timeout(req_b, base_time + PEER_TIMEOUT_INIT_CONNECTION); // 6000
        limiter.register_timeout(req_c, base_time + PEER_TIMEOUT_REQUEST_OBJECT); // 16000

        // At time 3500: none timed out
        assert!(!limiter.is_timed_out(req_a, 3500));
        assert!(!limiter.is_timed_out(req_b, 3500));
        assert!(!limiter.is_timed_out(req_c, 3500));

        // At time 4500: only req_a timed out
        assert!(limiter.is_timed_out(req_a, 4500));
        assert!(!limiter.is_timed_out(req_b, 4500));
        assert!(!limiter.is_timed_out(req_c, 4500));

        // At time 7000: req_a and req_b timed out, req_c still active
        assert!(limiter.is_timed_out(req_a, 7000));
        assert!(limiter.is_timed_out(req_b, 7000));
        assert!(!limiter.is_timed_out(req_c, 7000));

        // At time 20000: all timed out
        assert!(limiter.is_timed_out(req_a, 20000));
        assert!(limiter.is_timed_out(req_b, 20000));
        assert!(limiter.is_timed_out(req_c, 20000));
    }

    // =========================================================================
    // Test 14: Timeout cleanup on request completion
    // =========================================================================
    #[test]
    fn test_timeout_cleanup_on_completion() {
        let mut limiter = MockRateLimiter::new();

        let req_a = 10u64;
        let req_b = 20u64;

        limiter.register_timeout(req_a, 5000);
        limiter.register_timeout(req_b, 8000);

        assert_eq!(limiter.request_timeouts.len(), 2);

        // Complete request A (clear its timeout)
        limiter.clear_timeout(req_a);

        assert_eq!(limiter.request_timeouts.len(), 1);
        assert!(!limiter.request_timeouts.contains_key(&req_a));
        assert!(limiter.request_timeouts.contains_key(&req_b));

        // A should no longer report timeout (not tracked)
        assert!(!limiter.is_timed_out(req_a, 10000));

        // B should still report timeout
        assert!(limiter.is_timed_out(req_b, 10000));

        // Complete request B
        limiter.clear_timeout(req_b);
        assert!(limiter.request_timeouts.is_empty());
    }

    // =========================================================================
    // Test 15: Key rotation triggered at 1GB threshold
    // =========================================================================
    #[test]
    fn test_key_rotation_at_1gb_threshold() {
        let mut limiter = MockRateLimiter::new();

        assert_eq!(ROTATE_EVERY_N_BYTES, 1_073_741_824); // 1 GB

        // Below threshold: no rotation needed
        let needs_rotation = limiter.add_bytes_sent(500_000_000); // 500 MB
        assert!(!needs_rotation);
        assert_eq!(limiter.bytes_sent, 500_000_000);

        // Still below threshold
        let needs_rotation = limiter.add_bytes_sent(500_000_000); // Total: 1000 MB
        assert!(!needs_rotation);
        assert_eq!(limiter.bytes_sent, 1_000_000_000);

        // At threshold: rotation needed
        let needs_rotation = limiter.add_bytes_sent(73_741_824); // Total: 1 GB exactly
        assert!(needs_rotation);
        assert_eq!(limiter.bytes_sent, ROTATE_EVERY_N_BYTES as u64);

        // Reset after rotation
        limiter.reset_bytes_counter();
        assert_eq!(limiter.bytes_sent, 0);

        // Can accumulate again
        let needs_rotation = limiter.add_bytes_sent(100_000);
        assert!(!needs_rotation);
        assert_eq!(limiter.bytes_sent, 100_000);

        // Exceeding threshold in one shot also triggers rotation
        limiter.reset_bytes_counter();
        let needs_rotation = limiter.add_bytes_sent(2_000_000_000); // 2 GB in one shot
        assert!(needs_rotation);
    }
}
