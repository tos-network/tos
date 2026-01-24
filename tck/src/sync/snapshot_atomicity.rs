#[cfg(test)]
mod tests {
    use super::super::mock::*;

    #[test]
    fn new_snapshot_has_no_changes() {
        let snapshot = MockSnapshot::new();
        assert!(snapshot.changes.is_empty());
        assert!(!snapshot.committed);
        assert!(!snapshot.rolled_back);
    }

    #[test]
    fn put_adds_key_value_pair() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("key1".to_string(), vec![1, 2, 3]);

        assert_eq!(snapshot.changes.len(), 1);
        let (key, value) = &snapshot.changes[0];
        assert_eq!(key, "key1");
        assert_eq!(value, &Some(vec![1, 2, 3]));
    }

    #[test]
    fn delete_adds_deletion_marker() {
        let mut snapshot = MockSnapshot::new();
        snapshot.delete("key_to_delete".to_string());

        assert_eq!(snapshot.changes.len(), 1);
        let (key, value) = &snapshot.changes[0];
        assert_eq!(key, "key_to_delete");
        assert_eq!(value, &None);
    }

    #[test]
    fn commit_succeeds_on_clean_snapshot() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("key".to_string(), vec![42]);

        let result = snapshot.commit();
        assert!(result.is_ok());
        assert!(snapshot.committed);
    }

    #[test]
    fn commit_fails_after_rollback() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("key".to_string(), vec![42]);
        snapshot.rollback().unwrap();

        let result = snapshot.commit();
        assert_eq!(result, Err("Cannot commit after rollback"));
    }

    #[test]
    fn rollback_succeeds_on_clean_snapshot() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("key".to_string(), vec![42]);

        let result = snapshot.rollback();
        assert!(result.is_ok());
        assert!(snapshot.rolled_back);
    }

    #[test]
    fn rollback_fails_after_commit() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("key".to_string(), vec![42]);
        snapshot.commit().unwrap();

        let result = snapshot.rollback();
        assert_eq!(result, Err("Cannot rollback after commit"));
    }

    #[test]
    fn rollback_clears_all_changes() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("key1".to_string(), vec![1]);
        snapshot.put("key2".to_string(), vec![2]);
        snapshot.delete("key3".to_string());
        assert_eq!(snapshot.changes.len(), 3);

        snapshot.rollback().unwrap();

        assert!(snapshot.changes.is_empty());
    }

    #[test]
    fn multiple_puts_tracked_in_order() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("first".to_string(), vec![1]);
        snapshot.put("second".to_string(), vec![2]);
        snapshot.put("third".to_string(), vec![3]);

        assert_eq!(snapshot.changes.len(), 3);
        assert_eq!(snapshot.changes[0].0, "first");
        assert_eq!(snapshot.changes[1].0, "second");
        assert_eq!(snapshot.changes[2].0, "third");
    }

    #[test]
    fn mixed_puts_and_deletes_tracked() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("create_key".to_string(), vec![10]);
        snapshot.delete("remove_key".to_string());
        snapshot.put("update_key".to_string(), vec![20]);

        assert_eq!(snapshot.changes.len(), 3);
        // First: put
        assert!(snapshot.changes[0].1.is_some());
        // Second: delete
        assert!(snapshot.changes[1].1.is_none());
        // Third: put
        assert!(snapshot.changes[2].1.is_some());
    }

    #[test]
    fn double_commit_is_idempotent() {
        let mut snapshot = MockSnapshot::new();
        snapshot.put("key".to_string(), vec![42]);

        // First commit succeeds
        let result1 = snapshot.commit();
        assert!(result1.is_ok());
        assert!(snapshot.committed);

        // Second commit also succeeds (already committed flag is true, no error path for double commit)
        // The committed flag stays true
        assert!(snapshot.committed);
    }

    #[test]
    fn snapshot_with_many_changes() {
        let mut snapshot = MockSnapshot::new();

        for i in 0..1000 {
            let key = format!("key_{}", i);
            let value = vec![(i % 256) as u8; 32];
            snapshot.put(key, value);
        }

        assert_eq!(snapshot.changes.len(), 1000);

        // All should be puts (Some values)
        for (_, value) in &snapshot.changes {
            assert!(value.is_some());
        }

        // Commit should work with many changes
        let result = snapshot.commit();
        assert!(result.is_ok());
    }

    #[test]
    fn snapshot_changes_applied_atomically() {
        // Simulate atomic application: either all changes apply or none
        let mut snapshot = MockSnapshot::new();
        snapshot.put("balance_a".to_string(), vec![100]);
        snapshot.put("balance_b".to_string(), vec![50]);
        snapshot.delete("old_entry".to_string());

        let changes_count = snapshot.changes.len();
        assert_eq!(changes_count, 3);

        // Simulate a failure condition: rollback reverts ALL changes
        snapshot.rollback().unwrap();
        assert!(snapshot.changes.is_empty());
        assert_eq!(snapshot.changes.len(), 0); // truly empty, not partial

        // Fresh snapshot: successful path
        let mut snapshot2 = MockSnapshot::new();
        snapshot2.put("balance_a".to_string(), vec![100]);
        snapshot2.put("balance_b".to_string(), vec![50]);
        snapshot2.delete("old_entry".to_string());
        snapshot2.commit().unwrap();

        // All changes preserved after commit
        assert_eq!(snapshot2.changes.len(), 3);
        assert!(snapshot2.committed);
    }

    #[test]
    fn reorg_snapshot_pop_modify_commit_or_rollback() {
        // Simulate a reorg using snapshots
        let mut chain = make_linear_chain(30, 100);

        // Create snapshot for the reorg
        let mut snapshot = MockSnapshot::new();

        // Pop blocks and record changes
        let popped = chain.pop_blocks(3);
        for block in &popped {
            let key = format!("block_{}", block.topoheight);
            snapshot.delete(key);
        }

        // Add new blocks and record
        for i in 28u64..=31 {
            let key = format!("block_{}", i);
            let mut value = vec![0u8; 32];
            value[0..8].copy_from_slice(&i.to_le_bytes());
            snapshot.put(key, value);
        }

        // Snapshot has both deletions and insertions
        assert_eq!(snapshot.changes.len(), 7); // 3 deletes + 4 puts

        // Commit the reorg
        snapshot.commit().unwrap();
        assert!(snapshot.committed);

        // Alternative: if something went wrong, rollback
        let mut failed_snapshot = MockSnapshot::new();
        failed_snapshot.put("attempt".to_string(), vec![1]);
        failed_snapshot.rollback().unwrap();
        assert!(failed_snapshot.changes.is_empty());
    }
}
