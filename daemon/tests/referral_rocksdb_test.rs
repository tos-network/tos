//! RocksStorage Referral System Integration Tests
//!
//! These tests verify the RocksStorage implementation of the referral system,
//! specifically testing edge cases that were missed by MockReferralState tests:
//!
//! A. BindReferrer (RocksStorage integration)
//!    - Stub record does not block binding
//!    - Preserve direct_referrals_count when binding
//!    - Preserve team_size when binding
//!    - has_referrer semantics with stub records
//!
//! B. State transition ordering
//!    - Order-dependent path: become referrer → bind own upline
//!    - Rebinding prevention after stub
//!
//! C. Consistency assertions
//!    - List vs count consistency
//!
//! Reference: memo/13-Referral-System/Referral-Test-Analysis.md

#![allow(clippy::disallowed_methods)]

use tempdir::TempDir;
use tos_common::{
    crypto::{Hash, KeyPair, PublicKey},
    network::Network,
};
use tos_daemon::core::{
    config::RocksDBConfig,
    storage::{
        rocksdb::{CacheMode, CompressionMode, RocksStorage},
        ReferralProvider,
    },
};

/// Create a RocksDBConfig with test defaults
fn test_rocksdb_config() -> RocksDBConfig {
    RocksDBConfig {
        parallelism: 2,
        max_background_jobs: 2,
        max_subcompaction_jobs: 1,
        low_priority_background_threads: 1,
        max_open_files: 100,
        keep_max_log_files: 1,
        compression_mode: CompressionMode::None,
        cache_mode: CacheMode::None,
        cache_size: 1024 * 1024,
        write_buffer_size: 1024 * 1024,
        write_buffer_shared: false,
    }
}

/// Create a test RocksStorage instance
fn create_test_storage(temp_dir: &TempDir) -> RocksStorage {
    let config = test_rocksdb_config();
    RocksStorage::new(
        temp_dir.path().to_str().unwrap(),
        Network::Devnet,
        &config,
    )
}

/// Generate a random public key for testing
fn random_pubkey() -> PublicKey {
    KeyPair::new().get_public_key().compress()
}

// ============================================================================
// A. BindReferrer (RocksStorage integration)
// ============================================================================

/// Test A.1: Stub record does not block binding
///
/// Scenario:
/// 1. Alice binds Bob as referrer (creates stub record for Bob)
/// 2. Bob should still be able to bind his own referrer (Charlie)
///
/// This tests the fix for: has_referrer checks referrer.is_some(), not record existence
#[tokio::test]
async fn test_stub_record_does_not_block_binding() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    let alice = random_pubkey();
    let bob = random_pubkey();
    let charlie = random_pubkey();

    let height = 100;
    let timestamp = 1000;

    // Step 1: Alice binds Bob as referrer
    // This creates a stub record for Bob (with referrer = None)
    storage
        .bind_referrer(&alice, &bob, height, Hash::zero(), timestamp)
        .await
        .expect("Alice should bind Bob as referrer");

    // Verify Alice has referrer
    assert!(
        storage.has_referrer(&alice).await.unwrap(),
        "Alice should have a referrer"
    );

    // Step 2: Bob should NOT have a referrer (stub record has referrer = None)
    assert!(
        !storage.has_referrer(&bob).await.unwrap(),
        "Bob should NOT have a referrer (only has stub record)"
    );

    // Step 3: Bob binds Charlie as his referrer
    // This should succeed even though Bob has a stub record
    storage
        .bind_referrer(&bob, &charlie, height + 1, Hash::zero(), timestamp + 1)
        .await
        .expect("Bob should be able to bind Charlie as referrer");

    // Verify Bob now has referrer
    assert!(
        storage.has_referrer(&bob).await.unwrap(),
        "Bob should now have a referrer"
    );

    // Verify Bob's referrer is Charlie
    let bob_referrer = storage.get_referrer(&bob).await.unwrap();
    assert_eq!(bob_referrer, Some(charlie), "Bob's referrer should be Charlie");

    println!("Test A.1 passed: Stub record does not block binding");
}

/// Test A.2: Preserve direct_referrals_count when binding
///
/// Scenario:
/// 1. 5 users bind Bob as their referrer (Bob's direct_count = 5)
/// 2. Bob binds Charlie as his referrer
/// 3. Bob's direct_referrals_count should still be 5
///
/// This tests the fix for: binding preserves existing counts from stub record
#[tokio::test]
async fn test_binding_preserves_direct_referrals_count() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    let bob = random_pubkey();
    let charlie = random_pubkey();

    let height = 100;
    let timestamp = 1000;

    // Step 1: 5 users bind Bob as referrer
    for i in 0..5u64 {
        let user = random_pubkey();
        storage
            .bind_referrer(&user, &bob, height + i, Hash::zero(), timestamp + i)
            .await
            .expect("User should bind Bob as referrer");
    }

    // Verify Bob has 5 direct referrals
    let count_before = storage.get_direct_referrals_count(&bob).await.unwrap();
    assert_eq!(count_before, 5, "Bob should have 5 direct referrals before binding");

    // Step 2: Bob binds Charlie as his referrer
    storage
        .bind_referrer(&bob, &charlie, height + 10, Hash::zero(), timestamp + 10)
        .await
        .expect("Bob should bind Charlie as referrer");

    // Step 3: Verify count is preserved
    let count_after = storage.get_direct_referrals_count(&bob).await.unwrap();
    assert_eq!(
        count_after, 5,
        "Bob should still have 5 direct referrals after binding his own referrer"
    );

    // Also verify Bob's referrer is correctly set
    let bob_referrer = storage.get_referrer(&bob).await.unwrap();
    assert_eq!(bob_referrer, Some(charlie), "Bob's referrer should be Charlie");

    println!("Test A.2 passed: Preserve direct_referrals_count when binding");
}

/// Test A.3: Preserve team_size when binding
///
/// Scenario:
/// 1. Build a tree: User1 → User2 → Bob (Bob gains team_size from downstream)
/// 2. Bob binds Charlie as his referrer
/// 3. Bob's team_size should be preserved
#[tokio::test]
async fn test_binding_preserves_team_size() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    let bob = random_pubkey();
    let charlie = random_pubkey();
    let user1 = random_pubkey();
    let user2 = random_pubkey();

    let height = 100;
    let timestamp = 1000;

    // Build tree: user1 → user2 → bob
    // This creates stub for bob, then user2, then updates counts
    storage
        .bind_referrer(&user1, &user2, height, Hash::zero(), timestamp)
        .await
        .unwrap();
    storage
        .bind_referrer(&user2, &bob, height + 1, Hash::zero(), timestamp + 1)
        .await
        .unwrap();

    // Set and verify a non-zero cached team size before binding
    let team_size_before = 2;
    storage
        .update_team_size_cache(&bob, team_size_before)
        .await
        .unwrap();
    let record_before = storage.get_referral_record(&bob).await.unwrap().unwrap();
    assert_eq!(
        record_before.team_size, team_size_before,
        "Bob's team_size cache should be set before binding"
    );

    // Bob binds Charlie
    storage
        .bind_referrer(&bob, &charlie, height + 2, Hash::zero(), timestamp + 2)
        .await
        .unwrap();

    // Get Bob's record after binding
    let record_after = storage.get_referral_record(&bob).await.unwrap().unwrap();
    let team_size_after = record_after.team_size;

    assert_eq!(
        team_size_after, team_size_before,
        "Bob's team_size should be preserved after binding"
    );

    println!("Test A.3 passed: Preserve team_size when binding");
}

/// Test A.4: has_referrer semantics with stub records
///
/// Verify has_referrer returns false for stub records (referrer = None)
/// and true only for actual bindings (referrer = Some)
#[tokio::test]
async fn test_has_referrer_semantics_with_stub() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    let alice = random_pubkey();
    let bob = random_pubkey();
    let charlie = random_pubkey();

    let height = 100;
    let timestamp = 1000;

    // Initially, no one has a referrer
    assert!(!storage.has_referrer(&alice).await.unwrap());
    assert!(!storage.has_referrer(&bob).await.unwrap());
    assert!(!storage.has_referrer(&charlie).await.unwrap());

    // Alice binds Bob (creates stub for Bob)
    storage
        .bind_referrer(&alice, &bob, height, Hash::zero(), timestamp)
        .await
        .unwrap();

    // Alice has referrer, Bob doesn't (stub only)
    assert!(storage.has_referrer(&alice).await.unwrap());
    assert!(!storage.has_referrer(&bob).await.unwrap());

    // Verify Bob has a record but referrer is None
    let bob_record = storage.get_referral_record(&bob).await.unwrap();
    assert!(bob_record.is_some(), "Bob should have a record (stub)");
    assert!(
        bob_record.unwrap().referrer.is_none(),
        "Bob's referrer should be None (stub record)"
    );

    println!("Test A.4 passed: has_referrer semantics with stub records");
}

// ============================================================================
// B. State transition ordering
// ============================================================================

/// Test B.1: Order-dependent path
///
/// Scenario: become referrer (stub) → bind own upline → validate counts
#[tokio::test]
async fn test_order_dependent_state_transition() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    let alice = random_pubkey();
    let bob = random_pubkey();
    let charlie = random_pubkey();
    let david = random_pubkey();

    let height = 100;
    let timestamp = 1000;

    // Phase 1: Bob becomes a referrer (gets stub record)
    // Alice and Charlie both bind Bob
    storage
        .bind_referrer(&alice, &bob, height, Hash::zero(), timestamp)
        .await
        .unwrap();
    storage
        .bind_referrer(&charlie, &bob, height + 1, Hash::zero(), timestamp + 1)
        .await
        .unwrap();

    // Verify Bob has 2 direct referrals
    assert_eq!(storage.get_direct_referrals_count(&bob).await.unwrap(), 2);
    assert!(!storage.has_referrer(&bob).await.unwrap());

    // Phase 2: Bob binds his own referrer
    storage
        .bind_referrer(&bob, &david, height + 2, Hash::zero(), timestamp + 2)
        .await
        .unwrap();

    // Verify state after transition:
    // - Bob has referrer (David)
    // - Bob still has 2 direct referrals
    // - David has 1 direct referral (Bob)
    assert!(storage.has_referrer(&bob).await.unwrap());
    assert_eq!(storage.get_referrer(&bob).await.unwrap(), Some(david.clone()));
    assert_eq!(storage.get_direct_referrals_count(&bob).await.unwrap(), 2);
    assert_eq!(storage.get_direct_referrals_count(&david).await.unwrap(), 1);

    println!("Test B.1 passed: Order-dependent state transition");
}

/// Test B.2: Rebinding prevention after stub
///
/// Once a user binds a referrer, they cannot rebind (even if they had a stub)
#[tokio::test]
async fn test_rebinding_prevention_after_stub() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    let alice = random_pubkey();
    let bob = random_pubkey();
    let charlie = random_pubkey();
    let david = random_pubkey();

    let height = 100;
    let timestamp = 1000;

    // Alice binds Bob (creates stub for Bob)
    storage
        .bind_referrer(&alice, &bob, height, Hash::zero(), timestamp)
        .await
        .unwrap();

    // Bob binds Charlie
    storage
        .bind_referrer(&bob, &charlie, height + 1, Hash::zero(), timestamp + 1)
        .await
        .unwrap();

    // Bob tries to rebind to David - should fail
    let result = storage
        .bind_referrer(&bob, &david, height + 2, Hash::zero(), timestamp + 2)
        .await;

    assert!(
        result.is_err(),
        "Bob should not be able to rebind after already having a referrer"
    );

    // Verify Bob's referrer is still Charlie
    assert_eq!(storage.get_referrer(&bob).await.unwrap(), Some(charlie));

    println!("Test B.2 passed: Rebinding prevention after stub");
}

// ============================================================================
// C. Consistency assertions
// ============================================================================

/// Test C.1: List vs count consistency
///
/// Verify direct_referrals list length equals direct_referrals_count
#[tokio::test]
async fn test_list_vs_count_consistency() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    let bob = random_pubkey();

    let height = 100;
    let timestamp = 1000;

    // Bind 10 users to Bob
    let mut users = Vec::new();
    for i in 0..10u64 {
        let user = random_pubkey();
        users.push(user.clone());
        storage
            .bind_referrer(&user, &bob, height + i, Hash::zero(), timestamp + i)
            .await
            .unwrap();
    }

    // Get count
    let count = storage.get_direct_referrals_count(&bob).await.unwrap();

    // Get list
    let result = storage.get_direct_referrals(&bob, 0, 100).await.unwrap();
    let list_len = result.referrals.len();

    assert_eq!(
        count as usize, list_len,
        "direct_referrals_count ({}) should equal list length ({})",
        count, list_len
    );

    // Verify all users are in the list
    for user in &users {
        assert!(
            result.referrals.contains(user),
            "User should be in direct referrals list"
        );
    }

    println!("Test C.1 passed: List vs count consistency");
}

/// Test C.1b: List vs count consistency across pagination boundary
///
/// Verify direct_referrals_count matches paged list results
#[tokio::test]
async fn test_list_vs_count_pagination_consistency() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    let bob = random_pubkey();
    let height = 100;
    let timestamp = 1000;

    // Bind enough users to cross DIRECT_REFERRALS_PAGE_SIZE (1000)
    let total_users = 1001u64;
    let mut last_user = None;
    for i in 0..total_users {
        let user = random_pubkey();
        last_user = Some(user.clone());
        storage
            .bind_referrer(&user, &bob, height + i, Hash::zero(), timestamp + i)
            .await
            .unwrap();
    }

    let count = storage.get_direct_referrals_count(&bob).await.unwrap();
    assert_eq!(count, total_users as u32, "Count should match total referrals");

    // Page 1: first 1000
    let page1 = storage.get_direct_referrals(&bob, 0, 1000).await.unwrap();
    assert_eq!(page1.referrals.len(), 1000, "First page should have 1000 referrals");
    assert_eq!(page1.total_count, total_users as u32);

    // Page 2: remaining 1
    let page2 = storage.get_direct_referrals(&bob, 1000, 1000).await.unwrap();
    assert_eq!(page2.referrals.len(), 1, "Second page should have 1 referral");
    assert_eq!(page2.total_count, total_users as u32);

    // Verify the last bound user appears in page 2
    let last_user = last_user.expect("last_user should be set");
    assert!(
        page2.referrals.contains(&last_user),
        "Last user should appear in the second page"
    );

    println!("Test C.1b passed: List vs count pagination consistency");
}

/// Test C.2: Upline chain consistency
///
/// Verify get_uplines returns correct chain
#[tokio::test]
async fn test_upline_chain_consistency() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    // Build chain: user1 → user2 → user3 → user4 → user5
    let users: Vec<PublicKey> = (0..5).map(|_| random_pubkey()).collect();

    let height = 100;
    let timestamp = 1000;

    for i in 0..4 {
        storage
            .bind_referrer(&users[i], &users[i + 1], height + i as u64, Hash::zero(), timestamp + i as u64)
            .await
            .unwrap();
    }

    // Verify upline chain from user1
    let result = storage.get_uplines(&users[0], 10).await.unwrap();

    assert_eq!(result.uplines.len(), 4, "Should have 4 uplines");
    assert_eq!(result.uplines[0], users[1], "First upline should be user2");
    assert_eq!(result.uplines[1], users[2], "Second upline should be user3");
    assert_eq!(result.uplines[2], users[3], "Third upline should be user4");
    assert_eq!(result.uplines[3], users[4], "Fourth upline should be user5");

    // Verify levels
    let level = storage.get_level(&users[0]).await.unwrap();
    assert_eq!(level, 4, "user1 should be at level 4");

    println!("Test C.2 passed: Upline chain consistency");
}

/// Test C.3: is_downline consistency
///
/// Verify is_downline correctly identifies relationships
#[tokio::test]
async fn test_is_downline_consistency() {
    let temp_dir = TempDir::new("referral_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    // Build chain: alice → bob → charlie
    let alice = random_pubkey();
    let bob = random_pubkey();
    let charlie = random_pubkey();
    let david = random_pubkey(); // Not in chain

    let height = 100;
    let timestamp = 1000;

    storage
        .bind_referrer(&alice, &bob, height, Hash::zero(), timestamp)
        .await
        .unwrap();
    storage
        .bind_referrer(&bob, &charlie, height + 1, Hash::zero(), timestamp + 1)
        .await
        .unwrap();

    // is_downline(ancestor, descendant, max_depth) - checks if descendant is in ancestor's downline tree
    // Alice is in Bob's downline, and also in Charlie's downline (through Bob)
    assert!(storage.is_downline(&bob, &alice, 10).await.unwrap());
    assert!(storage.is_downline(&charlie, &alice, 10).await.unwrap());

    // Bob is in Charlie's downline only
    assert!(storage.is_downline(&charlie, &bob, 10).await.unwrap());
    assert!(!storage.is_downline(&alice, &bob, 10).await.unwrap());

    // Charlie is the root - no one has Charlie as their downline
    assert!(!storage.is_downline(&alice, &charlie, 10).await.unwrap());
    assert!(!storage.is_downline(&bob, &charlie, 10).await.unwrap());

    // David is not in the chain - not in anyone's downline
    assert!(!storage.is_downline(&alice, &david, 10).await.unwrap());
    assert!(!storage.is_downline(&bob, &david, 10).await.unwrap());
    assert!(!storage.is_downline(&charlie, &david, 10).await.unwrap());

    println!("Test C.3 passed: is_downline consistency");
}

// ============================================================================
// Summary test
// ============================================================================

#[test]
fn test_referral_rocksdb_test_summary() {
    println!("\n========================================");
    println!("RocksStorage Referral Integration Tests");
    println!("========================================");
    println!("\nTests implemented based on Referral-Test-Analysis.md:");
    println!();
    println!("A. BindReferrer (RocksStorage integration):");
    println!("   A.1 test_stub_record_does_not_block_binding");
    println!("   A.2 test_binding_preserves_direct_referrals_count");
    println!("   A.3 test_binding_preserves_team_size");
    println!("   A.4 test_has_referrer_semantics_with_stub");
    println!();
    println!("B. State transition ordering:");
    println!("   B.1 test_order_dependent_state_transition");
    println!("   B.2 test_rebinding_prevention_after_stub");
    println!();
    println!("C. Consistency assertions:");
    println!("   C.1 test_list_vs_count_consistency");
    println!("   C.2 test_upline_chain_consistency");
    println!("   C.3 test_is_downline_consistency");
    println!();
    println!("These tests verify fixes for bugs found during PR #25 code review:");
    println!("- Stub record blocking future binding (HIGH)");
    println!("- Binding overwrites direct_referrals_count (HIGH)");
    println!("========================================\n");
}
