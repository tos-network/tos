//! Account setup and management utilities for tests

use tos_common::crypto::{Hash, PublicKey};

use crate::storage::MockStorage;
use crate::TestResult;

/// Setup account with initial balance and nonce in MockStorage
///
/// This is the primary helper for test account setup. It avoids sled deadlock
/// issues by using in-memory MockStorage.
///
/// # Example
///
/// ```rust,ignore
/// use tos_testing_integration::{MockStorage, setup_account_mock};
/// use tos_common::crypto::PublicKey;
///
/// let storage = MockStorage::new_with_tos_asset();
/// let account = PublicKey::default();
///
/// setup_account_mock(&storage, &account, 1000, 0);
/// ```
pub fn setup_account_mock(storage: &MockStorage, account: &PublicKey, balance: u64, nonce: u64) {
    storage.setup_account(account, balance, nonce);
}

/// Setup account at specific topoheight
///
/// **Note**: The simplified MockStorage no longer supports topoheight versioning.
/// This function now ignores the topoheight parameter and calls the simple setup method.
/// For real topoheight-aware storage operations, use storage_helpers.rs with TestDaemon.
pub fn setup_account_mock_at_topoheight(
    storage: &MockStorage,
    account: &PublicKey,
    balance: u64,
    nonce: u64,
    _topoheight: u64,
) {
    storage.setup_account(account, balance, nonce);
}

/// Get balance from MockStorage
///
/// This is a convenience helper for reading balances in tests.
pub async fn get_balance_from_storage(
    storage: &MockStorage,
    account: &PublicKey,
    asset: &Hash,
) -> TestResult<u64> {
    Ok(storage.get_balance(account, asset))
}

/// Get nonce from MockStorage
pub async fn get_nonce_from_storage(storage: &MockStorage, account: &PublicKey) -> TestResult<u64> {
    Ok(storage.get_nonce(account))
}

/// Create multiple test accounts with balances
///
/// Returns vector of (PublicKey, balance, nonce) tuples.
///
/// # Example
///
/// ```rust,ignore
/// let storage = MockStorage::new_with_tos_asset();
/// let accounts = setup_multiple_accounts(&storage, vec![1000, 2000, 3000]);
///
/// // accounts[0] has balance 1000
/// // accounts[1] has balance 2000
/// // accounts[2] has balance 3000
/// ```
pub fn setup_multiple_accounts(storage: &MockStorage, balances: Vec<u64>) -> Vec<PublicKey> {
    use tos_common::serializer::{Reader, Serializer};

    balances
        .iter()
        .enumerate()
        .map(|(i, &balance)| {
            // Create deterministic test accounts
            let data = [i as u8; 32];
            let mut reader = Reader::new(&data);
            let account = tos_common::crypto::elgamal::CompressedPublicKey::read(&mut reader)
                .expect("Failed to create test pubkey");

            storage.setup_account(&account, balance, 0);
            account
        })
        .collect()
}
