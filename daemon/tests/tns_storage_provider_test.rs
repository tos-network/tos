#![allow(clippy::disallowed_methods)]

//! TNS Storage Provider Unit Tests
//!
//! Tests for the ConfigurableTnsProvider implementation.
//! These tests verify all storage provider operations including:
//! - Name registration (CRUD operations)
//! - Error conditions and fault injection
//! - Invariant verification

use tos_daemon::core::storage::{test_hash, test_public_key, ConfigurableTnsProvider, TnsProvider};

// ============================================================================
// Name Registration Tests - Happy Path
// ============================================================================

#[tokio::test]
async fn test_register_name_success() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    let result = provider
        .register_name(name_hash.clone(), owner.clone())
        .await;
    assert!(result.is_ok());

    // Verify registration
    assert!(provider.is_name_registered(&name_hash).await.unwrap());
    assert!(provider.account_has_name(&owner).await.unwrap());
    assert_eq!(provider.name_count(), 1);
}

#[tokio::test]
async fn test_register_name_multiple_different_names() {
    let mut provider = ConfigurableTnsProvider::new();

    // Register multiple different names for different owners
    for i in 1..=5 {
        let name_hash = test_hash(i);
        let owner = test_public_key(i);
        provider.register_name(name_hash, owner).await.unwrap();
    }

    assert_eq!(provider.name_count(), 5);
}

#[tokio::test]
async fn test_get_name_owner_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();

    let result = provider.get_name_owner(&name_hash).await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_bytes(), owner.as_bytes());
}

#[tokio::test]
async fn test_get_name_owner_not_exists() {
    let provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);

    let result = provider.get_name_owner(&name_hash).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_account_name_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();

    let result = provider.get_account_name(&owner).await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), name_hash);
}

#[tokio::test]
async fn test_get_account_name_not_exists() {
    let provider = ConfigurableTnsProvider::new();
    let owner = test_public_key(1);

    let result = provider.get_account_name(&owner).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_is_name_registered_true() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner)
        .await
        .unwrap();

    assert!(provider.is_name_registered(&name_hash).await.unwrap());
}

#[tokio::test]
async fn test_is_name_registered_false() {
    let provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);

    assert!(!provider.is_name_registered(&name_hash).await.unwrap());
}

#[tokio::test]
async fn test_account_has_name_true() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash, owner.clone())
        .await
        .unwrap();

    assert!(provider.account_has_name(&owner).await.unwrap());
}

#[tokio::test]
async fn test_account_has_name_false() {
    let provider = ConfigurableTnsProvider::new();
    let owner = test_public_key(1);

    assert!(!provider.account_has_name(&owner).await.unwrap());
}

// ============================================================================
// Name Registration Tests - Error Cases
// ============================================================================

#[tokio::test]
async fn test_register_name_already_registered() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner1 = test_public_key(1);
    let owner2 = test_public_key(2);

    // First registration should succeed
    provider
        .register_name(name_hash.clone(), owner1)
        .await
        .unwrap();

    // Second registration with same name should fail
    let result = provider.register_name(name_hash, owner2).await;
    assert!(result.is_err());
    // Check error type
    let err = result.unwrap_err();
    assert!(format!("{:?}", err).contains("TnsNameAlreadyRegistered"));
}

#[tokio::test]
async fn test_register_name_account_already_has_name() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash1 = test_hash(1);
    let name_hash2 = test_hash(2);
    let owner = test_public_key(1);

    // First registration should succeed
    provider
        .register_name(name_hash1, owner.clone())
        .await
        .unwrap();

    // Second registration with same owner should fail
    let result = provider.register_name(name_hash2, owner).await;
    assert!(result.is_err());
    // Check error type
    let err = result.unwrap_err();
    assert!(format!("{:?}", err).contains("TnsAccountAlreadyHasName"));
}

// ============================================================================
// Name Registration Tests - Delete Operations
// ============================================================================

#[tokio::test]
async fn test_delete_name_registration_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();
    assert_eq!(provider.name_count(), 1);

    // Delete the registration
    provider.delete_name_registration(&name_hash).await.unwrap();

    // Verify deletion
    assert!(!provider.is_name_registered(&name_hash).await.unwrap());
    assert!(!provider.account_has_name(&owner).await.unwrap());
    assert_eq!(provider.name_count(), 0);
}

#[tokio::test]
async fn test_delete_name_registration_not_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);

    // Deleting non-existent registration should not error (idempotent)
    let result = provider.delete_name_registration(&name_hash).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_account_name_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();
    assert_eq!(provider.name_count(), 1);

    // Delete by account
    provider.delete_account_name(&owner).await.unwrap();

    // Verify deletion
    assert!(!provider.is_name_registered(&name_hash).await.unwrap());
    assert!(!provider.account_has_name(&owner).await.unwrap());
    assert_eq!(provider.name_count(), 0);
}

#[tokio::test]
async fn test_delete_account_name_not_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let owner = test_public_key(1);

    // Deleting non-existent account name should not error (idempotent)
    let result = provider.delete_account_name(&owner).await;
    assert!(result.is_ok());
}

// ============================================================================
// Fault Injection Tests
// ============================================================================

#[tokio::test]
async fn test_fault_injection_register() {
    let mut provider = ConfigurableTnsProvider::new().fail_on_register();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    let result = provider.register_name(name_hash, owner).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_injection_lookup() {
    let provider = ConfigurableTnsProvider::new().fail_on_lookup();
    let name_hash = test_hash(1);

    let result = provider.is_name_registered(&name_hash).await;
    assert!(result.is_err());

    let result = provider.get_name_owner(&name_hash).await;
    assert!(result.is_err());

    let owner = test_public_key(1);
    let result = provider.account_has_name(&owner).await;
    assert!(result.is_err());

    let result = provider.get_account_name(&owner).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_injection_delete() {
    let mut provider = ConfigurableTnsProvider::new().fail_on_delete();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    let result = provider.delete_name_registration(&name_hash).await;
    assert!(result.is_err());

    let result = provider.delete_account_name(&owner).await;
    assert!(result.is_err());
}

// ============================================================================
// Builder Pattern Tests
// ============================================================================

#[tokio::test]
async fn test_builder_with_registered_name() {
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    let provider = ConfigurableTnsProvider::new().with_registered_name(name_hash.clone(), &owner);

    assert!(provider.is_name_registered(&name_hash).await.unwrap());
    assert!(provider.account_has_name(&owner).await.unwrap());
    assert_eq!(provider.name_count(), 1);
}

#[tokio::test]
async fn test_builder_chaining() {
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    let provider = ConfigurableTnsProvider::new()
        .with_registered_name(name_hash.clone(), &owner)
        .with_mainnet(true);

    assert!(provider.is_name_registered(&name_hash).await.unwrap());
    assert!(provider.is_mainnet());
}

// ============================================================================
// Invariant Tests
// ============================================================================

#[tokio::test]
async fn test_invariant_one_name_per_account() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash1 = test_hash(1);
    let name_hash2 = test_hash(2);
    let owner = test_public_key(1);

    // Register first name
    provider
        .register_name(name_hash1.clone(), owner.clone())
        .await
        .unwrap();

    // Attempt to register second name for same account should fail
    let result = provider.register_name(name_hash2, owner).await;
    assert!(result.is_err());

    // Original registration should still exist
    assert!(provider.is_name_registered(&name_hash1).await.unwrap());
    assert_eq!(provider.name_count(), 1);
}

#[tokio::test]
async fn test_invariant_one_account_per_name() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner1 = test_public_key(1);
    let owner2 = test_public_key(2);

    // Register name for first owner
    provider
        .register_name(name_hash.clone(), owner1.clone())
        .await
        .unwrap();

    // Attempt to register same name for different owner should fail
    let result = provider.register_name(name_hash.clone(), owner2).await;
    assert!(result.is_err());

    // Original owner should still own the name
    let retrieved_owner = provider.get_name_owner(&name_hash).await.unwrap().unwrap();
    assert_eq!(retrieved_owner.as_bytes(), owner1.as_bytes());
}

#[tokio::test]
async fn test_invariant_bidirectional_consistency() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    // Register
    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();

    // Verify bidirectional lookup
    let owner_from_name = provider.get_name_owner(&name_hash).await.unwrap().unwrap();
    let name_from_owner = provider.get_account_name(&owner).await.unwrap().unwrap();

    assert_eq!(owner_from_name.as_bytes(), owner.as_bytes());
    assert_eq!(name_from_owner, name_hash);

    // Delete and verify both directions are cleared
    provider.delete_name_registration(&name_hash).await.unwrap();

    assert!(provider.get_name_owner(&name_hash).await.unwrap().is_none());
    assert!(provider.get_account_name(&owner).await.unwrap().is_none());
}
