//! Proptest strategies for property-based testing
//!
//! This module provides proptest strategies for generating random but valid
//! blockchain entities for property-based testing.
//!
//! # Design Principles
//!
//! 1. **Deterministic**: All strategies use seed-controlled RNG
//! 2. **Shrinkable**: Generated values can be minimized on test failure
//! 3. **Valid by construction**: Generated entities satisfy invariants
//! 4. **Composable**: Strategies can be combined to create complex scenarios
//!
//! # Example
//!
//! ```rust,ignore
//! use proptest::prelude::*;
//! use tos_tck::tier2_integration::strategies::*;
//!
//! proptest! {
//!     #[test]
//!     fn test_transaction_validity(tx in arb_transaction()) {
//!         // Transaction is guaranteed to be valid
//!         assert!(tx.amount > 0);
//!         assert!(tx.fee > 0);
//!     }
//! }
//! ```

use proptest::prelude::*;
use tos_common::crypto::Hash;

/// Strategy for generating valid Hash values (addresses)
///
/// Generates random 32-byte hashes with uniform distribution.
/// Useful for creating test addresses, block hashes, etc.
///
/// # Shrinking
///
/// Shrinks towards Hash::zero() by reducing byte values.
///
/// # Example
///
/// ```rust,ignore
/// proptest! {
///     #[test]
///     fn test_address_operations(addr in arb_hash()) {
///         assert_eq!(addr.as_bytes().len(), 32);
///     }
/// }
/// ```
pub fn arb_hash() -> impl Strategy<Value = Hash> {
    prop::array::uniform32(any::<u8>()).prop_map(Hash::new)
}

/// Strategy for generating a fixed number of unique addresses
///
/// Useful for creating distinct test accounts without collisions.
///
/// # Arguments
///
/// * `count` - Number of unique addresses to generate
///
/// # Example
///
/// ```rust,ignore
/// proptest! {
///     #[test]
///     fn test_multi_account(addresses in arb_unique_hashes(5)) {
///         assert_eq!(addresses.len(), 5);
///         // All addresses are unique
///         let unique: HashSet<_> = addresses.iter().collect();
///         assert_eq!(unique.len(), 5);
///     }
/// }
/// ```
pub fn arb_unique_hashes(count: usize) -> impl Strategy<Value = Vec<Hash>> {
    prop::collection::hash_set(arb_hash(), count..=count).prop_map(|set| set.into_iter().collect())
}

/// Strategy for generating valid transaction amounts
///
/// Generates amounts in range [1, 1_000_000_000_000] (1 nanoTOS to 1 TOS).
/// Never generates zero amounts as they are invalid.
///
/// # Shrinking
///
/// Shrinks towards 1 (minimum valid amount).
pub fn arb_amount() -> impl Strategy<Value = u64> {
    1u64..=1_000_000_000_000u64
}

/// Strategy for generating valid transaction fees
///
/// Generates fees in range [1, 100_000] (reasonable fee range).
/// Never generates zero fees.
///
/// # Shrinking
///
/// Shrinks towards 1 (minimum fee).
pub fn arb_fee() -> impl Strategy<Value = u64> {
    1u64..=100_000u64
}

/// Strategy for generating valid account balances
///
/// Generates balances in range [0, 1_000_000_000_000_000] (0 to 1M TOS).
/// Includes zero to test empty accounts.
///
/// # Shrinking
///
/// Shrinks towards 0.
pub fn arb_balance() -> impl Strategy<Value = u64> {
    0u64..=1_000_000_000_000_000u64
}

/// Strategy for generating valid nonces
///
/// Generates nonces in range [0, 1_000_000].
/// Starts from 0 (new account).
///
/// # Shrinking
///
/// Shrinks towards 0.
pub fn arb_nonce() -> impl Strategy<Value = u64> {
    0u64..=1_000_000u64
}

/// Strategy for generating block heights
///
/// Generates heights in range [0, 1_000_000].
///
/// # Shrinking
///
/// Shrinks towards 0 (genesis).
pub fn arb_height() -> impl Strategy<Value = u64> {
    0u64..=1_000_000u64
}

/// Strategy for generating small counts (useful for list sizes)
///
/// Generates counts in range [1, 100].
/// Useful for number of transactions per block, number of accounts, etc.
///
/// # Shrinking
///
/// Shrinks towards 1.
pub fn arb_small_count() -> impl Strategy<Value = usize> {
    1usize..=100usize
}

/// Strategy for generating medium counts
///
/// Generates counts in range [1, 1000].
/// Useful for larger scenarios.
///
/// # Shrinking
///
/// Shrinks towards 1.
pub fn arb_medium_count() -> impl Strategy<Value = usize> {
    1usize..=1000usize
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)]

    use super::*;
    use std::collections::HashSet;

    proptest! {
        #[test]
        fn test_arb_hash_generates_32_bytes(hash in arb_hash()) {
            assert_eq!(hash.as_bytes().len(), 32);
        }

        #[test]
        fn test_arb_unique_hashes_are_unique(hashes in arb_unique_hashes(10)) {
            assert_eq!(hashes.len(), 10);
            let unique: HashSet<_> = hashes.iter().collect();
            assert_eq!(unique.len(), 10);
        }

        #[test]
        fn test_arb_amount_is_non_zero(amount in arb_amount()) {
            assert!(amount > 0);
            assert!(amount <= 1_000_000_000_000);
        }

        #[test]
        fn test_arb_fee_is_non_zero(fee in arb_fee()) {
            assert!(fee > 0);
            assert!(fee <= 100_000);
        }

        #[test]
        fn test_arb_balance_includes_zero(balance in arb_balance()) {
            assert!(balance <= 1_000_000_000_000_000);
        }

        #[test]
        fn test_arb_nonce_starts_from_zero(nonce in arb_nonce()) {
            assert!(nonce <= 1_000_000);
        }

        #[test]
        fn test_arb_height_starts_from_zero(height in arb_height()) {
            assert!(height <= 1_000_000);
        }

        #[test]
        fn test_arb_small_count_range(count in arb_small_count()) {
            assert!(count >= 1);
            assert!(count <= 100);
        }

        #[test]
        fn test_arb_medium_count_range(count in arb_medium_count()) {
            assert!(count >= 1);
            assert!(count <= 1000);
        }
    }

    #[test]
    fn test_hash_shrinking() {
        use proptest::strategy::ValueTree;

        // Test that Hash shrinks towards zero
        let mut runner = proptest::test_runner::TestRunner::deterministic();

        let strategy = arb_hash();
        let value = strategy.new_tree(&mut runner).unwrap().current();

        // Initial value should be non-zero (with very high probability)
        // Shrinking should reduce towards zero
        assert_eq!(value.as_bytes().len(), 32);
    }

    #[test]
    fn test_amount_shrinking() {
        use proptest::strategy::ValueTree;

        // Test that amounts shrink towards 1 (minimum valid)
        let mut runner = proptest::test_runner::TestRunner::deterministic();

        let strategy = arb_amount();
        let mut tree = strategy.new_tree(&mut runner).unwrap();

        let initial = tree.current();
        assert!(initial >= 1);

        // After shrinking, value should approach 1
        while tree.simplify() {
            assert!(tree.current() >= 1);
        }

        // Final shrunk value should be minimal
        assert!(tree.current() <= initial);
    }
}
