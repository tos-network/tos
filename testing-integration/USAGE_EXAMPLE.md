# Genesis-Funded Accounts Helper - Usage Example

## Overview

The new `create_test_storage_with_funded_accounts()` helper function eliminates the need to mine 300+ blocks to get funded accounts in TOS integration tests. This is inspired by Kaspa's genesis UTXO initialization pattern.

## Performance Comparison

### OLD APPROACH: Mining 300+ blocks
```rust
// Create storage
let storage = create_test_rocksdb_storage().await;

// Mine 300+ blocks to accumulate balance
// This could take 30+ seconds per test!
for _ in 0..300 {
    mine_block(&storage, &miner_keypair).await?;
}

// Now accounts have funds to test with
```

**Time**: ~30-60 seconds per test (depending on block complexity)

### NEW APPROACH: Genesis-funded accounts
```rust
use tos_testing_integration::create_test_storage_with_funded_accounts;
use tos_common::config::COIN_VALUE;

// Create storage with 10 accounts, each with 1000 TOS
let (storage, keypairs) = create_test_storage_with_funded_accounts(10, 1000 * COIN_VALUE)
    .await
    .unwrap();

// Ready immediately! No mining needed.
let alice = &keypairs[0];
let bob = &keypairs[1];
```

**Time**: ~0.3 seconds (100x faster!)

## API Examples

### Example 1: Create N Random Accounts

```rust
use tos_testing_integration::create_test_storage_with_funded_accounts;
use tos_common::config::COIN_VALUE;

#[tokio::test]
async fn test_parallel_transfers() {
    // Create 50 accounts with 1000 TOS each
    let (storage, keypairs) = create_test_storage_with_funded_accounts(50, 1000 * COIN_VALUE)
        .await
        .unwrap();

    // All accounts are funded at genesis (topoheight 0)
    // Immediately ready for transactions!

    let alice = &keypairs[0];
    let bob = &keypairs[1];

    // Create and execute transactions...
}
```

### Example 2: Fund Specific Keypairs

```rust
use tos_testing_integration::utils::storage_helpers::{
    create_test_rocksdb_storage,
    fund_accounts_at_genesis,
};
use tos_common::crypto::KeyPair;
use tos_common::config::COIN_VALUE;

#[tokio::test]
async fn test_with_specific_accounts() {
    let storage = create_test_rocksdb_storage().await;

    // Create specific keypairs
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let charlie = KeyPair::new();

    // Fund with different balances
    fund_accounts_at_genesis(&storage, &[
        (alice.get_public_key().compress(), 10000 * COIN_VALUE),
        (bob.get_public_key().compress(), 5000 * COIN_VALUE),
        (charlie.get_public_key().compress(), 2000 * COIN_VALUE),
    ])
    .await
    .unwrap();

    // Ready for testing!
}
```

### Example 3: Add More Accounts to Existing Storage

```rust
use tos_testing_integration::{
    create_test_storage_with_funded_accounts,
    fund_accounts_at_genesis,
};
use tos_common::crypto::KeyPair;
use tos_common::config::COIN_VALUE;

#[tokio::test]
async fn test_incremental_funding() {
    // Create initial accounts
    let (storage, initial_keypairs) = create_test_storage_with_funded_accounts(5, 1000 * COIN_VALUE)
        .await
        .unwrap();

    // Add more specific accounts later
    let special_account = KeyPair::new();
    fund_accounts_at_genesis(&storage, &[
        (special_account.get_public_key().compress(), 50000 * COIN_VALUE),
    ])
    .await
    .unwrap();

    // All 6 accounts are now available
}
```

## Real-World Test Migration Example

### Before (with mining):
```rust
#[tokio::test]
#[ignore] // Takes too long, marked as ignored
async fn test_complex_transaction_flow() {
    let storage = create_test_rocksdb_storage().await;
    let miner = KeyPair::new();

    // Mine 300 blocks to get funds (30+ seconds)
    for _ in 0..300 {
        mine_block(&storage, &miner).await.unwrap();
    }

    // Test logic...
}
```

### After (with genesis funding):
```rust
#[tokio::test]
// No longer ignored - runs in 0.3s!
async fn test_complex_transaction_flow() {
    let (storage, keypairs) = create_test_storage_with_funded_accounts(10, 1000 * COIN_VALUE)
        .await
        .unwrap();

    // Test logic immediately...
}
```

## Implementation Details

The helper functions:

1. **create_test_storage_with_funded_accounts(count, balance)**
   - Creates RocksDB storage
   - Registers TOS asset at genesis
   - Generates `count` random keypairs
   - Sets initial balance and nonce (0) at topoheight 0
   - Returns (storage, keypairs)

2. **fund_accounts_at_genesis(storage, accounts)**
   - Funds specific accounts at topoheight 0
   - Useful when you need control over keypairs
   - Can be called multiple times to add more accounts

## Key Benefits

1. **100x Faster Tests**: 0.3s vs 30s per test
2. **No Mining Infrastructure**: Simplified test setup
3. **Production-Like Storage**: Uses RocksDB (matches production)
4. **Flexible API**: Random accounts or specific keypairs
5. **Kaspa-Inspired**: Proven pattern from mature GHOSTDAG implementation

## Migration Path

To migrate existing tests:

1. Replace `mine_blocks()` calls with `create_test_storage_with_funded_accounts()`
2. Remove `#[ignore]` attributes from slow tests
3. Adjust balance amounts as needed
4. Run tests and verify 100x speedup!

## Notes

- All accounts are funded at **topoheight 0** (genesis)
- Initial nonce is always **0**
- Uses **RocksDB** storage (production default)
- No delays or flushes needed (unlike Sled)
- Thread-safe and deadlock-free

## Test Results

```
running 10 tests
test utils::storage_helpers::rocksdb_tests::test_create_rocksdb_storage ... ok
test utils::storage_helpers::rocksdb_tests::test_create_test_storage_with_zero_accounts ... ok
test utils::storage_helpers::rocksdb_tests::test_create_test_storage_with_funded_accounts_different_balances ... ok
test utils::storage_helpers::rocksdb_tests::test_create_test_storage_with_funded_accounts ... ok
test utils::storage_helpers::rocksdb_tests::test_fund_accounts_at_genesis_with_existing_storage ... ok
test utils::storage_helpers::rocksdb_tests::test_fund_accounts_at_genesis ... ok
test utils::storage_helpers::rocksdb_tests::test_create_rocksdb_storage_with_accounts ... ok
test utils::storage_helpers::rocksdb_tests::test_create_test_storage_with_large_number_of_accounts ... ok
test utils::storage_helpers::rocksdb_tests::test_rocksdb_no_deadlock_immediate_use ... ok
test utils::storage_helpers::rocksdb_tests::test_setup_account_rocksdb ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.28s
```

All tests pass in **0.28 seconds** for 10 tests, including one that creates **50 accounts**!

## References

- **Kaspa genesis pattern**: `rusty-kaspa/consensus/core/src/config/genesis.rs`
- **TOS migration summary**: `~/tos-network/tos/ROCKSDB_MIGRATION_SUMMARY.md`
- **Test helpers**: `testing-integration/src/utils/storage_helpers.rs`
