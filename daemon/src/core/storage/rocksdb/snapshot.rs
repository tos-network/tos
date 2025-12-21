//! RocksDB-specific snapshot type alias.
//!
//! This module provides a type alias for the generic Snapshot type
//! specialized for RocksDB's Column type.

use super::Column;
use crate::core::storage::snapshot::Snapshot as GenericSnapshot;

/// Type alias for Snapshot specialized to RocksDB Column type
pub type Snapshot = GenericSnapshot<Column>;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    #[test]
    fn test_btreemap_prefix_iteration_behavior() {
        let mut map = BTreeMap::new();

        // Helper to encode a u64 prefix + suffix
        fn make_key(prefix: u64, suffix: &[u8]) -> Vec<u8> {
            let mut key = prefix.to_be_bytes().to_vec();
            key.extend_from_slice(suffix);
            key
        }

        // Insert test entries
        map.insert(make_key(0, b"zero"), b"value0".to_vec());
        map.insert(make_key(1, b"aaaa"), b"value1".to_vec());
        map.insert(make_key(2, b"bbbb"), b"value2".to_vec());

        // First test: iterator on range
        {
            let prefix = 1u64.to_be_bytes().to_vec();
            let results: Vec<_> = map
                .range(prefix..)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            let prefixes: Vec<u64> = results
                .iter()
                .map(|(k, _)| {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&k[..8]);
                    u64::from_be_bytes(buf)
                })
                .collect();

            assert_eq!(prefixes, vec![1, 2]);
            assert_eq!(results[0].1, b"value1");
            assert_eq!(results[1].1, b"value2");
        }

        // Second test: Reverse iteration starting at prefix
        {
            let prefix = 2u64.to_be_bytes().to_vec();
            let results: Vec<_> = map
                .range(..=prefix)
                .rev()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            let prefixes: Vec<u64> = results
                .iter()
                .map(|(k, _)| {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&k[..8]);
                    u64::from_be_bytes(buf)
                })
                .collect();

            assert_eq!(prefixes, vec![1, 0]);
            assert_eq!(results[0].1, b"value1");
            assert_eq!(results[1].1, b"value0");
        }

        // Third test: Only matching prefix (simulated prefix iteration)
        {
            let target_prefix = 1u64.to_be_bytes();

            let results: Vec<_> = map
                .iter()
                .filter(|(k, _)| k.starts_with(&target_prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            let prefixes: Vec<u64> = results
                .iter()
                .map(|(k, _)| {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&k[..8]);
                    u64::from_be_bytes(buf)
                })
                .collect();

            assert_eq!(prefixes, vec![1]);
            assert_eq!(results[0].1, b"value1");
        }
    }
}
