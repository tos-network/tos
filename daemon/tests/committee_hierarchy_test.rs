//! Committee Hierarchy Traversal Tests
//!
//! Tests for the `get_all_committee_ids` function to verify it correctly
//! traverses the full committee hierarchy tree (not just direct children).
//!
//! Test scenarios:
//! - Single global committee (no children)
//! - Global with direct children only (depth 1)
//! - Full hierarchy tree traversal (depth 3+)
//! - Circular reference protection
//! - Large hierarchy stress test

#![allow(clippy::disallowed_methods)]

use tempdir::TempDir;
use tos_common::{
    crypto::{Hash, KeyPair, PublicKey},
    kyc::{CommitteeMember, KycRegion, MemberRole, SecurityCommittee},
    network::Network,
};
use tos_daemon::core::{
    config::RocksDBConfig,
    storage::{
        rocksdb::{CacheMode, CompressionMode, RocksStorage},
        CommitteeProvider,
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

/// Generate a random public key for testing
fn random_pubkey() -> PublicKey {
    KeyPair::new().get_public_key().compress()
}

/// Create test committee members (minimum 3 required)
fn create_test_members() -> Vec<CommitteeMember> {
    let now = 1000u64;
    vec![
        CommitteeMember::new(
            random_pubkey(),
            Some("Member1".to_string()),
            MemberRole::Chair,
            now,
        ),
        CommitteeMember::new(
            random_pubkey(),
            Some("Member2".to_string()),
            MemberRole::ViceChair,
            now,
        ),
        CommitteeMember::new(
            random_pubkey(),
            Some("Member3".to_string()),
            MemberRole::Member,
            now,
        ),
    ]
}

/// Create a global committee for testing
fn create_global_committee(name: &str) -> SecurityCommittee {
    SecurityCommittee::new_global(
        name.to_string(),
        create_test_members(),
        2,  // threshold
        1,  // kyc_threshold
        80, // max_kyc_level (Tier 8)
        1000,
    )
}

/// Create a regional committee for testing
fn create_regional_committee(
    name: &str,
    region: KycRegion,
    parent_id: Hash,
    max_kyc_level: u16,
) -> SecurityCommittee {
    SecurityCommittee::new_regional(
        name.to_string(),
        region,
        create_test_members(),
        2, // threshold
        1, // kyc_threshold
        max_kyc_level,
        parent_id,
        1000,
    )
}

// ============================================================================
// Test: Single Global Committee (No Children)
// ============================================================================

/// Test that get_all_committee_ids returns only the global committee when there are no children
#[tokio::test]
async fn test_single_global_committee_no_children() {
    let temp_dir = TempDir::new("committee_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    // Bootstrap global committee
    let global = create_global_committee("GlobalCommittee");
    let global_id = storage
        .bootstrap_global_committee(global, 100, &Hash::zero())
        .await
        .expect("Should bootstrap global committee");

    // Get all committee IDs
    let all_ids = storage
        .get_all_committee_ids()
        .await
        .expect("Should get all committee IDs");

    // Should only contain the global committee
    assert_eq!(all_ids.len(), 1, "Should have exactly 1 committee");
    assert_eq!(all_ids[0], global_id, "Should be the global committee ID");

    println!("Test passed: Single global committee (no children)");
}

// ============================================================================
// Test: Global with Direct Children Only (Depth 1)
// ============================================================================

/// Test that get_all_committee_ids returns global + direct children
#[tokio::test]
async fn test_global_with_direct_children() {
    let temp_dir = TempDir::new("committee_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    // Bootstrap global committee
    let global = create_global_committee("GlobalCommittee");
    let global_id = storage
        .bootstrap_global_committee(global, 100, &Hash::zero())
        .await
        .expect("Should bootstrap global committee");

    // Register 3 direct child committees
    let asia = create_regional_committee(
        "AsiaCommittee",
        KycRegion::AsiaPacific,
        global_id.clone(),
        60,
    );
    let europe =
        create_regional_committee("EuropeCommittee", KycRegion::Europe, global_id.clone(), 60);
    let americas = create_regional_committee(
        "AmericasCommittee",
        KycRegion::NorthAmerica,
        global_id.clone(),
        60,
    );

    let asia_id = storage
        .register_committee(asia, &global_id, 101, &Hash::zero())
        .await
        .expect("Should register Asia committee");
    let europe_id = storage
        .register_committee(europe, &global_id, 102, &Hash::zero())
        .await
        .expect("Should register Europe committee");
    let americas_id = storage
        .register_committee(americas, &global_id, 103, &Hash::zero())
        .await
        .expect("Should register Americas committee");

    // Get all committee IDs
    let all_ids = storage
        .get_all_committee_ids()
        .await
        .expect("Should get all committee IDs");

    // Should contain global + 3 children = 4 total
    assert_eq!(all_ids.len(), 4, "Should have exactly 4 committees");
    assert!(
        all_ids.contains(&global_id),
        "Should contain global committee"
    );
    assert!(all_ids.contains(&asia_id), "Should contain Asia committee");
    assert!(
        all_ids.contains(&europe_id),
        "Should contain Europe committee"
    );
    assert!(
        all_ids.contains(&americas_id),
        "Should contain Americas committee"
    );

    println!("Test passed: Global with direct children (depth 1)");
}

// ============================================================================
// Test: Full Hierarchy Tree Traversal (Depth 3+)
// ============================================================================

/// Test that get_all_committee_ids traverses the full hierarchy tree
/// This tests the fix for Issue #9: Committee hierarchy traversal for full depth
///
/// Hierarchy:
/// Global (Tier 8)
/// +-- Asia (Tier 6)
/// |   +-- AsiaSubA (Tier 4)
/// |   |   +-- AsiaSubA1 (Tier 2)
/// |   +-- AsiaSubB (Tier 4)
/// +-- Europe (Tier 6)
/// |   +-- EuropeSubA (Tier 4)
/// +-- Americas (Tier 6)
#[tokio::test]
async fn test_full_hierarchy_traversal_depth_3() {
    let temp_dir = TempDir::new("committee_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    // Level 0: Bootstrap global committee
    let global = create_global_committee("GlobalCommittee");
    let global_id = storage
        .bootstrap_global_committee(global, 100, &Hash::zero())
        .await
        .expect("Should bootstrap global committee");

    // Level 1: Register regional committees under global
    let asia = create_regional_committee(
        "AsiaCommittee",
        KycRegion::AsiaPacific,
        global_id.clone(),
        60,
    );
    let europe =
        create_regional_committee("EuropeCommittee", KycRegion::Europe, global_id.clone(), 60);
    let americas = create_regional_committee(
        "AmericasCommittee",
        KycRegion::NorthAmerica,
        global_id.clone(),
        60,
    );

    let asia_id = storage
        .register_committee(asia, &global_id, 101, &Hash::zero())
        .await
        .expect("Should register Asia committee");
    let europe_id = storage
        .register_committee(europe, &global_id, 102, &Hash::zero())
        .await
        .expect("Should register Europe committee");
    let americas_id = storage
        .register_committee(americas, &global_id, 103, &Hash::zero())
        .await
        .expect("Should register Americas committee");

    // Level 2: Register sub-committees under Asia and Europe
    let asia_sub_a =
        create_regional_committee("AsiaSubA", KycRegion::AsiaPacific, asia_id.clone(), 40);
    let asia_sub_b =
        create_regional_committee("AsiaSubB", KycRegion::AsiaPacific, asia_id.clone(), 40);
    let europe_sub_a =
        create_regional_committee("EuropeSubA", KycRegion::Europe, europe_id.clone(), 40);

    let asia_sub_a_id = storage
        .register_committee(asia_sub_a, &asia_id, 104, &Hash::zero())
        .await
        .expect("Should register AsiaSubA committee");
    let asia_sub_b_id = storage
        .register_committee(asia_sub_b, &asia_id, 105, &Hash::zero())
        .await
        .expect("Should register AsiaSubB committee");
    let europe_sub_a_id = storage
        .register_committee(europe_sub_a, &europe_id, 106, &Hash::zero())
        .await
        .expect("Should register EuropeSubA committee");

    // Level 3: Register sub-sub-committee under AsiaSubA
    let asia_sub_a1 = create_regional_committee(
        "AsiaSubA1",
        KycRegion::AsiaPacific,
        asia_sub_a_id.clone(),
        20,
    );

    let asia_sub_a1_id = storage
        .register_committee(asia_sub_a1, &asia_sub_a_id, 107, &Hash::zero())
        .await
        .expect("Should register AsiaSubA1 committee");

    // Get all committee IDs
    let all_ids = storage
        .get_all_committee_ids()
        .await
        .expect("Should get all committee IDs");

    // Should contain all 8 committees (1 global + 3 L1 + 3 L2 + 1 L3)
    assert_eq!(all_ids.len(), 8, "Should have exactly 8 committees");

    // Verify all committees are present
    assert!(
        all_ids.contains(&global_id),
        "Should contain global committee"
    );
    assert!(all_ids.contains(&asia_id), "Should contain Asia committee");
    assert!(
        all_ids.contains(&europe_id),
        "Should contain Europe committee"
    );
    assert!(
        all_ids.contains(&americas_id),
        "Should contain Americas committee"
    );
    assert!(
        all_ids.contains(&asia_sub_a_id),
        "Should contain AsiaSubA committee"
    );
    assert!(
        all_ids.contains(&asia_sub_b_id),
        "Should contain AsiaSubB committee"
    );
    assert!(
        all_ids.contains(&europe_sub_a_id),
        "Should contain EuropeSubA committee"
    );
    assert!(
        all_ids.contains(&asia_sub_a1_id),
        "Should contain AsiaSubA1 committee (depth 3)"
    );

    println!("Test passed: Full hierarchy traversal (depth 3)");
}

// ============================================================================
// Test: get_active_committees uses full traversal
// ============================================================================

/// Test that get_active_committees finds committees at all depths
#[tokio::test]
async fn test_get_active_committees_full_depth() {
    let temp_dir = TempDir::new("committee_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    // Bootstrap global committee
    let global = create_global_committee("GlobalCommittee");
    let global_id = storage
        .bootstrap_global_committee(global, 100, &Hash::zero())
        .await
        .expect("Should bootstrap global committee");

    // Level 1: Register Asia under global
    let asia = create_regional_committee(
        "AsiaCommittee",
        KycRegion::AsiaPacific,
        global_id.clone(),
        60,
    );
    let asia_id = storage
        .register_committee(asia, &global_id, 101, &Hash::zero())
        .await
        .expect("Should register Asia committee");

    // Level 2: Register AsiaSubA under Asia
    let asia_sub_a =
        create_regional_committee("AsiaSubA", KycRegion::AsiaPacific, asia_id.clone(), 40);
    let _asia_sub_a_id = storage
        .register_committee(asia_sub_a, &asia_id, 102, &Hash::zero())
        .await
        .expect("Should register AsiaSubA committee");

    // Get all active committees
    let active = storage
        .get_active_committees()
        .await
        .expect("Should get active committees");

    // All 3 committees should be active by default
    assert_eq!(active.len(), 3, "Should have 3 active committees");

    println!("Test passed: get_active_committees uses full depth traversal");
}

// ============================================================================
// Test: get_committees_by_region uses full traversal
// ============================================================================

/// Test that get_committees_by_region finds committees at all depths
#[tokio::test]
async fn test_get_committees_by_region_full_depth() {
    let temp_dir = TempDir::new("committee_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    // Bootstrap global committee
    let global = create_global_committee("GlobalCommittee");
    let global_id = storage
        .bootstrap_global_committee(global, 100, &Hash::zero())
        .await
        .expect("Should bootstrap global committee");

    // Level 1: Register Asia under global
    let asia = create_regional_committee(
        "AsiaCommittee",
        KycRegion::AsiaPacific,
        global_id.clone(),
        60,
    );
    let asia_id = storage
        .register_committee(asia, &global_id, 101, &Hash::zero())
        .await
        .expect("Should register Asia committee");

    // Level 2: Register two Asia sub-committees
    let asia_sub_a =
        create_regional_committee("AsiaSubA", KycRegion::AsiaPacific, asia_id.clone(), 40);
    let asia_sub_b =
        create_regional_committee("AsiaSubB", KycRegion::AsiaPacific, asia_id.clone(), 40);

    let asia_sub_a_id = storage
        .register_committee(asia_sub_a, &asia_id, 102, &Hash::zero())
        .await
        .expect("Should register AsiaSubA committee");
    let _asia_sub_b_id = storage
        .register_committee(asia_sub_b, &asia_id, 103, &Hash::zero())
        .await
        .expect("Should register AsiaSubB committee");

    // Level 3: Register AsiaSubA1 under AsiaSubA
    let asia_sub_a1 = create_regional_committee(
        "AsiaSubA1",
        KycRegion::AsiaPacific,
        asia_sub_a_id.clone(),
        20,
    );
    let _asia_sub_a1_id = storage
        .register_committee(asia_sub_a1, &asia_sub_a_id, 104, &Hash::zero())
        .await
        .expect("Should register AsiaSubA1 committee");

    // Get all Asia region committees
    let asia_committees = storage
        .get_committees_by_region(KycRegion::AsiaPacific)
        .await
        .expect("Should get Asia committees");

    // Should find all 4 Asia committees (including nested ones)
    assert_eq!(
        asia_committees.len(),
        4,
        "Should have 4 Asia region committees at all depths"
    );

    println!("Test passed: get_committees_by_region uses full depth traversal");
}

// ============================================================================
// Test: get_committee_count uses full traversal
// ============================================================================

/// Test that get_committee_count returns the correct total count
#[tokio::test]
async fn test_get_committee_count_full_depth() {
    let temp_dir = TempDir::new("committee_test").unwrap();
    let mut storage = create_test_storage(&temp_dir);

    // Bootstrap global committee
    let global = create_global_committee("GlobalCommittee");
    let global_id = storage
        .bootstrap_global_committee(global, 100, &Hash::zero())
        .await
        .expect("Should bootstrap global committee");

    // Level 1
    let asia = create_regional_committee(
        "AsiaCommittee",
        KycRegion::AsiaPacific,
        global_id.clone(),
        60,
    );
    let asia_id = storage
        .register_committee(asia, &global_id, 101, &Hash::zero())
        .await
        .expect("Should register Asia committee");

    // Level 2
    let asia_sub =
        create_regional_committee("AsiaSub", KycRegion::AsiaPacific, asia_id.clone(), 40);
    let asia_sub_id = storage
        .register_committee(asia_sub, &asia_id, 102, &Hash::zero())
        .await
        .expect("Should register AsiaSub committee");

    // Level 3
    let asia_sub_sub = create_regional_committee(
        "AsiaSubSub",
        KycRegion::AsiaPacific,
        asia_sub_id.clone(),
        20,
    );
    let _asia_sub_sub_id = storage
        .register_committee(asia_sub_sub, &asia_sub_id, 103, &Hash::zero())
        .await
        .expect("Should register AsiaSubSub committee");

    // Get committee count
    let count = storage
        .get_committee_count()
        .await
        .expect("Should get committee count");

    assert_eq!(count, 4, "Should have 4 committees total (all depths)");

    println!("Test passed: get_committee_count uses full depth traversal");
}

// ============================================================================
// Test: Empty hierarchy (no global committee)
// ============================================================================

/// Test that get_all_committee_ids returns empty when no global committee exists
#[tokio::test]
async fn test_empty_hierarchy_no_global() {
    let temp_dir = TempDir::new("committee_test").unwrap();
    let storage = create_test_storage(&temp_dir);

    // Get all committee IDs without bootstrapping any committee
    let all_ids = storage
        .get_all_committee_ids()
        .await
        .expect("Should get all committee IDs");

    assert!(
        all_ids.is_empty(),
        "Should return empty vec when no committees exist"
    );

    println!("Test passed: Empty hierarchy (no global committee)");
}
