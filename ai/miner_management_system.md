# AI矿工管理系统

## 矿工注册和管理核心架构

### 1. 矿工注册管理器 (daemon/src/ai/miner_registry.rs)

```rust
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};
use std::collections::{HashMap, BTreeSet, VecDeque};
use chrono::{DateTime, Utc, Duration};
use crate::{
    ai::{types::*, state::*, storage::*},
    crypto::{Hash, CompressedPublicKey, Signature},
    blockchain::BlockchainInterface,
};

pub struct MinerRegistry {
    storage: Arc<RwLock<dyn AIStorageProvider>>,
    blockchain_interface: Arc<dyn BlockchainInterface>,
    registration_validator: RegistrationValidator,
    reputation_manager: ReputationManager,
    certification_manager: CertificationManager,
    performance_tracker: PerformanceTracker,
    specialization_indexer: SpecializationIndexer,
    activity_monitor: ActivityMonitor,
    onboarding_system: OnboardingSystem,
}

impl MinerRegistry {
    pub fn new(
        storage: Arc<RwLock<dyn AIStorageProvider>>,
        blockchain_interface: Arc<dyn BlockchainInterface>,
    ) -> Self {
        Self {
            storage,
            blockchain_interface,
            registration_validator: RegistrationValidator::new(),
            reputation_manager: ReputationManager::new(),
            certification_manager: CertificationManager::new(),
            performance_tracker: PerformanceTracker::new(),
            specialization_indexer: SpecializationIndexer::new(),
            activity_monitor: ActivityMonitor::new(),
            onboarding_system: OnboardingSystem::new(),
        }
    }

    pub async fn register_miner(
        &mut self,
        registration_data: RegisterMinerPayload,
        signature: Signature,
        block_height: u64,
    ) -> Result<MinerRegistrationResult, MinerError> {
        // 验证注册数据的有效性
        self.registration_validator.validate_registration(&registration_data, &signature).await?;

        // 检查是否已经注册
        let existing_miner = {
            let storage = self.storage.read().await;
            storage.get_miner_state(&registration_data.miner_address).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?
        };

        if existing_miner.is_some() {
            return Err(MinerError::AlreadyRegistered(registration_data.miner_address.clone()));
        }

        // 验证初始质押
        self.validate_initial_stake(&registration_data).await?;

        // 验证专业化声明
        self.validate_specialization_claims(&registration_data.specializations).await?;

        // 验证认证证明（如果提供）
        if let Some(ref cert_proof) = registration_data.certification_proof {
            self.certification_manager.validate_certification_proof(
                &registration_data.miner_address,
                cert_proof,
            ).await?;
        }

        // 创建初始矿工状态
        let miner_state = self.create_initial_miner_state(registration_data, block_height);

        // 存储矿工状态
        {
            let mut storage = self.storage.write().await;
            storage.store_miner_state(&miner_state.address, &miner_state).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?;
        }

        // 更新专业化索引
        self.specialization_indexer.index_miner_specializations(
            &miner_state.address,
            &miner_state.specializations,
        ).await?;

        // 启动新手引导流程
        let onboarding_plan = self.onboarding_system.create_onboarding_plan(&miner_state).await?;

        // 开始活动监控
        self.activity_monitor.start_monitoring(&miner_state.address).await?;

        // 创建初始化交易（质押等）
        let init_transactions = self.create_initialization_transactions(&miner_state).await?;

        Ok(MinerRegistrationResult {
            miner_id: miner_state.address.clone(),
            registration_status: RegistrationStatus::Active,
            initial_reputation: miner_state.reputation.current_score,
            specialization_matches: self.calculate_specialization_market_demand(&miner_state.specializations).await?,
            onboarding_plan,
            recommended_tasks: self.get_recommended_initial_tasks(&miner_state).await?,
            initialization_transactions: init_transactions,
        })
    }

    pub async fn update_miner_specializations(
        &mut self,
        miner_address: &CompressedPublicKey,
        new_specializations: Vec<TaskType>,
        certification_proofs: Vec<CertificationProof>,
    ) -> Result<(), MinerError> {
        // 获取当前矿工状态
        let mut miner_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(miner_address).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?
                .ok_or(MinerError::MinerNotFound(miner_address.clone()))?
        };

        // 验证新的专业化声明
        self.validate_specialization_claims(&new_specializations).await?;

        // 验证认证证明
        for proof in &certification_proofs {
            self.certification_manager.validate_certification_proof(miner_address, proof).await?;
        }

        // 检查专业化变更限制
        self.validate_specialization_changes(&miner_state, &new_specializations).await?;

        // 更新专业化
        let old_specializations = miner_state.specializations.clone();
        miner_state.specializations = new_specializations.clone();

        // 处理认证证明
        for proof in certification_proofs {
            let certification = self.certification_manager.process_certification_proof(proof).await?;
            miner_state.certifications.push(certification);
        }

        // 更新索引
        self.specialization_indexer.update_miner_specializations(
            miner_address,
            &old_specializations,
            &new_specializations,
        ).await?;

        // 存储更新后的状态
        {
            let mut storage = self.storage.write().await;
            storage.store_miner_state(miner_address, &miner_state).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?;
        }

        // 重新计算推荐任务
        self.update_miner_task_recommendations(miner_address, &miner_state).await?;

        Ok(())
    }

    pub async fn update_miner_performance(
        &mut self,
        miner_address: &CompressedPublicKey,
        performance_update: PerformanceUpdate,
    ) -> Result<ReputationChange, MinerError> {
        let mut miner_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(miner_address).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?
                .ok_or(MinerError::MinerNotFound(miner_address.clone()))?
        };

        // 计算声誉变化
        let reputation_change = self.reputation_manager.calculate_reputation_change(
            &miner_state.reputation,
            &performance_update,
        ).await?;

        // 应用声誉变化
        miner_state.reputation = self.reputation_manager.apply_reputation_change(
            miner_state.reputation,
            &reputation_change,
        )?;

        // 更新性能统计
        self.performance_tracker.update_performance_stats(
            &mut miner_state.performance_stats,
            &performance_update,
        ).await?;

        // 更新活动历史
        miner_state.activity_history.recent_activity.push_back(ActivityRecord {
            activity_type: performance_update.activity_type.clone(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            task_id: performance_update.task_id,
            duration: performance_update.duration,
        });

        // 限制活动历史大小
        if miner_state.activity_history.recent_activity.len() > 1000 {
            miner_state.activity_history.recent_activity.pop_front();
        }

        // 检查等级提升
        let level_change = self.check_level_advancement(&miner_state).await?;

        // 存储更新后的状态
        {
            let mut storage = self.storage.write().await;
            storage.store_miner_state(miner_address, &miner_state).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?;
        }

        // 如果有等级变化，发送通知
        if let Some(new_level) = level_change {
            self.handle_level_advancement(miner_address, &miner_state, new_level).await?;
        }

        Ok(reputation_change)
    }

    pub async fn get_miner_profile(&self, miner_address: &CompressedPublicKey) -> Result<MinerProfile, MinerError> {
        let miner_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(miner_address).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?
                .ok_or(MinerError::MinerNotFound(miner_address.clone()))?
        };

        // 计算当前排名
        let current_ranking = self.calculate_miner_ranking(miner_address).await?;

        // 获取最近任务历史
        let recent_tasks = self.get_recent_task_history(miner_address, 10).await?;

        // 计算专业化匹配度
        let specialization_strengths = self.calculate_specialization_strengths(&miner_state).await?;

        // 获取推荐任务
        let recommended_tasks = self.get_recommended_tasks_for_miner(&miner_state).await?;

        // 计算收益统计
        let earnings_stats = self.calculate_earnings_statistics(&miner_state).await?;

        Ok(MinerProfile {
            address: miner_address.clone(),
            reputation: miner_state.reputation.clone(),
            certification_level: miner_state.certification_level.clone(),
            specializations: miner_state.specializations.clone(),
            performance_stats: miner_state.performance_stats.clone(),
            current_ranking,
            recent_tasks,
            specialization_strengths,
            recommended_tasks,
            earnings_stats,
            registration_date: miner_state.registration_block,
            last_activity: miner_state.last_activity,
            status: self.calculate_miner_status(&miner_state),
        })
    }

    pub async fn find_miners_for_task(
        &self,
        task_type: &TaskType,
        difficulty: &DifficultyLevel,
        required_count: u32,
    ) -> Result<Vec<MinerMatch>, MinerError> {
        // 根据专业化查找候选矿工
        let candidate_miners = {
            let storage = self.storage.read().await;
            storage.list_miners_by_specialization(task_type).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?
        };

        let mut miner_matches = Vec::new();

        for miner_address in candidate_miners {
            let miner_state = {
                let storage = self.storage.read().await;
                storage.get_miner_state(&miner_address).await
                    .map_err(|e| MinerError::StorageError(e.to_string()))?
            };

            if let Some(miner) = miner_state {
                // 计算匹配度
                let match_score = self.calculate_task_match_score(&miner, task_type, difficulty).await?;

                if match_score > 0.3 { // 最低匹配阈值
                    let estimated_completion_time = self.estimate_completion_time(&miner, task_type, difficulty);
                    let availability = self.check_miner_availability(&miner).await?;

                    miner_matches.push(MinerMatch {
                        miner_address: miner_address.clone(),
                        match_score,
                        reputation_score: miner.reputation.overall_score,
                        specialization_strength: self.get_specialization_strength(&miner, task_type),
                        estimated_completion_time,
                        availability,
                        recent_success_rate: self.calculate_recent_success_rate(&miner),
                        stake_capacity: self.calculate_stake_capacity(&miner),
                    });
                }
            }
        }

        // 按匹配度排序
        miner_matches.sort_by(|a, b| {
            b.match_score.partial_cmp(&a.match_score).unwrap_or(std::cmp::Ordering::Equal)
        });

        // 返回前N个最佳匹配
        Ok(miner_matches.into_iter().take(required_count as usize).collect())
    }

    pub async fn handle_miner_inactivity(&mut self, miner_address: &CompressedPublicKey) -> Result<(), MinerError> {
        let mut miner_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(miner_address).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?
                .ok_or(MinerError::MinerNotFound(miner_address.clone()))?
        };

        let current_time = chrono::Utc::now().timestamp() as u64;
        let inactivity_duration = current_time - miner_state.last_activity;

        match inactivity_duration {
            // 7天不活跃：警告
            duration if duration > 7 * 24 * 3600 && duration <= 30 * 24 * 3600 => {
                self.send_inactivity_warning(miner_address, InactivityLevel::Warning).await?;
            },
            // 30天不活跃：暂停推荐
            duration if duration > 30 * 24 * 3600 && duration <= 90 * 24 * 3600 => {
                miner_state.status = MinerStatus::Inactive;
                self.send_inactivity_warning(miner_address, InactivityLevel::Suspended).await?;
            },
            // 90天不活跃：标记为休眠
            duration if duration > 90 * 24 * 3600 => {
                miner_state.status = MinerStatus::Dormant;
                self.handle_dormant_miner(miner_address, &miner_state).await?;
            },
            _ => {} // 活跃矿工，无需处理
        }

        // 应用声誉衰减
        if inactivity_duration > 7 * 24 * 3600 {
            let decay_factor = self.calculate_reputation_decay_factor(inactivity_duration);
            miner_state.reputation.overall_score =
                (miner_state.reputation.overall_score as f64 * decay_factor) as u32;
        }

        // 更新状态
        {
            let mut storage = self.storage.write().await;
            storage.store_miner_state(miner_address, &miner_state).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?;
        }

        Ok(())
    }

    pub async fn get_miner_leaderboard(
        &self,
        category: LeaderboardCategory,
        time_period: TimePeriod,
        limit: u32,
    ) -> Result<Vec<MinerLeaderboardEntry>, MinerError> {
        let storage = self.storage.read().await;
        let mut entries = Vec::new();

        // 根据类别获取排序数据
        match category {
            LeaderboardCategory::Reputation => {
                let top_miners = storage.get_top_miners_by_reputation(limit).await
                    .map_err(|e| MinerError::StorageError(e.to_string()))?;

                for (address, reputation) in top_miners {
                    if let Some(miner) = storage.get_miner_state(&address).await
                        .map_err(|e| MinerError::StorageError(e.to_string()))? {
                        entries.push(MinerLeaderboardEntry {
                            rank: entries.len() as u32 + 1,
                            miner_address: address,
                            score: reputation as f64,
                            change_from_previous: self.calculate_rank_change(&address, category, time_period).await?,
                            display_name: miner.display_name.unwrap_or_else(|| format!("Miner-{}", &address.to_string()[..8])),
                            certification_level: miner.certification_level,
                        });
                    }
                }
            },
            LeaderboardCategory::TasksCompleted => {
                // 实现按完成任务数排序的逻辑
                entries = self.get_leaderboard_by_tasks_completed(time_period, limit).await?;
            },
            LeaderboardCategory::QualityScore => {
                // 实现按质量分数排序的逻辑
                entries = self.get_leaderboard_by_quality_score(time_period, limit).await?;
            },
            LeaderboardCategory::Earnings => {
                // 实现按收益排序的逻辑
                entries = self.get_leaderboard_by_earnings(time_period, limit).await?;
            },
        }

        Ok(entries)
    }
}

// 内部实现方法
impl MinerRegistry {
    fn create_initial_miner_state(
        &self,
        registration_data: RegisterMinerPayload,
        block_height: u64,
    ) -> MinerState {
        let current_time = chrono::Utc::now().timestamp() as u64;

        MinerState {
            address: registration_data.miner_address.clone(),
            registration_data: registration_data.clone(),
            reputation: ReputationState {
                current_score: 1000, // 初始声誉分数
                historical_scores: VecDeque::new(),
                domain_reputations: HashMap::new(),
                penalties: Vec::new(),
                achievements: Vec::new(),
                peer_ratings: PeerRatings {
                    ratings_received: Vec::new(),
                    ratings_given: Vec::new(),
                    average_received: 0.0,
                    rating_consistency: 0.0,
                },
            },
            financial_state: MinerFinancialState {
                total_stake: registration_data.initial_stake,
                available_stake: registration_data.initial_stake,
                locked_stake: 0,
                total_earnings: 0,
                total_penalties: 0,
                unclaimed_rewards: 0,
                transaction_history: VecDeque::new(),
            },
            activity_history: ActivityHistory {
                tasks_participated: Vec::new(),
                tasks_completed: Vec::new(),
                validations_performed: Vec::new(),
                recent_activity: VecDeque::new(),
                activity_patterns: ActivityPatterns {
                    preferred_task_types: registration_data.specializations.clone(),
                    working_hours: WorkingHoursPattern {
                        timezone: "UTC".to_string(),
                        active_hours: (0..24).collect(),
                        active_days: (0..7).collect(),
                        peak_performance_hours: vec![9, 10, 11, 14, 15, 16], // 默认工作时间
                    },
                    collaboration_style: CollaborationStyle::Independent,
                    response_time_average: 3600, // 1小时默认响应时间
                },
            },
            certifications: Vec::new(),
            preferences: MinerPreferences {
                task_type_preferences: registration_data.specializations.iter()
                    .map(|t| (t.clone(), 1.0))
                    .collect(),
                difficulty_preferences: HashMap::from([
                    (DifficultyLevel::Beginner, 0.8),
                    (DifficultyLevel::Intermediate, 1.0),
                    (DifficultyLevel::Advanced, 0.6),
                    (DifficultyLevel::Expert, 0.3),
                ]),
                reward_threshold: 10, // 最低接受奖励
                max_concurrent_tasks: 3,
                notification_settings: NotificationSettings::default(),
            },
            performance_analytics: PerformanceAnalytics {
                success_rate_trend: Vec::new(),
                quality_score_trend: Vec::new(),
                earnings_trend: Vec::new(),
                specialization_performance: HashMap::new(),
                peer_comparison_metrics: PeerComparisonMetrics::default(),
            },
            specializations: registration_data.specializations,
            certification_level: CertificationLevel::Unverified,
            last_activity: current_time,
            status: MinerStatus::Active,
            display_name: registration_data.contact_info
                .and_then(|info| info.get("display_name"))
                .map(|name| name.to_string()),
        }
    }

    async fn calculate_task_match_score(
        &self,
        miner: &MinerState,
        task_type: &TaskType,
        difficulty: &DifficultyLevel,
    ) -> Result<f64, MinerError> {
        let mut score = 0.0;

        // 专业化匹配 (40%)
        let specialization_score = if miner.specializations.iter().any(|s| self.task_types_match(s, task_type)) {
            let domain_reputation = miner.reputation.domain_reputations.get(task_type)
                .map(|dr| dr.proficiency)
                .unwrap_or(0.5);
            domain_reputation
        } else {
            0.1 // 非专业领域，很低的匹配度
        };
        score += specialization_score * 0.4;

        // 难度匹配 (20%)
        let difficulty_preference = miner.preferences.difficulty_preferences.get(difficulty)
            .copied()
            .unwrap_or(0.5);
        score += difficulty_preference * 0.2;

        // 声誉分数 (20%)
        let reputation_score = (miner.reputation.current_score as f64 / 10000.0).min(1.0);
        score += reputation_score * 0.2;

        // 成功率 (10%)
        let success_rate = miner.reputation.success_rate;
        score += success_rate * 0.1;

        // 可用性 (10%)
        let availability_score = if miner.status == MinerStatus::Active &&
                                   miner.activity_history.tasks_participated.len() < miner.preferences.max_concurrent_tasks {
            1.0
        } else {
            0.3
        };
        score += availability_score * 0.1;

        Ok(score.min(1.0))
    }

    async fn estimate_completion_time(
        &self,
        miner: &MinerState,
        task_type: &TaskType,
        difficulty: &DifficultyLevel,
    ) -> u64 {
        // 基础完成时间（基于难度）
        let base_time = match difficulty {
            DifficultyLevel::Beginner => 3600,      // 1小时
            DifficultyLevel::Intermediate => 7200,   // 2小时
            DifficultyLevel::Advanced => 14400,     // 4小时
            DifficultyLevel::Expert => 28800,       // 8小时
        };

        // 专业化加成
        let specialization_multiplier = if miner.specializations.iter().any(|s| self.task_types_match(s, task_type)) {
            0.7 // 专业领域快30%
        } else {
            1.5 // 非专业领域慢50%
        };

        // 经验加成
        let experience_multiplier = match miner.certification_level {
            CertificationLevel::Master => 0.6,
            CertificationLevel::Expert => 0.7,
            CertificationLevel::Professional => 0.8,
            CertificationLevel::Basic => 0.9,
            CertificationLevel::Unverified => 1.0,
        };

        // 最近表现调整
        let performance_multiplier = if miner.reputation.success_rate > 0.9 {
            0.8 // 高成功率的矿工通常更快
        } else if miner.reputation.success_rate < 0.7 {
            1.2 // 低成功率的矿工可能需要更多时间
        } else {
            1.0
        };

        ((base_time as f64) * specialization_multiplier * experience_multiplier * performance_multiplier) as u64
    }

    fn task_types_match(&self, miner_spec: &TaskType, task_type: &TaskType) -> bool {
        match (miner_spec, task_type) {
            (TaskType::CodeAnalysis { language: lang1, .. }, TaskType::CodeAnalysis { language: lang2, .. }) => {
                lang1 == lang2 || matches!(lang1, ProgrammingLanguage::Other(_)) // 通用编程技能
            },
            (TaskType::SecurityAudit { scope: scope1, .. }, TaskType::SecurityAudit { scope: scope2, .. }) => {
                scope1 == scope2
            },
            (TaskType::DataAnalysis { data_type: dt1, .. }, TaskType::DataAnalysis { data_type: dt2, .. }) => {
                dt1 == dt2
            },
            (TaskType::AlgorithmOptimization { domain: dom1, .. }, TaskType::AlgorithmOptimization { domain: dom2, .. }) => {
                dom1 == dom2
            },
            (TaskType::LogicReasoning { .. }, TaskType::LogicReasoning { .. }) => true,
            (TaskType::GeneralTask { category: cat1, .. }, TaskType::GeneralTask { category: cat2, .. }) => {
                cat1 == cat2
            },
            _ => false,
        }
    }
}

// 注册验证器
pub struct RegistrationValidator {
    min_stake_amounts: HashMap<TaskType, u64>,
    blacklisted_addresses: BTreeSet<CompressedPublicKey>,
    verification_requirements: VerificationRequirements,
}

impl RegistrationValidator {
    pub fn new() -> Self {
        Self {
            min_stake_amounts: Self::default_stake_amounts(),
            blacklisted_addresses: BTreeSet::new(),
            verification_requirements: VerificationRequirements::default(),
        }
    }

    pub async fn validate_registration(
        &self,
        registration_data: &RegisterMinerPayload,
        signature: &Signature,
    ) -> Result<(), MinerError> {
        // 验证签名
        if !self.verify_signature(registration_data, signature)? {
            return Err(MinerError::InvalidSignature);
        }

        // 检查黑名单
        if self.blacklisted_addresses.contains(&registration_data.miner_address) {
            return Err(MinerError::AddressBlacklisted(registration_data.miner_address.clone()));
        }

        // 验证质押金额
        for specialization in &registration_data.specializations {
            let min_stake = self.min_stake_amounts.get(specialization)
                .copied()
                .unwrap_or(100); // 默认最低质押

            if registration_data.initial_stake < min_stake {
                return Err(MinerError::InsufficientInitialStake {
                    required: min_stake,
                    provided: registration_data.initial_stake,
                });
            }
        }

        // 验证专业化数量限制
        if registration_data.specializations.len() > self.verification_requirements.max_specializations {
            return Err(MinerError::TooManySpecializations {
                max: self.verification_requirements.max_specializations,
                provided: registration_data.specializations.len(),
            });
        }

        // 验证联系信息格式（如果提供）
        if let Some(ref contact_info) = registration_data.contact_info {
            self.validate_contact_info(contact_info)?;
        }

        Ok(())
    }

    fn default_stake_amounts() -> HashMap<TaskType, u64> {
        HashMap::from([
            (TaskType::CodeAnalysis {
                language: ProgrammingLanguage::Rust,
                complexity: ComplexityLevel::Simple
            }, 50),
            (TaskType::SecurityAudit {
                scope: AuditScope::SmartContract,
                standards: vec![]
            }, 200),
            (TaskType::DataAnalysis {
                data_type: DataType::Structured,
                analysis_type: AnalysisType::Descriptive
            }, 100),
        ])
    }
}

// 声誉管理器
pub struct ReputationManager {
    reputation_algorithms: Vec<Box<dyn ReputationAlgorithm>>,
    reputation_weights: ReputationWeights,
    reputation_decay_config: ReputationDecayConfig,
}

pub trait ReputationAlgorithm: Send + Sync {
    fn calculate_reputation_delta(
        &self,
        current_reputation: &ReputationState,
        performance_update: &PerformanceUpdate,
    ) -> Result<i32, ReputationError>;

    fn get_algorithm_name(&self) -> &str;
    fn get_weight(&self) -> f64;
}

impl ReputationManager {
    pub fn new() -> Self {
        Self {
            reputation_algorithms: vec![
                Box::new(TaskCompletionAlgorithm::new()),
                Box::new(QualityScoreAlgorithm::new()),
                Box::new(ValidationAccuracyAlgorithm::new()),
                Box::new(PeerRatingAlgorithm::new()),
                Box::new(ConsistencyAlgorithm::new()),
            ],
            reputation_weights: ReputationWeights::default(),
            reputation_decay_config: ReputationDecayConfig::default(),
        }
    }

    pub async fn calculate_reputation_change(
        &self,
        current_reputation: &ReputationState,
        performance_update: &PerformanceUpdate,
    ) -> Result<ReputationChange, MinerError> {
        let mut total_delta = 0i32;
        let mut algorithm_contributions = Vec::new();

        for algorithm in &self.reputation_algorithms {
            let delta = algorithm.calculate_reputation_delta(current_reputation, performance_update)
                .map_err(|e| MinerError::ReputationCalculationError(format!("{:?}", e)))?;

            let weighted_delta = (delta as f64 * algorithm.get_weight()) as i32;
            total_delta += weighted_delta;

            algorithm_contributions.push(AlgorithmContribution {
                algorithm_name: algorithm.get_algorithm_name().to_string(),
                raw_delta: delta,
                weighted_delta,
                weight: algorithm.get_weight(),
            });
        }

        // 应用声誉上限和下限
        let new_score = (current_reputation.current_score as i32 + total_delta)
            .max(0)
            .min(10000) as u32;

        Ok(ReputationChange {
            old_score: current_reputation.current_score,
            new_score,
            delta: total_delta,
            algorithm_contributions,
            change_reason: performance_update.performance_type.clone(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        })
    }
}

// 认证管理器
pub struct CertificationManager {
    certified_authorities: HashMap<String, AuthorityInfo>,
    certification_validators: HashMap<CertificationType, Box<dyn CertificationValidator>>,
    certification_cache: HashMap<Hash, CachedCertification>,
}

pub trait CertificationValidator: Send + Sync {
    async fn validate_certification(&self, proof: &CertificationProof) -> Result<bool, CertificationError>;
    fn get_certification_type(&self) -> CertificationType;
}

// 数据类型定义
#[derive(Debug, Clone)]
pub struct MinerRegistrationResult {
    pub miner_id: CompressedPublicKey,
    pub registration_status: RegistrationStatus,
    pub initial_reputation: u32,
    pub specialization_matches: Vec<SpecializationMarketInfo>,
    pub onboarding_plan: OnboardingPlan,
    pub recommended_tasks: Vec<TaskRecommendation>,
    pub initialization_transactions: Vec<Hash>,
}

#[derive(Debug, Clone)]
pub enum RegistrationStatus {
    Active,
    PendingVerification,
    Suspended,
    Rejected(String),
}

#[derive(Debug, Clone)]
pub struct MinerProfile {
    pub address: CompressedPublicKey,
    pub reputation: ReputationState,
    pub certification_level: CertificationLevel,
    pub specializations: Vec<TaskType>,
    pub performance_stats: PerformanceStats,
    pub current_ranking: Option<u32>,
    pub recent_tasks: Vec<TaskSummary>,
    pub specialization_strengths: HashMap<TaskType, f64>,
    pub recommended_tasks: Vec<TaskRecommendation>,
    pub earnings_stats: EarningsStatistics,
    pub registration_date: u64,
    pub last_activity: u64,
    pub status: MinerStatus,
}

#[derive(Debug, Clone)]
pub struct MinerMatch {
    pub miner_address: CompressedPublicKey,
    pub match_score: f64,
    pub reputation_score: u32,
    pub specialization_strength: f64,
    pub estimated_completion_time: u64,
    pub availability: AvailabilityStatus,
    pub recent_success_rate: f64,
    pub stake_capacity: u64,
}

#[derive(Debug, Clone)]
pub enum AvailabilityStatus {
    Available,
    Busy { available_in: u64 },
    Offline,
}

#[derive(Debug, Clone)]
pub enum LeaderboardCategory {
    Reputation,
    TasksCompleted,
    QualityScore,
    Earnings,
}

#[derive(Debug, Clone)]
pub enum TimePeriod {
    Daily,
    Weekly,
    Monthly,
    AllTime,
}

#[derive(Debug, Clone)]
pub struct MinerLeaderboardEntry {
    pub rank: u32,
    pub miner_address: CompressedPublicKey,
    pub score: f64,
    pub change_from_previous: i32,
    pub display_name: String,
    pub certification_level: CertificationLevel,
}

// 错误类型
#[derive(Debug, Clone)]
pub enum MinerError {
    AlreadyRegistered(CompressedPublicKey),
    MinerNotFound(CompressedPublicKey),
    InvalidSignature,
    AddressBlacklisted(CompressedPublicKey),
    InsufficientInitialStake { required: u64, provided: u64 },
    TooManySpecializations { max: usize, provided: usize },
    InvalidContactInfo(String),
    StorageError(String),
    ReputationCalculationError(String),
    CertificationError(String),
    BlockchainError(String),
}

impl std::fmt::Display for MinerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MinerError::AlreadyRegistered(addr) => write!(f, "Miner already registered: {}", addr),
            MinerError::MinerNotFound(addr) => write!(f, "Miner not found: {}", addr),
            MinerError::InvalidSignature => write!(f, "Invalid signature"),
            MinerError::AddressBlacklisted(addr) => write!(f, "Address blacklisted: {}", addr),
            MinerError::InsufficientInitialStake { required, provided } => {
                write!(f, "Insufficient initial stake: required {}, provided {}", required, provided)
            },
            MinerError::TooManySpecializations { max, provided } => {
                write!(f, "Too many specializations: max {}, provided {}", max, provided)
            },
            MinerError::InvalidContactInfo(msg) => write!(f, "Invalid contact info: {}", msg),
            MinerError::StorageError(msg) => write!(f, "Storage error: {}", msg),
            MinerError::ReputationCalculationError(msg) => write!(f, "Reputation calculation error: {}", msg),
            MinerError::CertificationError(msg) => write!(f, "Certification error: {}", msg),
            MinerError::BlockchainError(msg) => write!(f, "Blockchain error: {}", msg),
        }
    }
}

impl std::error::Error for MinerError {}
```

这个矿工管理系统实现了：

1. **完整的矿工生命周期管理**：注册、专业化更新、性能跟踪、不活跃处理
2. **智能匹配算法**：基于多维度评分的任务-矿工匹配
3. **声誉系统**：多算法综合评估的动态声誉管理
4. **认证体系**：支持多种认证类型和验证机制
5. **性能分析**：详细的表现跟踪和趋势分析
6. **排行榜系统**：多维度的矿工排名和展示
7. **活跃度监控**：自动检测和处理不活跃矿工

接下来我将继续完善API接口和RPC调用设计。

<function_calls>
<invoke name="TodoWrite">
<parameter name="todos">[{"content": "\u8bbe\u8ba1API\u63a5\u53e3\u548cRPC\u8c03\u7528", "status": "in_progress", "activeForm": "\u8bbe\u8ba1API\u63a5\u53e3"}, {"content": "\u5236\u5b9a\u6d4b\u8bd5\u548c\u90e8\u7f72\u7b56\u7565", "status": "pending", "activeForm": "\u5236\u5b9a\u6d4b\u8bd5\u7b56\u7565"}, {"content": "\u521b\u5efa\u5b8c\u6574\u7684\u5b58\u50a8\u5c42\u5b9e\u73b0", "status": "completed", "activeForm": "\u521b\u5efa\u5b58\u50a8\u5c42\u5b9e\u73b0"}, {"content": "\u8bbe\u8ba1\u4efb\u52a1\u7ba1\u7406\u5668\u6838\u5fc3\u903b\u8f91", "status": "completed", "activeForm": "\u8bbe\u8ba1\u4efb\u52a1\u7ba1\u7406\u5668"}, {"content": "\u5b9e\u73b0\u77ff\u5de5\u6ce8\u518c\u548c\u7ba1\u7406\u7cfb\u7edf", "status": "completed", "activeForm": "\u5b9e\u73b0\u77ff\u5de5\u7ba1\u7406\u7cfb\u7edf"}, {"content": "\u521b\u5efa\u7f51\u7edc\u901a\u4fe1\u548c\u540c\u6b65\u673a\u5236", "status": "pending", "activeForm": "\u521b\u5efa\u7f51\u7edc\u901a\u4fe1\u673a\u5236"}]