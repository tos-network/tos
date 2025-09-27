# AI Mining Task Management System

## Task Manager Core Architecture

### 1. Task Manager Main Interface (daemon/src/ai/task_manager.rs)

```rust
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, Mutex};
use std::collections::{HashMap, VecDeque, BTreeSet};
use chrono::{DateTime, Utc, Duration};
use crate::{
    ai::{types::*, state::*, storage::*, validation::*, rewards::*},
    crypto::{Hash, CompressedPublicKey},
    blockchain::BlockchainInterface,
};

pub struct TaskManager {
    storage: Arc<RwLock<dyn AIStorageProvider>>,
    validator_registry: Arc<ValidatorRegistry>,
    reward_engine: Arc<RewardDistributionEngine>,
    fraud_detector: Arc<FraudDetectionEngine>,
    task_scheduler: TaskScheduler,
    notification_system: NotificationSystem,
    metrics_collector: TaskMetricsCollector,
    blockchain_interface: Arc<dyn BlockchainInterface>,
}

impl TaskManager {
    pub fn new(
        storage: Arc<RwLock<dyn AIStorageProvider>>,
        blockchain_interface: Arc<dyn BlockchainInterface>,
    ) -> Self {
        Self {
            storage,
            validator_registry: Arc::new(ValidatorRegistry::new()),
            reward_engine: Arc::new(RewardDistributionEngine::new(EconomicParameters::default())),
            fraud_detector: Arc::new(FraudDetectionEngine::new()),
            task_scheduler: TaskScheduler::new(),
            notification_system: NotificationSystem::new(),
            metrics_collector: TaskMetricsCollector::new(),
            blockchain_interface,
        }
    }

    pub async fn publish_task(
        &mut self,
        publisher: CompressedPublicKey,
        task_data: PublishTaskPayload,
        block_height: u64,
    ) -> Result<Hash, TaskError> {
        // Validate publisher eligibility and task data
        self.validate_task_publication(&publisher, &task_data).await?;

        // Create task state
        let task_id = self.generate_task_id(&task_data, block_height);
        let task_state = self.create_initial_task_state(
            task_id,
            publisher,
            task_data,
            block_height,
        );

        // Store task state
        {
            let mut storage = self.storage.write().await;
            storage.store_task_state(&task_id, &task_state).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
        }

        // Schedule task lifecycle events
        self.task_scheduler.schedule_task_events(&task_state).await?;

        // Send notifications
        self.notification_system.notify_task_published(&task_state).await?;

        // Record metrics
        self.metrics_collector.record_task_published(&task_state);

        Ok(task_id)
    }

    pub async fn submit_answer(
        &mut self,
        submitter: CompressedPublicKey,
        submission: SubmitAnswerPayload,
        block_height: u64,
    ) -> Result<Hash, TaskError> {
        // Get task state
        let task_state = {
            let storage = self.storage.read().await;
            storage.get_task_state(&submission.task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::TaskNotFound(submission.task_id))?
        };

        // Validate submission eligibility
        self.validate_submission_eligibility(&submitter, &submission, &task_state).await?;

        // Perform fraud detection
        let miner_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(&submitter).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::MinerNotRegistered(submitter.clone()))?
        };

        let fraud_analysis = self.fraud_detector.analyze_submission(
            &task_state,
            &submission,
            &miner_state,
            &self.get_network_context().await,
        ).await;

        // Decide whether to accept submission based on fraud analysis
        if fraud_analysis.overall_risk_score > 0.8 {
            return Err(TaskError::SubmissionRejected(format!(
                "High fraud risk detected: {}",
                fraud_analysis.overall_risk_score
            )));
        }

        // Create submission state
        let submission_id = self.generate_submission_id(&submission, block_height);
        let submission_state = self.create_submission_state(
            submission_id,
            submitter,
            submission,
            block_height,
            fraud_analysis.overall_risk_score,
        );

        // Store submission state and fraud analysis
        {
            let mut storage = self.storage.write().await;
            storage.store_submission(&submission_state).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
            storage.store_fraud_analysis(&fraud_analysis).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
        }

        // Update task status with new submission
        self.update_task_with_new_submission(&submission.task_id, &submission_id).await?;

        // If risk score is moderate, request additional validation
        if fraud_analysis.overall_risk_score > 0.4 {
            self.request_additional_validation(&submission_id, &fraud_analysis).await?;
        }

        // Check if submission deadline or max participants reached
        self.check_submission_phase_completion(&submission.task_id).await?;

        // Send notifications
        self.notification_system.notify_submission_received(&submission_state, &task_state).await?;

        // Record metrics
        self.metrics_collector.record_submission_received(&submission_state);

        Ok(submission_id)
    }

    pub async fn validate_submission(
        &mut self,
        validator: CompressedPublicKey,
        validation: ValidateAnswerPayload,
        block_height: u64,
    ) -> Result<Hash, TaskError> {
        // Get task and submission states
        let (task_state, submission_state) = {
            let storage = self.storage.read().await;
            let task = storage.get_task_state(&validation.task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::TaskNotFound(validation.task_id))?;
            let submission = storage.get_submission(&validation.answer_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::SubmissionNotFound(validation.answer_id))?;
            (task, submission)
        };

        // Validate validator eligibility
        self.validate_validator_eligibility(&validator, &validation, &task_state).await?;

        // Get validator state
        let validator_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(&validator).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::ValidatorNotRegistered(validator.clone()))?
        };

        // Perform validation
        let validation_result = self.perform_validation(
            &validator,
            &validator_state,
            &task_state,
            &submission_state,
            &validation,
        ).await?;

        // Create validation record
        let validation_record = ValidationRecord {
            validation_id: self.generate_validation_id(&validation, block_height),
            task_id: validation.task_id,
            submission_id: validation.answer_id,
            validator: validator.clone(),
            validation_result: validation_result.clone(),
            validation_time: block_height,
            stake_amount: validation.validator_stake,
            validation_proof: validation.validation_proof.clone(),
        };

        // Store validation result
        {
            let mut storage = self.storage.write().await;
            storage.store_validation_result(&validation_record).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
        }

        // Update submission validation status
        self.update_submission_validation_status(
            &validation.answer_id,
            &validation_record,
        ).await?;

        // Check if validation consensus reached
        let consensus_result = self.check_validation_consensus(&validation.task_id).await?;

        if let Some(consensus) = consensus_result {
            self.finalize_task_validation(&validation.task_id, consensus).await?;
        }

        // Send notifications
        self.notification_system.notify_validation_completed(&validation_record).await?;

        // Record metrics
        self.metrics_collector.record_validation_completed(&validation_record);

        Ok(validation_record.validation_id)
    }

    pub async fn process_task_completion(&mut self, task_id: Hash) -> Result<RewardDistribution, TaskError> {
        // Get complete task state
        let task_state = {
            let storage = self.storage.read().await;
            storage.get_task_state(&task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::TaskNotFound(task_id))?
        };

        // Validate task can be completed
        if !matches!(task_state.status, TaskStatus::UnderValidation) {
            return Err(TaskError::InvalidTaskStatus(task_state.status));
        }

        // Get all validation results
        let validation_results = self.collect_all_validation_results(&task_id).await?;

        // Calculate reward distribution
        let reward_distribution = self.reward_engine.calculate_task_rewards(
            &task_state,
            &validation_results,
            &self.get_network_context().await,
        ).await.map_err(|e| TaskError::RewardCalculationError(format!("{:?}", e)))?;

        // Store reward distribution record
        {
            let mut storage = self.storage.write().await;
            storage.store_reward_distribution(&reward_distribution).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
        }

        // Update task status to completed
        self.update_task_status(&task_id, TaskStatus::Completed).await?;

        // Update participant reputations
        self.update_participant_reputations(&task_state, &validation_results, &reward_distribution).await?;

        // Send reward distribution notifications
        self.notification_system.notify_rewards_distributed(&reward_distribution).await?;

        // Create on-chain transactions for reward distribution
        self.create_reward_distribution_transactions(&reward_distribution).await?;

        // Record metrics
        self.metrics_collector.record_task_completed(&task_state, &reward_distribution);

        Ok(reward_distribution)
    }

    pub async fn handle_task_expiration(&mut self, task_id: Hash) -> Result<(), TaskError> {
        let task_state = {
            let storage = self.storage.read().await;
            storage.get_task_state(&task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::TaskNotFound(task_id))?
        };

        match task_state.status {
            TaskStatus::Published => {
                // Task published but no participants, refund publisher
                self.refund_publisher(&task_state).await?;
                self.update_task_status(&task_id, TaskStatus::Expired).await?;
            },
            TaskStatus::InProgress => {
                // Has participants but no submissions, partial refund
                self.handle_partial_completion(&task_state).await?;
                self.update_task_status(&task_id, TaskStatus::Expired).await?;
            },
            TaskStatus::AnswersSubmitted => {
                // Has submissions but validation timeout, force validation phase
                self.force_validation_phase(&task_id).await?;
            },
            _ => {
                return Err(TaskError::InvalidExpirationState(task_state.status));
            }
        }

        self.notification_system.notify_task_expired(&task_state).await?;
        Ok(())
    }

    pub async fn handle_dispute(
        &mut self,
        dispute: DisputeCase,
    ) -> Result<DisputeResolution, TaskError> {
        // Validate dispute validity
        self.validate_dispute(&dispute).await?;

        // Suspend related task reward distribution
        self.suspend_task_rewards(&dispute.task_id).await?;

        // Start dispute resolution process
        let resolution = self.resolve_dispute(dispute).await?;

        // Adjust rewards based on resolution result
        self.apply_dispute_resolution(&resolution).await?;

        // Resume task reward distribution
        self.resume_task_rewards(&resolution.task_id).await?;

        Ok(resolution)
    }

    // Task lifecycle management
    pub async fn run_background_tasks(&mut self) -> Result<(), TaskError> {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

        loop {
            interval.tick().await;

            // Check task timeouts
            if let Err(e) = self.check_task_timeouts().await {
                log::error!("Error checking task timeouts: {:?}", e);
            }

            // Process pending validations
            if let Err(e) = self.process_pending_validations().await {
                log::error!("Error processing pending validations: {:?}", e);
            }

            // Complete ready tasks
            if let Err(e) = self.complete_ready_tasks().await {
                log::error!("Error completing ready tasks: {:?}", e);
            }

            // Clean up expired data
            if let Err(e) = self.cleanup_expired_data().await {
                log::error!("Error cleaning up expired data: {:?}", e);
            }

            // Update network statistics
            if let Err(e) = self.update_network_statistics().await {
                log::error!("Error updating network statistics: {:?}", e);
            }
        }
    }
}

// Task Manager Internal Implementation
impl TaskManager {
    async fn validate_task_publication(
        &self,
        publisher: &CompressedPublicKey,
        task_data: &PublishTaskPayload,
    ) -> Result<(), TaskError> {
        // Check if publisher has sufficient TOS balance
        let publisher_balance = self.blockchain_interface.get_balance(publisher).await
            .map_err(|e| TaskError::BlockchainError(e.to_string()))?;

        let required_amount = task_data.reward_amount + self.calculate_publishing_fee(task_data);
        if publisher_balance < required_amount {
            return Err(TaskError::InsufficientBalance {
                required: required_amount,
                available: publisher_balance,
            });
        }

        // Validate task data integrity
        if task_data.description_hash.is_empty() || task_data.encrypted_data.is_empty() {
            return Err(TaskError::InvalidTaskData("Missing required data".to_string()));
        }

        // Check if reward amount is reasonable
        let min_reward = self.get_minimum_reward_for_task_type(&task_data.task_type);
        if task_data.reward_amount < min_reward {
            return Err(TaskError::RewardTooLow {
                provided: task_data.reward_amount,
                minimum: min_reward,
            });
        }

        // Check if deadline is reasonable
        let current_time = chrono::Utc::now().timestamp() as u64;
        let min_duration = self.get_minimum_task_duration(&task_data.task_type);
        if task_data.deadline < current_time + min_duration {
            return Err(TaskError::DeadlineTooSoon {
                provided: task_data.deadline,
                minimum: current_time + min_duration,
            });
        }

        Ok(())
    }

    async fn validate_submission_eligibility(
        &self,
        submitter: &CompressedPublicKey,
        submission: &SubmitAnswerPayload,
        task_state: &TaskState,
    ) -> Result<(), TaskError> {
        // Check task status
        if !matches!(task_state.status, TaskStatus::Published | TaskStatus::InProgress) {
            return Err(TaskError::SubmissionNotAllowed(task_state.status));
        }

        // Check if deadline has passed
        let current_time = chrono::Utc::now().timestamp() as u64;
        if current_time > task_state.lifecycle.submission_deadline {
            return Err(TaskError::SubmissionDeadlinePassed);
        }

        // Check if max participants reached
        if task_state.participants.len() >= task_state.task_data.max_participants as usize {
            return Err(TaskError::MaxParticipantsReached);
        }

        // Check if submitter already participating
        if task_state.participants.contains_key(submitter) {
            return Err(TaskError::AlreadyParticipating);
        }

        // Check stake amount
        if submission.stake_amount < task_state.task_data.stake_required {
            return Err(TaskError::InsufficientStake {
                required: task_state.task_data.stake_required,
                provided: submission.stake_amount,
            });
        }

        // Check submitter balance
        let submitter_balance = self.blockchain_interface.get_balance(submitter).await
            .map_err(|e| TaskError::BlockchainError(e.to_string()))?;

        if submitter_balance < submission.stake_amount {
            return Err(TaskError::InsufficientBalance {
                required: submission.stake_amount,
                available: submitter_balance,
            });
        }

        Ok(())
    }

    async fn validate_validator_eligibility(
        &self,
        validator: &CompressedPublicKey,
        validation: &ValidateAnswerPayload,
        task_state: &TaskState,
    ) -> Result<(), TaskError> {
        // Check if validator is the submitter (cannot validate own submission)
        let submission_state = {
            let storage = self.storage.read().await;
            storage.get_submission(&validation.answer_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::SubmissionNotFound(validation.answer_id))?
        };

        if submission_state.submitter == *validator {
            return Err(TaskError::SelfValidationNotAllowed);
        }

        // Check validator reputation
        let validator_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(validator).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::ValidatorNotRegistered(validator.clone()))?
        };

        let min_reputation = self.get_minimum_validator_reputation(&task_state.task_data.task_type);
        if validator_state.reputation.overall_score < min_reputation {
            return Err(TaskError::InsufficientValidatorReputation {
                required: min_reputation,
                current: validator_state.reputation.overall_score,
            });
        }

        // Check specialization match
        let has_specialization = validator_state.specializations.iter()
            .any(|spec| self.task_types_match(spec, &task_state.task_data.task_type));

        if !has_specialization && task_state.task_data.verification_type.requires_specialization() {
            return Err(TaskError::LackOfSpecialization);
        }

        // Check if validator already validated this submission
        let existing_validations = {
            let storage = self.storage.read().await;
            storage.get_validation_results(&validation.answer_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
        };

        if existing_validations.iter().any(|v| v.validator == *validator) {
            return Err(TaskError::AlreadyValidated);
        }

        Ok(())
    }

    async fn perform_validation(
        &self,
        validator: &CompressedPublicKey,
        validator_state: &MinerState,
        task_state: &TaskState,
        submission_state: &SubmissionState,
        validation_payload: &ValidateAnswerPayload,
    ) -> Result<ValidationResult, TaskError> {
        // Execute different validation logic based on verification type
        match &task_state.task_data.verification_type {
            VerificationType::Automatic => {
                // Automatic validation
                let auto_validator = AutomaticValidator::new();
                auto_validator.validate_submission(
                    task_state,
                    submission_state,
                    &self.get_validation_context().await,
                ).await.map_err(|e| TaskError::ValidationError(format!("{:?}", e)))
            },
            VerificationType::PeerReview { .. } => {
                // Peer validation
                let peer_validator = PeerValidator::new(
                    validator.clone(),
                    validator_state.reputation.clone(),
                    validator_state.specializations.clone(),
                );
                peer_validator.validate_submission(
                    task_state,
                    submission_state,
                    &self.get_validation_context().await,
                ).await.map_err(|e| TaskError::ValidationError(format!("{:?}", e)))
            },
            VerificationType::ExpertReview { .. } => {
                // Expert validation
                let expert_validator = ExpertValidator::new(
                    validator.clone(),
                    self.get_expert_certifications(validator).await?,
                );
                expert_validator.validate_submission(
                    task_state,
                    submission_state,
                    &self.get_validation_context().await,
                ).await.map_err(|e| TaskError::ValidationError(format!("{:?}", e)))
            },
            VerificationType::Hybrid { .. } => {
                // Hybrid validation
                self.perform_hybrid_validation(
                    validator,
                    validator_state,
                    task_state,
                    submission_state,
                    validation_payload,
                ).await
            },
        }
    }

    async fn check_validation_consensus(&self, task_id: &Hash) -> Result<Option<ConsensusResult>, TaskError> {
        // Get all submissions for the task
        let submissions = {
            let storage = self.storage.read().await;
            storage.list_submissions_for_task(task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
        };

        let mut consensus_results = Vec::new();

        for submission_id in submissions {
            // Get validation results for each submission
            let validations = {
                let storage = self.storage.read().await;
                storage.get_validation_results(&submission_id).await
                    .map_err(|e| TaskError::StorageError(e.to_string()))?
            };

            if validations.is_empty() {
                continue;
            }

            // Check if consensus reached
            let consensus = self.calculate_validation_consensus(&validations).await?;

            if consensus.consensus_reached {
                consensus_results.push(SubmissionConsensus {
                    submission_id,
                    consensus_score: consensus.final_score.unwrap_or(0),
                    validator_count: validations.len(),
                    confidence: consensus.confidence_level,
                });
            }
        }

        // Check if all submissions have consensus results
        let task_state = {
            let storage = self.storage.read().await;
            storage.get_task_state(task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::TaskNotFound(*task_id))?
        };

        let required_consensus_count = task_state.submissions.len();
        let achieved_consensus_count = consensus_results.len();

        if achieved_consensus_count >= required_consensus_count.min(1) {
            // All submissions have consensus, can complete task
            Ok(Some(ConsensusResult {
                task_id: *task_id,
                submission_consensuses: consensus_results,
                overall_confidence: self.calculate_overall_confidence(&consensus_results),
                consensus_achieved_at: chrono::Utc::now().timestamp() as u64,
            }))
        } else {
            Ok(None)
        }
    }

    async fn calculate_validation_consensus(
        &self,
        validations: &[ValidationRecord],
    ) -> Result<ValidationConsensus, TaskError> {
        if validations.is_empty() {
            return Ok(ValidationConsensus {
                validators: vec![],
                consensus_reached: false,
                final_score: None,
                confidence_level: 0.0,
            });
        }

        let mut validator_infos = Vec::new();
        let mut weighted_scores = Vec::new();
        let mut total_weight = 0.0;

        for validation in validations {
            let weight = self.calculate_validator_weight(&validation.validator).await?;
            let score = self.extract_validation_score(&validation.validation_result)?;

            validator_infos.push(ValidatorInfo {
                validator: validation.validator.clone(),
                score_given: score,
                weight,
                validation_time: validation.validation_time,
                reasoning: self.extract_validation_reasoning(&validation.validation_result),
            });

            weighted_scores.push(score as f64 * weight);
            total_weight += weight;
        }

        if total_weight == 0.0 {
            return Ok(ValidationConsensus {
                validators: validator_infos,
                consensus_reached: false,
                final_score: None,
                confidence_level: 0.0,
            });
        }

        let weighted_average = weighted_scores.iter().sum::<f64>() / total_weight;

        // Calculate consensus degree (score variance)
        let variance = weighted_scores.iter()
            .map(|&score| (score - weighted_average).powi(2))
            .sum::<f64>() / weighted_scores.len() as f64;

        let standard_deviation = variance.sqrt();

        // Consensus threshold: standard deviation less than 15 points indicates consensus
        let consensus_reached = standard_deviation < 15.0 && validations.len() >= 2;

        let confidence_level = if consensus_reached {
            (1.0 - (standard_deviation / 50.0)).max(0.5) // At least 50% confidence
        } else {
            0.0
        };

        Ok(ValidationConsensus {
            validators: validator_infos,
            consensus_reached,
            final_score: if consensus_reached { Some(weighted_average.round() as u8) } else { None },
            confidence_level,
        })
    }

    async fn create_reward_distribution_transactions(
        &self,
        distribution: &RewardDistribution,
    ) -> Result<Vec<Hash>, TaskError> {
        let mut transaction_hashes = Vec::new();

        for reward_entry in &distribution.distributions {
            // Create reward transfer transaction
            let tx_hash = self.blockchain_interface.create_reward_transfer(
                &distribution.task_id,
                &reward_entry.recipient,
                reward_entry.amount,
                &reward_entry.reward_type,
            ).await.map_err(|e| TaskError::BlockchainError(e.to_string()))?;

            transaction_hashes.push(tx_hash);
        }

        Ok(transaction_hashes)
    }

    // Helper methods
    fn generate_task_id(&self, task_data: &PublishTaskPayload, block_height: u64) -> Hash {
        use crate::crypto::Hashable;

        let mut hasher = crate::crypto::Hash::new();
        hasher.update(&task_data.description_hash.as_bytes());
        hasher.update(&task_data.encrypted_data);
        hasher.update(&block_height.to_le_bytes());
        hasher.update(&chrono::Utc::now().timestamp().to_le_bytes());

        hasher.finalize()
    }

    fn create_initial_task_state(
        &self,
        task_id: Hash,
        publisher: CompressedPublicKey,
        task_data: PublishTaskPayload,
        block_height: u64,
    ) -> TaskState {
        let current_time = chrono::Utc::now().timestamp() as u64;

        TaskState {
            task_id,
            publisher,
            status: TaskStatus::Published,
            lifecycle: TaskLifecycle {
                published_at: current_time,
                submission_deadline: task_data.deadline,
                validation_deadline: task_data.deadline + 86400, // 24-hour validation period
                completion_time: None,
                phase_transitions: vec![PhaseTransition {
                    from_status: TaskStatus::Published,
                    to_status: TaskStatus::Published,
                    timestamp: current_time,
                    trigger: TransitionTrigger::TaskCreated,
                }],
            },
            task_data,
            participants: HashMap::new(),
            submissions: HashMap::new(),
            validations: Vec::new(),
            dispute: None,
            final_results: None,
        }
    }

    async fn get_network_context(&self) -> NetworkContext {
        let current_block = self.blockchain_interface.get_current_block_height().await
            .unwrap_or(0);

        NetworkContext {
            current_block_height: current_block,
            network_difficulty: self.blockchain_interface.get_network_difficulty().await.unwrap_or(1.0),
            total_miners: self.get_total_registered_miners().await.unwrap_or(0),
            average_task_completion_time: self.metrics_collector.get_average_completion_time(),
        }
    }

    async fn get_validation_context(&self) -> ValidationContext {
        ValidationContext {
            current_block: self.blockchain_interface.get_current_block_height().await.unwrap_or(0),
            network_params: self.get_network_parameters(),
            economic_params: self.reward_engine.get_economic_parameters().clone(),
            existing_validations: Vec::new(), // Need to populate based on specific context
            task_history: HashMap::new(), // Need to populate based on specific context
        }
    }
}

// Task Scheduler
pub struct TaskScheduler {
    scheduled_events: BTreeSet<ScheduledEvent>,
    event_queue: mpsc::UnboundedSender<TaskEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScheduledEvent {
    pub execution_time: u64,
    pub event_type: TaskEventType,
    pub task_id: Hash,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskEventType {
    SubmissionDeadline,
    ValidationDeadline,
    TaskExpiration,
    RewardDistribution,
}

impl TaskScheduler {
    pub fn new() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        Self {
            scheduled_events: BTreeSet::new(),
            event_queue: tx,
        }
    }

    pub async fn schedule_task_events(&mut self, task_state: &TaskState) -> Result<(), TaskError> {
        // Schedule submission deadline event
        self.scheduled_events.insert(ScheduledEvent {
            execution_time: task_state.lifecycle.submission_deadline,
            event_type: TaskEventType::SubmissionDeadline,
            task_id: task_state.task_id,
        });

        // Schedule validation deadline event
        self.scheduled_events.insert(ScheduledEvent {
            execution_time: task_state.lifecycle.validation_deadline,
            event_type: TaskEventType::ValidationDeadline,
            task_id: task_state.task_id,
        });

        // Schedule task expiration event
        let expiration_time = task_state.lifecycle.validation_deadline + 86400; // Expire 24 hours after validation
        self.scheduled_events.insert(ScheduledEvent {
            execution_time: expiration_time,
            event_type: TaskEventType::TaskExpiration,
            task_id: task_state.task_id,
        });

        Ok(())
    }

    pub async fn process_due_events(&mut self, current_time: u64) -> Vec<TaskEvent> {
        let mut due_events = Vec::new();
        let mut events_to_remove = Vec::new();

        for event in &self.scheduled_events {
            if event.execution_time <= current_time {
                due_events.push(TaskEvent {
                    event_type: event.event_type.clone(),
                    task_id: event.task_id,
                    timestamp: current_time,
                });
                events_to_remove.push(event.clone());
            } else {
                break; // BTreeSet is ordered, later events are not due yet
            }
        }

        // Remove processed events
        for event in events_to_remove {
            self.scheduled_events.remove(&event);
        }

        due_events
    }
}

// Notification System
pub struct NotificationSystem {
    subscribers: HashMap<NotificationType, Vec<NotificationSubscriber>>,
}

#[derive(Hash, Eq, PartialEq)]
pub enum NotificationType {
    TaskPublished,
    SubmissionReceived,
    ValidationCompleted,
    TaskCompleted,
    RewardsDistributed,
    TaskExpired,
    DisputeRaised,
}

pub struct NotificationSubscriber {
    pub id: String,
    pub endpoint: String,
    pub notification_method: NotificationMethod,
}

pub enum NotificationMethod {
    Webhook,
    Email,
    Push,
}

impl NotificationSystem {
    pub fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }

    pub async fn notify_task_published(&self, task_state: &TaskState) -> Result<(), TaskError> {
        let notification = TaskNotification {
            notification_type: NotificationType::TaskPublished,
            task_id: task_state.task_id,
            message: format!("New task published: {}", task_state.task_data.task_type.name()),
            timestamp: chrono::Utc::now().timestamp() as u64,
            metadata: serde_json::json!({
                "task_type": task_state.task_data.task_type,
                "reward_amount": task_state.task_data.reward_amount,
                "deadline": task_state.lifecycle.submission_deadline,
            }),
        };

        self.send_notification(&notification).await
    }

    async fn send_notification(&self, notification: &TaskNotification) -> Result<(), TaskError> {
        if let Some(subscribers) = self.subscribers.get(&notification.notification_type) {
            for subscriber in subscribers {
                match subscriber.notification_method {
                    NotificationMethod::Webhook => {
                        // Send webhook notification
                        self.send_webhook(&subscriber.endpoint, notification).await?;
                    },
                    NotificationMethod::Email => {
                        // Send email notification
                        self.send_email(&subscriber.endpoint, notification).await?;
                    },
                    NotificationMethod::Push => {
                        // Send push notification
                        self.send_push(&subscriber.endpoint, notification).await?;
                    },
                }
            }
        }
        Ok(())
    }
}

// Metrics Collector
pub struct TaskMetricsCollector {
    task_metrics: HashMap<Hash, TaskMetrics>,
    global_metrics: GlobalTaskMetrics,
}

#[derive(Default)]
pub struct TaskMetrics {
    pub published_at: u64,
    pub first_submission_at: Option<u64>,
    pub completion_time: Option<u64>,
    pub participant_count: u32,
    pub submission_count: u32,
    pub validation_count: u32,
    pub total_reward_distributed: u64,
    pub quality_scores: Vec<u8>,
}

#[derive(Default)]
pub struct GlobalTaskMetrics {
    pub total_tasks_published: u64,
    pub total_tasks_completed: u64,
    pub total_tasks_expired: u64,
    pub average_completion_time: f64,
    pub average_participant_count: f64,
    pub total_rewards_distributed: u64,
    pub average_quality_score: f64,
}

// Error Types
#[derive(Debug, Clone)]
pub enum TaskError {
    TaskNotFound(Hash),
    SubmissionNotFound(Hash),
    MinerNotRegistered(CompressedPublicKey),
    ValidatorNotRegistered(CompressedPublicKey),
    InvalidTaskData(String),
    InvalidTaskStatus(TaskStatus),
    InvalidExpirationState(TaskStatus),
    SubmissionNotAllowed(TaskStatus),
    SubmissionDeadlinePassed,
    MaxParticipantsReached,
    AlreadyParticipating,
    AlreadyValidated,
    SelfValidationNotAllowed,
    InsufficientBalance { required: u64, available: u64 },
    InsufficientStake { required: u64, provided: u64 },
    RewardTooLow { provided: u64, minimum: u64 },
    DeadlineTooSoon { provided: u64, minimum: u64 },
    InsufficientValidatorReputation { required: u32, current: u32 },
    LackOfSpecialization,
    SubmissionRejected(String),
    ValidationError(String),
    RewardCalculationError(String),
    StorageError(String),
    BlockchainError(String),
    NotificationError(String),
}

// Helper Types
pub struct NetworkContext {
    pub current_block_height: u64,
    pub network_difficulty: f64,
    pub total_miners: u32,
    pub average_task_completion_time: f64,
}

pub struct TaskEvent {
    pub event_type: TaskEventType,
    pub task_id: Hash,
    pub timestamp: u64,
}

pub struct TaskNotification {
    pub notification_type: NotificationType,
    pub task_id: Hash,
    pub message: String,
    pub timestamp: u64,
    pub metadata: serde_json::Value,
}

pub struct ConsensusResult {
    pub task_id: Hash,
    pub submission_consensuses: Vec<SubmissionConsensus>,
    pub overall_confidence: f64,
    pub consensus_achieved_at: u64,
}

pub struct SubmissionConsensus {
    pub submission_id: Hash,
    pub consensus_score: u8,
    pub validator_count: usize,
    pub confidence: f64,
}
```

This task management system implements:

1. **Complete Task Lifecycle Management**: Full flow control from publication to completion
2. **Strict Validation and Eligibility Checks**: Ensuring all participants meet requirements
3. **Anti-Fraud Integration**: Real-time detection and handling of suspicious behavior
4. **Automated Scheduling**: Time-based event triggering and processing
5. **Notification System**: Multi-channel real-time notification mechanism
6. **Metrics Collection**: Complete performance and statistical data collection
7. **Dispute Handling**: Fair dispute resolution mechanism
8. **Error Handling**: Detailed error classification and handling

Next, I will continue to improve the miner management system and API interface design.