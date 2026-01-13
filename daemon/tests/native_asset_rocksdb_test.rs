//! RocksStorage Native Asset Integration Tests
//!
//! These tests verify the RocksStorage implementation of the native asset system,
//! specifically testing the index operations added for TAKO syscall integration:
//!
//! A. Lock Index Operations
//!    - Add/remove lock IDs to index
//!    - Get lock IDs with pagination
//!    - Duplicate prevention
//!
//! B. User Escrow Index Operations
//!    - Add/remove escrow IDs
//!    - Multi-user escrow tracking
//!
//! C. Owner Agent Index Operations
//!    - Add/remove agents per owner
//!    - Duplicate prevention
//!
//! D. Role Members Index Operations
//!    - Add/remove role members
//!    - Get member by index
//!    - Enumerate all members
//!
//! E. Admin Proposal Operations
//!    - Set/get/clear pending admin

#![allow(clippy::disallowed_methods)]

use tempdir::TempDir;
use tos_common::{
    crypto::Hash,
    native_asset::{
        BalanceCheckpoint, DelegationCheckpoint, Escrow, EscrowStatus, NativeAssetData,
        ReleaseCondition, TokenLock,
    },
    network::Network,
};
use tos_daemon::core::{
    config::RocksDBConfig,
    storage::{
        rocksdb::{CacheMode, CompressionMode, RocksStorage},
        NativeAssetProvider,
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
    RocksStorage::new(temp_dir.path().to_str().unwrap(), Network::Devnet, &config)
}

/// Generate a random asset hash for testing
fn random_asset() -> Hash {
    Hash::new(rand::random())
}

/// Generate a random account for testing
fn random_account() -> [u8; 32] {
    rand::random()
}

// ============================================================================
// A. Lock Index Operations
// ============================================================================

/// Test A.1: Add and get lock IDs
#[tokio::test]
async fn test_lock_index_add_and_get() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    // Initially empty
    let locks = storage
        .get_native_asset_lock_ids(&asset, &account)
        .await
        .expect("Should get empty lock list");
    assert!(locks.is_empty(), "Lock list should be empty initially");

    // Add some lock IDs
    storage
        .add_native_asset_lock_id(&asset, &account, 1)
        .await
        .expect("Should add lock ID 1");
    storage
        .add_native_asset_lock_id(&asset, &account, 5)
        .await
        .expect("Should add lock ID 5");
    storage
        .add_native_asset_lock_id(&asset, &account, 3)
        .await
        .expect("Should add lock ID 3");

    // Verify all IDs are present
    let locks = storage
        .get_native_asset_lock_ids(&asset, &account)
        .await
        .expect("Should get lock list");
    assert_eq!(locks.len(), 3, "Should have 3 lock IDs");
    assert!(locks.contains(&1), "Should contain lock ID 1");
    assert!(locks.contains(&5), "Should contain lock ID 5");
    assert!(locks.contains(&3), "Should contain lock ID 3");

    println!("Test A.1 passed: Add and get lock IDs");
}

/// Test A.2: Remove lock IDs
#[tokio::test]
async fn test_lock_index_remove() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    // Add lock IDs
    for id in [1, 2, 3, 4, 5] {
        storage
            .add_native_asset_lock_id(&asset, &account, id)
            .await
            .expect("Should add lock ID");
    }

    // Remove middle one
    storage
        .remove_native_asset_lock_id(&asset, &account, 3)
        .await
        .expect("Should remove lock ID 3");

    let locks = storage
        .get_native_asset_lock_ids(&asset, &account)
        .await
        .expect("Should get lock list");
    assert_eq!(locks.len(), 4, "Should have 4 lock IDs after removal");
    assert!(!locks.contains(&3), "Should not contain removed ID 3");

    // Remove all remaining
    for id in [1, 2, 4, 5] {
        storage
            .remove_native_asset_lock_id(&asset, &account, id)
            .await
            .expect("Should remove lock ID");
    }

    let locks = storage
        .get_native_asset_lock_ids(&asset, &account)
        .await
        .expect("Should get empty lock list");
    assert!(
        locks.is_empty(),
        "Lock list should be empty after removing all"
    );

    println!("Test A.2 passed: Remove lock IDs");
}

/// Test A.3: Duplicate prevention in lock index
#[tokio::test]
async fn test_lock_index_duplicate_prevention() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    // Add same lock ID multiple times
    storage
        .add_native_asset_lock_id(&asset, &account, 42)
        .await
        .expect("Should add lock ID");
    storage
        .add_native_asset_lock_id(&asset, &account, 42)
        .await
        .expect("Should handle duplicate");
    storage
        .add_native_asset_lock_id(&asset, &account, 42)
        .await
        .expect("Should handle duplicate");

    let locks = storage
        .get_native_asset_lock_ids(&asset, &account)
        .await
        .expect("Should get lock list");
    assert_eq!(locks.len(), 1, "Should have only 1 lock ID (no duplicates)");
    assert_eq!(locks[0], 42, "Lock ID should be 42");

    println!("Test A.3 passed: Duplicate prevention in lock index");
}

/// Test A.4: Removing non-existent lock ID is safe
#[tokio::test]
async fn test_lock_index_remove_nonexistent() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    // Remove from empty index should succeed silently
    storage
        .remove_native_asset_lock_id(&asset, &account, 999)
        .await
        .expect("Should handle removal from empty index");

    // Add one, remove different one
    storage
        .add_native_asset_lock_id(&asset, &account, 1)
        .await
        .expect("Should add lock ID");
    storage
        .remove_native_asset_lock_id(&asset, &account, 999)
        .await
        .expect("Should handle removal of non-existent ID");

    let locks = storage
        .get_native_asset_lock_ids(&asset, &account)
        .await
        .expect("Should get lock list");
    assert_eq!(locks.len(), 1, "Should still have 1 lock ID");

    println!("Test A.4 passed: Removing non-existent lock ID is safe");
}

// ============================================================================
// B. User Escrow Index Operations
// ============================================================================

/// Test B.1: Add and get user escrows
#[tokio::test]
async fn test_user_escrow_index_add_and_get() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let user = random_account();

    // Initially empty
    let escrows = storage
        .get_native_asset_user_escrows(&asset, &user)
        .await
        .expect("Should get empty escrow list");
    assert!(escrows.is_empty(), "Escrow list should be empty initially");

    // Add escrow IDs
    storage
        .add_native_asset_user_escrow(&asset, &user, 100)
        .await
        .expect("Should add escrow 100");
    storage
        .add_native_asset_user_escrow(&asset, &user, 200)
        .await
        .expect("Should add escrow 200");

    let escrows = storage
        .get_native_asset_user_escrows(&asset, &user)
        .await
        .expect("Should get escrow list");
    assert_eq!(escrows.len(), 2, "Should have 2 escrows");
    assert!(escrows.contains(&100));
    assert!(escrows.contains(&200));

    println!("Test B.1 passed: Add and get user escrows");
}

/// Test B.2: Remove user escrows
#[tokio::test]
async fn test_user_escrow_index_remove() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let user = random_account();

    // Add and remove
    storage
        .add_native_asset_user_escrow(&asset, &user, 1)
        .await
        .expect("Should add escrow");
    storage
        .add_native_asset_user_escrow(&asset, &user, 2)
        .await
        .expect("Should add escrow");

    storage
        .remove_native_asset_user_escrow(&asset, &user, 1)
        .await
        .expect("Should remove escrow");

    let escrows = storage
        .get_native_asset_user_escrows(&asset, &user)
        .await
        .expect("Should get escrow list");
    assert_eq!(escrows.len(), 1);
    assert!(!escrows.contains(&1));
    assert!(escrows.contains(&2));

    println!("Test B.2 passed: Remove user escrows");
}

/// Test B.3: Multi-user escrow tracking
#[tokio::test]
async fn test_user_escrow_multi_user() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let user1 = random_account();
    let user2 = random_account();

    // Same escrow tracked for both users (sender and recipient)
    storage
        .add_native_asset_user_escrow(&asset, &user1, 1)
        .await
        .expect("Should add escrow for user1");
    storage
        .add_native_asset_user_escrow(&asset, &user2, 1)
        .await
        .expect("Should add escrow for user2");

    // Each user should see the escrow
    let escrows1 = storage
        .get_native_asset_user_escrows(&asset, &user1)
        .await
        .expect("Should get escrows for user1");
    let escrows2 = storage
        .get_native_asset_user_escrows(&asset, &user2)
        .await
        .expect("Should get escrows for user2");

    assert!(escrows1.contains(&1));
    assert!(escrows2.contains(&1));

    println!("Test B.3 passed: Multi-user escrow tracking");
}

// ============================================================================
// C. Owner Agent Index Operations
// ============================================================================

/// Test C.1: Add and get owner agents
#[tokio::test]
async fn test_owner_agents_add_and_get() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let owner = random_account();
    let agent1 = random_account();
    let agent2 = random_account();

    // Initially empty
    let agents = storage
        .get_native_asset_owner_agents(&asset, &owner)
        .await
        .expect("Should get empty agent list");
    assert!(agents.is_empty());

    // Add agents
    storage
        .add_native_asset_owner_agent(&asset, &owner, &agent1)
        .await
        .expect("Should add agent1");
    storage
        .add_native_asset_owner_agent(&asset, &owner, &agent2)
        .await
        .expect("Should add agent2");

    let agents = storage
        .get_native_asset_owner_agents(&asset, &owner)
        .await
        .expect("Should get agent list");
    assert_eq!(agents.len(), 2);
    assert!(agents.contains(&agent1));
    assert!(agents.contains(&agent2));

    println!("Test C.1 passed: Add and get owner agents");
}

/// Test C.2: Remove owner agents
#[tokio::test]
async fn test_owner_agents_remove() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let owner = random_account();
    let agent1 = random_account();
    let agent2 = random_account();

    storage
        .add_native_asset_owner_agent(&asset, &owner, &agent1)
        .await
        .expect("Should add agent");
    storage
        .add_native_asset_owner_agent(&asset, &owner, &agent2)
        .await
        .expect("Should add agent");

    storage
        .remove_native_asset_owner_agent(&asset, &owner, &agent1)
        .await
        .expect("Should remove agent");

    let agents = storage
        .get_native_asset_owner_agents(&asset, &owner)
        .await
        .expect("Should get agent list");
    assert_eq!(agents.len(), 1);
    assert!(!agents.contains(&agent1));
    assert!(agents.contains(&agent2));

    println!("Test C.2 passed: Remove owner agents");
}

/// Test C.3: Agent duplicate prevention
#[tokio::test]
async fn test_owner_agents_duplicate_prevention() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let owner = random_account();
    let agent = random_account();

    // Add same agent multiple times
    for _ in 0..5 {
        storage
            .add_native_asset_owner_agent(&asset, &owner, &agent)
            .await
            .expect("Should handle duplicate");
    }

    let agents = storage
        .get_native_asset_owner_agents(&asset, &owner)
        .await
        .expect("Should get agent list");
    assert_eq!(agents.len(), 1, "Should have only 1 agent (no duplicates)");

    println!("Test C.3 passed: Agent duplicate prevention");
}

// ============================================================================
// D. Role Members Index Operations
// ============================================================================

/// Test D.1: Add and get role members
#[tokio::test]
async fn test_role_members_add_and_get() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let role: [u8; 32] = tos_common::native_asset::MINTER_ROLE;
    let member1 = random_account();
    let member2 = random_account();
    let member3 = random_account();

    // Initially empty
    let members = storage
        .get_native_asset_role_members(&asset, &role)
        .await
        .expect("Should get empty member list");
    assert!(members.is_empty());

    // Add members
    storage
        .add_native_asset_role_member(&asset, &role, &member1)
        .await
        .expect("Should add member1");
    storage
        .add_native_asset_role_member(&asset, &role, &member2)
        .await
        .expect("Should add member2");
    storage
        .add_native_asset_role_member(&asset, &role, &member3)
        .await
        .expect("Should add member3");

    let members = storage
        .get_native_asset_role_members(&asset, &role)
        .await
        .expect("Should get member list");
    assert_eq!(members.len(), 3);
    assert!(members.contains(&member1));
    assert!(members.contains(&member2));
    assert!(members.contains(&member3));

    println!("Test D.1 passed: Add and get role members");
}

/// Test D.2: Get role member by index
#[tokio::test]
async fn test_role_members_get_by_index() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let role: [u8; 32] = tos_common::native_asset::BURNER_ROLE;
    let member1 = random_account();
    let member2 = random_account();

    storage
        .add_native_asset_role_member(&asset, &role, &member1)
        .await
        .expect("Should add member");
    storage
        .add_native_asset_role_member(&asset, &role, &member2)
        .await
        .expect("Should add member");

    // Get by valid indices
    let m0 = storage
        .get_native_asset_role_member(&asset, &role, 0)
        .await
        .expect("Should get member at index 0");
    let m1 = storage
        .get_native_asset_role_member(&asset, &role, 1)
        .await
        .expect("Should get member at index 1");

    assert!(m0 == member1 || m0 == member2);
    assert!(m1 == member1 || m1 == member2);
    assert_ne!(m0, m1);

    // Get by invalid index should fail
    let result = storage
        .get_native_asset_role_member(&asset, &role, 999)
        .await;
    assert!(result.is_err(), "Should fail for invalid index");

    println!("Test D.2 passed: Get role member by index");
}

/// Test D.3: Remove role members
#[tokio::test]
async fn test_role_members_remove() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let role: [u8; 32] = tos_common::native_asset::PAUSER_ROLE;
    let member1 = random_account();
    let member2 = random_account();

    storage
        .add_native_asset_role_member(&asset, &role, &member1)
        .await
        .expect("Should add member");
    storage
        .add_native_asset_role_member(&asset, &role, &member2)
        .await
        .expect("Should add member");

    storage
        .remove_native_asset_role_member(&asset, &role, &member1)
        .await
        .expect("Should remove member");

    let members = storage
        .get_native_asset_role_members(&asset, &role)
        .await
        .expect("Should get member list");
    assert_eq!(members.len(), 1);
    assert!(!members.contains(&member1));
    assert!(members.contains(&member2));

    println!("Test D.3 passed: Remove role members");
}

/// Test D.4: Role member duplicate prevention
#[tokio::test]
async fn test_role_members_duplicate_prevention() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let role: [u8; 32] = tos_common::native_asset::FREEZER_ROLE;
    let member = random_account();

    // Add same member multiple times
    for _ in 0..3 {
        storage
            .add_native_asset_role_member(&asset, &role, &member)
            .await
            .expect("Should handle duplicate");
    }

    let members = storage
        .get_native_asset_role_members(&asset, &role)
        .await
        .expect("Should get member list");
    assert_eq!(
        members.len(),
        1,
        "Should have only 1 member (no duplicates)"
    );

    println!("Test D.4 passed: Role member duplicate prevention");
}

// ============================================================================
// E. Admin Proposal Operations
// ============================================================================

/// Test E.1: Set and get pending admin
#[tokio::test]
async fn test_pending_admin_set_and_get() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let new_admin = random_account();

    // Initially none
    let pending = storage
        .get_native_asset_pending_admin(&asset)
        .await
        .expect("Should get pending admin");
    assert!(pending.is_none(), "Should have no pending admin initially");

    // Set pending admin
    storage
        .set_native_asset_pending_admin(&asset, Some(&new_admin))
        .await
        .expect("Should set pending admin");

    let pending = storage
        .get_native_asset_pending_admin(&asset)
        .await
        .expect("Should get pending admin");
    assert_eq!(pending, Some(new_admin), "Should have pending admin set");

    println!("Test E.1 passed: Set and get pending admin");
}

/// Test E.2: Clear pending admin
#[tokio::test]
async fn test_pending_admin_clear() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let new_admin = random_account();

    // Set and then clear
    storage
        .set_native_asset_pending_admin(&asset, Some(&new_admin))
        .await
        .expect("Should set pending admin");

    storage
        .set_native_asset_pending_admin(&asset, None)
        .await
        .expect("Should clear pending admin");

    let pending = storage
        .get_native_asset_pending_admin(&asset)
        .await
        .expect("Should get pending admin");
    assert!(pending.is_none(), "Pending admin should be cleared");

    println!("Test E.2 passed: Clear pending admin");
}

/// Test E.3: Pending admin per asset isolation
#[tokio::test]
async fn test_pending_admin_per_asset() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset1 = random_asset();
    let asset2 = random_asset();
    let admin1 = random_account();
    let admin2 = random_account();

    // Set different pending admins for different assets
    storage
        .set_native_asset_pending_admin(&asset1, Some(&admin1))
        .await
        .expect("Should set pending admin for asset1");
    storage
        .set_native_asset_pending_admin(&asset2, Some(&admin2))
        .await
        .expect("Should set pending admin for asset2");

    let pending1 = storage
        .get_native_asset_pending_admin(&asset1)
        .await
        .expect("Should get pending admin for asset1");
    let pending2 = storage
        .get_native_asset_pending_admin(&asset2)
        .await
        .expect("Should get pending admin for asset2");

    assert_eq!(pending1, Some(admin1));
    assert_eq!(pending2, Some(admin2));

    println!("Test E.3 passed: Pending admin per asset isolation");
}

// ============================================================================
// F. Integration Tests
// ============================================================================

/// Test F.1: Full lock lifecycle with index
#[tokio::test]
async fn test_lock_lifecycle_with_index() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    // Create asset first
    let data = NativeAssetData {
        name: "Test Token".to_string(),
        symbol: "TEST".to_string(),
        decimals: 18,
        total_supply: 1_000_000,
        max_supply: Some(10_000_000),
        mintable: true,
        burnable: true,
        pausable: true,
        freezable: true,
        governance: false,
        creator: account,
        metadata_uri: None,
        created_at: 100,
    };
    storage
        .set_native_asset(&asset, &data)
        .await
        .expect("Should create asset");

    // Set initial balance
    storage
        .set_native_asset_balance(&asset, &account, 10000)
        .await
        .expect("Should set balance");

    // Create locks and add to index
    for lock_id in 1..=5 {
        let lock = TokenLock {
            id: lock_id,
            amount: 100,
            unlock_at: 1000 + lock_id,
            transferable: true,
            locker: account,
            created_at: 100,
        };
        storage
            .set_native_asset_lock(&asset, &account, &lock)
            .await
            .expect("Should create lock");
        storage
            .add_native_asset_lock_id(&asset, &account, lock_id)
            .await
            .expect("Should add to index");
    }

    // Verify all locks are in index
    let lock_ids = storage
        .get_native_asset_lock_ids(&asset, &account)
        .await
        .expect("Should get lock IDs");
    assert_eq!(lock_ids.len(), 5);

    // Unlock (remove) some locks
    for lock_id in [2, 4] {
        storage
            .delete_native_asset_lock(&asset, &account, lock_id)
            .await
            .expect("Should delete lock");
        storage
            .remove_native_asset_lock_id(&asset, &account, lock_id)
            .await
            .expect("Should remove from index");
    }

    // Verify remaining locks
    let lock_ids = storage
        .get_native_asset_lock_ids(&asset, &account)
        .await
        .expect("Should get lock IDs");
    assert_eq!(lock_ids.len(), 3);
    assert!(lock_ids.contains(&1));
    assert!(!lock_ids.contains(&2));
    assert!(lock_ids.contains(&3));
    assert!(!lock_ids.contains(&4));
    assert!(lock_ids.contains(&5));

    println!("Test F.1 passed: Full lock lifecycle with index");
}

/// Test F.2: Full escrow lifecycle with user index
#[tokio::test]
async fn test_escrow_lifecycle_with_user_index() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let sender = random_account();
    let recipient = random_account();

    // Create escrow
    let escrow = Escrow {
        id: 1,
        asset: asset.clone(),
        sender,
        recipient,
        amount: 1000,
        condition: ReleaseCondition::TimeRelease {
            release_after: 1000,
        },
        status: EscrowStatus::Active,
        approvals: vec![],
        expires_at: Some(2000),
        created_at: 100,
        metadata: None,
    };

    storage
        .set_native_asset_escrow(&asset, &escrow)
        .await
        .expect("Should create escrow");

    // Add to both user indices
    storage
        .add_native_asset_user_escrow(&asset, &sender, 1)
        .await
        .expect("Should add to sender index");
    storage
        .add_native_asset_user_escrow(&asset, &recipient, 1)
        .await
        .expect("Should add to recipient index");

    // Both users should see the escrow
    let sender_escrows = storage
        .get_native_asset_user_escrows(&asset, &sender)
        .await
        .expect("Should get sender escrows");
    let recipient_escrows = storage
        .get_native_asset_user_escrows(&asset, &recipient)
        .await
        .expect("Should get recipient escrows");

    assert!(sender_escrows.contains(&1));
    assert!(recipient_escrows.contains(&1));

    // Complete escrow - remove from indices
    storage
        .remove_native_asset_user_escrow(&asset, &sender, 1)
        .await
        .expect("Should remove from sender index");
    storage
        .remove_native_asset_user_escrow(&asset, &recipient, 1)
        .await
        .expect("Should remove from recipient index");

    let sender_escrows = storage
        .get_native_asset_user_escrows(&asset, &sender)
        .await
        .expect("Should get sender escrows");
    let recipient_escrows = storage
        .get_native_asset_user_escrows(&asset, &recipient)
        .await
        .expect("Should get recipient escrows");

    assert!(sender_escrows.is_empty());
    assert!(recipient_escrows.is_empty());

    println!("Test F.2 passed: Full escrow lifecycle with user index");
}

/// Test F.3: Role grant/revoke with member index
#[tokio::test]
async fn test_role_lifecycle_with_member_index() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let role = tos_common::native_asset::MINTER_ROLE;
    let account1 = random_account();
    let account2 = random_account();

    // Grant roles with index update
    storage
        .grant_native_asset_role(&asset, &role, &account1, 100)
        .await
        .expect("Should grant role");
    storage
        .add_native_asset_role_member(&asset, &role, &account1)
        .await
        .expect("Should add to member index");

    storage
        .grant_native_asset_role(&asset, &role, &account2, 100)
        .await
        .expect("Should grant role");
    storage
        .add_native_asset_role_member(&asset, &role, &account2)
        .await
        .expect("Should add to member index");

    // Verify both have role
    assert!(storage
        .has_native_asset_role(&asset, &role, &account1)
        .await
        .expect("Should check role"));
    assert!(storage
        .has_native_asset_role(&asset, &role, &account2)
        .await
        .expect("Should check role"));

    // Verify member index
    let members = storage
        .get_native_asset_role_members(&asset, &role)
        .await
        .expect("Should get members");
    assert_eq!(members.len(), 2);

    // Revoke one role
    storage
        .revoke_native_asset_role(&asset, &role, &account1)
        .await
        .expect("Should revoke role");
    storage
        .remove_native_asset_role_member(&asset, &role, &account1)
        .await
        .expect("Should remove from member index");

    // Verify
    assert!(!storage
        .has_native_asset_role(&asset, &role, &account1)
        .await
        .expect("Should check role"));
    assert!(storage
        .has_native_asset_role(&asset, &role, &account2)
        .await
        .expect("Should check role"));

    let members = storage
        .get_native_asset_role_members(&asset, &role)
        .await
        .expect("Should get members");
    assert_eq!(members.len(), 1);
    assert!(members.contains(&account2));

    println!("Test F.3 passed: Role grant/revoke with member index");
}

// ============================================================================
// G. Balance Checkpoint Operations
// ============================================================================

/// Test G.1: Set and get balance checkpoint count
#[tokio::test]
async fn test_balance_checkpoint_count() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    // Initially zero
    let count = storage
        .get_native_asset_balance_checkpoint_count(&asset, &account)
        .await
        .expect("Should get checkpoint count");
    assert_eq!(count, 0, "Count should be 0 initially");

    // Set count
    storage
        .set_native_asset_balance_checkpoint_count(&asset, &account, 5)
        .await
        .expect("Should set checkpoint count");

    let count = storage
        .get_native_asset_balance_checkpoint_count(&asset, &account)
        .await
        .expect("Should get checkpoint count");
    assert_eq!(count, 5, "Count should be 5");

    println!("Test G.1 passed: Set and get balance checkpoint count");
}

/// Test G.2: Set and get balance checkpoint
#[tokio::test]
async fn test_balance_checkpoint_set_and_get() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    let checkpoint = BalanceCheckpoint {
        from_block: 100,
        balance: 1000,
    };

    // Set checkpoint at index 0
    storage
        .set_native_asset_balance_checkpoint(&asset, &account, 0, &checkpoint)
        .await
        .expect("Should set checkpoint");

    // Get checkpoint at index 0
    let retrieved = storage
        .get_native_asset_balance_checkpoint(&asset, &account, 0)
        .await
        .expect("Should get checkpoint");

    assert_eq!(retrieved.from_block, 100, "from_block should match");
    assert_eq!(retrieved.balance, 1000, "balance should match");

    println!("Test G.2 passed: Set and get balance checkpoint");
}

/// Test G.3: Multiple balance checkpoints (binary search simulation)
#[tokio::test]
async fn test_balance_checkpoint_multiple() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    // Create multiple checkpoints at different block heights
    let checkpoints = [
        BalanceCheckpoint {
            from_block: 100,
            balance: 1000,
        },
        BalanceCheckpoint {
            from_block: 200,
            balance: 1500,
        },
        BalanceCheckpoint {
            from_block: 300,
            balance: 500,
        },
        BalanceCheckpoint {
            from_block: 400,
            balance: 2000,
        },
    ];

    // Store all checkpoints
    for (i, checkpoint) in checkpoints.iter().enumerate() {
        storage
            .set_native_asset_balance_checkpoint(&asset, &account, i as u32, checkpoint)
            .await
            .expect("Should set checkpoint");
    }

    // Update count
    storage
        .set_native_asset_balance_checkpoint_count(&asset, &account, checkpoints.len() as u32)
        .await
        .expect("Should set count");

    // Verify count
    let count = storage
        .get_native_asset_balance_checkpoint_count(&asset, &account)
        .await
        .expect("Should get count");
    assert_eq!(count, 4);

    // Verify each checkpoint
    for (i, expected) in checkpoints.iter().enumerate() {
        let retrieved = storage
            .get_native_asset_balance_checkpoint(&asset, &account, i as u32)
            .await
            .expect("Should get checkpoint");
        assert_eq!(retrieved.from_block, expected.from_block);
        assert_eq!(retrieved.balance, expected.balance);
    }

    println!("Test G.3 passed: Multiple balance checkpoints");
}

/// Test G.4: Balance checkpoints per asset/account isolation
#[tokio::test]
async fn test_balance_checkpoint_isolation() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset1 = random_asset();
    let asset2 = random_asset();
    let account1 = random_account();
    let account2 = random_account();

    // Set checkpoint for asset1/account1
    let cp1 = BalanceCheckpoint {
        from_block: 100,
        balance: 1000,
    };
    storage
        .set_native_asset_balance_checkpoint(&asset1, &account1, 0, &cp1)
        .await
        .expect("Should set checkpoint");
    storage
        .set_native_asset_balance_checkpoint_count(&asset1, &account1, 1)
        .await
        .expect("Should set count");

    // Set checkpoint for asset2/account2
    let cp2 = BalanceCheckpoint {
        from_block: 200,
        balance: 2000,
    };
    storage
        .set_native_asset_balance_checkpoint(&asset2, &account2, 0, &cp2)
        .await
        .expect("Should set checkpoint");
    storage
        .set_native_asset_balance_checkpoint_count(&asset2, &account2, 1)
        .await
        .expect("Should set count");

    // Verify isolation - asset1/account1 should not see asset2/account2's checkpoints
    let count1 = storage
        .get_native_asset_balance_checkpoint_count(&asset1, &account1)
        .await
        .expect("Should get count");
    let count2 = storage
        .get_native_asset_balance_checkpoint_count(&asset2, &account2)
        .await
        .expect("Should get count");

    // asset1/account2 should be zero
    let cross_count = storage
        .get_native_asset_balance_checkpoint_count(&asset1, &account2)
        .await
        .expect("Should get count");

    assert_eq!(count1, 1);
    assert_eq!(count2, 1);
    assert_eq!(cross_count, 0, "Cross-account checkpoint should not exist");

    println!("Test G.4 passed: Balance checkpoints per asset/account isolation");
}

// ============================================================================
// H. Delegation Checkpoint Operations
// ============================================================================

/// Test H.1: Set and get delegation checkpoint count
#[tokio::test]
async fn test_delegation_checkpoint_count() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();

    // Initially zero
    let count = storage
        .get_native_asset_delegation_checkpoint_count(&asset, &account)
        .await
        .expect("Should get checkpoint count");
    assert_eq!(count, 0, "Count should be 0 initially");

    // Set count
    storage
        .set_native_asset_delegation_checkpoint_count(&asset, &account, 3)
        .await
        .expect("Should set checkpoint count");

    let count = storage
        .get_native_asset_delegation_checkpoint_count(&asset, &account)
        .await
        .expect("Should get checkpoint count");
    assert_eq!(count, 3, "Count should be 3");

    println!("Test H.1 passed: Set and get delegation checkpoint count");
}

/// Test H.2: Set and get delegation checkpoint
#[tokio::test]
async fn test_delegation_checkpoint_set_and_get() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();
    let delegatee = random_account();

    let checkpoint = DelegationCheckpoint {
        from_block: 150,
        delegatee,
    };

    // Set checkpoint at index 0
    storage
        .set_native_asset_delegation_checkpoint(&asset, &account, 0, &checkpoint)
        .await
        .expect("Should set checkpoint");

    // Get checkpoint at index 0
    let retrieved = storage
        .get_native_asset_delegation_checkpoint(&asset, &account, 0)
        .await
        .expect("Should get checkpoint");

    assert_eq!(retrieved.from_block, 150, "from_block should match");
    assert_eq!(retrieved.delegatee, delegatee, "delegatee should match");

    println!("Test H.2 passed: Set and get delegation checkpoint");
}

/// Test H.3: Multiple delegation checkpoints (delegation history)
#[tokio::test]
async fn test_delegation_checkpoint_multiple() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset = random_asset();
    let account = random_account();
    let delegatee1 = random_account();
    let delegatee2 = random_account();
    let delegatee3 = random_account();

    // Create delegation history: account delegates to different accounts over time
    let checkpoints = [
        DelegationCheckpoint {
            from_block: 100,
            delegatee: delegatee1,
        },
        DelegationCheckpoint {
            from_block: 200,
            delegatee: delegatee2,
        },
        DelegationCheckpoint {
            from_block: 300,
            delegatee: delegatee3,
        },
        DelegationCheckpoint {
            from_block: 400,
            delegatee: [0u8; 32], // Self-delegation (zero = self)
        },
    ];

    // Store all checkpoints
    for (i, checkpoint) in checkpoints.iter().enumerate() {
        storage
            .set_native_asset_delegation_checkpoint(&asset, &account, i as u32, checkpoint)
            .await
            .expect("Should set checkpoint");
    }

    // Update count
    storage
        .set_native_asset_delegation_checkpoint_count(&asset, &account, checkpoints.len() as u32)
        .await
        .expect("Should set count");

    // Verify count
    let count = storage
        .get_native_asset_delegation_checkpoint_count(&asset, &account)
        .await
        .expect("Should get count");
    assert_eq!(count, 4);

    // Verify each checkpoint
    for (i, expected) in checkpoints.iter().enumerate() {
        let retrieved = storage
            .get_native_asset_delegation_checkpoint(&asset, &account, i as u32)
            .await
            .expect("Should get checkpoint");
        assert_eq!(retrieved.from_block, expected.from_block);
        assert_eq!(retrieved.delegatee, expected.delegatee);
    }

    println!("Test H.3 passed: Multiple delegation checkpoints");
}

/// Test H.4: Delegation checkpoints per asset/account isolation
#[tokio::test]
async fn test_delegation_checkpoint_isolation() {
    let temp_dir = TempDir::new("native_asset_test").expect("Failed to create temp dir");
    let mut storage = create_test_storage(&temp_dir);

    let asset1 = random_asset();
    let asset2 = random_asset();
    let account1 = random_account();
    let account2 = random_account();
    let delegatee1 = random_account();
    let delegatee2 = random_account();

    // Set checkpoint for asset1/account1
    let cp1 = DelegationCheckpoint {
        from_block: 100,
        delegatee: delegatee1,
    };
    storage
        .set_native_asset_delegation_checkpoint(&asset1, &account1, 0, &cp1)
        .await
        .expect("Should set checkpoint");
    storage
        .set_native_asset_delegation_checkpoint_count(&asset1, &account1, 1)
        .await
        .expect("Should set count");

    // Set checkpoint for asset2/account2
    let cp2 = DelegationCheckpoint {
        from_block: 200,
        delegatee: delegatee2,
    };
    storage
        .set_native_asset_delegation_checkpoint(&asset2, &account2, 0, &cp2)
        .await
        .expect("Should set checkpoint");
    storage
        .set_native_asset_delegation_checkpoint_count(&asset2, &account2, 1)
        .await
        .expect("Should set count");

    // Verify isolation
    let count1 = storage
        .get_native_asset_delegation_checkpoint_count(&asset1, &account1)
        .await
        .expect("Should get count");
    let count2 = storage
        .get_native_asset_delegation_checkpoint_count(&asset2, &account2)
        .await
        .expect("Should get count");

    // asset1/account2 should be zero
    let cross_count = storage
        .get_native_asset_delegation_checkpoint_count(&asset1, &account2)
        .await
        .expect("Should get count");

    assert_eq!(count1, 1);
    assert_eq!(count2, 1);
    assert_eq!(cross_count, 0, "Cross-account checkpoint should not exist");

    println!("Test H.4 passed: Delegation checkpoints per asset/account isolation");
}

// ============================================================================
// Summary test
// ============================================================================

#[test]
fn test_native_asset_rocksdb_test_summary() {
    println!("\n========================================");
    println!("RocksStorage Native Asset Integration Tests");
    println!("========================================");
    println!("\nTests implemented for TAKO syscall integration:");
    println!();
    println!("A. Lock Index Operations:");
    println!("   A.1 test_lock_index_add_and_get");
    println!("   A.2 test_lock_index_remove");
    println!("   A.3 test_lock_index_duplicate_prevention");
    println!("   A.4 test_lock_index_remove_nonexistent");
    println!();
    println!("B. User Escrow Index Operations:");
    println!("   B.1 test_user_escrow_index_add_and_get");
    println!("   B.2 test_user_escrow_index_remove");
    println!("   B.3 test_user_escrow_multi_user");
    println!();
    println!("C. Owner Agent Index Operations:");
    println!("   C.1 test_owner_agents_add_and_get");
    println!("   C.2 test_owner_agents_remove");
    println!("   C.3 test_owner_agents_duplicate_prevention");
    println!();
    println!("D. Role Members Index Operations:");
    println!("   D.1 test_role_members_add_and_get");
    println!("   D.2 test_role_members_get_by_index");
    println!("   D.3 test_role_members_remove");
    println!("   D.4 test_role_members_duplicate_prevention");
    println!();
    println!("E. Admin Proposal Operations:");
    println!("   E.1 test_pending_admin_set_and_get");
    println!("   E.2 test_pending_admin_clear");
    println!("   E.3 test_pending_admin_per_asset");
    println!();
    println!("F. Integration Tests:");
    println!("   F.1 test_lock_lifecycle_with_index");
    println!("   F.2 test_escrow_lifecycle_with_user_index");
    println!("   F.3 test_role_lifecycle_with_member_index");
    println!();
    println!("G. Balance Checkpoint Operations:");
    println!("   G.1 test_balance_checkpoint_count");
    println!("   G.2 test_balance_checkpoint_set_and_get");
    println!("   G.3 test_balance_checkpoint_multiple");
    println!("   G.4 test_balance_checkpoint_isolation");
    println!();
    println!("H. Delegation Checkpoint Operations:");
    println!("   H.1 test_delegation_checkpoint_count");
    println!("   H.2 test_delegation_checkpoint_set_and_get");
    println!("   H.3 test_delegation_checkpoint_multiple");
    println!("   H.4 test_delegation_checkpoint_isolation");
    println!();
    println!("These tests verify the storage layer for native asset syscalls.");
    println!("========================================\n");
}
