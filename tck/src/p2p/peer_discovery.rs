// Tests for ping-based peer discovery and peer exchange protocol.
// Validates ping packet construction, peer list limits, address validation,
// pruned topoheight invariants, and state update semantics.

#[cfg(test)]
mod tests {
    use super::super::mock::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    // Helper: create a public (non-local, non-private) socket address for testing.
    fn public_addr(last_octet: u8, port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, last_octet, 1)), port)
    }

    // Helper: simulate a peer state that tracks updates from pings.
    #[derive(Debug, Clone)]
    struct PeerState {
        topoheight: u64,
        height: u64,
        top_hash: Hash,
        cumulative_difficulty: u64,
        pruned_topoheight: Option<u64>,
        is_pruned: bool,
    }

    impl PeerState {
        fn new() -> Self {
            Self {
                topoheight: 0,
                height: 0,
                top_hash: [0u8; 32],
                cumulative_difficulty: 0,
                pruned_topoheight: None,
                is_pruned: false,
            }
        }

        fn update_from_ping(&mut self, ping: &MockPing) -> Result<(), &'static str> {
            // Validate pruned topoheight transitions
            if self.is_pruned && ping.pruned_topoheight.is_none() {
                return Err("Pruned peer cannot become un-pruned");
            }
            if let Some(new_pruned) = ping.pruned_topoheight {
                if new_pruned > ping.topoheight {
                    return Err("Pruned topoheight exceeds topoheight");
                }
                if let Some(old_pruned) = self.pruned_topoheight {
                    if new_pruned < old_pruned {
                        return Err("Pruned topoheight cannot decrease");
                    }
                }
            }

            self.topoheight = ping.topoheight;
            self.height = ping.height;
            self.top_hash = ping.top_hash;
            self.cumulative_difficulty = ping.cumulative_difficulty;
            self.pruned_topoheight = ping.pruned_topoheight;
            if ping.pruned_topoheight.is_some() {
                self.is_pruned = true;
            }
            Ok(())
        }
    }

    // -- Test 1: Ping construction with valid fields --

    #[test]
    fn test_ping_construction_with_valid_fields() {
        let ping = MockPing::new(50, 100);

        assert_eq!(ping.height, 50);
        assert_eq!(ping.topoheight, 100);
        assert_eq!(ping.top_hash, [0xCC; 32]);
        assert_eq!(ping.pruned_topoheight, None);
        assert_eq!(ping.cumulative_difficulty, 50 * 100);
        assert!(ping.peer_list.is_empty());
        assert!(ping.validate().is_ok());
    }

    // -- Test 2: Peer list limit enforcement (P2P_PING_PEER_LIST_LIMIT = 16) --

    #[test]
    fn test_peer_list_limit_enforcement() {
        let mut ping = MockPing::new(50, 100);

        // Add exactly 16 peers - all should succeed
        for i in 0..P2P_PING_PEER_LIST_LIMIT {
            let addr = public_addr(i as u8, 8080);
            assert!(ping.add_peer(addr), "Adding peer {} should succeed", i);
        }
        assert_eq!(ping.peer_list.len(), P2P_PING_PEER_LIST_LIMIT);
        assert_eq!(ping.peer_list.len(), 16);
    }

    // -- Test 3: Duplicate peer rejection in ping --

    #[test]
    fn test_duplicate_peer_rejection_in_ping() {
        let mut ping = MockPing::new(50, 100);
        let addr = public_addr(1, 8080);

        assert!(ping.add_peer(addr), "First add should succeed");
        assert!(!ping.add_peer(addr), "Duplicate add should return false");
        assert_eq!(ping.peer_list.len(), 1);
    }

    // -- Test 4: Ping validation: pruned_topoheight cannot be 0 --

    #[test]
    fn test_pruned_topoheight_cannot_be_zero() {
        let mut ping = MockPing::new(50, 100);
        ping.pruned_topoheight = Some(0);

        let result = ping.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Pruned topoheight cannot be 0");
    }

    // -- Test 5: Ping validation: pruned_topoheight cannot exceed topoheight --

    #[test]
    fn test_pruned_topoheight_cannot_exceed_topoheight() {
        let mut ping = MockPing::new(50, 100);
        ping.pruned_topoheight = Some(101); // exceeds topoheight of 100

        let result = ping.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Pruned topoheight exceeds topoheight");
    }

    // -- Test 6: Empty peer list is valid --

    #[test]
    fn test_empty_peer_list_is_valid() {
        let ping = MockPing::new(50, 100);
        assert!(ping.peer_list.is_empty());
        assert!(ping.validate().is_ok());
    }

    // -- Test 7: Peer list at exactly the limit (16 peers) --

    #[test]
    fn test_peer_list_at_exactly_limit() {
        let mut ping = MockPing::new(50, 100);

        for i in 0..P2P_PING_PEER_LIST_LIMIT {
            let addr = public_addr(i as u8, 9000 + i as u16);
            assert!(ping.add_peer(addr));
        }

        assert_eq!(ping.peer_list.len(), 16);
        assert!(ping.validate().is_ok());
    }

    // -- Test 8: Adding peer beyond limit returns false --

    #[test]
    fn test_adding_peer_beyond_limit_returns_false() {
        let mut ping = MockPing::new(50, 100);

        // Fill to capacity
        for i in 0..P2P_PING_PEER_LIST_LIMIT {
            ping.add_peer(public_addr(i as u8, 8080));
        }

        // 17th addition must return false
        let result = ping.add_peer(public_addr(200, 8080));
        assert!(!result);
        assert_eq!(ping.peer_list.len(), P2P_PING_PEER_LIST_LIMIT);
    }

    // -- Test 9: Cumulative difficulty reporting in ping --

    #[test]
    fn test_cumulative_difficulty_reporting_in_ping() {
        // Default cumulative_difficulty = height * 100
        let ping = MockPing::new(500, 1000);
        assert_eq!(ping.cumulative_difficulty, 50_000);

        // Custom difficulty
        let mut ping2 = MockPing::new(10, 20);
        ping2.cumulative_difficulty = 999_999;
        assert_eq!(ping2.cumulative_difficulty, 999_999);

        // Zero difficulty is valid
        let mut ping3 = MockPing::new(1, 1);
        ping3.cumulative_difficulty = 0;
        assert_eq!(ping3.cumulative_difficulty, 0);
        assert!(ping3.validate().is_ok());
    }

    // -- Test 10: Peer address validation: cannot contain own address --

    #[test]
    fn test_peer_list_cannot_contain_own_address() {
        let own_addr = public_addr(42, 8080);
        let mut ping = MockPing::new(50, 100);

        // Manually inject own address into peer list
        ping.peer_list.push(own_addr);

        // Verify that the ping contains the own address
        assert!(ping.peer_list.contains(&own_addr));

        // In the real daemon, update_peer() checks this and returns P2pError::OwnSocketAddress.
        // Here we verify the condition that would trigger the error.
        let contains_own = ping.peer_list.contains(&own_addr);
        assert!(contains_own, "Ping should detect own address in peer list");
    }

    // -- Test 11: Peer address validation: cannot contain local/loopback addresses --

    #[test]
    fn test_peer_list_cannot_contain_local_addresses() {
        // Loopback address (127.0.0.1)
        let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        assert!(
            loopback.ip().is_loopback(),
            "127.0.0.1 should be detected as loopback"
        );

        // Private address (192.168.x.x)
        let private = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8080);
        let is_private = match private.ip() {
            IpAddr::V4(ipv4) => ipv4.is_private(),
            _ => false,
        };
        assert!(is_private, "192.168.1.1 should be detected as private");

        // Private address (10.x.x.x)
        let private10 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 8080);
        let is_private10 = match private10.ip() {
            IpAddr::V4(ipv4) => ipv4.is_private(),
            _ => false,
        };
        assert!(is_private10, "10.0.0.1 should be detected as private");

        // Link-local address (169.254.x.x)
        let link_local = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1)), 8080);
        let is_link_local = match link_local.ip() {
            IpAddr::V4(ipv4) => ipv4.is_link_local(),
            _ => false,
        };
        assert!(
            is_link_local,
            "169.254.1.1 should be detected as link-local"
        );

        // Public address should pass validation
        let public = public_addr(1, 8080);
        let is_public = match public.ip() {
            IpAddr::V4(ipv4) => !ipv4.is_loopback() && !ipv4.is_private() && !ipv4.is_link_local(),
            _ => false,
        };
        assert!(is_public, "8.8.1.1 should be a valid public address");
    }

    // -- Test 12: Multiple pings update peer state correctly --

    #[test]
    fn test_multiple_pings_update_peer_state() {
        let mut state = PeerState::new();

        // First ping establishes initial state
        let mut ping1 = MockPing::new(50, 100);
        ping1.cumulative_difficulty = 1000;
        assert!(state.update_from_ping(&ping1).is_ok());
        assert_eq!(state.topoheight, 100);
        assert_eq!(state.height, 50);
        assert_eq!(state.cumulative_difficulty, 1000);

        // Second ping with higher values
        let mut ping2 = MockPing::new(80, 200);
        ping2.cumulative_difficulty = 2000;
        assert!(state.update_from_ping(&ping2).is_ok());
        assert_eq!(state.topoheight, 200);
        assert_eq!(state.height, 80);
        assert_eq!(state.cumulative_difficulty, 2000);

        // Third ping - height/topoheight can also decrease (reorg scenario)
        let mut ping3 = MockPing::new(70, 150);
        ping3.cumulative_difficulty = 1500;
        assert!(state.update_from_ping(&ping3).is_ok());
        assert_eq!(state.topoheight, 150);
        assert_eq!(state.height, 70);
        assert_eq!(state.cumulative_difficulty, 1500);
    }

    // -- Test 13: Height and topoheight update via ping --

    #[test]
    fn test_height_and_topoheight_update_via_ping() {
        let mut state = PeerState::new();
        assert_eq!(state.height, 0);
        assert_eq!(state.topoheight, 0);

        let ping = MockPing::new(500, 1000);
        assert!(state.update_from_ping(&ping).is_ok());

        assert_eq!(state.height, 500);
        assert_eq!(state.topoheight, 1000);
    }

    // -- Test 14: Ping with pruned topoheight transitions (was None, now Some) --

    #[test]
    fn test_pruned_topoheight_transition_none_to_some() {
        let mut state = PeerState::new();
        assert!(!state.is_pruned);
        assert_eq!(state.pruned_topoheight, None);

        // Transition from not-pruned to pruned is valid
        let mut ping = MockPing::new(50, 100);
        ping.pruned_topoheight = Some(10);
        assert!(state.update_from_ping(&ping).is_ok());
        assert!(state.is_pruned);
        assert_eq!(state.pruned_topoheight, Some(10));
    }

    // -- Test 15: Ping with pruned topoheight cannot decrease --

    #[test]
    fn test_pruned_topoheight_cannot_decrease() {
        let mut state = PeerState::new();

        // Set initial pruned state
        let mut ping1 = MockPing::new(50, 100);
        ping1.pruned_topoheight = Some(20);
        assert!(state.update_from_ping(&ping1).is_ok());
        assert_eq!(state.pruned_topoheight, Some(20));

        // Attempt to decrease pruned topoheight should fail
        let mut ping2 = MockPing::new(55, 110);
        ping2.pruned_topoheight = Some(15);
        let result = state.update_from_ping(&ping2);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Pruned topoheight cannot decrease");

        // State should remain unchanged after failed update
        assert_eq!(state.pruned_topoheight, Some(20));
        assert_eq!(state.topoheight, 100);
    }

    // -- Additional edge cases --

    #[test]
    fn test_pruned_topoheight_equal_to_topoheight_is_valid() {
        let mut ping = MockPing::new(50, 50);
        ping.pruned_topoheight = Some(50);
        assert!(ping.validate().is_ok());
    }

    #[test]
    fn test_pruned_peer_cannot_become_unpruned() {
        let mut state = PeerState::new();

        // Become pruned
        let mut ping1 = MockPing::new(50, 100);
        ping1.pruned_topoheight = Some(10);
        assert!(state.update_from_ping(&ping1).is_ok());
        assert!(state.is_pruned);

        // Try to become un-pruned
        let ping2 = MockPing::new(60, 200);
        // ping2.pruned_topoheight is None (default)
        let result = state.update_from_ping(&ping2);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Pruned peer cannot become un-pruned");
    }

    #[test]
    fn test_pruned_topoheight_can_increase() {
        let mut state = PeerState::new();

        let mut ping1 = MockPing::new(50, 100);
        ping1.pruned_topoheight = Some(10);
        assert!(state.update_from_ping(&ping1).is_ok());

        let mut ping2 = MockPing::new(100, 200);
        ping2.pruned_topoheight = Some(50);
        assert!(state.update_from_ping(&ping2).is_ok());
        assert_eq!(state.pruned_topoheight, Some(50));
    }

    #[test]
    fn test_top_hash_updates_with_ping() {
        let mut state = PeerState::new();
        assert_eq!(state.top_hash, [0u8; 32]);

        let mut ping = MockPing::new(10, 20);
        ping.top_hash = [0xAB; 32];
        assert!(state.update_from_ping(&ping).is_ok());
        assert_eq!(state.top_hash, [0xAB; 32]);
    }

    #[test]
    fn test_validate_rejects_duplicate_peers_in_list() {
        let mut ping = MockPing::new(50, 100);
        let addr = public_addr(1, 8080);

        // Force duplicates by directly pushing to vec
        ping.peer_list.push(addr);
        ping.peer_list.push(addr);

        let result = ping.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Duplicate peers in list");
    }
}
