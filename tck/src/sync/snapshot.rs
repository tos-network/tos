// Layer 1: Real Snapshot Tests
//
// Tests the actual tos_daemon::core::storage::snapshot::Snapshot:
// - put/get/delete roundtrip
// - EntryState transitions (Absent → Stored → Deleted)
// - Overwrite behavior
// - Multiple columns isolation
// - clone_mut independence
// - contains/contains_key correctness
// - Changes struct behavior

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use tos_daemon::core::storage::snapshot::EntryState;
    use tos_daemon::core::storage::snapshot::Snapshot;
    use tos_daemon::core::storage::StorageCache;

    // Use u8 as a simple column type (implements Hash + Eq)
    type TestSnapshot = Snapshot<u8>;

    fn entry_state_name<T>(state: &EntryState<T>) -> &'static str {
        match state {
            EntryState::Stored(_) => "Stored",
            EntryState::Deleted => "Deleted",
            EntryState::Absent => "Absent",
        }
    }

    const COL_A: u8 = 0;
    const COL_B: u8 = 1;
    const COL_C: u8 = 2;

    fn make_snapshot() -> TestSnapshot {
        Snapshot::new(StorageCache::new(None))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Basic put/get roundtrip
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_put_then_get() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("value1"));

        match snap.get(COL_A, b"key1") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"value1"),
            other => panic!("Expected Stored, got {}", entry_state_name(&other)),
        }
    }

    #[test]
    fn test_snapshot_get_absent_key() {
        let snap = make_snapshot();
        match snap.get(COL_A, b"nonexistent") {
            EntryState::Absent => {}
            other => panic!("Expected Absent, got {}", entry_state_name(&other)),
        }
    }

    #[test]
    fn test_snapshot_put_overwrite() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("first"));
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("second"));

        match snap.get(COL_A, b"key1") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"second"),
            other => panic!(
                "Expected Stored with 'second', got {}",
                entry_state_name(&other)
            ),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Delete behavior
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_delete_existing_key() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("value1"));
        snap.delete(COL_A, Bytes::from("key1"));

        match snap.get(COL_A, b"key1") {
            EntryState::Deleted => {}
            other => panic!("Expected Deleted, got {}", entry_state_name(&other)),
        }
    }

    #[test]
    fn test_snapshot_delete_nonexistent_key() {
        let mut snap = make_snapshot();
        snap.delete(COL_A, Bytes::from("key1"));

        match snap.get(COL_A, b"key1") {
            EntryState::Deleted => {}
            other => panic!("Expected Deleted, got {}", entry_state_name(&other)),
        }
    }

    #[test]
    fn test_snapshot_put_after_delete_restores() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("original"));
        snap.delete(COL_A, Bytes::from("key1"));
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("restored"));

        match snap.get(COL_A, b"key1") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"restored"),
            other => panic!(
                "Expected Stored with 'restored', got {}",
                entry_state_name(&other)
            ),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Column isolation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_columns_are_isolated() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("value_a"));
        snap.put(COL_B, Bytes::from("key1"), Bytes::from("value_b"));

        match snap.get(COL_A, b"key1") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"value_a"),
            other => panic!(
                "Expected Stored for COL_A, got {}",
                entry_state_name(&other)
            ),
        }

        match snap.get(COL_B, b"key1") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"value_b"),
            other => panic!(
                "Expected Stored for COL_B, got {}",
                entry_state_name(&other)
            ),
        }

        // COL_C was never written
        match snap.get(COL_C, b"key1") {
            EntryState::Absent => {}
            other => panic!(
                "Expected Absent for COL_C, got {}",
                entry_state_name(&other)
            ),
        }
    }

    #[test]
    fn test_snapshot_delete_one_column_doesnt_affect_other() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("value_a"));
        snap.put(COL_B, Bytes::from("key1"), Bytes::from("value_b"));

        snap.delete(COL_A, Bytes::from("key1"));

        match snap.get(COL_A, b"key1") {
            EntryState::Deleted => {}
            other => panic!(
                "Expected Deleted for COL_A, got {}",
                entry_state_name(&other)
            ),
        }

        match snap.get(COL_B, b"key1") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"value_b"),
            other => panic!(
                "Expected Stored for COL_B, got {}",
                entry_state_name(&other)
            ),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // contains / contains_key
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_contains_stored_key() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("value1"));

        assert_eq!(snap.contains(COL_A, b"key1"), Some(true));
        assert!(snap.contains_key(COL_A, b"key1"));
    }

    #[test]
    fn test_snapshot_contains_deleted_key() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("value1"));
        snap.delete(COL_A, Bytes::from("key1"));

        assert_eq!(snap.contains(COL_A, b"key1"), Some(false));
        assert!(!snap.contains_key(COL_A, b"key1"));
    }

    #[test]
    fn test_snapshot_contains_absent_key() {
        let snap = make_snapshot();

        // Absent returns None (not in snapshot, must check disk)
        assert_eq!(snap.contains(COL_A, b"nonexistent"), None);
        // contains_key for absent returns false
        assert!(!snap.contains_key(COL_A, b"nonexistent"));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Multiple keys in same column
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_multiple_keys_same_column() {
        let mut snap = make_snapshot();
        for i in 0u8..10 {
            snap.put(COL_A, Bytes::from(vec![i]), Bytes::from(vec![i + 100]));
        }

        for i in 0u8..10 {
            match snap.get(COL_A, [i]) {
                EntryState::Stored(v) => assert_eq!(v.as_ref(), &[i + 100]),
                other => panic!(
                    "Expected Stored for key {}, got {}",
                    i,
                    entry_state_name(&other)
                ),
            }
        }
    }

    #[test]
    fn test_snapshot_partial_delete() {
        let mut snap = make_snapshot();
        for i in 0u8..5 {
            snap.put(COL_A, Bytes::from(vec![i]), Bytes::from(vec![i]));
        }

        // Delete only even keys
        snap.delete(COL_A, Bytes::from(vec![0u8]));
        snap.delete(COL_A, Bytes::from(vec![2u8]));
        snap.delete(COL_A, Bytes::from(vec![4u8]));

        assert_eq!(snap.contains(COL_A, [0u8]), Some(false));
        assert_eq!(snap.contains(COL_A, [1u8]), Some(true));
        assert_eq!(snap.contains(COL_A, [2u8]), Some(false));
        assert_eq!(snap.contains(COL_A, [3u8]), Some(true));
        assert_eq!(snap.contains(COL_A, [4u8]), Some(false));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // clone_mut independence
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_clone_mut_is_independent() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("original"));

        let mut cloned = snap.clone_mut();

        // Modify the clone
        cloned.put(COL_A, Bytes::from("key1"), Bytes::from("cloned_value"));
        cloned.put(COL_A, Bytes::from("key2"), Bytes::from("new_in_clone"));

        // Original should be unaffected
        match snap.get(COL_A, b"key1") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"original"),
            other => panic!(
                "Original should be unchanged, got {}",
                entry_state_name(&other)
            ),
        }
        match snap.get(COL_A, b"key2") {
            EntryState::Absent => {}
            other => panic!(
                "key2 should be absent in original, got {}",
                entry_state_name(&other)
            ),
        }

        // Clone should have new values
        match cloned.get(COL_A, b"key1") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"cloned_value"),
            other => panic!(
                "Clone should have cloned_value, got {}",
                entry_state_name(&other)
            ),
        }
        match cloned.get(COL_A, b"key2") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"new_in_clone"),
            other => panic!(
                "Clone should have new_in_clone, got {}",
                entry_state_name(&other)
            ),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // get_size
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_get_size_stored() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("hello"));

        match snap.get_size(COL_A, b"key1") {
            EntryState::Stored(size) => assert_eq!(size, 5),
            other => panic!("Expected Stored(5), got {}", entry_state_name(&other)),
        }
    }

    #[test]
    fn test_snapshot_get_size_deleted() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("hello"));
        snap.delete(COL_A, Bytes::from("key1"));

        match snap.get_size(COL_A, b"key1") {
            EntryState::Deleted => {}
            other => panic!("Expected Deleted, got {}", entry_state_name(&other)),
        }
    }

    #[test]
    fn test_snapshot_get_size_absent() {
        let snap = make_snapshot();
        match snap.get_size(COL_A, b"key1") {
            EntryState::Absent => {}
            other => panic!("Expected Absent, got {}", entry_state_name(&other)),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EntryState put return values (previous state)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_put_returns_previous_state_absent() {
        let mut snap = make_snapshot();
        let prev = snap.put(COL_A, Bytes::from("key1"), Bytes::from("value1"));
        assert!(matches!(prev, EntryState::Absent));
    }

    #[test]
    fn test_snapshot_put_returns_previous_state_stored() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("first"));
        let prev = snap.put(COL_A, Bytes::from("key1"), Bytes::from("second"));
        match prev {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"first"),
            other => panic!(
                "Expected Stored with 'first', got {}",
                entry_state_name(&other)
            ),
        }
    }

    #[test]
    fn test_snapshot_put_returns_previous_state_deleted() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("first"));
        snap.delete(COL_A, Bytes::from("key1"));
        let prev = snap.put(COL_A, Bytes::from("key1"), Bytes::from("restored"));
        assert!(matches!(prev, EntryState::Deleted));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EntryState delete return values
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_delete_returns_previous_stored() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from("value1"));
        let prev = snap.delete(COL_A, Bytes::from("key1"));
        match prev {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"value1"),
            other => panic!("Expected Stored, got {}", entry_state_name(&other)),
        }
    }

    #[test]
    fn test_snapshot_delete_returns_absent_for_new_key() {
        let mut snap = make_snapshot();
        let prev = snap.delete(COL_A, Bytes::from("key1"));
        assert!(matches!(prev, EntryState::Absent));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Empty value handling
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_empty_value() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from("key1"), Bytes::from(""));

        match snap.get(COL_A, b"key1") {
            EntryState::Stored(v) => assert!(v.is_empty()),
            other => panic!(
                "Expected Stored with empty value, got {}",
                entry_state_name(&other)
            ),
        }
        assert_eq!(snap.contains(COL_A, b"key1"), Some(true));
    }

    #[test]
    fn test_snapshot_empty_key() {
        let mut snap = make_snapshot();
        snap.put(COL_A, Bytes::from(""), Bytes::from("value_for_empty_key"));

        match snap.get(COL_A, b"") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), b"value_for_empty_key"),
            other => panic!("Expected Stored, got {}", entry_state_name(&other)),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Large data
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_snapshot_large_value() {
        let mut snap = make_snapshot();
        let large_value: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
        snap.put(COL_A, Bytes::from("big"), Bytes::from(large_value.clone()));

        match snap.get(COL_A, b"big") {
            EntryState::Stored(v) => assert_eq!(v.as_ref(), large_value.as_slice()),
            other => panic!(
                "Expected Stored with large value, got {}",
                entry_state_name(&other)
            ),
        }
    }

    #[test]
    fn test_snapshot_many_entries() {
        let mut snap = make_snapshot();
        for i in 0u32..1000 {
            let key = Bytes::from(i.to_le_bytes().to_vec());
            let value = Bytes::from((i * 2).to_le_bytes().to_vec());
            snap.put(COL_A, key, value);
        }

        for i in 0u32..1000 {
            let key = i.to_le_bytes();
            match snap.get(COL_A, key) {
                EntryState::Stored(v) => {
                    let expected = (i * 2).to_le_bytes();
                    assert_eq!(v.as_ref(), &expected);
                }
                other => panic!(
                    "Expected Stored for key {}, got {}",
                    i,
                    entry_state_name(&other)
                ),
            }
        }
    }
}
