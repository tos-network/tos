//! AI Mining transaction validation logic

use crate::{
    ai_mining::{
        AIMiningError, AIMiningPayload, AIMiningResult, AIMiningState, AIMiningTask,
        AntiSybilDetector, DifficultyLevel, ReputationActivity, SubmittedAnswer, TaskStatus,
        ValidationScore,
    },
    crypto::{elgamal::CompressedPublicKey, Hash},
};

/// Validation context for AI mining transactions
pub struct AIMiningValidator<'a> {
    /// Current AI mining state
    pub state: &'a mut AIMiningState,
    /// Current block height
    pub block_height: u64,
    /// Current timestamp
    pub current_time: u64,
    /// Transaction source address
    pub source: CompressedPublicKey,
}

impl<'a> AIMiningValidator<'a> {
    /// Create a new AI mining validator
    pub fn new(
        state: &'a mut AIMiningState,
        block_height: u64,
        current_time: u64,
        source: CompressedPublicKey,
    ) -> Self {
        Self {
            state,
            block_height,
            current_time,
            source,
        }
    }

    /// Validate and apply an AI mining transaction payload
    pub fn validate_and_apply(&mut self, payload: &AIMiningPayload) -> AIMiningResult<()> {
        payload.validate()?;

        match payload {
            AIMiningPayload::RegisterMiner {
                miner_address,
                registration_fee,
            } => self.validate_register_miner(miner_address, *registration_fee),
            AIMiningPayload::PublishTask {
                task_id,
                reward_amount,
                difficulty,
                deadline,
                description,
            } => self.validate_publish_task(
                task_id,
                *reward_amount,
                difficulty,
                *deadline,
                description,
            ),
            AIMiningPayload::SubmitAnswer {
                task_id,
                answer_hash,
                stake_amount,
                answer_content,
            } => self.validate_submit_answer(task_id, answer_hash, answer_content, *stake_amount),
            AIMiningPayload::ValidateAnswer {
                task_id,
                answer_id,
                validation_score,
            } => self.validate_answer_validation(task_id, answer_id, *validation_score),
        }
    }

    /// Validate miner registration
    fn validate_register_miner(
        &mut self,
        miner_address: &CompressedPublicKey,
        registration_fee: u64,
    ) -> AIMiningResult<()> {
        // Check if source matches miner address
        if self.source != *miner_address {
            return Err(AIMiningError::ValidationFailed(
                "Transaction source must match miner address".to_string(),
            ));
        }

        // Check minimum registration fee (1 TOS)
        let min_registration_fee = 1_000_000_000; // 1 TOS in nanoTOS
        if registration_fee < min_registration_fee {
            return Err(AIMiningError::ValidationFailed(format!(
                "Registration fee {} is below minimum {}",
                registration_fee, min_registration_fee
            )));
        }

        // Register the miner
        self.state
            .register_miner(miner_address.clone(), registration_fee, self.block_height)?;
        self.state
            .ensure_account_reputation(&self.source, self.current_time);

        Ok(())
    }

    /// Validate task publishing
    fn validate_publish_task(
        &mut self,
        task_id: &Hash,
        reward_amount: u64,
        difficulty: &DifficultyLevel,
        deadline: u64,
        description: &String,
    ) -> AIMiningResult<()> {
        // Check if publisher is registered miner
        if !self.state.is_miner_registered(&self.source) {
            return Err(AIMiningError::MinerNotRegistered(self.source.clone()));
        }

        // Validate deadline is in the future
        if deadline <= self.current_time {
            return Err(AIMiningError::ValidationFailed(
                "Task deadline must be in the future".to_string(),
            ));
        }

        // Validate deadline is not too far in the future (max 30 days)
        let max_deadline = self.current_time + (30 * 24 * 60 * 60); // 30 days in seconds
        if deadline > max_deadline {
            return Err(AIMiningError::ValidationFailed(
                "Task deadline cannot be more than 30 days in the future".to_string(),
            ));
        }

        // Validate reward amount against difficulty
        let (min_reward, max_reward) = difficulty.reward_range();
        if reward_amount < min_reward || reward_amount > max_reward {
            return Err(AIMiningError::InvalidTaskConfig(format!(
                "Reward amount {} is outside valid range [{}, {}] for difficulty {:?}",
                reward_amount, min_reward, max_reward, difficulty
            )));
        }

        // Check publisher reputation for large rewards
        if let Some(miner) = self.state.get_miner(&self.source) {
            let reputation_threshold = match difficulty {
                DifficultyLevel::Expert => 800,
                DifficultyLevel::Advanced => 600,
                DifficultyLevel::Intermediate => 400,
                DifficultyLevel::Beginner => 200,
            };

            if miner.reputation < reputation_threshold {
                return Err(AIMiningError::ValidationFailed(format!(
                    "Publisher reputation {} is below required {} for difficulty {:?}",
                    miner.reputation, reputation_threshold, difficulty
                )));
            }
        }

        {
            let reputation = self
                .state
                .ensure_account_reputation(&self.source, self.current_time);

            if !reputation.can_submit_now(self.current_time) {
                let remaining = reputation.get_remaining_cooldown(self.current_time);
                return Err(AIMiningError::ValidationFailed(format!(
                    "Account is in cooldown for {} more seconds",
                    remaining
                )));
            }

            reputation.calculate_reputation_score(self.current_time);
            if !reputation.can_participate_in_difficulty(difficulty) {
                return Err(AIMiningError::ValidationFailed(
                    "Account reputation too low to publish tasks at this difficulty".to_string(),
                ));
            }

            let anti_sybil = AntiSybilDetector::detect_sybil_risk(reputation, self.current_time);
            if !anti_sybil.is_valid {
                return Err(AIMiningError::ValidationFailed(format!(
                    "Anti-Sybil check failed: {}",
                    anti_sybil.details.join(", ")
                )));
            }
        }

        // Create and publish the task
        let task = AIMiningTask::new(
            task_id.clone(),
            self.source.clone(),
            description.clone(),
            reward_amount,
            difficulty.clone(),
            deadline,
            self.block_height,
        )?;

        self.state.publish_task(task)?;

        if let Some(reputation) = self.state.get_account_reputation_mut(&self.source) {
            reputation.update_submission_time(self.current_time);
        }

        // Update miner statistics
        if let Some(miner) = self.state.get_miner_mut(&self.source) {
            miner.tasks_published += 1;
            miner.update_reputation(ReputationActivity::TaskPublish, true);
        }

        Ok(())
    }

    /// Validate answer submission
    fn validate_submit_answer(
        &mut self,
        task_id: &Hash,
        answer_hash: &Hash,
        answer_content: &String,
        stake_amount: u64,
    ) -> AIMiningResult<()> {
        // Check if submitter is registered miner
        if !self.state.is_miner_registered(&self.source) {
            return Err(AIMiningError::MinerNotRegistered(self.source.clone()));
        }

        // Get task information and validate basic constraints
        let (reward_amount, task_difficulty) = {
            let task = self
                .state
                .get_task(task_id)
                .ok_or_else(|| AIMiningError::TaskNotFound(task_id.clone()))?;

            if task.status != TaskStatus::Active {
                return Err(AIMiningError::ValidationFailed(
                    "Task is not active".to_string(),
                ));
            }

            if task.is_expired(self.current_time) {
                return Err(AIMiningError::ValidationFailed(
                    "Task has expired".to_string(),
                ));
            }

            if task.publisher == self.source {
                return Err(AIMiningError::ValidationFailed(
                    "Task publisher cannot submit answers to their own task".to_string(),
                ));
            }

            (task.reward_amount, task.difficulty.clone())
        };

        // Validate minimum stake amount (10% of reward)
        let min_stake = reward_amount / 10;
        if stake_amount < min_stake {
            return Err(AIMiningError::InsufficientStake {
                required: min_stake,
                available: stake_amount,
            });
        }

        // Validate maximum stake amount (50% of reward)
        let max_stake = reward_amount / 2;
        if stake_amount > max_stake {
            return Err(AIMiningError::ValidationFailed(format!(
                "Stake amount {} exceeds maximum {} (50% of reward)",
                stake_amount, max_stake
            )));
        }

        // Check submitter reputation
        if let Some(miner) = self.state.get_miner(&self.source) {
            let min_reputation = match task_difficulty {
                DifficultyLevel::Expert => 700,
                DifficultyLevel::Advanced => 500,
                DifficultyLevel::Intermediate => 300,
                DifficultyLevel::Beginner => 100,
            };

            if miner.reputation < min_reputation {
                return Err(AIMiningError::ValidationFailed(format!(
                    "Submitter reputation {} is below required {} for difficulty {:?}",
                    miner.reputation, min_reputation, task_difficulty
                )));
            }
        }

        {
            let reputation = self
                .state
                .ensure_account_reputation(&self.source, self.current_time);

            if !reputation.can_submit_now(self.current_time) {
                let remaining = reputation.get_remaining_cooldown(self.current_time);
                return Err(AIMiningError::ValidationFailed(format!(
                    "Account is in cooldown for {} more seconds",
                    remaining
                )));
            }

            reputation.update_stake(stake_amount);
            reputation.calculate_reputation_score(self.current_time);
            if !reputation.can_participate_in_difficulty(&task_difficulty) {
                return Err(AIMiningError::ValidationFailed(
                    "Account reputation too low for this task difficulty".to_string(),
                ));
            }

            let anti_sybil = AntiSybilDetector::detect_sybil_risk(reputation, self.current_time);
            if !anti_sybil.is_valid {
                return Err(AIMiningError::ValidationFailed(format!(
                    "Anti-Sybil check failed: {}",
                    anti_sybil.details.join(", ")
                )));
            }
        }

        // Ensure provided content matches declared hash
        let computed_hash = crate::crypto::hash(answer_content.as_bytes());
        if &computed_hash != answer_hash {
            return Err(AIMiningError::ValidationFailed(
                "Answer hash does not match provided content".to_string(),
            ));
        }

        // Create the submitted answer
        let mut hasher = blake3::Hasher::new();
        hasher.update(task_id.as_bytes());
        hasher.update(self.source.as_bytes());
        hasher.update(answer_hash.as_bytes());
        hasher.update(&self.block_height.to_be_bytes());
        hasher.update(&self.current_time.to_be_bytes());
        let answer_id = Hash::new(hasher.finalize().into());

        let answer = SubmittedAnswer::new(
            answer_id,
            answer_content.clone(),
            answer_hash.clone(),
            self.source.clone(),
            stake_amount,
            self.block_height,
        );

        // Add answer to task
        let task = self.state.get_task_mut(task_id).unwrap();
        task.add_answer(answer)?;

        // Update miner statistics
        if let Some(miner) = self.state.get_miner_mut(&self.source) {
            miner.answers_submitted += 1;
            miner.update_reputation(ReputationActivity::AnswerSubmit, true);
        }

        if let Some(reputation) = self.state.get_account_reputation_mut(&self.source) {
            reputation.update_submission_time(self.current_time);
        }

        Ok(())
    }

    /// Validate answer validation
    fn validate_answer_validation(
        &mut self,
        task_id: &Hash,
        answer_id: &Hash,
        validation_score: u8,
    ) -> AIMiningResult<()> {
        // Check if validator is registered miner
        if !self.state.is_miner_registered(&self.source) {
            return Err(AIMiningError::MinerNotRegistered(self.source.clone()));
        }

        // Validate score range (0-100)
        if validation_score > 100 {
            return Err(AIMiningError::ValidationFailed(
                "Validation score must be between 0-100".to_string(),
            ));
        }
        // Get task information and ensure validation constraints
        let task_difficulty = {
            let task = self
                .state
                .get_task(task_id)
                .ok_or_else(|| AIMiningError::TaskNotFound(task_id.clone()))?;

            if matches!(task.status, TaskStatus::Cancelled) {
                return Err(AIMiningError::ValidationFailed(
                    "Cannot validate answers for cancelled task".to_string(),
                ));
            }

            let answer = task
                .submitted_answers
                .iter()
                .find(|a| a.answer_id == *answer_id)
                .ok_or_else(|| {
                    AIMiningError::ValidationFailed("Answer not found in task".to_string())
                })?;

            if answer.submitter == self.source {
                return Err(AIMiningError::ValidationFailed(
                    "Cannot validate your own answer".to_string(),
                ));
            }

            if task.publisher == self.source {
                return Err(AIMiningError::ValidationFailed(
                    "Task publisher cannot validate answers".to_string(),
                ));
            }

            task.difficulty.clone()
        };

        if let Some(miner) = self.state.get_miner(&self.source) {
            let min_reputation = match task_difficulty {
                DifficultyLevel::Expert => 750,
                DifficultyLevel::Advanced => 550,
                DifficultyLevel::Intermediate => 350,
                DifficultyLevel::Beginner => 150,
            };

            if miner.reputation < min_reputation {
                return Err(AIMiningError::ValidationFailed(format!(
                    "Validator reputation {} is below required {} for difficulty {:?}",
                    miner.reputation, min_reputation, task_difficulty
                )));
            }
        }

        {
            let reputation = self
                .state
                .ensure_account_reputation(&self.source, self.current_time);

            if !reputation.can_submit_now(self.current_time) {
                let remaining = reputation.get_remaining_cooldown(self.current_time);
                return Err(AIMiningError::ValidationFailed(format!(
                    "Account is in cooldown for {} more seconds",
                    remaining
                )));
            }

            reputation.calculate_reputation_score(self.current_time);
            if !reputation.can_participate_in_difficulty(&task_difficulty) {
                return Err(AIMiningError::ValidationFailed(
                    "Account reputation too low to validate answers for this difficulty"
                        .to_string(),
                ));
            }

            let anti_sybil = AntiSybilDetector::detect_sybil_risk(reputation, self.current_time);
            if !anti_sybil.is_valid {
                return Err(AIMiningError::ValidationFailed(format!(
                    "Anti-Sybil check failed: {}",
                    anti_sybil.details.join(", ")
                )));
            }
        }

        let validation_success = validation_score >= 70;
        let validation = ValidationScore {
            validator: self.source.clone(),
            score: validation_score,
            validated_at: self.block_height,
        };

        let task = self.state.get_task_mut(task_id).unwrap();
        let answer = task
            .submitted_answers
            .iter_mut()
            .find(|a| a.answer_id == *answer_id)
            .unwrap();

        answer.add_validation(validation)?;

        task.update_status(self.current_time);

        if let Some(miner) = self.state.get_miner_mut(&self.source) {
            miner.validations_performed += 1;
            miner.update_reputation(ReputationActivity::Validation, true);
        }

        if let Some(reputation) = self.state.get_account_reputation_mut(&self.source) {
            reputation.update_submission_time(self.current_time);
            reputation.record_validation(validation_success);
        }

        Ok(())
    }

    /// Update expired tasks and finalize completed ones
    pub fn update_tasks(&mut self) -> AIMiningResult<()> {
        self.state.update_task_statuses(self.current_time);

        // Process completed tasks to distribute rewards
        let completed_tasks: Vec<Hash> = self
            .state
            .tasks
            .iter()
            .filter(|(_, task)| task.status == TaskStatus::Completed && !task.rewards_processed)
            .map(|(id, _)| id.clone())
            .collect();

        for task_id in completed_tasks {
            self.process_completed_task(&task_id)?;
        }

        Ok(())
    }

    /// Process a completed task and distribute rewards
    fn process_completed_task(&mut self, task_id: &Hash) -> AIMiningResult<()> {
        // First collect all the addresses we need to update
        let (publisher, reward_amount, best_answer_opt) = {
            let task = self
                .state
                .get_task(task_id)
                .ok_or_else(|| AIMiningError::TaskNotFound(task_id.clone()))?;

            if task.rewards_processed {
                return Ok(());
            }

            (
                task.publisher.clone(),
                task.reward_amount,
                task.get_best_answer().cloned(),
            )
        };

        let mut addresses_to_update = Vec::new();

        if let Some(best_answer) = best_answer_opt.as_ref() {
            // Collect submitter address
            addresses_to_update.push((
                best_answer.submitter.clone(),
                ReputationActivity::AnswerSubmit,
                true,
            ));

            // Collect validator addresses
            for validation in &best_answer.validation_scores {
                let validation_quality = validation.score >= 70;
                addresses_to_update.push((
                    validation.validator.clone(),
                    ReputationActivity::Validation,
                    validation_quality,
                ));
            }

            // Collect publisher address
            addresses_to_update.push((publisher.clone(), ReputationActivity::TaskPublish, true));
        }

        // Now update all the miners
        for (address, activity, success) in addresses_to_update {
            if let Some(miner) = self.state.get_miner_mut(&address) {
                miner.update_reputation(activity, success);
            }
        }

        if let Some(best_answer) = best_answer_opt.as_ref() {
            if let Some(reputation) = self
                .state
                .get_account_reputation_mut(&best_answer.submitter)
            {
                reputation.record_reward(reward_amount);
            }

            for validation in &best_answer.validation_scores {
                if let Some(reputation) =
                    self.state.get_account_reputation_mut(&validation.validator)
                {
                    reputation.record_validation(validation.score >= 70);
                }
            }
        }

        if let Some(task) = self.state.get_task_mut(task_id) {
            task.rewards_processed = true;
        }

        Ok(())
    }

    /// Get validation summary for debugging
    pub fn get_validation_summary(&self) -> ValidationSummary {
        ValidationSummary {
            total_miners: self.state.statistics.total_miners,
            active_tasks: self.state.statistics.active_tasks,
            completed_tasks: self.state.statistics.completed_tasks,
            total_staked: self.state.statistics.total_staked,
            block_height: self.block_height,
            current_time: self.current_time,
        }
    }
}

/// Summary of validation state for debugging
#[derive(Debug, Clone)]
pub struct ValidationSummary {
    pub total_miners: u64,
    pub active_tasks: u64,
    pub completed_tasks: u64,
    pub total_staked: u64,
    pub block_height: u64,
    pub current_time: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::ristretto::CompressedRistretto;

    fn create_test_pubkey(bytes: [u8; 32]) -> CompressedPublicKey {
        CompressedPublicKey::new(CompressedRistretto::from_slice(&bytes).unwrap())
    }

    #[test]
    fn test_miner_registration_validation() {
        let mut state = AIMiningState::new();
        let miner_address = create_test_pubkey([1u8; 32]);
        let mut validator = AIMiningValidator::new(&mut state, 100, 1000, miner_address.clone());

        let payload = AIMiningPayload::RegisterMiner {
            miner_address: miner_address.clone(),
            registration_fee: 1_000_000_000, // 1 TOS
        };

        assert!(validator.validate_and_apply(&payload).is_ok());
        assert!(validator.state.is_miner_registered(&miner_address));
    }

    #[test]
    fn test_insufficient_registration_fee() {
        let mut state = AIMiningState::new();
        let miner_address = create_test_pubkey([1u8; 32]);
        let mut validator = AIMiningValidator::new(&mut state, 100, 1000, miner_address.clone());

        let payload = AIMiningPayload::RegisterMiner {
            miner_address: miner_address.clone(),
            registration_fee: 500_000_000, // 0.5 TOS (below minimum)
        };

        assert!(validator.validate_and_apply(&payload).is_err());
    }

    #[test]
    fn test_task_publishing_validation() {
        let mut state = AIMiningState::new();
        let publisher = create_test_pubkey([1u8; 32]);

        // Register miner first
        state
            .register_miner(publisher.clone(), 1_000_000_000, 100)
            .unwrap();

        let mut validator = AIMiningValidator::new(&mut state, 100, 1000, publisher.clone());

        let payload = AIMiningPayload::PublishTask {
            task_id: Hash::new([1u8; 32]),
            reward_amount: 10_000_000_000, // 10 TOS
            difficulty: DifficultyLevel::Beginner,
            deadline: 2000, // Future deadline
            description: "Test task description".to_string(),
        };

        assert!(validator.validate_and_apply(&payload).is_ok());
        assert_eq!(validator.state.statistics.total_tasks, 1);
    }

    #[test]
    fn test_past_deadline_rejection() {
        let mut state = AIMiningState::new();
        let publisher = create_test_pubkey([1u8; 32]);

        state
            .register_miner(publisher.clone(), 1_000_000_000, 100)
            .unwrap();

        let mut validator = AIMiningValidator::new(&mut state, 100, 1000, publisher.clone());

        let payload = AIMiningPayload::PublishTask {
            task_id: Hash::new([1u8; 32]),
            reward_amount: 10_000_000_000,
            difficulty: DifficultyLevel::Beginner,
            deadline: 500, // Past deadline
            description: "Test task description".to_string(),
        };

        assert!(validator.validate_and_apply(&payload).is_err());
    }

    #[test]
    fn test_answer_submission_validation() {
        let mut state = AIMiningState::new();
        let publisher = create_test_pubkey([1u8; 32]);
        let submitter = create_test_pubkey([2u8; 32]);

        // Register both users
        state
            .register_miner(publisher.clone(), 1_000_000_000, 100)
            .unwrap();
        state
            .register_miner(submitter.clone(), 1_000_000_000, 100)
            .unwrap();

        // Create task
        let task_id = Hash::new([1u8; 32]);
        let task = AIMiningTask::new(
            task_id.clone(),
            publisher.clone(),
            "Test task".to_string(),
            10_000_000_000,
            DifficultyLevel::Beginner,
            2000,
            100,
        )
        .unwrap();
        state.publish_task(task).unwrap();

        // Submit answer
        let mut validator = AIMiningValidator::new(&mut state, 150, 1500, submitter.clone());
        let answer_content = "Test answer content for validation".to_string();
        let answer_hash = crate::crypto::hash(answer_content.as_bytes());
        let payload = AIMiningPayload::SubmitAnswer {
            task_id: task_id.clone(),
            answer_content,
            answer_hash,
            stake_amount: 1_000_000_000, // 10% of reward
        };

        assert!(validator.validate_and_apply(&payload).is_ok());
    }

    #[test]
    fn test_self_answer_rejection() {
        let mut state = AIMiningState::new();
        let publisher = create_test_pubkey([1u8; 32]);

        state
            .register_miner(publisher.clone(), 1_000_000_000, 100)
            .unwrap();

        let task_id = Hash::new([1u8; 32]);
        let task = AIMiningTask::new(
            task_id.clone(),
            publisher.clone(),
            "Test task".to_string(),
            10_000_000_000,
            DifficultyLevel::Beginner,
            2000,
            100,
        )
        .unwrap();
        state.publish_task(task).unwrap();

        // Try to submit answer to own task
        let mut validator = AIMiningValidator::new(&mut state, 150, 1500, publisher.clone());
        let answer_content = "Test answer content for validation".to_string();
        let answer_hash = crate::crypto::hash(answer_content.as_bytes());
        let payload = AIMiningPayload::SubmitAnswer {
            task_id: task_id.clone(),
            answer_content,
            answer_hash,
            stake_amount: 1_000_000_000,
        };

        assert!(validator.validate_and_apply(&payload).is_err());
    }

    #[test]
    fn test_validation_summary() {
        let mut state = AIMiningState::new();
        let miner = create_test_pubkey([1u8; 32]);

        state
            .register_miner(miner.clone(), 1_000_000_000, 100)
            .unwrap();

        let validator = AIMiningValidator::new(&mut state, 100, 1000, miner);
        let summary = validator.get_validation_summary();

        assert_eq!(summary.total_miners, 1);
        assert_eq!(summary.block_height, 100);
        assert_eq!(summary.current_time, 1000);
    }
}
