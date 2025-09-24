# AI Miner Management System

## Miner Registration and Management Core Architecture

### 1. Miner Registration Manager (daemon/src/ai/miner_registry.rs)

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
        // Validate registration data validity
        self.registration_validator.validate_registration(&registration_data, &signature).await?;

        // Check if already registered
        let existing_miner = {
            let storage = self.storage.read().await;
            storage.get_miner_state(&registration_data.miner_address).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?
        };

        if existing_miner.is_some() {
            return Err(MinerError::AlreadyRegistered(registration_data.miner_address.clone()));
        }

        // Validate initial stake
        self.validate_initial_stake(&registration_data).await?;

        // Validate specialization claims
        self.validate_specialization_claims(&registration_data.specializations).await?;

        // Validate certification proof (if provided)
        if let Some(ref cert_proof) = registration_data.certification_proof {
            self.certification_manager.validate_certification_proof(
                &registration_data.miner_address,
                cert_proof,
            ).await?;
        }

        // Create initial miner state
        let miner_state = self.create_initial_miner_state(registration_data, block_height);

        // Store miner state
        {
            let mut storage = self.storage.write().await;
            storage.store_miner_state(&miner_state.address, &miner_state).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?;
        }

        // Update specialization index
        self.specialization_indexer.index_miner_specializations(
            &miner_state.address,
            &miner_state.specializations,
        ).await?;

        // Start onboarding process
        let onboarding_plan = self.onboarding_system.create_onboarding_plan(&miner_state).await?;

        // Begin activity monitoring
        self.activity_monitor.start_monitoring(&miner_state.address).await?;

        // Create initialization transactions (staking, etc.)
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
        // Get current miner state
        let mut miner_state = {
            let storage = self.storage.read().await;
            storage.get_miner_state(miner_address).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?
                .ok_or(MinerError::MinerNotFound(miner_address.clone()))?
        };

        // Validate new specialization claims
        self.validate_specialization_claims(&new_specializations).await?;

        // Validate certification proofs
        for proof in &certification_proofs {
            self.certification_manager.validate_certification_proof(miner_address, proof).await?;
        }

        // Check specialization change restrictions
        self.validate_specialization_changes(&miner_state, &new_specializations).await?;

        // Update specializations
        let old_specializations = miner_state.specializations.clone();
        miner_state.specializations = new_specializations.clone();

        // Process certification proofs
        for proof in certification_proofs {
            let certification = self.certification_manager.process_certification_proof(proof).await?;
            miner_state.certifications.push(certification);
        }

        // Update index
        self.specialization_indexer.update_miner_specializations(
            miner_address,
            &old_specializations,
            &new_specializations,
        ).await?;

        // Store updated state
        {
            let mut storage = self.storage.write().await;
            storage.store_miner_state(miner_address, &miner_state).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?;
        }

        // Recalculate recommended tasks
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

        // Calculate reputation change
        let reputation_change = self.reputation_manager.calculate_reputation_change(
            &miner_state.reputation,
            &performance_update,
        ).await?;

        // Apply reputation change
        miner_state.reputation = self.reputation_manager.apply_reputation_change(
            miner_state.reputation,
            &reputation_change,
        )?;

        // Update performance statistics
        self.performance_tracker.update_performance_stats(
            &mut miner_state.performance_stats,
            &performance_update,
        ).await?;

        // Update activity history
        miner_state.activity_history.recent_activity.push_back(ActivityRecord {
            activity_type: performance_update.activity_type.clone(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            task_id: performance_update.task_id,
            duration: performance_update.duration,
        });

        // Limit activity history size
        if miner_state.activity_history.recent_activity.len() > 1000 {
            miner_state.activity_history.recent_activity.pop_front();
        }

        // Check level advancement
        let level_change = self.check_level_advancement(&miner_state).await?;

        // Store updated state
        {
            let mut storage = self.storage.write().await;
            storage.store_miner_state(miner_address, &miner_state).await
                .map_err(|e| MinerError::StorageError(e.to_string()))?;
        }

        // If level changed, send notification
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

        // Calculate current ranking
        let current_ranking = self.calculate_miner_ranking(miner_address).await?;

        // Get recent task history
        let recent_tasks = self.get_recent_task_history(miner_address, 10).await?;

        // Calculate specialization strength
        let specialization_strengths = self.calculate_specialization_strengths(&miner_state).await?;

        // Get recommended tasks
        let recommended_tasks = self.get_recommended_tasks_for_miner(&miner_state).await?;

        // Calculate earnings statistics
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
        // Find candidate miners based on specialization
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
                // Calculate match score
                let match_score = self.calculate_task_match_score(&miner, task_type, difficulty).await?;

                if match_score > 0.3 { // Minimum match threshold
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

        // Sort by match score
        miner_matches.sort_by(|a, b| {
            b.match_score.partial_cmp(&a.match_score).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Return top N best matches
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
            // 7 days inactive: warning
            duration if duration > 7 * 24 * 3600 && duration <= 30 * 24 * 3600 => {
                self.send_inactivity_warning(miner_address, InactivityLevel::Warning).await?;
            },
            // 30 days inactive: suspend recommendations
            duration if duration > 30 * 24 * 3600 && duration <= 90 * 24 * 3600 => {
                miner_state.status = MinerStatus::Inactive;
                self.send_inactivity_warning(miner_address, InactivityLevel::Suspended).await?;
            },
            // 90 days inactive: mark as dormant
            duration if duration > 90 * 24 * 3600 => {
                miner_state.status = MinerStatus::Dormant;
                self.handle_dormant_miner(miner_address, &miner_state).await?;
            },
            _ => {} // Active miner, no action needed
        }

        // Apply reputation decay
        if inactivity_duration > 7 * 24 * 3600 {
            let decay_factor = self.calculate_reputation_decay_factor(inactivity_duration);
            miner_state.reputation.overall_score =
                (miner_state.reputation.overall_score as f64 * decay_factor) as u32;
        }

        // Update state
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

        // Get sorted data based on category
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
                // Implement sorting logic by completed tasks
                entries = self.get_leaderboard_by_tasks_completed(time_period, limit).await?;
            },
            LeaderboardCategory::QualityScore => {
                // Implement sorting logic by quality score
                entries = self.get_leaderboard_by_quality_score(time_period, limit).await?;
            },
            LeaderboardCategory::Earnings => {
                // Implement sorting logic by earnings
                entries = self.get_leaderboard_by_earnings(time_period, limit).await?;
            },
        }

        Ok(entries)
    }
}

// Internal implementation methods
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
                current_score: 1000, // Initial reputation score
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
                        peak_performance_hours: vec![9, 10, 11, 14, 15, 16], // Default working hours
                    },
                    collaboration_style: CollaborationStyle::Independent,
                    response_time_average: 3600, // 1 hour default response time
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
                reward_threshold: 10, // Minimum acceptable reward
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

        // Specialization match (40%)
        let specialization_score = if miner.specializations.iter().any(|s| self.task_types_match(s, task_type)) {
            let domain_reputation = miner.reputation.domain_reputations.get(task_type)
                .map(|dr| dr.proficiency)
                .unwrap_or(0.5);
            domain_reputation
        } else {
            0.1 // Non-specialized field, very low match
        };
        score += specialization_score * 0.4;

        // Difficulty match (20%)
        let difficulty_preference = miner.preferences.difficulty_preferences.get(difficulty)
            .copied()
            .unwrap_or(0.5);
        score += difficulty_preference * 0.2;

        // Reputation score (20%)
        let reputation_score = (miner.reputation.current_score as f64 / 10000.0).min(1.0);
        score += reputation_score * 0.2;

        // Success rate (10%)
        let success_rate = miner.reputation.success_rate;
        score += success_rate * 0.1;

        // Availability (10%)
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
        // Base completion time (based on difficulty)
        let base_time = match difficulty {
            DifficultyLevel::Beginner => 3600,      // 1 hour
            DifficultyLevel::Intermediate => 7200,   // 2 hours
            DifficultyLevel::Advanced => 14400,     // 4 hours
            DifficultyLevel::Expert => 28800,       // 8 hours
        };

        // Specialization bonus
        let specialization_multiplier = if miner.specializations.iter().any(|s| self.task_types_match(s, task_type)) {
            0.7 // 30% faster in specialized field
        } else {
            1.5 // 50% slower in non-specialized field
        };

        // Experience bonus
        let experience_multiplier = match miner.certification_level {
            CertificationLevel::Master => 0.6,
            CertificationLevel::Expert => 0.7,
            CertificationLevel::Professional => 0.8,
            CertificationLevel::Basic => 0.9,
            CertificationLevel::Unverified => 1.0,
        };

        // Recent performance adjustment
        let performance_multiplier = if miner.reputation.success_rate > 0.9 {
            0.8 // High success rate miners usually faster
        } else if miner.reputation.success_rate < 0.7 {
            1.2 // Low success rate miners may need more time
        } else {
            1.0
        };

        ((base_time as f64) * specialization_multiplier * experience_multiplier * performance_multiplier) as u64
    }

    fn task_types_match(&self, miner_spec: &TaskType, task_type: &TaskType) -> bool {
        match (miner_spec, task_type) {
            (TaskType::CodeAnalysis { language: lang1, .. }, TaskType::CodeAnalysis { language: lang2, .. }) => {
                lang1 == lang2 || matches!(lang1, ProgrammingLanguage::Other(_)) // General programming skills
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

// Registration Validator
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
        // Verify signature
        if !self.verify_signature(registration_data, signature)? {
            return Err(MinerError::InvalidSignature);
        }

        // Check blacklist
        if self.blacklisted_addresses.contains(&registration_data.miner_address) {
            return Err(MinerError::AddressBlacklisted(registration_data.miner_address.clone()));
        }

        // Validate stake amount
        for specialization in &registration_data.specializations {
            let min_stake = self.min_stake_amounts.get(specialization)
                .copied()
                .unwrap_or(100); // Default minimum stake

            if registration_data.initial_stake < min_stake {
                return Err(MinerError::InsufficientInitialStake {
                    required: min_stake,
                    provided: registration_data.initial_stake,
                });
            }
        }

        // Validate specialization count limit
        if registration_data.specializations.len() > self.verification_requirements.max_specializations {
            return Err(MinerError::TooManySpecializations {
                max: self.verification_requirements.max_specializations,
                provided: registration_data.specializations.len(),
            });
        }

        // Validate contact info format (if provided)
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

// Reputation Manager
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

        // Apply reputation upper and lower limits
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

// Certification Manager
pub struct CertificationManager {
    certified_authorities: HashMap<String, AuthorityInfo>,
    certification_validators: HashMap<CertificationType, Box<dyn CertificationValidator>>,
    certification_cache: HashMap<Hash, CachedCertification>,
}

pub trait CertificationValidator: Send + Sync {
    async fn validate_certification(&self, proof: &CertificationProof) -> Result<bool, CertificationError>;
    fn get_certification_type(&self) -> CertificationType;
}

// Data Type Definitions
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

// Error Types
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

This miner management system implements:

1. **Complete Miner Lifecycle Management**: Registration, specialization updates, performance tracking, inactivity handling
2. **Intelligent Matching Algorithm**: Multi-dimensional scoring for task-miner matching
3. **Reputation System**: Dynamic reputation management with multi-algorithm comprehensive assessment
4. **Certification System**: Support for multiple certification types and validation mechanisms
5. **Performance Analysis**: Detailed performance tracking and trend analysis
6. **Leaderboard System**: Multi-dimensional miner ranking and display
7. **Activity Monitoring**: Automatic detection and handling of inactive miners

Next, I will continue to improve the API interface and RPC call design.