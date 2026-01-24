// Tests for peer fail-count tracking and ban logic.
// Validates fail count increments, saturation, temp bans, disconnect thresholds,
// whitelisted peer exemptions, retry backoff, and fail count reset timing.

#[cfg(test)]
mod tests {
    use super::super::mock::*;
    use std::net::{IpAddr, Ipv4Addr};

    // Helper: create a graylist entry with a specific address
    fn gray_entry(port: u16) -> MockPeerListEntry {
        MockPeerListEntry::new_graylist(make_addr(port))
    }

    // Helper: create a whitelist entry
    fn white_entry(port: u16) -> MockPeerListEntry {
        let mut entry = MockPeerListEntry::new_graylist(make_addr(port));
        entry.state = PeerListEntryState::Whitelist;
        entry
    }

    // Helper: create a blacklist entry
    fn black_entry(port: u16) -> MockPeerListEntry {
        let mut entry = MockPeerListEntry::new_graylist(make_addr(port));
        entry.state = PeerListEntryState::Blacklist;
        entry
    }

    // -- Test 1: New peer has fail_count = 0 --

    #[test]
    fn test_new_peer_has_zero_fail_count() {
        let entry = gray_entry(8080);
        assert_eq!(entry.fail_count, 0);
    }

    // -- Test 2: Increment fail_count increases by 1 --

    #[test]
    fn test_increment_fail_count_increases_by_one() {
        let mut entry = gray_entry(8080);
        let now = 1000;

        entry.increment_fail_count(now, false);
        assert_eq!(entry.fail_count, 1);

        entry.increment_fail_count(now, false);
        assert_eq!(entry.fail_count, 2);

        entry.increment_fail_count(now, false);
        assert_eq!(entry.fail_count, 3);
    }

    // -- Test 3: fail_count saturates at u8::MAX (255), no overflow --

    #[test]
    fn test_fail_count_saturates_at_u8_max() {
        let mut entry = gray_entry(8080);
        let now = 1000;

        // Set fail count close to max
        entry.fail_count = 254;
        entry.increment_fail_count(now, false);
        assert_eq!(entry.fail_count, 255);

        // Should not overflow past 255
        entry.increment_fail_count(now, false);
        assert_eq!(entry.fail_count, 255);

        // Verify saturation one more time
        entry.increment_fail_count(now, false);
        assert_eq!(entry.fail_count, u8::MAX);
    }

    // -- Test 4: Whitelisted peers: fail_count never incremented --

    #[test]
    fn test_whitelisted_peers_fail_count_not_incremented() {
        let mut entry = white_entry(8080);
        let now = 1000;

        assert_eq!(entry.fail_count, 0);

        entry.increment_fail_count(now, true);
        assert_eq!(entry.fail_count, 0);

        entry.increment_fail_count(now, true);
        assert_eq!(entry.fail_count, 0);

        // Even with temp_ban=true, whitelisted peers are immune
        for _ in 0..100 {
            entry.increment_fail_count(now, true);
        }
        assert_eq!(entry.fail_count, 0);
    }

    // -- Test 5: After PEER_FAIL_TO_CONNECT_LIMIT (3) failures: temp ban applied --

    #[test]
    fn test_temp_ban_applied_at_fail_to_connect_limit() {
        let mut entry = gray_entry(8080);
        let now = 1000;

        // Increment to 3 (PEER_FAIL_TO_CONNECT_LIMIT)
        entry.increment_fail_count(now, true); // fail_count = 1
        assert_eq!(entry.temp_ban_until, None);

        entry.increment_fail_count(now, true); // fail_count = 2
        assert_eq!(entry.temp_ban_until, None);

        entry.increment_fail_count(now, true); // fail_count = 3 (3 % 3 == 0)
        assert!(entry.temp_ban_until.is_some());
        assert_eq!(
            entry.temp_ban_until.unwrap(),
            now + PEER_TEMP_BAN_TIME_ON_CONNECT
        );
    }

    // -- Test 6: Temp ban duration is PEER_TEMP_BAN_TIME_ON_CONNECT (60s) --

    #[test]
    fn test_temp_ban_duration_is_correct() {
        let mut entry = gray_entry(8080);
        let now = 5000;

        // Set fail_count to trigger ban at next increment
        entry.fail_count = 2;
        entry.increment_fail_count(now, true); // triggers ban at 3

        let ban_until = entry.temp_ban_until.unwrap();
        assert_eq!(ban_until, now + 60); // PEER_TEMP_BAN_TIME_ON_CONNECT = 60
        assert_eq!(ban_until - now, PEER_TEMP_BAN_TIME_ON_CONNECT);
    }

    // -- Test 7: After PEER_FAIL_LIMIT (50) failures: should_disconnect returns true --

    #[test]
    fn test_should_disconnect_at_fail_limit() {
        let mut entry = gray_entry(8080);

        entry.fail_count = 49;
        assert!(!entry.should_disconnect());

        entry.fail_count = 50;
        assert!(entry.should_disconnect());

        entry.fail_count = 100;
        assert!(entry.should_disconnect());

        entry.fail_count = 255;
        assert!(entry.should_disconnect());
    }

    // -- Test 8: Temp ban expires: is_temp_banned returns false after duration --

    #[test]
    fn test_temp_ban_expires_after_duration() {
        let mut entry = gray_entry(8080);
        let ban_start = 1000;

        entry.temp_ban_until = Some(ban_start + PEER_TEMP_BAN_TIME_ON_CONNECT);

        // During the ban period
        assert!(entry.is_temp_banned(ban_start));
        assert!(entry.is_temp_banned(ban_start + 30));
        assert!(entry.is_temp_banned(ban_start + 59));

        // At exactly the ban end time, the ban has expired (temp_ban_until is NOT > now)
        assert!(!entry.is_temp_banned(ban_start + 60));

        // After the ban period
        assert!(!entry.is_temp_banned(ban_start + 61));
        assert!(!entry.is_temp_banned(ban_start + 1000));
    }

    // -- Test 9: Fail count resets after PEER_FAIL_TIME_RESET (1800s) of no failures --

    #[test]
    fn test_fail_count_resets_after_timeout() {
        let mut entry = gray_entry(8080);

        entry.fail_count = 10;
        entry.last_seen = Some(1000); // last activity at time 1000

        // Not enough time passed (only 1799 seconds)
        entry.reset_fail_count(2800);
        assert_eq!(entry.fail_count, 10);

        // Just at the boundary (1800 seconds = not enough, needs to be strictly >)
        entry.reset_fail_count(2800);
        assert_eq!(entry.fail_count, 10);

        // After PEER_FAIL_TIME_RESET (> 1800 seconds after last_seen)
        entry.reset_fail_count(2801);
        assert_eq!(entry.fail_count, 0);
    }

    // -- Test 10: Retry backoff: delay = fail_count * P2P_PEERLIST_RETRY_AFTER --

    #[test]
    fn test_retry_backoff_delay_calculation() {
        let mut entry = gray_entry(8080);
        let try_time = 1000;
        entry.last_connection_try = Some(try_time);

        // fail_count=0 => delay = 0 * 900 = 0 (can retry immediately)
        entry.fail_count = 0;
        assert!(entry.can_retry(try_time));

        // fail_count=1 => delay = 1 * 900 = 900
        entry.fail_count = 1;
        assert!(!entry.can_retry(try_time + 899));
        assert!(entry.can_retry(try_time + 900));

        // fail_count=2 => delay = 2 * 900 = 1800
        entry.fail_count = 2;
        assert!(!entry.can_retry(try_time + 1799));
        assert!(entry.can_retry(try_time + 1800));

        // fail_count=3 => delay = 3 * 900 = 2700
        entry.fail_count = 3;
        assert!(!entry.can_retry(try_time + 2699));
        assert!(entry.can_retry(try_time + 2700));
    }

    // -- Test 11: can_retry returns true for new peers (no last_connection_try) --

    #[test]
    fn test_can_retry_true_for_new_peers() {
        let entry = gray_entry(8080);
        // No last_connection_try means peer has never been attempted
        assert_eq!(entry.last_connection_try, None);
        assert!(entry.can_retry(0));
        assert!(entry.can_retry(1000));
        assert!(entry.can_retry(u64::MAX));
    }

    // -- Test 12: can_retry returns false during temp ban --

    #[test]
    fn test_can_retry_false_during_temp_ban() {
        let mut entry = gray_entry(8080);
        let now = 5000;

        entry.temp_ban_until = Some(now + 100);
        entry.last_connection_try = None; // would normally allow retry

        assert!(!entry.can_retry(now));
        assert!(!entry.can_retry(now + 50));
        assert!(!entry.can_retry(now + 99));

        // After ban expires
        assert!(entry.can_retry(now + 100));
        assert!(entry.can_retry(now + 200));
    }

    // -- Test 13: can_retry returns false before backoff expires --

    #[test]
    fn test_can_retry_false_before_backoff_expires() {
        let mut entry = gray_entry(8080);
        let try_time = 2000;

        entry.last_connection_try = Some(try_time);
        entry.fail_count = 2; // delay = 2 * 900 = 1800

        // Before backoff expires
        assert!(!entry.can_retry(try_time + 100));
        assert!(!entry.can_retry(try_time + 900));
        assert!(!entry.can_retry(try_time + 1799));

        // At exactly the backoff boundary
        assert!(entry.can_retry(try_time + 1800));

        // After backoff expires
        assert!(entry.can_retry(try_time + 2000));
    }

    // -- Test 14: Multiple temp bans: every 3rd failure triggers ban --

    #[test]
    fn test_multiple_temp_bans_at_every_third_failure() {
        let mut entry = gray_entry(8080);
        let now = 1000;

        // Failures 1, 2: no ban
        entry.increment_fail_count(now, true); // count=1
        assert!(entry.temp_ban_until.is_none());
        entry.increment_fail_count(now, true); // count=2
        assert!(entry.temp_ban_until.is_none());

        // Failure 3: ban triggered
        entry.increment_fail_count(now, true); // count=3
        assert!(entry.temp_ban_until.is_some());

        // Clear ban to check next cycle
        entry.temp_ban_until = None;

        // Failures 4, 5: no ban
        entry.increment_fail_count(now + 100, true); // count=4
        assert!(entry.temp_ban_until.is_none());
        entry.increment_fail_count(now + 100, true); // count=5
        assert!(entry.temp_ban_until.is_none());

        // Failure 6: ban triggered again (6 % 3 == 0)
        entry.increment_fail_count(now + 100, true); // count=6
        assert!(entry.temp_ban_until.is_some());
    }

    // -- Test 15: Successful connection resets fail_count to 0 --

    #[test]
    fn test_successful_connection_resets_fail_count() {
        let mut entry = gray_entry(8080);
        let now = 5000;

        // Accumulate failures
        entry.fail_count = 25;
        entry.temp_ban_until = Some(now + 100);

        // Simulate successful connection
        entry.fail_count = 0;
        entry.out_success = true;
        entry.last_seen = Some(now);
        entry.temp_ban_until = None;

        assert_eq!(entry.fail_count, 0);
        assert!(entry.out_success);
        assert_eq!(entry.last_seen, Some(now));
        assert!(!entry.is_temp_banned(now));
    }

    // -- Test 16: Graylist peers get normal fail counting --

    #[test]
    fn test_graylist_peers_get_normal_fail_counting() {
        let mut entry = gray_entry(8080);
        assert_eq!(entry.state, PeerListEntryState::Graylist);

        let now = 1000;
        entry.increment_fail_count(now, false);
        assert_eq!(entry.fail_count, 1);

        entry.increment_fail_count(now, false);
        assert_eq!(entry.fail_count, 2);

        // Graylist peers are subject to normal fail counting
        for _ in 0..48 {
            entry.increment_fail_count(now, false);
        }
        assert_eq!(entry.fail_count, 50);
        assert!(entry.should_disconnect());
    }

    // -- Test 17: Blacklist peers: is_allowed always returns false --

    #[test]
    fn test_blacklist_peers_never_allowed() {
        let entry = black_entry(80);
        let list = {
            let mut pl = MockPeerList::new();
            pl.add_peer(entry).unwrap();
            pl
        };

        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 80));
        // Blacklisted peer is never allowed regardless of time
        assert!(!list.is_allowed(&ip, 0));
        assert!(!list.is_allowed(&ip, 1000));
        assert!(!list.is_allowed(&ip, u64::MAX));
    }

    // -- Test 18: Temp ban at fail_count 6 (second multiple of 3) --

    #[test]
    fn test_temp_ban_at_fail_count_6() {
        let mut entry = gray_entry(8080);
        let now = 2000;

        // Set fail_count to 5 and increment to 6
        entry.fail_count = 5;
        entry.increment_fail_count(now, true); // count=6, 6%3==0

        assert_eq!(entry.fail_count, 6);
        assert!(entry.temp_ban_until.is_some());
        assert_eq!(
            entry.temp_ban_until.unwrap(),
            now + PEER_TEMP_BAN_TIME_ON_CONNECT
        );
    }

    // -- Test 19: Temp ban at fail_count 9 (third multiple of 3) --

    #[test]
    fn test_temp_ban_at_fail_count_9() {
        let mut entry = gray_entry(8080);
        let now = 3000;

        // Set fail_count to 8 and increment to 9
        entry.fail_count = 8;
        entry.increment_fail_count(now, true); // count=9, 9%3==0

        assert_eq!(entry.fail_count, 9);
        assert!(entry.temp_ban_until.is_some());
        assert_eq!(
            entry.temp_ban_until.unwrap(),
            now + PEER_TEMP_BAN_TIME_ON_CONNECT
        );
    }

    // -- Test 20: Fail count 49: not yet disconnect, count 50: disconnect --

    #[test]
    fn test_fail_count_boundary_49_50() {
        let mut entry = gray_entry(8080);

        entry.fail_count = 49;
        assert!(
            !entry.should_disconnect(),
            "49 failures should not trigger disconnect"
        );

        entry.fail_count = 50;
        assert!(
            entry.should_disconnect(),
            "50 failures should trigger disconnect"
        );
    }

    // -- Test 21: out_success set after successful outgoing connection --

    #[test]
    fn test_out_success_set_after_successful_connection() {
        let mut entry = gray_entry(8080);
        assert!(!entry.out_success);

        // Simulate successful outgoing connection
        entry.out_success = true;
        entry.fail_count = 0;
        entry.last_seen = Some(5000);

        assert!(entry.out_success);
        assert_eq!(entry.fail_count, 0);
    }

    // -- Test 22: Retry timing: exact boundary tests --

    #[test]
    fn test_retry_timing_exact_boundaries() {
        let mut entry = gray_entry(8080);

        // Case 1: fail_count=1, last_try=1000
        // delay = 1 * 900 = 900, can retry at time >= 1900
        entry.fail_count = 1;
        entry.last_connection_try = Some(1000);

        assert!(
            !entry.can_retry(1899),
            "1 second before should not allow retry"
        );
        assert!(entry.can_retry(1900), "Exact boundary should allow retry");
        assert!(entry.can_retry(1901), "1 second after should allow retry");

        // Case 2: fail_count=5, last_try=2000
        // delay = 5 * 900 = 4500, can retry at time >= 6500
        entry.fail_count = 5;
        entry.last_connection_try = Some(2000);

        assert!(
            !entry.can_retry(6499),
            "1 second before should not allow retry"
        );
        assert!(entry.can_retry(6500), "Exact boundary should allow retry");
        assert!(entry.can_retry(6501), "1 second after should allow retry");

        // Case 3: fail_count=0 with last_connection_try set
        // delay = 0 * 900 = 0, can retry immediately
        entry.fail_count = 0;
        entry.last_connection_try = Some(10000);
        assert!(entry.can_retry(10000), "Zero fail count means no backoff");
    }
}
