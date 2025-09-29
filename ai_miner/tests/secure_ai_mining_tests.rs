use anyhow::Result;
use tos_ai_miner::{
    transaction_builder::AIMiningTransactionBuilder,
};
use tos_common::{
    ai_mining::{
        calculate_base_reward, calculate_final_reward, calculate_secure_gas_cost, AIMiningPayload,
        AccountReputation, AntiSybilDetector, DifficultyLevel, ADVANCED_TASK_BASE_REWARD,
        BASIC_TASK_BASE_REWARD, EXPERT_TASK_BASE_REWARD, INTERMEDIATE_TASK_BASE_REWARD,
        LONG_CONTENT_GAS_RATE, MEDIUM_CONTENT_GAS_RATE, MEDIUM_CONTENT_THRESHOLD,
        MIN_REPUTATION_FOR_BASIC, MIN_TRANSACTION_COST, SHORT_CONTENT_GAS_RATE,
        SHORT_CONTENT_THRESHOLD,
    },
    crypto::{elgamal::CompressedPublicKey, Hash},
    network::Network,
    serializer::Serializer,
};

/// Test new secure economic model
/// Including tiered pricing, reputation system, anti-Sybil mechanisms, etc.

#[test]
fn test_tiered_gas_pricing() {
    println!("=== Testing Tiered Gas Pricing Model ===");

    // Test 1: Short content pricing (0-200 bytes)
    let short_content = "A".repeat(100); // 100 bytes
    let short_hash = tos_common::crypto::hash(short_content.as_bytes());
    let short_payload = AIMiningPayload::SubmitAnswer {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        answer_content: short_content.clone(),
        answer_hash: short_hash,
        stake_amount: 50000,
    };

    let short_gas = short_payload.calculate_content_gas_cost();
    let expected_short = (100 * SHORT_CONTENT_GAS_RATE).max(MIN_TRANSACTION_COST);
    assert_eq!(short_gas, expected_short);
    println!("✓ Short content (100 bytes): {} nanoTOS", short_gas);

    // Test 2: Medium content pricing (200-1000 bytes)
    let medium_content = "B".repeat(500); // 500 bytes
    let medium_hash = tos_common::crypto::hash(medium_content.as_bytes());
    let medium_payload = AIMiningPayload::SubmitAnswer {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        answer_content: medium_content.clone(),
        answer_hash: medium_hash,
        stake_amount: 50000,
    };

    let medium_gas = medium_payload.calculate_content_gas_cost();
    let expected_medium = (SHORT_CONTENT_THRESHOLD as u64 * SHORT_CONTENT_GAS_RATE)
        + ((500 - SHORT_CONTENT_THRESHOLD) as u64 * MEDIUM_CONTENT_GAS_RATE);
    assert_eq!(medium_gas, expected_medium.max(MIN_TRANSACTION_COST));
    println!("✓ Medium content (500 bytes): {} nanoTOS", medium_gas);

    // Test 3: Long content pricing (1000+ bytes)
    let long_content = "C".repeat(1500); // 1500 bytes
    let long_hash = tos_common::crypto::hash(long_content.as_bytes());
    let long_payload = AIMiningPayload::SubmitAnswer {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        answer_content: long_content.clone(),
        answer_hash: long_hash,
        stake_amount: 50000,
    };

    let long_gas = long_payload.calculate_content_gas_cost();
    let expected_long = (SHORT_CONTENT_THRESHOLD as u64 * SHORT_CONTENT_GAS_RATE)
        + ((MEDIUM_CONTENT_THRESHOLD - SHORT_CONTENT_THRESHOLD) as u64 * MEDIUM_CONTENT_GAS_RATE)
        + ((1500 - MEDIUM_CONTENT_THRESHOLD) as u64 * LONG_CONTENT_GAS_RATE);
    assert_eq!(long_gas, expected_long.max(MIN_TRANSACTION_COST));
    println!("✓ Long content (1500 bytes): {} nanoTOS", long_gas);

    // Verify fee increment
    assert!(
        medium_gas > short_gas,
        "Medium content should cost more than short"
    );
    assert!(
        long_gas > medium_gas,
        "Long content should cost more than medium"
    );

    println!("=== Tiered Gas Pricing Test PASSED ===\n");
}

#[test]
fn test_reputation_system() {
    println!("=== Testing Reputation System ===");

    let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();
    let created_at = 1000000;
    let current_time = 1000000 + 30 * 24 * 3600; // 30 days later

    let mut reputation = AccountReputation::new(account.clone(), created_at);
    reputation.transaction_count = 100; // 100 transactions
    reputation.stake_amount = 1_000_000; // 0.001 TOS stake
    reputation.successful_validations = 45;
    reputation.total_validations = 50;

    let score = reputation.calculate_reputation_score(current_time);

    // Verify reputation score calculation
    // Full age score(1.0 * 0.3) + full transaction score(1.0 * 0.4) + full stake score(1.0 * 0.3) = 1.0
    // Validation accuracy bonus: (0.9 - 0.8) * 1.0 = 0.1
    // Long-term participation bonus: 0.1
    // Total: 1.0 + 0.1 + 0.1 = 1.2, but capped at 1.0
    assert_eq!(score, 1.0);
    assert_eq!(reputation.reputation_score, 1.0);

    println!("✓ High reputation account score: {:.2}", score);

    // Test permission checks
    assert!(reputation.can_participate_in_difficulty(&DifficultyLevel::Beginner));
    assert!(reputation.can_participate_in_difficulty(&DifficultyLevel::Intermediate));
    assert!(reputation.can_participate_in_difficulty(&DifficultyLevel::Advanced));
    assert!(reputation.can_participate_in_difficulty(&DifficultyLevel::Expert));

    println!("✓ Permission checks passed for all difficulty levels");

    // Test new account (low reputation)
    let mut new_reputation = AccountReputation::new(account, current_time - 3600); // Created 1 hour ago
    new_reputation.transaction_count = 0;
    new_reputation.stake_amount = 0;

    let new_score = new_reputation.calculate_reputation_score(current_time);
    assert!(new_score < MIN_REPUTATION_FOR_BASIC);
    assert!(!new_reputation.can_participate_in_difficulty(&DifficultyLevel::Beginner));

    println!(
        "✓ New account correctly restricted: score = {:.3}",
        new_score
    );

    println!("=== Reputation System Test PASSED ===\n");
}

#[test]
fn test_anti_sybil_detection() {
    println!("=== Testing Anti-Sybil Detection ===");

    let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();
    let current_time = 10_000_000u64;

    // Test high-risk account (new account, no transactions, no stake)
    let high_risk_reputation = AccountReputation::new(account.clone(), current_time - 3600); // Created 1 hour ago
    let result = AntiSybilDetector::detect_sybil_risk(&high_risk_reputation, current_time);

    assert!(!result.is_valid, "High risk account should be flagged");
    println!("✓ High risk account detection: {:?}", result.risk_level);
    println!("  Details: {:?}", result.details);

    // Test normal account
    let base_time = 10_000_000u64;
    let mut normal_reputation = AccountReputation::new(account, base_time - 30 * 24 * 3600); // Created 30 days ago
    normal_reputation.transaction_count = 50;
    normal_reputation.stake_amount = 500_000; // 0.0005 TOS
    normal_reputation.calculate_reputation_score(base_time);

    let normal_result = AntiSybilDetector::detect_sybil_risk(&normal_reputation, base_time);
    assert!(normal_result.is_valid, "Normal account should pass");
    println!("✓ Normal account detection: {:?}", normal_result.risk_level);

    println!("=== Anti-Sybil Detection Test PASSED ===\n");
}

#[test]
fn test_secure_gas_calculation() {
    println!("=== Testing Secure Gas Calculation ===");

    let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();

    // High reputation user
    let base_time = 10_000_000u64; // Use a larger base time to avoid overflow
    let mut high_rep = AccountReputation::new(account.clone(), base_time - 90 * 24 * 3600); // Created 90 days ago
    high_rep.transaction_count = 200;
    high_rep.stake_amount = 2_000_000; // 0.002 TOS
    high_rep.calculate_reputation_score(base_time);

    let high_rep_gas = calculate_secure_gas_cost(
        300, // 300 bytes content
        &DifficultyLevel::Advanced,
        &high_rep,
        2_000_000, // High stake
    );

    // Low reputation user
    let mut low_rep = AccountReputation::new(account, base_time - 3600); // Created 1 hour ago
    low_rep.calculate_reputation_score(base_time);

    let low_rep_gas = calculate_secure_gas_cost(
        300, // 300 bytes content
        &DifficultyLevel::Advanced,
        &low_rep,
        0, // No stake
    );

    assert!(
        low_rep_gas > high_rep_gas,
        "Low reputation should pay higher fees"
    );
    assert!(
        high_rep_gas >= MIN_TRANSACTION_COST,
        "Should meet minimum cost"
    );
    assert!(
        low_rep_gas >= MIN_TRANSACTION_COST,
        "Should meet minimum cost"
    );

    println!("✓ High reputation gas cost: {} nanoTOS", high_rep_gas);
    println!("✓ Low reputation gas cost: {} nanoTOS", low_rep_gas);
    println!(
        "✓ Fee difference: {}x",
        low_rep_gas as f64 / high_rep_gas as f64
    );

    println!("=== Secure Gas Calculation Test PASSED ===\n");
}

#[test]
fn test_base_reward_calculation() {
    println!("=== Testing Base Reward Calculation ===");

    let basic_reward = calculate_base_reward(&DifficultyLevel::Beginner);
    let intermediate_reward = calculate_base_reward(&DifficultyLevel::Intermediate);
    let advanced_reward = calculate_base_reward(&DifficultyLevel::Advanced);
    let expert_reward = calculate_base_reward(&DifficultyLevel::Expert);

    assert_eq!(basic_reward, BASIC_TASK_BASE_REWARD);
    assert_eq!(intermediate_reward, INTERMEDIATE_TASK_BASE_REWARD);
    assert_eq!(advanced_reward, ADVANCED_TASK_BASE_REWARD);
    assert_eq!(expert_reward, EXPERT_TASK_BASE_REWARD);

    // Verify reward increment
    assert!(intermediate_reward > basic_reward);
    assert!(advanced_reward > intermediate_reward);
    assert!(expert_reward > advanced_reward);

    println!(
        "✓ Basic reward: {} nanoTOS ({} TOS)",
        basic_reward,
        basic_reward as f64 / 1_000_000_000.0
    );
    println!(
        "✓ Intermediate reward: {} nanoTOS ({} TOS)",
        intermediate_reward,
        intermediate_reward as f64 / 1_000_000_000.0
    );
    println!(
        "✓ Advanced reward: {} nanoTOS ({} TOS)",
        advanced_reward,
        advanced_reward as f64 / 1_000_000_000.0
    );
    println!(
        "✓ Expert reward: {} nanoTOS ({} TOS)",
        expert_reward,
        expert_reward as f64 / 1_000_000_000.0
    );

    println!("=== Base Reward Calculation Test PASSED ===\n");
}

#[test]
fn test_final_reward_calculation() {
    println!("=== Testing Final Reward Calculation ===");

    let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();
    let base_time = 10_000_000u64;
    let mut reputation = AccountReputation::new(account, base_time - 90 * 24 * 3600);
    reputation.transaction_count = 200;
    reputation.stake_amount = 2_000_000;
    reputation.calculate_reputation_score(base_time);

    let base_reward = ADVANCED_TASK_BASE_REWARD;

    // High quality answer (95 points)
    let high_quality_reward = calculate_final_reward(base_reward, 95, &reputation);

    // Medium quality answer (85 points)
    let medium_quality_reward = calculate_final_reward(base_reward, 85, &reputation);

    // Low quality answer (70 points)
    let low_quality_reward = calculate_final_reward(base_reward, 70, &reputation);

    // Verify reward differences
    assert!(high_quality_reward > medium_quality_reward);
    assert!(medium_quality_reward > low_quality_reward);
    assert!(high_quality_reward > base_reward); // Should have quality bonus

    println!("✓ Base reward: {} nanoTOS", base_reward);
    println!("✓ High quality (95%): {} nanoTOS", high_quality_reward);
    println!("✓ Medium quality (85%): {} nanoTOS", medium_quality_reward);
    println!("✓ Low quality (70%): {} nanoTOS", low_quality_reward);

    let quality_bonus = high_quality_reward as f64 / base_reward as f64;
    println!("✓ Quality bonus multiplier: {:.2}x", quality_bonus);

    println!("=== Final Reward Calculation Test PASSED ===\n");
}

#[test]
fn test_economic_incentive_balance() {
    println!("=== Testing Economic Incentive Balance ===");

    let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();

    // Simulate economic performance of high reputation user
    let base_time = 10_000_000u64;
    let mut high_rep = AccountReputation::new(account.clone(), base_time - 90 * 24 * 3600);
    high_rep.transaction_count = 200;
    high_rep.stake_amount = 2_000_000;
    high_rep.calculate_reputation_score(1000000);

    // Calculate cost
    let content_length = 800; // 800 bytes high quality answer
    let gas_cost = calculate_secure_gas_cost(
        content_length,
        &DifficultyLevel::Advanced,
        &high_rep,
        2_000_000,
    );

    // Calculate reward
    let base_reward = calculate_base_reward(&DifficultyLevel::Advanced);
    let final_reward = calculate_final_reward(base_reward, 92, &high_rep);

    // Calculate profitability
    let profit_ratio = final_reward as f64 / gas_cost as f64;

    println!(
        "✓ Gas cost: {} nanoTOS ({:.3} TOS)",
        gas_cost,
        gas_cost as f64 / 1_000_000_000.0
    );
    println!(
        "✓ Final reward: {} nanoTOS ({:.3} TOS)",
        final_reward,
        final_reward as f64 / 1_000_000_000.0
    );
    println!("✓ Profit ratio: {:.2}x", profit_ratio);

    // Verify economic incentives
    assert!(
        profit_ratio > 1.0,
        "High reputation users should be profitable"
    );
    assert!(
        profit_ratio > 2.0,
        "Should provide meaningful economic incentive"
    );

    // Test low reputation user (should be unprofitable or low profit)
    let mut low_rep = AccountReputation::new(account, base_time - 3600);
    low_rep.calculate_reputation_score(base_time);

    let low_gas_cost = calculate_secure_gas_cost(
        100, // Shorter answer
        &DifficultyLevel::Beginner,
        &low_rep,
        0, // No stake
    );

    let low_base_reward = calculate_base_reward(&DifficultyLevel::Beginner);
    let low_final_reward = calculate_final_reward(low_base_reward, 75, &low_rep);
    let low_profit_ratio = low_final_reward as f64 / low_gas_cost as f64;

    println!("✓ Low reputation profit ratio: {:.2}x", low_profit_ratio);

    // Low reputation users should have lower profits or losses to incentivize reputation improvement
    assert!(
        low_profit_ratio < profit_ratio,
        "Low reputation should be less profitable"
    );

    println!("=== Economic Incentive Balance Test PASSED ===\n");
}

#[tokio::test]
async fn test_workflow_with_security_model() -> Result<()> {
    println!("=== Testing Complete Workflow with Security Model ===");

    // Create high reputation user transaction
    let builder = AIMiningTransactionBuilder::new(Network::Testnet);
    let task_id = Hash::from_bytes(&[1u8; 32])?;

    // Publish task (medium difficulty, detailed description)
    let task_description = "Please analyze the given Rust code for performance optimization opportunities. Identify bottlenecks, suggest improvements, and provide code examples. Consider memory usage, algorithmic complexity, and potential parallelization opportunities.";

    let task_metadata = builder.build_publish_task_transaction(
        task_id.clone(),
        INTERMEDIATE_TASK_BASE_REWARD, // Use new base reward
        DifficultyLevel::Intermediate,
        1234567890,
        task_description.to_string(),
        1,
        0,
    )?;

    println!("✓ Task publication with new reward model:");
    println!(
        "  - Reward: {} nanoTOS ({} TOS)",
        INTERMEDIATE_TASK_BASE_REWARD,
        INTERMEDIATE_TASK_BASE_REWARD as f64 / 1_000_000_000.0
    );
    println!("  - Description length: {} bytes", task_description.len());
    println!("  - Estimated fee: {} nanoTOS", task_metadata.estimated_fee);

    // Submit answer (detailed response)
    let answer_content = "Performance Analysis Report:\n\n1. Identified bottlenecks:\n   - Loop inefficiency in sorting algorithm\n   - Unnecessary memory allocations\n   - Missing compiler optimizations\n\n2. Suggested improvements:\n   - Use Vec::with_capacity() for known sizes\n   - Replace bubble sort with quicksort\n   - Add #[inline] hints for hot functions\n\n3. Code examples:\n```rust\n// Before\nlet mut vec = Vec::new();\nfor i in 0..1000 {\n    vec.push(i);\n}\n\n// After\nlet mut vec = Vec::with_capacity(1000);\nfor i in 0..1000 {\n    vec.push(i);\n}\n```\n\n4. Parallelization opportunities:\n   - Use rayon for data parallel operations\n   - Consider async/await for I/O bound tasks";

    let answer_metadata = builder.build_submit_answer_transaction(
        task_id.clone(),
        answer_content.to_string(),
        Hash::from_bytes(&[2u8; 32])?,
        100_000, // Stake amount
        2,
        0,
    )?;

    println!("✓ Answer submission with detailed content:");
    println!("  - Answer length: {} bytes", answer_content.len());
    println!(
        "  - Estimated fee: {} nanoTOS",
        answer_metadata.estimated_fee
    );

    // Validate answer
    let validation_metadata = builder.build_validate_answer_transaction(
        task_id.clone(),
        Hash::from_bytes(&[2u8; 32])?,
        88, // 88% validation score
        3,
        0,
    )?;

    println!("✓ Answer validation:");
    println!("  - Validation score: 88%");
    println!(
        "  - Estimated fee: {} nanoTOS",
        validation_metadata.estimated_fee
    );

    // Calculate total cost and reward
    let total_cost = task_metadata.estimated_fee
        + answer_metadata.estimated_fee
        + validation_metadata.estimated_fee;
    let base_reward = INTERMEDIATE_TASK_BASE_REWARD;

    // Simulate final reward for high reputation user
    let account = CompressedPublicKey::from_bytes(&[0u8; 32])?;
    let base_time = 10_000_000u64;
    let mut reputation = AccountReputation::new(account, base_time - 60 * 24 * 3600); // 60 days history
    reputation.transaction_count = 150;
    reputation.stake_amount = 1_000_000;
    reputation.calculate_reputation_score(base_time);

    let final_reward = calculate_final_reward(base_reward, 88, &reputation);
    let miner_share = (final_reward as f64 * 0.7) as u64; // 70% to miner

    let profit_ratio = miner_share as f64 / total_cost as f64;

    println!("✓ Economic summary:");
    println!(
        "  - Total cost: {} nanoTOS ({:.3} TOS)",
        total_cost,
        total_cost as f64 / 1_000_000_000.0
    );
    println!(
        "  - Miner reward: {} nanoTOS ({:.3} TOS)",
        miner_share,
        miner_share as f64 / 1_000_000_000.0
    );
    println!("  - Profit ratio: {:.2}x", profit_ratio);

    // Verify profitability of new model
    assert!(profit_ratio > 1.0, "New model should be profitable");
    assert!(profit_ratio > 1.5, "Should provide good economic incentive");

    println!("=== Workflow with Security Model Test PASSED ===\n");
    Ok(())
}

#[test]
fn test_spam_prevention() {
    println!("=== Testing Spam Prevention ===");

    // Test minimum length requirement
    let too_short = "Hi"; // 2 bytes
    let too_short_hash = tos_common::crypto::hash(too_short.as_bytes());
    let short_payload = AIMiningPayload::SubmitAnswer {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        answer_content: too_short.to_string(),
        answer_hash: too_short_hash,
        stake_amount: 0,
    };

    assert!(
        short_payload.validate().is_err(),
        "Too short content should be rejected"
    );
    println!("✓ Short content rejected");

    // Test all transactions have minimum fee
    let minimal_content = "A".repeat(10); // Minimum length
    let minimal_hash = tos_common::crypto::hash(minimal_content.as_bytes());
    let minimal_payload = AIMiningPayload::SubmitAnswer {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        answer_content: minimal_content,
        answer_hash: minimal_hash,
        stake_amount: 0,
    };

    let minimal_gas = minimal_payload.calculate_content_gas_cost();
    assert!(
        minimal_gas >= MIN_TRANSACTION_COST,
        "Should meet minimum transaction cost"
    );
    println!("✓ Minimum fee enforced: {} nanoTOS", minimal_gas);

    // Test high cost for spam content
    let spam_content = "spam ".repeat(400); // 2000 bytes spam content
    let spam_hash = tos_common::crypto::hash(spam_content.as_bytes());
    let spam_payload = AIMiningPayload::SubmitAnswer {
        task_id: Hash::from_bytes(&[1u8; 32]).unwrap(),
        answer_content: spam_content,
        answer_hash: spam_hash,
        stake_amount: 0,
    };

    let spam_gas = spam_payload.calculate_content_gas_cost();
    println!(
        "✓ Spam content cost: {} nanoTOS ({:.3} TOS)",
        spam_gas,
        spam_gas as f64 / 1_000_000_000.0
    );

    // Spam content cost should be significantly higher than minimum
    assert!(
        spam_gas > MIN_TRANSACTION_COST * 10,
        "Spam should be expensive (>10x minimum cost)"
    );
    assert!(spam_gas > 1_000_000, "Spam should cost more than 0.001 TOS");

    println!("=== Spam Prevention Test PASSED ===\n");
}

// Helper functions for testing
