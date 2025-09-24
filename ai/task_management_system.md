# AI挖矿任务管理系统

## 任务管理器核心架构

### 1. 任务管理器主要接口 (daemon/src/ai/task_manager.rs)

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
        // 验证发布者资格和任务数据
        self.validate_task_publication(&publisher, &task_data).await?;

        // 创建任务状态
        let task_id = self.generate_task_id(&task_data, block_height);
        let task_state = self.create_initial_task_state(
            task_id,
            publisher,
            task_data,
            block_height,
        );

        // 存储任务状态
        {
            let mut storage = self.storage.write().await;
            storage.store_task_state(&task_id, &task_state).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
        }

        // 调度任务生命周期事件
        self.task_scheduler.schedule_task_events(&task_state).await?;

        // 发送通知
        self.notification_system.notify_task_published(&task_state).await?;

        // 记录指标
        self.metrics_collector.record_task_published(&task_state);

        Ok(task_id)
    }

    pub async fn submit_answer(
        &mut self,
        submitter: CompressedPublicKey,
        submission: SubmitAnswerPayload,
        block_height: u64,
    ) -> Result<Hash, TaskError> {
        // 获取任务状态
        let task_state = {
            let storage = self.storage.read().await;
            storage.get_task_state(&submission.task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::TaskNotFound(submission.task_id))?
        };

        // 验证提交资格
        self.validate_submission_eligibility(&submitter, &submission, &task_state).await?;

        // 进行防作弊检测
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

        // 根据防作弊分析结果决定是否接受提交
        if fraud_analysis.overall_risk_score > 0.8 {
            return Err(TaskError::SubmissionRejected(format!(
                "High fraud risk detected: {}",
                fraud_analysis.overall_risk_score
            )));
        }

        // 创建提交状态
        let submission_id = self.generate_submission_id(&submission, block_height);
        let submission_state = self.create_submission_state(
            submission_id,
            submitter,
            submission,
            block_height,
            fraud_analysis.overall_risk_score,
        );

        // 存储提交状态和防作弊分析
        {
            let mut storage = self.storage.write().await;
            storage.store_submission(&submission_state).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
            storage.store_fraud_analysis(&fraud_analysis).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
        }

        // 更新任务状态
        self.update_task_with_new_submission(&submission.task_id, &submission_id).await?;

        // 如果风险分数中等，标记需要额外验证
        if fraud_analysis.overall_risk_score > 0.4 {
            self.request_additional_validation(&submission_id, &fraud_analysis).await?;
        }

        // 检查是否达到提交截止时间或最大参与者数量
        self.check_submission_phase_completion(&submission.task_id).await?;

        // 发送通知
        self.notification_system.notify_submission_received(&submission_state, &task_state).await?;

        // 记录指标
        self.metrics_collector.record_submission_received(&submission_state);

        Ok(submission_id)
    }

    pub async fn validate_submission(
        &mut self,
        validator: CompressedPublicKey,
        validation: ValidateAnswerPayload,
        block_height: u64,
    ) -> Result<Hash, TaskError> {
        // 获取任务和提交状态
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

        // 验证验证者资格
        self.validate_validator_eligibility(&validator, &validation, &task_state).await?;

        // 获取验证者状态
        let validator_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(&validator).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::ValidatorNotRegistered(validator.clone()))?
        };

        // 执行验证
        let validation_result = self.perform_validation(
            &validator,
            &validator_state,
            &task_state,
            &submission_state,
            &validation,
        ).await?;

        // 创建验证记录
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

        // 存储验证结果
        {
            let mut storage = self.storage.write().await;
            storage.store_validation_result(&validation_record).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
        }

        // 更新提交的验证状态
        self.update_submission_validation_status(
            &validation.answer_id,
            &validation_record,
        ).await?;

        // 检查是否达成验证共识
        let consensus_result = self.check_validation_consensus(&validation.task_id).await?;

        if let Some(consensus) = consensus_result {
            self.finalize_task_validation(&validation.task_id, consensus).await?;
        }

        // 发送通知
        self.notification_system.notify_validation_completed(&validation_record).await?;

        // 记录指标
        self.metrics_collector.record_validation_completed(&validation_record);

        Ok(validation_record.validation_id)
    }

    pub async fn process_task_completion(&mut self, task_id: Hash) -> Result<RewardDistribution, TaskError> {
        // 获取完整的任务状态
        let task_state = {
            let storage = self.storage.read().await;
            storage.get_task_state(&task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::TaskNotFound(task_id))?
        };

        // 验证任务可以完成
        if !matches!(task_state.status, TaskStatus::UnderValidation) {
            return Err(TaskError::InvalidTaskStatus(task_state.status));
        }

        // 获取所有验证结果
        let validation_results = self.collect_all_validation_results(&task_id).await?;

        // 计算奖励分发
        let reward_distribution = self.reward_engine.calculate_task_rewards(
            &task_state,
            &validation_results,
            &self.get_network_context().await,
        ).await.map_err(|e| TaskError::RewardCalculationError(format!("{:?}", e)))?;

        // 存储奖励分发记录
        {
            let mut storage = self.storage.write().await;
            storage.store_reward_distribution(&reward_distribution).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?;
        }

        // 更新任务状态为已完成
        self.update_task_status(&task_id, TaskStatus::Completed).await?;

        // 更新参与者声誉
        self.update_participant_reputations(&task_state, &validation_results, &reward_distribution).await?;

        // 发送奖励分发通知
        self.notification_system.notify_rewards_distributed(&reward_distribution).await?;

        // 创建链上交易记录奖励分发
        self.create_reward_distribution_transactions(&reward_distribution).await?;

        // 记录指标
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
                // 任务发布后无人参与，退还发布者资金
                self.refund_publisher(&task_state).await?;
                self.update_task_status(&task_id, TaskStatus::Expired).await?;
            },
            TaskStatus::InProgress => {
                // 有参与者但无提交，部分退款
                self.handle_partial_completion(&task_state).await?;
                self.update_task_status(&task_id, TaskStatus::Expired).await?;
            },
            TaskStatus::AnswersSubmitted => {
                // 有提交但验证超时，强制进入验证阶段
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
        // 验证争议的有效性
        self.validate_dispute(&dispute).await?;

        // 暂停相关任务的奖励分发
        self.suspend_task_rewards(&dispute.task_id).await?;

        // 启动争议解决流程
        let resolution = self.resolve_dispute(dispute).await?;

        // 根据解决结果调整奖励
        self.apply_dispute_resolution(&resolution).await?;

        // 恢复任务奖励分发
        self.resume_task_rewards(&resolution.task_id).await?;

        Ok(resolution)
    }

    // 任务生命周期管理
    pub async fn run_background_tasks(&mut self) -> Result<(), TaskError> {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

        loop {
            interval.tick().await;

            // 检查任务超时
            if let Err(e) = self.check_task_timeouts().await {
                log::error!("Error checking task timeouts: {:?}", e);
            }

            // 处理待验证的提交
            if let Err(e) = self.process_pending_validations().await {
                log::error!("Error processing pending validations: {:?}", e);
            }

            // 完成准备好的任务
            if let Err(e) = self.complete_ready_tasks().await {
                log::error!("Error completing ready tasks: {:?}", e);
            }

            // 清理过期数据
            if let Err(e) = self.cleanup_expired_data().await {
                log::error!("Error cleaning up expired data: {:?}", e);
            }

            // 更新网络统计
            if let Err(e) = self.update_network_statistics().await {
                log::error!("Error updating network statistics: {:?}", e);
            }
        }
    }
}

// 任务管理器内部实现
impl TaskManager {
    async fn validate_task_publication(
        &self,
        publisher: &CompressedPublicKey,
        task_data: &PublishTaskPayload,
    ) -> Result<(), TaskError> {
        // 检查发布者是否有足够的TOS余额
        let publisher_balance = self.blockchain_interface.get_balance(publisher).await
            .map_err(|e| TaskError::BlockchainError(e.to_string()))?;

        let required_amount = task_data.reward_amount + self.calculate_publishing_fee(task_data);
        if publisher_balance < required_amount {
            return Err(TaskError::InsufficientBalance {
                required: required_amount,
                available: publisher_balance,
            });
        }

        // 验证任务数据完整性
        if task_data.description_hash.is_empty() || task_data.encrypted_data.is_empty() {
            return Err(TaskError::InvalidTaskData("Missing required data".to_string()));
        }

        // 检查奖励金额是否合理
        let min_reward = self.get_minimum_reward_for_task_type(&task_data.task_type);
        if task_data.reward_amount < min_reward {
            return Err(TaskError::RewardTooLow {
                provided: task_data.reward_amount,
                minimum: min_reward,
            });
        }

        // 检查截止时间是否合理
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
        // 检查任务状态
        if !matches!(task_state.status, TaskStatus::Published | TaskStatus::InProgress) {
            return Err(TaskError::SubmissionNotAllowed(task_state.status));
        }

        // 检查是否已过截止时间
        let current_time = chrono::Utc::now().timestamp() as u64;
        if current_time > task_state.lifecycle.submission_deadline {
            return Err(TaskError::SubmissionDeadlinePassed);
        }

        // 检查是否已达到最大参与者数量
        if task_state.participants.len() >= task_state.task_data.max_participants as usize {
            return Err(TaskError::MaxParticipantsReached);
        }

        // 检查提交者是否已经参与
        if task_state.participants.contains_key(submitter) {
            return Err(TaskError::AlreadyParticipating);
        }

        // 检查质押金额
        if submission.stake_amount < task_state.task_data.stake_required {
            return Err(TaskError::InsufficientStake {
                required: task_state.task_data.stake_required,
                provided: submission.stake_amount,
            });
        }

        // 检查提交者余额
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
        // 检查验证者是否是提交者（不能验证自己的提交）
        let submission_state = {
            let storage = self.storage.read().await;
            storage.get_submission(&validation.answer_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::SubmissionNotFound(validation.answer_id))?
        };

        if submission_state.submitter == *validator {
            return Err(TaskError::SelfValidationNotAllowed);
        }

        // 检查验证者声誉
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

        // 检查专业匹配度
        let has_specialization = validator_state.specializations.iter()
            .any(|spec| self.task_types_match(spec, &task_state.task_data.task_type));

        if !has_specialization && task_state.task_data.verification_type.requires_specialization() {
            return Err(TaskError::LackOfSpecialization);
        }

        // 检查验证者是否已经验证过此提交
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
        // 根据验证类型执行不同的验证逻辑
        match &task_state.task_data.verification_type {
            VerificationType::Automatic => {
                // 自动验证
                let auto_validator = AutomaticValidator::new();
                auto_validator.validate_submission(
                    task_state,
                    submission_state,
                    &self.get_validation_context().await,
                ).await.map_err(|e| TaskError::ValidationError(format!("{:?}", e)))
            },
            VerificationType::PeerReview { .. } => {
                // 同行验证
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
                // 专家验证
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
                // 混合验证
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
        // 获取任务的所有提交
        let submissions = {
            let storage = self.storage.read().await;
            storage.list_submissions_for_task(task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
        };

        let mut consensus_results = Vec::new();

        for submission_id in submissions {
            // 获取每个提交的验证结果
            let validations = {
                let storage = self.storage.read().await;
                storage.get_validation_results(&submission_id).await
                    .map_err(|e| TaskError::StorageError(e.to_string()))?
            };

            if validations.is_empty() {
                continue;
            }

            // 检查是否达成共识
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

        // 检查是否所有提交都有共识结果
        let task_state = {
            let storage = self.storage.read().await;
            storage.get_task_state(task_id).await
                .map_err(|e| TaskError::StorageError(e.to_string()))?
                .ok_or(TaskError::TaskNotFound(*task_id))?
        };

        let required_consensus_count = task_state.submissions.len();
        let achieved_consensus_count = consensus_results.len();

        if achieved_consensus_count >= required_consensus_count.min(1) {
            // 所有提交都有共识，可以完成任务
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

        // 计算共识度（分数分散程度）
        let variance = weighted_scores.iter()
            .map(|&score| (score - weighted_average).powi(2))
            .sum::<f64>() / weighted_scores.len() as f64;

        let standard_deviation = variance.sqrt();

        // 共识度阈值：标准差小于15分视为达成共识
        let consensus_reached = standard_deviation < 15.0 && validations.len() >= 2;

        let confidence_level = if consensus_reached {
            (1.0 - (standard_deviation / 50.0)).max(0.5) // 至少50%置信度
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
            // 创建奖励转账交易
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

    // 辅助方法
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
                validation_deadline: task_data.deadline + 86400, // 24小时验证期
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
            existing_validations: Vec::new(), // 需要根据具体情况填充
            task_history: HashMap::new(), // 需要根据具体情况填充
        }
    }
}

// 任务调度器
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
        // 调度提交截止事件
        self.scheduled_events.insert(ScheduledEvent {
            execution_time: task_state.lifecycle.submission_deadline,
            event_type: TaskEventType::SubmissionDeadline,
            task_id: task_state.task_id,
        });

        // 调度验证截止事件
        self.scheduled_events.insert(ScheduledEvent {
            execution_time: task_state.lifecycle.validation_deadline,
            event_type: TaskEventType::ValidationDeadline,
            task_id: task_state.task_id,
        });

        // 调度任务过期事件
        let expiration_time = task_state.lifecycle.validation_deadline + 86400; // 验证后24小时过期
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
                break; // BTreeSet是有序的，后面的事件都还没到时间
            }
        }

        // 移除已处理的事件
        for event in events_to_remove {
            self.scheduled_events.remove(&event);
        }

        due_events
    }
}

// 通知系统
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
                        // 发送webhook通知
                        self.send_webhook(&subscriber.endpoint, notification).await?;
                    },
                    NotificationMethod::Email => {
                        // 发送邮件通知
                        self.send_email(&subscriber.endpoint, notification).await?;
                    },
                    NotificationMethod::Push => {
                        // 发送推送通知
                        self.send_push(&subscriber.endpoint, notification).await?;
                    },
                }
            }
        }
        Ok(())
    }
}

// 指标收集器
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

// 错误类型
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

// 辅助类型
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

这个任务管理系统实现了：

1. **完整的任务生命周期管理**：从发布到完成的全流程控制
2. **严格的验证和资格检查**：确保所有参与者符合要求
3. **防作弊集成**：实时检测和处理可疑行为
4. **自动化调度**：基于时间的事件触发和处理
5. **通知系统**：多渠道实时通知机制
6. **指标收集**：完整的性能和统计数据收集
7. **争议处理**：公平的争议解决机制
8. **错误处理**：详细的错误分类和处理

接下来我将继续完善矿工管理系统和API接口设计。

<function_calls>
<invoke name="TodoWrite">
<parameter name="todos">[{"content": "\u8bbe\u8ba1API\u63a5\u53e3\u548cRPC\u8c03\u7528", "status": "pending", "activeForm": "\u8bbe\u8ba1API\u63a5\u53e3"}, {"content": "\u5236\u5b9a\u6d4b\u8bd5\u548c\u90e8\u7f72\u7b56\u7565", "status": "pending", "activeForm": "\u5236\u5b9a\u6d4b\u8bd5\u7b56\u7565"}, {"content": "\u521b\u5efa\u5b8c\u6574\u7684\u5b58\u50a8\u5c42\u5b9e\u73b0", "status": "completed", "activeForm": "\u521b\u5efa\u5b58\u50a8\u5c42\u5b9e\u73b0"}, {"content": "\u8bbe\u8ba1\u4efb\u52a1\u7ba1\u7406\u5668\u6838\u5fc3\u903b\u8f91", "status": "completed", "activeForm": "\u8bbe\u8ba1\u4efb\u52a1\u7ba1\u7406\u5668"}, {"content": "\u5b9e\u73b0\u77ff\u5de5\u6ce8\u518c\u548c\u7ba1\u7406\u7cfb\u7edf", "status": "in_progress", "activeForm": "\u5b9e\u73b0\u77ff\u5de5\u7ba1\u7406\u7cfb\u7edf"}, {"content": "\u521b\u5efa\u7f51\u7edc\u901a\u4fe1\u548c\u540c\u6b65\u673a\u5236", "status": "pending", "activeForm": "\u521b\u5efa\u7f51\u7edc\u901a\u4fe1\u673a\u5236"}]