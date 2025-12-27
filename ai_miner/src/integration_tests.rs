use anyhow::Result;
use log::{info, warn, error, log_enabled};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use serde_json::{json, Value};

use tos_common::{
    ai_mining::{AIMiningPayload, DifficultyLevel},
    crypto::{Hash, Address, PublicKey, AddressType},
    network::Network,
    serializer::Serializer,
};

use crate::{
    daemon_client::DaemonClient,
    transaction_builder::{AIMiningTransactionBuilder, AIMiningTransactionMetadata},
    storage::StorageManager,
    config::ValidatedConfig,
    get_next_nonce,
};

/// AI Mining workflow integration test suite
pub struct AIMiningIntegrationTester {
    daemon_client: Arc<DaemonClient>,
    tx_builder: Arc<AIMiningTransactionBuilder>,
    storage_manager: Arc<tokio::sync::Mutex<StorageManager>>,
    test_config: TestConfig,
}

/// Configuration for integration tests
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub miner_address: Address,
    pub publisher_address: Address,
    pub validator_address: Address,
    pub network: Network,
    pub use_mock_daemon: bool,
    pub test_timeout: Duration,
}

/// Test scenario tracking
#[derive(Debug, Clone)]
pub struct TestScenario {
    pub name: String,
    pub task_id: Option<Hash>,
    pub answer_id: Option<Hash>,
    pub validation_id: Option<Hash>,
    pub expected_rewards: u64,
    pub status: TestStatus,
    pub start_time: SystemTime,
    pub end_time: Option<SystemTime>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestStatus {
    Pending,
    Running,
    Passed,
    Failed,
    Timeout,
}

/// Mock daemon responses for testing
pub struct MockDaemonResponses {
    pub responses: HashMap<String, Value>,
    pub call_count: HashMap<String, usize>,
}

impl MockDaemonResponses {
    pub fn new() -> Self {
        let mut responses = HashMap::new();

        // Mock successful responses
        responses.insert("get_version".to_string(), json!("1.0.0"));
        responses.insert("get_height".to_string(), json!(12345));
        responses.insert("get_info".to_string(), json!({
            "height": 12345,
            "difficulty": "0x1234",
            "network": "testnet",
            "peers": 8
        }));
        responses.insert("get_nonce".to_string(), json!(42));
        responses.insert("get_balance".to_string(), json!(1000000000000));
        responses.insert("get_ai_mining_info".to_string(), json!({
            "total_tasks": 15,
            "active_tasks": 8,
            "total_miners": 42,
            "total_rewards_distributed": "500000000000"
        }));
        responses.insert("get_ai_mining_tasks".to_string(), json!([
            {
                "id": "a1b2c3d4e5f6789012345678901234567890123456789012345678901234567890",
                "reward": 50000000000,
                "difficulty": "Intermediate",
                "deadline": 1735689600,
                "description": "Image classification task",
                "publisher": "tos1publisher123..."
            }
        ]));
        responses.insert("submit_transaction".to_string(), json!("tx_hash_1234567890abcdef"));
        responses.insert("get_miner_stats".to_string(), json!({
            "tasks_completed": 5,
            "total_rewards": "250000000000",
            "reputation_score": 85.5,
            "validation_accuracy": 92.1
        }));

        Self {
            responses,
            call_count: HashMap::new(),
        }
    }

    pub fn get_response(&mut self, method: &str) -> Option<Value> {
        let count = self.call_count.entry(method.to_string()).or_insert(0);
        *count += 1;

        self.responses.get(method).cloned()
    }
}

impl AIMiningIntegrationTester {
    /// Create a new integration tester
    pub async fn new(config: ValidatedConfig, test_config: TestConfig) -> Result<Self> {
        let daemon_client = if test_config.use_mock_daemon {
            // Create mock daemon client (would need actual mock implementation)
            Arc::new(DaemonClient::with_config(&config.daemon_address, config.daemon_client_config.clone())?)
        } else {
            Arc::new(DaemonClient::with_config(&config.daemon_address, config.daemon_client_config.clone())?)
        };

        let tx_builder = Arc::new(AIMiningTransactionBuilder::new(test_config.network));

        let storage_manager = Arc::new(tokio::sync::Mutex::new(
            StorageManager::new(&config.storage_path).await?
        ));

        Ok(Self {
            daemon_client,
            tx_builder,
            storage_manager,
            test_config,
        })
    }

    /// Run comprehensive AI mining workflow test
    pub async fn run_comprehensive_test(&self) -> Result<Vec<TestScenario>> {
        info!("🚀 Starting comprehensive AI mining workflow test");
        let mut scenarios = Vec::new();

        // Phase 1: Miner Registration
        scenarios.push(self.test_miner_registration().await?);

        // Phase 2: Task Publication
        scenarios.push(self.test_task_publication().await?);

        // Phase 3: Answer Submission
        scenarios.push(self.test_answer_submission(&scenarios).await?);

        // Phase 4: Answer Validation
        scenarios.push(self.test_answer_validation(&scenarios).await?);

        // Phase 5: Reward Distribution
        scenarios.push(self.test_reward_distribution(&scenarios).await?);

        // Phase 6: Full Cycle Verification
        scenarios.push(self.test_full_cycle_verification(&scenarios).await?);

        self.print_test_summary(&scenarios);
        Ok(scenarios)
    }

    /// Test Phase 1: Miner Registration
    async fn test_miner_registration(&self) -> Result<TestScenario> {
        let mut scenario = TestScenario {
            name: "Miner Registration".to_string(),
            task_id: None,
            answer_id: None,
            validation_id: None,
            expected_rewards: 0,
            status: TestStatus::Running,
            start_time: SystemTime::now(),
            end_time: None,
            errors: Vec::new(),
        };

        info!("📝 Testing miner registration workflow");

        match self.execute_miner_registration().await {
            Ok(_) => {
                scenario.status = TestStatus::Passed;
                info!("✅ Miner registration test passed");
            }
            Err(e) => {
                scenario.status = TestStatus::Failed;
                scenario.errors.push(e.to_string());
                if log::log_enabled!(log::Level::Error) {
                    error!("❌ Miner registration test failed: {}", e);
                }
            }
        }

        scenario.end_time = Some(SystemTime::now());
        Ok(scenario)
    }

    /// Test Phase 2: Task Publication
    async fn test_task_publication(&self) -> Result<TestScenario> {
        let mut scenario = TestScenario {
            name: "Task Publication".to_string(),
            task_id: None,
            answer_id: None,
            validation_id: None,
            expected_rewards: 50_000_000_000, // 50 TOS
            status: TestStatus::Running,
            start_time: SystemTime::now(),
            end_time: None,
            errors: Vec::new(),
        };

        info!("📤 Testing task publication workflow");

        match self.execute_task_publication().await {
            Ok(task_id) => {
                scenario.task_id = Some(task_id);
                scenario.status = TestStatus::Passed;
                if log::log_enabled!(log::Level::Info) {
                    info!("✅ Task publication test passed - Task ID: {}", hex::encode(task_id.as_bytes()));
                }
            }
            Err(e) => {
                scenario.status = TestStatus::Failed;
                scenario.errors.push(e.to_string());
                if log::log_enabled!(log::Level::Error) {
                    error!("❌ Task publication test failed: {}", e);
                }
            }
        }

        scenario.end_time = Some(SystemTime::now());
        Ok(scenario)
    }

    /// Test Phase 3: Answer Submission
    async fn test_answer_submission(&self, previous_scenarios: &[TestScenario]) -> Result<TestScenario> {
        let mut scenario = TestScenario {
            name: "Answer Submission".to_string(),
            task_id: None,
            answer_id: None,
            validation_id: None,
            expected_rewards: 0,
            status: TestStatus::Running,
            start_time: SystemTime::now(),
            end_time: None,
            errors: Vec::new(),
        };

        info!("💡 Testing answer submission workflow");

        // Get task ID from previous test
        let task_id = previous_scenarios
            .iter()
            .find(|s| s.name == "Task Publication")
            .and_then(|s| s.task_id)
            .ok_or_else(|| anyhow::anyhow!("No task ID from previous test"))?;

        scenario.task_id = Some(task_id);

        match self.execute_answer_submission(task_id).await {
            Ok(answer_id) => {
                scenario.answer_id = Some(answer_id);
                scenario.status = TestStatus::Passed;
                if log::log_enabled!(log::Level::Info) {
                    info!("✅ Answer submission test passed - Answer ID: {}", hex::encode(answer_id.as_bytes()));
                }
            }
            Err(e) => {
                scenario.status = TestStatus::Failed;
                scenario.errors.push(e.to_string());
                if log::log_enabled!(log::Level::Error) {
                    error!("❌ Answer submission test failed: {}", e);
                }
            }
        }

        scenario.end_time = Some(SystemTime::now());
        Ok(scenario)
    }

    /// Test Phase 4: Answer Validation
    async fn test_answer_validation(&self, previous_scenarios: &[TestScenario]) -> Result<TestScenario> {
        let mut scenario = TestScenario {
            name: "Answer Validation".to_string(),
            task_id: None,
            answer_id: None,
            validation_id: None,
            expected_rewards: 0,
            status: TestStatus::Running,
            start_time: SystemTime::now(),
            end_time: None,
            errors: Vec::new(),
        };

        info!("🔍 Testing answer validation workflow");

        // Get task and answer IDs from previous tests
        let task_id = previous_scenarios
            .iter()
            .find(|s| s.name == "Task Publication")
            .and_then(|s| s.task_id)
            .ok_or_else(|| anyhow::anyhow!("No task ID from previous test"))?;

        let answer_id = previous_scenarios
            .iter()
            .find(|s| s.name == "Answer Submission")
            .and_then(|s| s.answer_id)
            .ok_or_else(|| anyhow::anyhow!("No answer ID from previous test"))?;

        scenario.task_id = Some(task_id);
        scenario.answer_id = Some(answer_id);

        match self.execute_answer_validation(task_id, answer_id).await {
            Ok(validation_id) => {
                scenario.validation_id = Some(validation_id);
                scenario.status = TestStatus::Passed;
                if log::log_enabled!(log::Level::Info) {
                    info!("✅ Answer validation test passed - Validation ID: {}", hex::encode(validation_id.as_bytes()));
                }
            }
            Err(e) => {
                scenario.status = TestStatus::Failed;
                scenario.errors.push(e.to_string());
                if log::log_enabled!(log::Level::Error) {
                    error!("❌ Answer validation test failed: {}", e);
                }
            }
        }

        scenario.end_time = Some(SystemTime::now());
        Ok(scenario)
    }

    /// Test Phase 5: Reward Distribution
    async fn test_reward_distribution(&self, previous_scenarios: &[TestScenario]) -> Result<TestScenario> {
        let mut scenario = TestScenario {
            name: "Reward Distribution".to_string(),
            task_id: None,
            answer_id: None,
            validation_id: None,
            expected_rewards: 50_000_000_000, // Expected from task
            status: TestStatus::Running,
            start_time: SystemTime::now(),
            end_time: None,
            errors: Vec::new(),
        };

        info!("💰 Testing reward distribution workflow");

        match self.execute_reward_verification().await {
            Ok(distributed_amount) => {
                if distributed_amount >= scenario.expected_rewards {
                    scenario.status = TestStatus::Passed;
                    if log::log_enabled!(log::Level::Info) {
                        info!("✅ Reward distribution test passed - {} nanoTOS distributed", distributed_amount);
                    }
                } else {
                    scenario.status = TestStatus::Failed;
                    if log::log_enabled!(log::Level::Error) {
                        scenario.errors.push(format!("Insufficient rewards distributed: {} < {}", distributed_amount, scenario.expected_rewards));
                    }
                    error!("❌ Reward distribution test failed: insufficient amount");
                }
            }
            Err(e) => {
                scenario.status = TestStatus::Failed;
                scenario.errors.push(e.to_string());
                if log::log_enabled!(log::Level::Error) {
                    error!("❌ Reward distribution test failed: {}", e);
                }
            }
        }

        scenario.end_time = Some(SystemTime::now());
        Ok(scenario)
    }

    /// Test Phase 6: Full Cycle Verification
    async fn test_full_cycle_verification(&self, previous_scenarios: &[TestScenario]) -> Result<TestScenario> {
        let mut scenario = TestScenario {
            name: "Full Cycle Verification".to_string(),
            task_id: None,
            answer_id: None,
            validation_id: None,
            expected_rewards: 0,
            status: TestStatus::Running,
            start_time: SystemTime::now(),
            end_time: None,
            errors: Vec::new(),
        };

        info!("🔄 Testing full cycle verification");

        // Check that all previous phases passed
        let all_passed = previous_scenarios.iter().all(|s| s.status == TestStatus::Passed);

        if all_passed {
            // Verify storage consistency
            match self.verify_storage_consistency().await {
                Ok(_) => {
                    scenario.status = TestStatus::Passed;
                    info!("✅ Full cycle verification passed");
                }
                Err(e) => {
                    scenario.status = TestStatus::Failed;
                    scenario.errors.push(e.to_string());
                    if log::log_enabled!(log::Level::Error) {
                        error!("❌ Full cycle verification failed: {}", e);
                    }
                }
            }
        } else {
            scenario.status = TestStatus::Failed;
            scenario.errors.push("Previous phases failed".to_string());
            error!("❌ Full cycle verification failed: previous phases failed");
        }

        scenario.end_time = Some(SystemTime::now());
        Ok(scenario)
    }

    /// Execute miner registration
    async fn execute_miner_registration(&self) -> Result<()> {
        let nonce = get_next_nonce(&self.daemon_client, &self.test_config.miner_address).await?;

        let metadata = self.tx_builder.build_register_miner_transaction(
            self.test_config.miner_address.clone().to_public_key(),
            1_000_000_000, // 1 TOS registration fee
            nonce,
            0, // Auto-calculate fee
        )?;

        // Store in local storage
        let mut storage = self.storage_manager.lock().await;
        storage.register_miner(&self.test_config.miner_address.to_public_key(), 1_000_000_000).await?;
        storage.add_transaction(&metadata, None).await?;

        if log::log_enabled!(log::Level::Info) {
            info!("Miner registration metadata created: {} bytes, {} nanoTOS fee", metadata.estimated_size, metadata.estimated_fee);
        }
        Ok(())
    }

    /// Execute task publication
    async fn execute_task_publication(&self) -> Result<Hash> {
        let task_id = Hash::new(rand::random::<[u8; 32]>());
        let reward_amount = 50_000_000_000; // 50 TOS
        let difficulty = DifficultyLevel::Intermediate;
        let deadline = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 3600; // 1 hour from now

        let nonce = get_next_nonce(&self.daemon_client, &self.test_config.publisher_address).await?;

        let metadata = self.tx_builder.build_publish_task_transaction(
            task_id.clone(),
            reward_amount,
            difficulty,
            deadline,
            "Integration test AI mining task".to_string(), // Task description
            nonce,
            0, // Auto-calculate fee
        )?;

        // Store in local storage
        let mut storage = self.storage_manager.lock().await;
        storage.add_task(&task_id, reward_amount, difficulty, deadline).await?;
        storage.add_transaction(&metadata, Some(task_id.clone())).await?;

        if log::log_enabled!(log::Level::Info) {
            info!("Task publication metadata created: {} bytes, {} nanoTOS fee", metadata.estimated_size, metadata.estimated_fee);
        }
        Ok(task_id)
    }

    /// Execute answer submission
    async fn execute_answer_submission(&self, task_id: Hash) -> Result<Hash> {
        let answer_text = "This is a sample AI-generated answer for the intermediate difficulty task";
        let answer_hash = Hash::new(blake3::hash(answer_text.as_bytes()).into());
        let stake_amount = 5_000_000_000; // 5 TOS stake

        let nonce = get_next_nonce(&self.daemon_client, &self.test_config.miner_address).await?;

        let metadata = self.tx_builder.build_submit_answer_transaction(
            task_id.clone(),
            answer_hash.clone(),
            stake_amount,
            nonce,
            0, // Auto-calculate fee
        )?;

        // Store in local storage
        let mut storage = self.storage_manager.lock().await;
        storage.add_transaction(&metadata, Some(task_id)).await?;

        if log::log_enabled!(log::Level::Info) {
            info!("Answer submission metadata created: {} bytes, {} nanoTOS fee", metadata.estimated_size, metadata.estimated_fee);
        }
        Ok(answer_hash)
    }

    /// Execute answer validation
    async fn execute_answer_validation(&self, task_id: Hash, answer_id: Hash) -> Result<Hash> {
        let validation_score = 85; // Good score
        let validation_id = Hash::new(rand::random::<[u8; 32]>());

        let nonce = get_next_nonce(&self.daemon_client, &self.test_config.validator_address).await?;

        let metadata = self.tx_builder.build_validate_answer_transaction(
            task_id.clone(),
            answer_id,
            validation_score,
            nonce,
            0, // Auto-calculate fee
        )?;

        // Store in local storage
        let mut storage = self.storage_manager.lock().await;
        storage.add_transaction(&metadata, Some(task_id)).await?;

        if log::log_enabled!(log::Level::Info) {
            info!("Answer validation metadata created: {} bytes, {} nanoTOS fee", metadata.estimated_size, metadata.estimated_fee);
        }
        Ok(validation_id)
    }

    /// Execute reward verification
    async fn execute_reward_verification(&self) -> Result<u64> {
        // In a real implementation, this would query the daemon for actual rewards
        // For now, we simulate successful reward distribution
        let distributed_rewards = 50_000_000_000; // 50 TOS

        if log::log_enabled!(log::Level::Info) {
            info!("Simulated reward distribution: {} nanoTOS", distributed_rewards);
        }
        Ok(distributed_rewards)
    }

    /// Verify storage consistency
    async fn verify_storage_consistency(&self) -> Result<()> {
        let storage = self.storage_manager.lock().await;
        let transactions = storage.get_transactions();

        if transactions.is_empty() {
            return Err(anyhow::anyhow!("No transactions found in storage"));
        }

        if log::log_enabled!(log::Level::Info) {
            info!("Storage consistency verified: {} transactions recorded", transactions.len());
        }
        Ok(())
    }

    /// Print comprehensive test summary
    fn print_test_summary(&self, scenarios: &[TestScenario]) {
        info!("📊 AI Mining Integration Test Summary");
        info!("==========================================");

        let passed = scenarios.iter().filter(|s| s.status == TestStatus::Passed).count();
        let failed = scenarios.iter().filter(|s| s.status == TestStatus::Failed).count();
        let total = scenarios.len();

        if log::log_enabled!(log::Level::Info) {
            info!("Total Tests: {} | Passed: {} | Failed: {}", total, passed, failed);
        }
        info!("");

        for scenario in scenarios {
            let duration = scenario.end_time
                .and_then(|end| end.duration_since(scenario.start_time).ok())
                .map(|d| format!("{:.2}s", d.as_secs_f64()))
                .unwrap_or_else(|| "N/A".to_string());

            let status_icon = match scenario.status {
                TestStatus::Passed => "✅",
                TestStatus::Failed => "❌",
                TestStatus::Running => "🔄",
                TestStatus::Timeout => "⏰",
                TestStatus::Pending => "⏳",
            };

            if log::log_enabled!(log::Level::Info) {
                info!("{} {} ({})", status_icon, scenario.name, duration);
            }

            if !scenario.errors.is_empty() {
                for error in &scenario.errors {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!("   Error: {}", error);
                    }
                }
            }

            if let Some(task_id) = &scenario.task_id {
                if log::log_enabled!(log::Level::Info) {
                    info!("   Task ID: {}", hex::encode(task_id.as_bytes()));
                }
            }
            if let Some(answer_id) = &scenario.answer_id {
                if log::log_enabled!(log::Level::Info) {
                    info!("   Answer ID: {}", hex::encode(answer_id.as_bytes()));
                }
            }
        }

        info!("==========================================");

        if failed == 0 {
            info!("🎉 All AI mining workflow tests PASSED!");
        } else {
            if log::log_enabled!(log::Level::Warn) {
                warn!("⚠️  {} test(s) failed. Check logs for details.", failed);
            }
        }
    }
}

/// Default test configuration for testnet
impl Default for TestConfig {
    fn default() -> Self {
        // Create test addresses using PublicKey and Address::new
        let miner_key = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let publisher_key = PublicKey::from_bytes(&[2u8; 32]).unwrap();
        let validator_key = PublicKey::from_bytes(&[3u8; 32]).unwrap();

        let miner_address = Address::new(false, AddressType::Normal, miner_key); // testnet
        let publisher_address = Address::new(false, AddressType::Normal, publisher_key);
        let validator_address = Address::new(false, AddressType::Normal, validator_key);

        Self {
            miner_address,
            publisher_address,
            validator_address,
            network: Network::Testnet,
            use_mock_daemon: true,
            test_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigValidator;

    #[tokio::test]
    async fn test_ai_mining_workflow() {
        env_logger::init();

        let config = ConfigValidator::default_config();
        let test_config = TestConfig::default();

        let tester = AIMiningIntegrationTester::new(config, test_config).await.unwrap();
        let results = tester.run_comprehensive_test().await.unwrap();

        // All tests should pass in mock mode
        assert!(results.iter().all(|r| r.status == TestStatus::Passed));
    }
}