// Tests for Whitelist/Graylist/Blacklist peer list management.
// Validates add/remove operations, state transitions, capacity limits,
// allowed/banned status, and peer-to-connect selection logic.

#[cfg(test)]
mod tests {
    use super::super::mock::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    // Helper: create a graylist entry with a specific IP and port
    fn gray_entry_with_port(ip_last: u8, port: u16) -> MockPeerListEntry {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, ip_last)), port);
        let mut entry = MockPeerListEntry::new_graylist(addr);
        entry.local_port = Some(port);
        entry
    }

    // Helper: create a whitelist entry with out_success
    fn white_entry_success(ip_last: u8, port: u16) -> MockPeerListEntry {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, ip_last)), port);
        let mut entry = MockPeerListEntry::new_graylist(addr);
        entry.state = PeerListEntryState::Whitelist;
        entry.out_success = true;
        entry.local_port = Some(port);
        entry
    }

    // -- Test 1: New PeerList is empty --

    #[test]
    fn test_new_peer_list_is_empty() {
        let list = MockPeerList::new();
        assert!(list.peers.is_empty());
        assert_eq!(list.peers.len(), 0);
        assert_eq!(list.max_peers, P2P_DEFAULT_MAX_PEERS);
    }

    // -- Test 2: Add peer to empty list succeeds --

    #[test]
    fn test_add_peer_to_empty_list_succeeds() {
        let mut list = MockPeerList::new();
        let entry = gray_entry_with_port(1, 8080);

        let result = list.add_peer(entry);
        assert!(result.is_ok());
        assert_eq!(list.peers.len(), 1);
    }

    // -- Test 3: Add peer to full list (P2P_DEFAULT_MAX_PEERS=32) fails --

    #[test]
    fn test_add_peer_to_full_list_fails() {
        let mut list = MockPeerList::new();

        // Fill the list to capacity (32 peers)
        for i in 0..P2P_DEFAULT_MAX_PEERS {
            let entry = gray_entry_with_port(i as u8, 8080 + i as u16);
            list.add_peer(entry).unwrap();
        }
        assert_eq!(list.peers.len(), P2P_DEFAULT_MAX_PEERS);

        // Adding one more should fail
        let extra = gray_entry_with_port(200, 9999);
        let result = list.add_peer(extra);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Peer list full");
        assert_eq!(list.peers.len(), P2P_DEFAULT_MAX_PEERS);
    }

    // -- Test 4: Add duplicate peer fails --

    #[test]
    fn test_add_duplicate_peer_fails() {
        let mut list = MockPeerList::new();
        let entry1 = gray_entry_with_port(1, 8080);
        let entry2 = gray_entry_with_port(1, 8080); // Same IP

        list.add_peer(entry1).unwrap();
        let result = list.add_peer(entry2);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Peer already exists");
        assert_eq!(list.peers.len(), 1);
    }

    // -- Test 5: Remove peer from list returns entry --

    #[test]
    fn test_remove_peer_returns_entry() {
        let mut list = MockPeerList::new();
        let entry = gray_entry_with_port(1, 8080);
        let ip = entry.addr.ip();

        list.add_peer(entry).unwrap();
        assert_eq!(list.peers.len(), 1);

        let removed = list.remove_peer(&ip);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().addr.port(), 8080);
        assert_eq!(list.peers.len(), 0);
    }

    // -- Test 6: Remove non-existent peer returns None --

    #[test]
    fn test_remove_nonexistent_peer_returns_none() {
        let mut list = MockPeerList::new();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 99));

        let result = list.remove_peer(&ip);
        assert!(result.is_none());
    }

    // -- Test 7: Whitelist peer: state changes to Whitelist --

    #[test]
    fn test_whitelist_peer_changes_state() {
        let mut list = MockPeerList::new();
        let entry = gray_entry_with_port(1, 8080);
        let ip = entry.addr.ip();

        list.add_peer(entry).unwrap();

        // Peer starts as graylist
        assert_eq!(
            list.peers.get(&ip).unwrap().state,
            PeerListEntryState::Graylist
        );

        // Whitelist it
        list.whitelist(&ip).unwrap();
        assert_eq!(
            list.peers.get(&ip).unwrap().state,
            PeerListEntryState::Whitelist
        );
    }

    // -- Test 8: Blacklist peer: state changes to Blacklist --

    #[test]
    fn test_blacklist_peer_changes_state() {
        let mut list = MockPeerList::new();
        let entry = gray_entry_with_port(1, 8080);
        let ip = entry.addr.ip();

        list.add_peer(entry).unwrap();
        list.blacklist(&ip).unwrap();

        assert_eq!(
            list.peers.get(&ip).unwrap().state,
            PeerListEntryState::Blacklist
        );
    }

    // -- Test 9: Blacklisted peer: is_allowed returns false --

    #[test]
    fn test_blacklisted_peer_not_allowed() {
        let mut list = MockPeerList::new();
        let entry = gray_entry_with_port(1, 8080);
        let ip = entry.addr.ip();

        list.add_peer(entry).unwrap();
        list.blacklist(&ip).unwrap();

        assert!(!list.is_allowed(&ip, 0));
        assert!(!list.is_allowed(&ip, 1000));
        assert!(!list.is_allowed(&ip, u64::MAX));
    }

    // -- Test 10: Unknown peer (not in list): is_allowed returns true --

    #[test]
    fn test_unknown_peer_is_allowed() {
        let list = MockPeerList::new();
        let unknown_ip = IpAddr::V4(Ipv4Addr::new(99, 99, 99, 99));

        assert!(list.is_allowed(&unknown_ip, 0));
        assert!(list.is_allowed(&unknown_ip, 1000));
    }

    // -- Test 11: Temp-banned peer: is_allowed returns false --

    #[test]
    fn test_temp_banned_peer_not_allowed() {
        let mut list = MockPeerList::new();
        let mut entry = gray_entry_with_port(1, 8080);
        let ip = entry.addr.ip();
        entry.temp_ban_until = Some(5000);

        list.add_peer(entry).unwrap();

        // During ban
        assert!(!list.is_allowed(&ip, 4000));
        assert!(!list.is_allowed(&ip, 4999));

        // After ban expires
        assert!(list.is_allowed(&ip, 5000));
        assert!(list.is_allowed(&ip, 6000));
    }

    // -- Test 12: find_peer_to_connect: prefers whitelist with out_success --

    #[test]
    fn test_find_peer_prefers_whitelist_with_out_success() {
        let mut list = MockPeerList::new();
        let now = 10000;

        // Add a graylist peer
        let gray = gray_entry_with_port(1, 8080);
        list.add_peer(gray).unwrap();

        // Add a whitelist peer with out_success
        let white = white_entry_success(2, 9090);
        list.add_peer(white).unwrap();

        let result = list.find_peer_to_connect(now);
        assert!(result.is_some());

        // Should prefer the whitelist peer
        let addr = result.unwrap();
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)));
        assert_eq!(addr.port(), 9090);
    }

    // -- Test 13: find_peer_to_connect: falls back to graylist --

    #[test]
    fn test_find_peer_falls_back_to_graylist() {
        let mut list = MockPeerList::new();
        let now = 10000;

        // Add only graylist peers (no whitelist with out_success)
        let gray = gray_entry_with_port(1, 8080);
        list.add_peer(gray).unwrap();

        let result = list.find_peer_to_connect(now);
        assert!(result.is_some());

        let addr = result.unwrap();
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
    }

    // -- Test 14: find_peer_to_connect: skips blacklisted peers --

    #[test]
    fn test_find_peer_skips_blacklisted() {
        let mut list = MockPeerList::new();
        let now = 10000;

        // Add a blacklisted peer
        let mut entry = gray_entry_with_port(1, 8080);
        entry.state = PeerListEntryState::Blacklist;
        list.add_peer(entry).unwrap();

        let result = list.find_peer_to_connect(now);
        assert!(result.is_none(), "Blacklisted peers should be skipped");
    }

    // -- Test 15: find_peer_to_connect: skips temp-banned peers --

    #[test]
    fn test_find_peer_skips_temp_banned() {
        let mut list = MockPeerList::new();
        let now = 5000;

        // Add a temp-banned peer
        let mut entry = gray_entry_with_port(1, 8080);
        entry.temp_ban_until = Some(now + 100); // banned for 100 more seconds
        list.add_peer(entry).unwrap();

        let result = list.find_peer_to_connect(now);
        assert!(result.is_none(), "Temp-banned peers should be skipped");

        // After ban expires, peer should be findable
        let result_after = list.find_peer_to_connect(now + 100);
        assert!(result_after.is_some());
    }

    // -- Test 16: find_peer_to_connect: respects retry backoff --

    #[test]
    fn test_find_peer_respects_retry_backoff() {
        let mut list = MockPeerList::new();

        // Add a peer with fail_count=2, last_connection_try=1000
        // Backoff = 2 * 900 = 1800, can retry at time >= 2800
        let mut entry = gray_entry_with_port(1, 8080);
        entry.fail_count = 2;
        entry.last_connection_try = Some(1000);
        list.add_peer(entry).unwrap();

        // Before backoff expires
        let result = list.find_peer_to_connect(2799);
        assert!(
            result.is_none(),
            "Should not connect before backoff expires"
        );

        // After backoff expires
        let result = list.find_peer_to_connect(2800);
        assert!(result.is_some(), "Should connect after backoff expires");
    }

    // -- Test 17: get_whitelist/graylist/blacklist filtering --

    #[test]
    fn test_list_filtering_by_state() {
        let mut list = MockPeerList::new();

        // Add 2 whitelist, 3 graylist, 1 blacklist
        let mut w1 = gray_entry_with_port(1, 8001);
        w1.state = PeerListEntryState::Whitelist;
        list.add_peer(w1).unwrap();

        let mut w2 = gray_entry_with_port(2, 8002);
        w2.state = PeerListEntryState::Whitelist;
        list.add_peer(w2).unwrap();

        list.add_peer(gray_entry_with_port(3, 8003)).unwrap();
        list.add_peer(gray_entry_with_port(4, 8004)).unwrap();
        list.add_peer(gray_entry_with_port(5, 8005)).unwrap();

        let mut b1 = gray_entry_with_port(6, 8006);
        b1.state = PeerListEntryState::Blacklist;
        list.add_peer(b1).unwrap();

        assert_eq!(list.get_whitelist().len(), 2);
        assert_eq!(list.get_graylist().len(), 3);
        assert_eq!(list.get_blacklist().len(), 1);
        assert_eq!(list.peers.len(), 6);
    }

    // -- Test 18: Peer state transitions: Graylist -> Whitelist -> Blacklist --

    #[test]
    fn test_peer_state_transitions() {
        let mut list = MockPeerList::new();
        let entry = gray_entry_with_port(1, 8080);
        let ip = entry.addr.ip();

        list.add_peer(entry).unwrap();

        // Initial state: Graylist
        assert_eq!(
            list.peers.get(&ip).unwrap().state,
            PeerListEntryState::Graylist
        );
        assert!(list.is_allowed(&ip, 1000));

        // Transition: Graylist -> Whitelist
        list.whitelist(&ip).unwrap();
        assert_eq!(
            list.peers.get(&ip).unwrap().state,
            PeerListEntryState::Whitelist
        );
        assert!(list.is_allowed(&ip, 1000));

        // Transition: Whitelist -> Blacklist
        list.blacklist(&ip).unwrap();
        assert_eq!(
            list.peers.get(&ip).unwrap().state,
            PeerListEntryState::Blacklist
        );
        assert!(!list.is_allowed(&ip, 1000));
    }
}
