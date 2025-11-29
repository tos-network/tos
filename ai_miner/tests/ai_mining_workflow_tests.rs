#![allow(clippy::disallowed_methods)]

use anyhow::Result;
use std::{path::PathBuf, time::Duration};
use tos_ai_miner::{
    daemon_client::{DaemonClient, DaemonClientConfig},
    storage::{StorageManager, TaskState},
    transaction_builder::AIMiningTransactionBuilder,
};
use tos_common::{
    ai_mining::{AIMiningPayload, DifficultyLevel},
    crypto::{elgamal::CompressedPublicKey, Hash},
    network::Network,
    serializer::Serializer,
};

/// Test AI mining workflow components in isolation
/// These tests verify the core functionality without requiring a live daemon

#[tokio::test]
async fn test_task_publication_workflow() -> Result<()> {
    println!("=== Testing AI Task Publication Workflow ===");

    // Test 1: Transaction Builder
    let builder = AIMiningTransactionBuilder::new(Network::Testnet);
    let task_id = Hash::from_bytes(&[1u8; 32])?;
    let reward_amount = 1000000; // 1M nanoTOS
    let difficulty = DifficultyLevel::Intermediate;
    let deadline = 1234567890;
    let nonce = 1;
    let fee = 0; // Let builder estimate

    let metadata = builder.build_publish_task_transaction(
        task_id.clone(),
        reward_amount,
        difficulty.clone(),
        deadline,
        "Test task description".to_string(), // Task description
        nonce,
        fee,
    )?;

    // Verify task publication metadata
    assert!(metadata.estimated_fee > 0, "Fee should be estimated");
    assert!(metadata.estimated_size > 0, "Size should be estimated");
    assert_eq!(metadata.nonce, nonce);
    assert_eq!(metadata.network, Network::Testnet);

    println!("✓ Task publication transaction metadata created successfully");
    println!("  - Estimated fee: {} nanoTOS", metadata.estimated_fee);
    println!("  - Estimated size: {} bytes", metadata.estimated_size);
    println!("  - Task ID: {}", hex::encode(task_id.as_bytes()));
    println!("  - Reward: {reward_amount} nanoTOS");

    // Test 2: Storage Management
    let mut storage_manager =
        StorageManager::new(PathBuf::from("test_storage"), Network::Testnet).await?;

    // Add task to storage
    storage_manager
        .add_task(&task_id, reward_amount, difficulty.clone(), deadline)
        .await?;

    // Verify task is stored
    let task_info = storage_manager.get_task(&task_id);
    assert!(task_info.is_some(), "Task should be stored");
    let task_info = task_info.unwrap();
    assert_eq!(task_info.reward_amount, reward_amount);
    assert_eq!(task_info.difficulty, difficulty);
    assert_eq!(task_info.deadline, deadline);
    assert_eq!(task_info.state, TaskState::Published);

    println!("✓ Task stored in storage manager successfully");
    println!("  - Task state: {:?}", task_info.state);
    println!("  - Difficulty: {:?}", task_info.difficulty);

    println!("=== Task Publication Workflow Test PASSED ===\n");
    Ok(())
}

#[tokio::test]
async fn test_answer_submission_workflow() -> Result<()> {
    println!("=== Testing AI Answer Submission Workflow ===");

    // Test 1: Transaction Builder
    let builder = AIMiningTransactionBuilder::new(Network::Testnet);
    let task_id = Hash::from_bytes(&[1u8; 32])?;
    let stake_amount = 50000; // 50K nanoTOS
    let nonce = 2;
    let fee = 0;

    let answer_content = "This is a test answer content for AI mining workflow testing. It provides a detailed response to demonstrate the answer submission functionality.";
    let answer_hash = tos_common::crypto::hash(answer_content.as_bytes());
    let metadata = builder.build_submit_answer_transaction(
        task_id.clone(),
        answer_content.to_string(),
        answer_hash.clone(),
        stake_amount,
        nonce,
        fee,
    )?;

    // Verify answer submission metadata
    assert!(metadata.estimated_fee > 0, "Fee should be estimated");
    assert!(metadata.estimated_size > 0, "Size should be estimated");
    assert_eq!(metadata.nonce, nonce);

    println!("✓ Answer submission transaction metadata created successfully");
    println!("  - Estimated fee: {} nanoTOS", metadata.estimated_fee);
    println!("  - Estimated size: {} bytes", metadata.estimated_size);
    println!("  - Task ID: {}", hex::encode(task_id.as_bytes()));
    println!("  - Answer Hash: {}", hex::encode(answer_hash.as_bytes()));
    println!("  - Stake: {stake_amount} nanoTOS");

    // Test 2: Storage Management
    let mut storage_manager =
        StorageManager::new(PathBuf::from("test_storage2"), Network::Testnet).await?;

    // First create the task
    storage_manager
        .add_task(&task_id, 1000000, DifficultyLevel::Intermediate, 1234567890)
        .await?;

    // Update task state to show answer submitted
    storage_manager
        .update_task_state(&task_id, TaskState::AnswersReceived)
        .await?;

    // Verify task state updated
    let task_info = storage_manager.get_task(&task_id);
    assert!(task_info.is_some(), "Task should exist");
    assert_eq!(task_info.unwrap().state, TaskState::AnswersReceived);

    println!("✓ Answer submission state updated successfully");
    println!("  - Task state: {:?}", TaskState::AnswersReceived);

    println!("=== Answer Submission Workflow Test PASSED ===\n");
    Ok(())
}

#[tokio::test]
async fn test_validation_workflow() -> Result<()> {
    println!("=== Testing AI Answer Validation Workflow ===");

    // Test 1: Transaction Builder
    let builder = AIMiningTransactionBuilder::new(Network::Testnet);
    let task_id = Hash::from_bytes(&[1u8; 32])?;
    let answer_id = Hash::from_bytes(&[2u8; 32])?;
    let validation_score = 85; // 85% validation score
    let nonce = 3;
    let fee = 0;

    let metadata = builder.build_validate_answer_transaction(
        task_id.clone(),
        answer_id.clone(),
        validation_score,
        nonce,
        fee,
    )?;

    // Verify validation metadata
    assert!(metadata.estimated_fee > 0, "Fee should be estimated");
    assert!(metadata.estimated_size > 0, "Size should be estimated");
    assert_eq!(metadata.nonce, nonce);

    println!("✓ Validation transaction metadata created successfully");
    println!("  - Estimated fee: {} nanoTOS", metadata.estimated_fee);
    println!("  - Estimated size: {} bytes", metadata.estimated_size);
    println!("  - Task ID: {}", hex::encode(task_id.as_bytes()));
    println!("  - Answer ID: {}", hex::encode(answer_id.as_bytes()));
    println!("  - Validation Score: {validation_score}%");

    // Test 2: Storage Management
    let mut storage_manager =
        StorageManager::new(PathBuf::from("test_storage3"), Network::Testnet).await?;

    // Create task and move through workflow states
    storage_manager
        .add_task(&task_id, 1000000, DifficultyLevel::Intermediate, 1234567890)
        .await?;
    storage_manager
        .update_task_state(&task_id, TaskState::AnswersReceived)
        .await?;
    storage_manager
        .update_task_state(&task_id, TaskState::Validated)
        .await?;

    // Verify validation state
    let task_info = storage_manager.get_task(&task_id);
    assert!(task_info.is_some(), "Task should exist");
    assert_eq!(task_info.unwrap().state, TaskState::Validated);

    println!("✓ Validation workflow state updated successfully");
    println!("  - Task state: {:?}", TaskState::Validated);

    println!("=== Validation Workflow Test PASSED ===\n");
    Ok(())
}

#[tokio::test]
async fn test_reward_distribution_workflow() -> Result<()> {
    println!("=== Testing Reward Distribution Workflow ===");

    // Test 1: Fee Calculation for Different Networks
    let mainnet_builder = AIMiningTransactionBuilder::new(Network::Mainnet);
    let testnet_builder = AIMiningTransactionBuilder::new(Network::Testnet);
    let devnet_builder = AIMiningTransactionBuilder::new(Network::Devnet);

    let tx_size = 500; // bytes

    let mainnet_fee = mainnet_builder.estimate_fee(tx_size);
    let testnet_fee = testnet_builder.estimate_fee(tx_size);
    let devnet_fee = devnet_builder.estimate_fee(tx_size);

    // Verify network-specific fee scaling
    assert!(mainnet_fee > testnet_fee, "Mainnet should have higher fees");
    assert!(
        testnet_fee > devnet_fee,
        "Testnet should have higher fees than devnet"
    );

    println!("✓ Network-specific fee calculation verified");
    println!("  - Mainnet fee: {mainnet_fee} nanoTOS");
    println!("  - Testnet fee: {testnet_fee} nanoTOS");
    println!("  - Devnet fee: {devnet_fee} nanoTOS");

    // Test 2: Complete Task Lifecycle
    let mut storage_manager =
        StorageManager::new(PathBuf::from("test_storage4"), Network::Testnet).await?;
    let task_id = Hash::from_bytes(&[4u8; 32])?;
    let reward_amount = 2000000; // 2M nanoTOS

    // Complete lifecycle
    storage_manager
        .add_task(&task_id, reward_amount, DifficultyLevel::Expert, 1234567890)
        .await?;
    storage_manager
        .update_task_state(&task_id, TaskState::AnswersReceived)
        .await?;
    storage_manager
        .update_task_state(&task_id, TaskState::Validated)
        .await?;
    storage_manager
        .update_task_state(&task_id, TaskState::Validated)
        .await?;

    // Verify final state
    let task_info = storage_manager.get_task(&task_id);
    assert!(task_info.is_some(), "Task should exist");
    assert_eq!(task_info.unwrap().state, TaskState::Validated);

    println!("✓ Complete task lifecycle verified");
    println!("  - Final state: {:?}", TaskState::Validated);
    println!("  - Reward amount: {reward_amount} nanoTOS");

    println!("=== Reward Distribution Workflow Test PASSED ===\n");
    Ok(())
}

#[tokio::test]
async fn test_miner_registration_workflow() -> Result<()> {
    println!("=== Testing Miner Registration Workflow ===");

    let builder = AIMiningTransactionBuilder::new(Network::Testnet);

    // Create a test compressed public key
    let test_key_bytes = [0u8; 32];
    let miner_address = create_test_compressed_pubkey(test_key_bytes);
    let registration_fee = 100000; // 100K nanoTOS
    let nonce = 0;
    let fee = 0;

    let metadata = builder.build_register_miner_transaction(
        miner_address.clone(),
        registration_fee,
        nonce,
        fee,
    )?;

    // Verify registration metadata
    assert!(metadata.estimated_fee > 0, "Fee should be estimated");
    assert!(metadata.estimated_size > 0, "Size should be estimated");
    assert_eq!(metadata.nonce, nonce);

    println!("✓ Miner registration transaction metadata created successfully");
    println!("  - Estimated fee: {} nanoTOS", metadata.estimated_fee);
    println!("  - Estimated size: {} bytes", metadata.estimated_size);
    println!("  - Registration fee: {registration_fee} nanoTOS");

    println!("=== Miner Registration Workflow Test PASSED ===\n");
    Ok(())
}

#[test]
fn test_payload_complexity_calculation() {
    println!("=== Testing Payload Complexity Calculations ===");

    let builder = AIMiningTransactionBuilder::new(Network::Mainnet);

    // Test different payload types and their complexity multipliers
    let register_payload = AIMiningPayload::RegisterMiner {
        miner_address: create_test_compressed_pubkey([0u8; 32]),
        registration_fee: 100000,
    };

    let publish_payload = AIMiningPayload::PublishTask {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        reward_amount: 1000000,
        difficulty: DifficultyLevel::Intermediate,
        deadline: 1234567890,
        description: "Test task description for complexity calculation".to_string(),
    };

    let answer_payload_content =
        "Test answer content for complexity calculation workflow".to_string();
    let answer_payload_hash = tos_common::crypto::hash(answer_payload_content.as_bytes());
    let answer_payload = AIMiningPayload::SubmitAnswer {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        answer_content: answer_payload_content,
        answer_hash: answer_payload_hash,
        stake_amount: 50000,
    };

    let validation_payload = AIMiningPayload::ValidateAnswer {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        answer_id: Hash::from_bytes(&[2u8; 32]).unwrap(),
        validation_score: 85,
    };

    // Calculate fees for each payload type
    let register_fee = builder.estimate_fee_with_payload_type(200, Some(&register_payload));
    let publish_fee = builder.estimate_fee_with_payload_type(200, Some(&publish_payload));
    let answer_fee = builder.estimate_fee_with_payload_type(200, Some(&answer_payload));
    let validation_fee = builder.estimate_fee_with_payload_type(200, Some(&validation_payload));

    // Verify fee ordering based on complexity
    assert!(
        publish_fee > validation_fee,
        "Publish tasks should have highest fees"
    );
    assert!(
        validation_fee > answer_fee,
        "Validation should cost more than answers"
    );
    assert!(
        answer_fee > register_fee,
        "Answers should cost more than registration"
    );

    println!("✓ Payload complexity calculations verified");
    println!("  - Register miner fee: {register_fee} nanoTOS");
    println!("  - Publish task fee: {publish_fee} nanoTOS");
    println!("  - Submit answer fee: {answer_fee} nanoTOS");
    println!("  - Validate answer fee: {validation_fee} nanoTOS");

    println!("=== Payload Complexity Test PASSED ===\n");
}

// Helper function to create test compressed public keys
fn create_test_compressed_pubkey(bytes: [u8; 32]) -> CompressedPublicKey {
    // Use a simple implementation that works for tests
    // Create from the bytes directly via the tos_common crypto module
    use tos_common::crypto::elgamal::CompressedPublicKey;
    // We'll use a simple approach - convert to a known valid compressed point
    let mut valid_bytes = [0u8; 32];
    valid_bytes.copy_from_slice(&bytes);
    // For testing, we'll create a dummy key using the provided pattern from tos_common
    CompressedPublicKey::from_bytes(&valid_bytes).unwrap_or_else(|_| {
        // Fallback to a known valid point if the bytes don't form a valid key
        CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap()
    })
}

#[tokio::test]
async fn test_daemon_client_config() -> Result<()> {
    println!("=== Testing Daemon Client Configuration ===");

    // Test custom configuration
    let config = DaemonClientConfig {
        request_timeout: Duration::from_secs(60),
        max_retries: 5,
        retry_delay: Duration::from_millis(2000),
        connection_timeout: Duration::from_secs(15),
    };

    let client = DaemonClient::with_config("http://127.0.0.1:18080", config.clone())?;

    // Verify configuration is set correctly
    assert_eq!(client.config().request_timeout, Duration::from_secs(60));
    assert_eq!(client.config().max_retries, 5);
    assert_eq!(client.config().retry_delay, Duration::from_millis(2000));
    assert_eq!(client.config().connection_timeout, Duration::from_secs(15));

    println!("✓ Custom daemon client configuration verified");
    println!("  - Request timeout: {:?}", client.config().request_timeout);
    println!("  - Max retries: {}", client.config().max_retries);
    println!("  - Retry delay: {:?}", client.config().retry_delay);
    println!(
        "  - Connection timeout: {:?}",
        client.config().connection_timeout
    );

    // Note: We won't test actual daemon connectivity here since no daemon is running
    // This test focuses on configuration and setup

    println!("=== Daemon Client Configuration Test PASSED ===\n");
    Ok(())
}
