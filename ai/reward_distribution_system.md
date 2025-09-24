# AI挖矿奖励分发系统实现

## 奖励分发核心架构

### 1. 奖励分发引擎 (common/src/ai_mining/rewards.rs)

```rust
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, BTreeMap};
use crate::{
    crypto::{Hash, CompressedPublicKey},
    serializer::*,
    transaction::TransactionType,
};
use super::{types::*, state::*, validation::*};

pub struct RewardDistributionEngine {
    economic_params: EconomicParameters,
    reputation_system: ReputationSystem,
    quality_assessor: QualityAssessor,
    performance_tracker: PerformanceTracker,
    dispute_handler: DisputeHandler,
}

impl RewardDistributionEngine {
    pub fn new(economic_params: EconomicParameters) -> Self {
        Self {
            economic_params,
            reputation_system: ReputationSystem::new(),
            quality_assessor: QualityAssessor::new(),
            performance_tracker: PerformanceTracker::new(),
            dispute_handler: DisputeHandler::new(),
        }
    }

    pub async fn calculate_task_rewards(
        &self,
        task: &TaskState,
        validation_results: &[ValidationResult],
        network_context: &NetworkContext,
    ) -> Result<RewardDistribution, RewardError> {
        // 验证任务是否可以分发奖励
        self.validate_reward_eligibility(task)?;

        // 获取所有有效提交
        let valid_submissions = self.get_valid_submissions(task, validation_results);

        if valid_submissions.is_empty() {
            return Ok(RewardDistribution::no_valid_submissions(task.task_id));
        }

        // 计算基础奖励池
        let total_reward_pool = task.reward_pool.total_amount;
        let reward_structure = self.calculate_reward_structure(total_reward_pool, task);

        // 评估每个提交的表现
        let submission_scores = self.evaluate_submissions(
            &valid_submissions,
            validation_results,
            task,
        ).await?;

        // 确定获胜者
        let winners = self.determine_winners(&submission_scores, task);

        // 计算参与者奖励
        let participant_rewards = self.calculate_participant_rewards(
            &submission_scores,
            &winners,
            &reward_structure,
        );

        // 计算验证者奖励
        let validator_rewards = self.calculate_validator_rewards(
            validation_results,
            &reward_structure,
            network_context,
        ).await?;

        // 计算质量奖励和额外奖励
        let quality_bonuses = self.calculate_quality_bonuses(
            &submission_scores,
            &reward_structure,
        );

        let speed_bonuses = self.calculate_speed_bonuses(
            &submission_scores,
            task,
            &reward_structure,
        );

        // 创建最终奖励分发
        let mut distributions = Vec::new();

        // 添加获胜者奖励
        for winner in &winners {
            let base_reward = participant_rewards.get(&winner.submitter).unwrap_or(&0);
            let quality_bonus = quality_bonuses.get(&winner.submitter).unwrap_or(&0);
            let speed_bonus = speed_bonuses.get(&winner.submitter).unwrap_or(&0);

            distributions.push(RewardEntry {
                recipient: winner.submitter.clone(),
                amount: base_reward + quality_bonus + speed_bonus,
                reward_type: RewardType::Winner {
                    rank: winner.rank,
                    quality_score: winner.final_score,
                },
                bonus_breakdown: BonusBreakdown {
                    base_reward: *base_reward,
                    quality_bonus: *quality_bonus,
                    speed_bonus: *speed_bonus,
                    reputation_bonus: 0, // 计算声誉奖励
                },
            });
        }

        // 添加参与者奖励
        for (participant, reward) in participant_rewards {
            if !winners.iter().any(|w| w.submitter == participant) {
                distributions.push(RewardEntry {
                    recipient: participant,
                    amount: reward,
                    reward_type: RewardType::Participation,
                    bonus_breakdown: BonusBreakdown {
                        base_reward: reward,
                        quality_bonus: 0,
                        speed_bonus: 0,
                        reputation_bonus: 0,
                    },
                });
            }
        }

        // 添加验证者奖励
        for (validator, reward) in validator_rewards {
            distributions.push(RewardEntry {
                recipient: validator,
                amount: reward,
                reward_type: RewardType::Validation,
                bonus_breakdown: BonusBreakdown {
                    base_reward: reward,
                    quality_bonus: 0,
                    speed_bonus: 0,
                    reputation_bonus: 0,
                },
            });
        }

        // 计算网络费用
        let network_fee = self.calculate_network_fee(&reward_structure);

        Ok(RewardDistribution {
            task_id: task.task_id,
            total_reward_pool,
            distributions,
            network_fee,
            distribution_block: network_context.current_block_height,
            distribution_timestamp: chrono::Utc::now().timestamp() as u64,
            distribution_hash: Hash::default(), // 需要计算实际哈希
            reward_criteria: self.generate_reward_criteria(task),
        })
    }

    fn calculate_reward_structure(
        &self,
        total_pool: u64,
        task: &TaskState,
    ) -> RewardStructure {
        let task_type = &task.task_data.task_type;
        let difficulty = &task.task_data.difficulty_level;

        // 基于任务类型和难度的分配比例
        let allocation_ratios = match (task_type, difficulty) {
            (TaskType::SecurityAudit { .. }, DifficultyLevel::Expert) => {
                // 高难度安全审计：更多给获胜者和验证者
                AllocationRatios {
                    winner_share: 0.70,
                    participant_share: 0.10,
                    validator_share: 0.15,
                    network_fee: 0.05,
                }
            },
            (TaskType::CodeAnalysis { .. }, DifficultyLevel::Beginner) => {
                // 简单代码分析：更平均分配
                AllocationRatios {
                    winner_share: 0.60,
                    participant_share: 0.20,
                    validator_share: 0.15,
                    network_fee: 0.05,
                }
            },
            _ => {
                // 默认分配
                AllocationRatios {
                    winner_share: 0.65,
                    participant_share: 0.15,
                    validator_share: 0.15,
                    network_fee: 0.05,
                }
            }
        };

        RewardStructure {
            total_pool,
            winner_pool: (total_pool as f64 * allocation_ratios.winner_share) as u64,
            participant_pool: (total_pool as f64 * allocation_ratios.participant_share) as u64,
            validator_pool: (total_pool as f64 * allocation_ratios.validator_share) as u64,
            network_fee_pool: (total_pool as f64 * allocation_ratios.network_fee) as u64,
            quality_bonus_pool: (total_pool as f64 * 0.10) as u64, // 额外10%用于质量奖励
            speed_bonus_pool: (total_pool as f64 * 0.05) as u64,   // 额外5%用于速度奖励
        }
    }

    async fn evaluate_submissions(
        &self,
        submissions: &[&SubmissionState],
        validation_results: &[ValidationResult],
        task: &TaskState,
    ) -> Result<HashMap<Hash, SubmissionScore>, RewardError> {
        let mut scores = HashMap::new();

        for submission in submissions {
            let submission_validations = validation_results.iter()
                .filter(|v| self.validation_applies_to_submission(v, submission))
                .collect::<Vec<_>>();

            if submission_validations.is_empty() {
                continue;
            }

            // 计算综合质量分数
            let quality_score = self.calculate_composite_quality_score(
                &submission_validations,
                task,
            )?;

            // 计算创新性分数
            let innovation_score = self.calculate_innovation_score(
                submission,
                &submission_validations,
            );

            // 计算技术深度分数
            let technical_depth_score = self.calculate_technical_depth_score(
                submission,
                &submission_validations,
                task,
            );

            // 计算实用性分数
            let practicality_score = self.calculate_practicality_score(
                submission,
                task,
            );

            // 计算及时性分数
            let timeliness_score = self.calculate_timeliness_score(
                submission,
                task,
            );

            // 综合评分
            let final_score = self.calculate_final_score(
                quality_score,
                innovation_score,
                technical_depth_score,
                practicality_score,
                timeliness_score,
                task,
            );

            scores.insert(submission.submission_id, SubmissionScore {
                submission_id: submission.submission_id,
                submitter: submission.submitter.clone(),
                quality_score,
                innovation_score,
                technical_depth_score,
                practicality_score,
                timeliness_score,
                final_score,
                validation_consensus: self.calculate_validation_consensus(&submission_validations),
                bonus_eligibility: self.assess_bonus_eligibility(submission, &submission_validations),
            });
        }

        Ok(scores)
    }

    fn calculate_composite_quality_score(
        &self,
        validations: &[&ValidationResult],
        task: &TaskState,
    ) -> Result<u8, RewardError> {
        if validations.is_empty() {
            return Err(RewardError::NoValidations);
        }

        let mut weighted_scores = Vec::new();
        let mut total_weight = 0.0;

        for validation in validations {
            let (score, weight) = match validation {
                ValidationResult::Automatic(auto_result) => {
                    (auto_result.overall_score, self.get_auto_validation_weight(task))
                },
                ValidationResult::PeerReview(peer_result) => {
                    (peer_result.quality_score, self.get_peer_validation_weight(&peer_result.validator))
                },
                ValidationResult::ExpertReview(expert_result) => {
                    (expert_result.overall_score, self.get_expert_validation_weight(&expert_result.expert))
                },
                ValidationResult::Consensus(consensus_result) => {
                    (consensus_result.consensus_score, consensus_result.consensus_confidence)
                },
            };

            weighted_scores.push(score as f64 * weight);
            total_weight += weight;
        }

        if total_weight == 0.0 {
            return Err(RewardError::InvalidWeights);
        }

        let composite_score = weighted_scores.iter().sum::<f64>() / total_weight;
        Ok(composite_score.round() as u8)
    }

    fn get_peer_validation_weight(&self, validator: &CompressedPublicKey) -> f64 {
        // 基于验证者声誉计算权重
        self.reputation_system.get_validator_weight(validator)
    }

    fn get_expert_validation_weight(&self, expert: &CompressedPublicKey) -> f64 {
        // 专家验证权重更高
        self.reputation_system.get_expert_weight(expert)
    }

    fn determine_winners(
        &self,
        submission_scores: &HashMap<Hash, SubmissionScore>,
        task: &TaskState,
    ) -> Vec<Winner> {
        let mut sorted_submissions: Vec<_> = submission_scores.values().collect();
        sorted_submissions.sort_by(|a, b| b.final_score.cmp(&a.final_score));

        let mut winners = Vec::new();
        let max_winners = self.calculate_max_winners(task, sorted_submissions.len());

        for (rank, submission_score) in sorted_submissions.iter().take(max_winners).enumerate() {
            // 只有超过最低质量阈值的才能获胜
            if submission_score.final_score >= task.task_data.quality_threshold {
                winners.push(Winner {
                    rank: rank as u8 + 1,
                    submitter: submission_score.submitter.clone(),
                    submission_id: submission_score.submission_id,
                    final_score: submission_score.final_score,
                    score_breakdown: ScoreBreakdown {
                        quality: submission_score.quality_score,
                        innovation: submission_score.innovation_score,
                        technical_depth: submission_score.technical_depth_score,
                        practicality: submission_score.practicality_score,
                        timeliness: submission_score.timeliness_score,
                    },
                });
            }
        }

        winners
    }

    fn calculate_max_winners(&self, task: &TaskState, total_submissions: usize) -> usize {
        match &task.task_data.task_type {
            TaskType::CodeAnalysis { complexity, .. } => match complexity {
                ComplexityLevel::Simple => (total_submissions / 3).max(1).min(3),
                ComplexityLevel::Medium => (total_submissions / 4).max(1).min(2),
                ComplexityLevel::Complex => (total_submissions / 5).max(1).min(2),
                ComplexityLevel::Enterprise => 1,
            },
            TaskType::SecurityAudit { .. } => 1, // 安全审计通常只有一个最佳答案
            TaskType::AlgorithmOptimization { .. } => (total_submissions / 3).max(1).min(3),
            _ => (total_submissions / 4).max(1).min(2),
        }
    }

    fn calculate_participant_rewards(
        &self,
        submission_scores: &HashMap<Hash, SubmissionScore>,
        winners: &[Winner],
        reward_structure: &RewardStructure,
    ) -> HashMap<CompressedPublicKey, u64> {
        let mut rewards = HashMap::new();
        let winner_addresses: HashSet<_> = winners.iter().map(|w| &w.submitter).collect();

        // 为获胜者分配奖励
        let winner_pool_per_winner = if winners.is_empty() {
            0
        } else {
            reward_structure.winner_pool / winners.len() as u64
        };

        for winner in winners {
            let base_reward = winner_pool_per_winner;
            // 根据排名调整奖励
            let rank_multiplier = match winner.rank {
                1 => 1.0,
                2 => 0.6,
                3 => 0.4,
                _ => 0.2,
            };
            let final_reward = (base_reward as f64 * rank_multiplier) as u64;
            rewards.insert(winner.submitter.clone(), final_reward);
        }

        // 为非获胜参与者分配参与奖励
        let non_winners: Vec<_> = submission_scores.values()
            .filter(|score| !winner_addresses.contains(&score.submitter))
            .collect();

        if !non_winners.is_empty() {
            let participation_reward = reward_structure.participant_pool / non_winners.len() as u64;

            for submission_score in non_winners {
                // 基于质量分数调整参与奖励
                let quality_multiplier = (submission_score.final_score as f64 / 100.0).max(0.1);
                let adjusted_reward = (participation_reward as f64 * quality_multiplier) as u64;
                rewards.insert(submission_score.submitter.clone(), adjusted_reward);
            }
        }

        rewards
    }

    async fn calculate_validator_rewards(
        &self,
        validation_results: &[ValidationResult],
        reward_structure: &RewardStructure,
        network_context: &NetworkContext,
    ) -> Result<HashMap<CompressedPublicKey, u64>, RewardError> {
        let mut validator_contributions = HashMap::new();

        // 收集所有验证者的贡献
        for validation in validation_results {
            match validation {
                ValidationResult::PeerReview(peer_result) => {
                    let contribution = ValidatorContribution {
                        validator: peer_result.validator.clone(),
                        validation_type: ValidationType::PeerReview,
                        quality_score: peer_result.quality_score,
                        confidence: peer_result.confidence,
                        timeliness: self.calculate_validation_timeliness(&peer_result.validation_time),
                        accuracy: self.estimate_validation_accuracy(validation, network_context).await?,
                    };
                    validator_contributions.insert(peer_result.validator.clone(), contribution);
                },
                ValidationResult::ExpertReview(expert_result) => {
                    let contribution = ValidatorContribution {
                        validator: expert_result.expert.clone(),
                        validation_type: ValidationType::ExpertReview,
                        quality_score: expert_result.overall_score,
                        confidence: expert_result.confidence_level,
                        timeliness: self.calculate_validation_timeliness(&expert_result.validation_time),
                        accuracy: self.estimate_validation_accuracy(validation, network_context).await?,
                    };
                    validator_contributions.insert(expert_result.expert.clone(), contribution);
                },
                ValidationResult::Consensus(consensus_result) => {
                    // 为共识验证中的所有参与者分配奖励
                    let per_validator_score = consensus_result.consensus_score;
                    for validator in &consensus_result.participating_validators {
                        let contribution = ValidatorContribution {
                            validator: validator.clone(),
                            validation_type: ValidationType::Consensus,
                            quality_score: per_validator_score,
                            confidence: consensus_result.consensus_confidence,
                            timeliness: 1.0, // 共识验证的及时性
                            accuracy: consensus_result.consensus_confidence,
                        };
                        validator_contributions.insert(validator.clone(), contribution);
                    }
                },
                _ => {} // 自动验证不分配奖励
            }
        }

        // 计算奖励分配
        let mut rewards = HashMap::new();
        let total_contribution_score: f64 = validator_contributions.values()
            .map(|c| self.calculate_contribution_score(c))
            .sum();

        if total_contribution_score > 0.0 {
            for (validator, contribution) in validator_contributions {
                let contribution_score = self.calculate_contribution_score(&contribution);
                let reward_share = contribution_score / total_contribution_score;
                let reward_amount = (reward_structure.validator_pool as f64 * reward_share) as u64;

                // 应用声誉奖励乘数
                let reputation_multiplier = self.reputation_system.get_reputation_multiplier(&validator);
                let final_reward = (reward_amount as f64 * reputation_multiplier) as u64;

                rewards.insert(validator, final_reward);
            }
        }

        Ok(rewards)
    }

    fn calculate_contribution_score(&self, contribution: &ValidatorContribution) -> f64 {
        let base_score = contribution.quality_score as f64 / 100.0;
        let type_weight = match contribution.validation_type {
            ValidationType::ExpertReview => 2.0,
            ValidationType::PeerReview => 1.0,
            ValidationType::Consensus => 0.8,
        };

        base_score * type_weight * contribution.confidence * contribution.timeliness * contribution.accuracy
    }

    fn calculate_quality_bonuses(
        &self,
        submission_scores: &HashMap<Hash, SubmissionScore>,
        reward_structure: &RewardStructure,
    ) -> HashMap<CompressedPublicKey, u64> {
        let mut bonuses = HashMap::new();
        let quality_thresholds = &self.economic_params.quality_bonuses.thresholds;
        let quality_multipliers = &self.economic_params.quality_bonuses.multipliers;

        for submission_score in submission_scores.values() {
            let quality = submission_score.final_score;

            // 找到适用的质量阈值
            for (i, &threshold) in quality_thresholds.iter().enumerate() {
                if quality >= threshold {
                    let bonus_multiplier = quality_multipliers.get(i).unwrap_or(&1.0);
                    let bonus_amount = (reward_structure.quality_bonus_pool as f64 *
                                      (bonus_multiplier - 1.0) / quality_thresholds.len() as f64) as u64;

                    bonuses.insert(submission_score.submitter.clone(), bonus_amount);
                    break;
                }
            }

            // 特殊奖励：满分提交
            if quality == 100 {
                let exceptional_bonus = (reward_structure.quality_bonus_pool as f64 *
                                        self.economic_params.quality_bonuses.exceptional_bonus) as u64;
                *bonuses.entry(submission_score.submitter.clone()).or_insert(0) += exceptional_bonus;
            }
        }

        bonuses
    }

    fn calculate_speed_bonuses(
        &self,
        submission_scores: &HashMap<Hash, SubmissionScore>,
        task: &TaskState,
        reward_structure: &RewardStructure,
    ) -> HashMap<CompressedPublicKey, u64> {
        let mut bonuses = HashMap::new();
        let task_duration = task.lifecycle.submission_deadline - task.lifecycle.published_at;

        for submission_score in submission_scores.values() {
            let completion_ratio = submission_score.timeliness_score as f64 / 100.0;

            // 早期完成奖励
            if completion_ratio >= 0.8 {  // 前20%时间完成
                let speed_bonus_ratio = self.economic_params.speed_bonuses.early_completion_bonus;
                let bonus_amount = (reward_structure.speed_bonus_pool as f64 * speed_bonus_ratio) as u64;
                bonuses.insert(submission_score.submitter.clone(), bonus_amount);
            }
        }

        bonuses
    }

    fn calculate_network_fee(&self, reward_structure: &RewardStructure) -> u64 {
        reward_structure.network_fee_pool
    }

    async fn estimate_validation_accuracy(
        &self,
        validation: &ValidationResult,
        network_context: &NetworkContext,
    ) -> Result<f64, RewardError> {
        // 基于历史数据估算验证准确性
        // 这里需要实现复杂的准确性评估算法
        // 简化版本返回基于验证者声誉的估算

        match validation {
            ValidationResult::PeerReview(peer_result) => {
                Ok(self.reputation_system.get_validation_accuracy(&peer_result.validator))
            },
            ValidationResult::ExpertReview(expert_result) => {
                Ok(self.reputation_system.get_expert_accuracy(&expert_result.expert))
            },
            ValidationResult::Consensus(consensus_result) => {
                Ok(consensus_result.consensus_confidence)
            },
            _ => Ok(1.0),
        }
    }
}

// 声誉系统
pub struct ReputationSystem {
    reputation_cache: HashMap<CompressedPublicKey, CachedReputation>,
    decay_rate: f64,
    update_frequency: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CachedReputation {
    pub overall_score: u32,
    pub validation_accuracy: f64,
    pub domain_expertise: HashMap<TaskType, f64>,
    pub last_updated: u64,
}

impl ReputationSystem {
    pub fn new() -> Self {
        Self {
            reputation_cache: HashMap::new(),
            decay_rate: 0.01, // 每天1%的声誉衰减
            update_frequency: 86400, // 每天更新一次
        }
    }

    pub fn get_validator_weight(&self, validator: &CompressedPublicKey) -> f64 {
        self.reputation_cache.get(validator)
            .map(|rep| {
                let age_factor = self.calculate_age_factor(rep.last_updated);
                (rep.overall_score as f64 / 10000.0) * age_factor
            })
            .unwrap_or(0.5) // 新验证者默认权重
    }

    pub fn get_expert_weight(&self, expert: &CompressedPublicKey) -> f64 {
        self.get_validator_weight(expert) * 1.5 // 专家权重更高
    }

    pub fn get_reputation_multiplier(&self, address: &CompressedPublicKey) -> f64 {
        self.reputation_cache.get(address)
            .map(|rep| {
                match rep.overall_score {
                    9000..=10000 => 1.3,  // 顶级声誉
                    7000..=8999 => 1.2,   // 高声誉
                    5000..=6999 => 1.1,   // 中等声誉
                    3000..=4999 => 1.0,   // 基础声誉
                    _ => 0.9,             // 低声誉
                }
            })
            .unwrap_or(1.0)
    }

    pub fn get_validation_accuracy(&self, validator: &CompressedPublicKey) -> f64 {
        self.reputation_cache.get(validator)
            .map(|rep| rep.validation_accuracy)
            .unwrap_or(0.7) // 新验证者默认准确率
    }

    pub fn get_expert_accuracy(&self, expert: &CompressedPublicKey) -> f64 {
        self.get_validation_accuracy(expert).max(0.85) // 专家准确率更高
    }

    fn calculate_age_factor(&self, last_updated: u64) -> f64 {
        let current_time = chrono::Utc::now().timestamp() as u64;
        let age_days = (current_time - last_updated) / 86400;

        // 声誉随时间衰减
        (1.0 - self.decay_rate).powi(age_days as i32).max(0.5)
    }

    pub async fn update_reputation(
        &mut self,
        address: &CompressedPublicKey,
        performance: &PerformanceUpdate,
    ) -> Result<(), ReputationError> {
        let current_reputation = self.reputation_cache.get(address).cloned()
            .unwrap_or_else(|| CachedReputation::new());

        let updated_reputation = self.calculate_updated_reputation(
            &current_reputation,
            performance,
        )?;

        self.reputation_cache.insert(address.clone(), updated_reputation);
        Ok(())
    }

    fn calculate_updated_reputation(
        &self,
        current: &CachedReputation,
        update: &PerformanceUpdate,
    ) -> Result<CachedReputation, ReputationError> {
        let mut new_reputation = current.clone();

        // 更新总体分数
        match update.performance_type {
            PerformanceType::TaskCompletion { quality_score, innovation_score } => {
                let score_delta = self.calculate_task_completion_delta(quality_score, innovation_score);
                new_reputation.overall_score = (new_reputation.overall_score as i32 + score_delta)
                    .max(0)
                    .min(10000) as u32;
            },
            PerformanceType::ValidationAccuracy { accuracy, consensus_agreement } => {
                let accuracy_delta = self.calculate_validation_accuracy_delta(accuracy, consensus_agreement);
                new_reputation.validation_accuracy = (new_reputation.validation_accuracy + accuracy_delta)
                    .max(0.0)
                    .min(1.0);
            },
            PerformanceType::Penalty { violation_type, severity } => {
                let penalty = self.calculate_penalty_impact(&violation_type, &severity);
                new_reputation.overall_score = (new_reputation.overall_score as i32 - penalty)
                    .max(0) as u32;
            },
        }

        new_reputation.last_updated = chrono::Utc::now().timestamp() as u64;
        Ok(new_reputation)
    }

    fn calculate_task_completion_delta(&self, quality_score: u8, innovation_score: u8) -> i32 {
        let base_delta = match quality_score {
            90..=100 => 50,
            80..=89 => 30,
            70..=79 => 20,
            60..=69 => 10,
            50..=59 => 5,
            _ => -10,
        };

        let innovation_bonus = match innovation_score {
            90..=100 => 20,
            80..=89 => 10,
            70..=79 => 5,
            _ => 0,
        };

        base_delta + innovation_bonus
    }
}

// 争议处理系统
pub struct DisputeHandler {
    dispute_queue: VecDeque<DisputeCase>,
    arbitration_panel: ArbitrationPanel,
    dispute_resolution_time: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DisputeCase {
    pub dispute_id: Hash,
    pub task_id: Hash,
    pub disputed_submission: Hash,
    pub disputer: CompressedPublicKey,
    pub dispute_type: DisputeType,
    pub evidence: Vec<DisputeEvidence>,
    pub status: DisputeStatus,
    pub created_at: u64,
    pub resolution_deadline: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DisputeType {
    ValidationAccuracy,      // 验证准确性争议
    RewardDistribution,      // 奖励分配争议
    Plagiarism,             // 抄袭争议
    QualityAssessment,      // 质量评估争议
    TechnicalError,         // 技术错误争议
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DisputeEvidence {
    pub evidence_type: EvidenceType,
    pub content_hash: Hash,
    pub submitter: CompressedPublicKey,
    pub timestamp: u64,
    pub verification_proof: Option<Hash>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum EvidenceType {
    OriginalWork,           // 原创作品证明
    PriorArt,              // 现有技术证明
    ExpertOpinion,         // 专家意见
    TechnicalAnalysis,     // 技术分析
    CommunityFeedback,     // 社区反馈
}

impl DisputeHandler {
    pub fn new() -> Self {
        Self {
            dispute_queue: VecDeque::new(),
            arbitration_panel: ArbitrationPanel::new(),
            dispute_resolution_time: 7 * 24 * 3600, // 7天解决期限
        }
    }

    pub async fn handle_dispute(
        &mut self,
        dispute: DisputeCase,
    ) -> Result<DisputeResolution, DisputeError> {
        // 验证争议的有效性
        self.validate_dispute(&dispute)?;

        // 分配给仲裁委员会
        let arbitrators = self.arbitration_panel.assign_arbitrators(&dispute).await?;

        // 收集额外证据
        let additional_evidence = self.collect_additional_evidence(&dispute).await?;

        // 进行仲裁
        let arbitration_result = self.arbitration_panel.arbitrate(
            &dispute,
            &additional_evidence,
            &arbitrators,
        ).await?;

        // 生成解决方案
        let resolution = DisputeResolution {
            dispute_id: dispute.dispute_id,
            resolution_type: arbitration_result.resolution_type,
            arbitrators,
            evidence_considered: additional_evidence,
            resolution_reasoning: arbitration_result.reasoning,
            remedy_actions: arbitration_result.remedy_actions,
            resolved_at: chrono::Utc::now().timestamp() as u64,
        };

        Ok(resolution)
    }

    fn validate_dispute(&self, dispute: &DisputeCase) -> Result<(), DisputeError> {
        // 检查争议是否在时限内
        let current_time = chrono::Utc::now().timestamp() as u64;
        if current_time > dispute.resolution_deadline {
            return Err(DisputeError::DisputeExpired);
        }

        // 检查证据完整性
        if dispute.evidence.is_empty() {
            return Err(DisputeError::InsufficientEvidence);
        }

        // 检查争议者资格
        // 实现资格检查逻辑

        Ok(())
    }
}

// 数据类型定义
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RewardDistribution {
    pub task_id: Hash,
    pub total_reward_pool: u64,
    pub distributions: Vec<RewardEntry>,
    pub network_fee: u64,
    pub distribution_block: u64,
    pub distribution_timestamp: u64,
    pub distribution_hash: Hash,
    pub reward_criteria: RewardCriteria,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RewardEntry {
    pub recipient: CompressedPublicKey,
    pub amount: u64,
    pub reward_type: RewardType,
    pub bonus_breakdown: BonusBreakdown,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RewardType {
    Winner { rank: u8, quality_score: u8 },
    Participation,
    Validation,
    QualityBonus,
    SpeedBonus,
    InnovationBonus,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BonusBreakdown {
    pub base_reward: u64,
    pub quality_bonus: u64,
    pub speed_bonus: u64,
    pub reputation_bonus: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RewardStructure {
    pub total_pool: u64,
    pub winner_pool: u64,
    pub participant_pool: u64,
    pub validator_pool: u64,
    pub network_fee_pool: u64,
    pub quality_bonus_pool: u64,
    pub speed_bonus_pool: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AllocationRatios {
    pub winner_share: f64,
    pub participant_share: f64,
    pub validator_share: f64,
    pub network_fee: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubmissionScore {
    pub submission_id: Hash,
    pub submitter: CompressedPublicKey,
    pub quality_score: u8,
    pub innovation_score: u8,
    pub technical_depth_score: u8,
    pub practicality_score: u8,
    pub timeliness_score: u8,
    pub final_score: u8,
    pub validation_consensus: f64,
    pub bonus_eligibility: BonusEligibility,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Winner {
    pub rank: u8,
    pub submitter: CompressedPublicKey,
    pub submission_id: Hash,
    pub final_score: u8,
    pub score_breakdown: ScoreBreakdown,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScoreBreakdown {
    pub quality: u8,
    pub innovation: u8,
    pub technical_depth: u8,
    pub practicality: u8,
    pub timeliness: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ValidatorContribution {
    pub validator: CompressedPublicKey,
    pub validation_type: ValidationType,
    pub quality_score: u8,
    pub confidence: f64,
    pub timeliness: f64,
    pub accuracy: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ValidationType {
    PeerReview,
    ExpertReview,
    Consensus,
}

// 错误类型
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RewardError {
    TaskNotEligible,
    NoValidations,
    InvalidWeights,
    CalculationError,
    InsufficientFunds,
    DisputePending,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ReputationError {
    InvalidUpdate,
    CalculationError,
    DataInconsistency,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DisputeError {
    DisputeExpired,
    InsufficientEvidence,
    UnauthorizedDisputer,
    ArbitrationFailed,
}

// 实现Serializer trait
impl Serializer for RewardDistribution {
    fn write(&self, writer: &mut Writer) {
        self.task_id.write(writer);
        writer.write_u64(self.total_reward_pool);

        writer.write_u32(self.distributions.len() as u32);
        for entry in &self.distributions {
            entry.write(writer);
        }

        writer.write_u64(self.network_fee);
        writer.write_u64(self.distribution_block);
        writer.write_u64(self.distribution_timestamp);
        self.distribution_hash.write(writer);
        self.reward_criteria.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let task_id = Hash::read(reader)?;
        let total_reward_pool = reader.read_u64()?;

        let distributions_len = reader.read_u32()?;
        let mut distributions = Vec::with_capacity(distributions_len as usize);
        for _ in 0..distributions_len {
            distributions.push(RewardEntry::read(reader)?);
        }

        let network_fee = reader.read_u64()?;
        let distribution_block = reader.read_u64()?;
        let distribution_timestamp = reader.read_u64()?;
        let distribution_hash = Hash::read(reader)?;
        let reward_criteria = RewardCriteria::read(reader)?;

        Ok(RewardDistribution {
            task_id,
            total_reward_pool,
            distributions,
            network_fee,
            distribution_block,
            distribution_timestamp,
            distribution_hash,
            reward_criteria,
        })
    }

    fn size(&self) -> usize {
        self.task_id.size()
        + 8 // total_reward_pool
        + 4 // distributions.len()
        + self.distributions.iter().map(|d| d.size()).sum::<usize>()
        + 8 // network_fee
        + 8 // distribution_block
        + 8 // distribution_timestamp
        + self.distribution_hash.size()
        + self.reward_criteria.size()
    }
}

// 为其他类型实现Serializer trait...
```

这个奖励分发系统实现了：

1. **多维度评估**：质量、创新性、技术深度、实用性、及时性
2. **公平分配算法**：基于贡献度的动态奖励分配
3. **声誉系统集成**：声誉影响奖励权重和乘数
4. **质量和速度奖励**：额外奖励高质量和快速完成
5. **验证者激励**：基于验证准确性和及时性的奖励
6. **争议处理机制**：完整的争议解决流程
7. **动态参数调整**：根据任务类型和难度调整分配策略
8. **透明性和可追溯性**：详细的奖励记录和分发证明

这个系统确保了AI挖矿生态的公平性和可持续性，通过合理的激励机制促进高质量参与。

<function_calls>
<invoke name="TodoWrite">
<parameter name="todos">[{"content": "\u521b\u5efaai/Design.md\u6587\u4ef6\u5e76\u5199\u5165\u5b8c\u6574\u5b9e\u65bd\u65b9\u6848", "status": "completed", "activeForm": "\u521b\u5efaai/Design.md\u6587\u4ef6"}, {"content": "\u6df1\u5316AI\u6316\u77ff\u6280\u672f\u5b9e\u73b0\u7ec6\u8282", "status": "completed", "activeForm": "\u6df1\u5316\u6280\u672f\u5b9e\u73b0\u7ec6\u8282"}, {"content": "\u8bbe\u8ba1\u5177\u4f53\u7684Rust\u4ee3\u7801\u5b9e\u73b0", "status": "completed", "activeForm": "\u8bbe\u8ba1Rust\u4ee3\u7801\u5b9e\u73b0"}, {"content": "\u5b8c\u5584\u72b6\u6001\u7ba1\u7406\u548c\u5b58\u50a8\u65b9\u6848", "status": "in_progress", "activeForm": "\u5b8c\u5584\u72b6\u6001\u7ba1\u7406\u65b9\u6848"}, {"content": "\u8bbe\u8ba1API\u63a5\u53e3\u548cRPC\u8c03\u7528", "status": "pending", "activeForm": "\u8bbe\u8ba1API\u63a5\u53e3"}, {"content": "\u5236\u5b9a\u6d4b\u8bd5\u548c\u90e8\u7f72\u7b56\u7565", "status": "pending", "activeForm": "\u5236\u5b9a\u6d4b\u8bd5\u7b56\u7565"}, {"content": "\u521b\u5efa\u9a8c\u8bc1\u7cfb\u7edf\u6838\u5fc3\u5b9e\u73b0", "status": "completed", "activeForm": "\u521b\u5efa\u9a8c\u8bc1\u7cfb\u7edf\u5b9e\u73b0"}, {"content": "\u5b9e\u73b0\u9632\u4f5c\u5f0a\u68c0\u6d4b\u7b97\u6cd5", "status": "completed", "activeForm": "\u5b9e\u73b0\u9632\u4f5c\u5f0a\u7b97\u6cd5"}, {"content": "\u8bbe\u8ba1\u5956\u52b1\u5206\u53d1\u673a\u5236", "status": "completed", "activeForm": "\u8bbe\u8ba1\u5956\u52b1\u5206\u53d1\u673a\u5236"}]