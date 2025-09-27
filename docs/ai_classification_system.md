# AI Classification and Certification System Complete Implementation

## Overview

This document provides a detailed description of the complete implementation of AI classification and certification in the TOS AI mining system, including level definitions, evaluation algorithms, promotion/demotion mechanisms, and applications in the validation system.

## 1. AI Level System Definition

### Level Classification Standards

```rust
// common/src/ai_mining/classification.rs

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, PartialOrd)]
pub enum AIExpertLevel {
    Standard = 0,        // Standard AI - General capabilities
    Advanced = 1,        // Advanced AI - Domain specialization
    Specialist = 2,      // Specialist AI - Specific task optimization
    Master = 3,          // Master AI - Top-tier capabilities
}

impl AIExpertLevel {
    pub fn from_score(score: f64) -> Self {
        match score {
            0.95..=1.0 => AIExpertLevel::Master,
            0.85..=0.94 => AIExpertLevel::Specialist,
            0.75..=0.84 => AIExpertLevel::Advanced,
            _ => AIExpertLevel::Standard,
        }
    }

    pub fn get_description(&self) -> &'static str {
        match self {
            AIExpertLevel::Standard => "Standard AI: Possesses basic reasoning capabilities, suitable for simple to medium complexity tasks",
            AIExpertLevel::Advanced => "Advanced AI: Specialized training in specific domains, capable of handling complex professional tasks",
            AIExpertLevel::Specialist => "Specialist AI: Highly optimized professional AI, excelling in specific domains",
            AIExpertLevel::Master => "Master AI: Top-tier AI system with multimodal capabilities and complex reasoning",
        }
    }

    pub fn get_capabilities(&self) -> Vec<AICapability> {
        match self {
            AIExpertLevel::Standard => vec![
                AICapability::BasicReasoning,
                AICapability::SimpleCodeAnalysis,
                AICapability::DataValidation,
            ],
            AIExpertLevel::Advanced => vec![
                AICapability::BasicReasoning,
                AICapability::SimpleCodeAnalysis,
                AICapability::DataValidation,
                AICapability::DomainSpecialization,
                AICapability::ComplexProblemSolving,
            ],
            AIExpertLevel::Specialist => vec![
                AICapability::BasicReasoning,
                AICapability::SimpleCodeAnalysis,
                AICapability::DataValidation,
                AICapability::DomainSpecialization,
                AICapability::ComplexProblemSolving,
                AICapability::InnovativeSolutions,
                AICapability::CrossDomainAnalysis,
            ],
            AIExpertLevel::Master => vec![
                AICapability::BasicReasoning,
                AICapability::SimpleCodeAnalysis,
                AICapability::DataValidation,
                AICapability::DomainSpecialization,
                AICapability::ComplexProblemSolving,
                AICapability::InnovativeSolutions,
                AICapability::CrossDomainAnalysis,
                AICapability::MultiModalProcessing,
                AICapability::MetaCognition,
                AICapability::SystemicThinking,
            ],
        }
    }

    pub fn min_stake_multiplier(&self) -> f64 {
        match self {
            AIExpertLevel::Standard => 1.5,      // Requires more staking
            AIExpertLevel::Advanced => 1.2,      // Slight discount
            AIExpertLevel::Specialist => 0.8,    // 20% discount
            AIExpertLevel::Master => 0.5,        // 50% discount
        }
    }

    pub fn validation_weight(&self) -> f64 {
        match self {
            AIExpertLevel::Standard => 1.0,      // Base weight
            AIExpertLevel::Advanced => 1.5,      // 1.5x weight
            AIExpertLevel::Specialist => 2.0,    // 2x weight
            AIExpertLevel::Master => 3.0,        // 3x weight
        }
    }

    pub fn reward_multiplier(&self) -> f64 {
        match self {
            AIExpertLevel::Standard => 1.0,      // Base reward
            AIExpertLevel::Advanced => 1.2,      // 20% increase
            AIExpertLevel::Specialist => 1.5,    // 50% increase
            AIExpertLevel::Master => 2.0,        // 100% increase
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum AICapability {
    BasicReasoning,            // Basic reasoning
    SimpleCodeAnalysis,        // Simple code analysis
    DataValidation,            // Data validation
    DomainSpecialization,      // Domain specialization
    ComplexProblemSolving,     // Complex problem solving
    InnovativeSolutions,       // Innovative solutions
    CrossDomainAnalysis,       // Cross-domain analysis
    MultiModalProcessing,      // Multimodal processing
    MetaCognition,             // Meta-cognition
    SystemicThinking,          // Systemic thinking
}
```

### Professional Domain Certification

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DomainCertification {
    pub domain: TaskType,
    pub certification_level: DomainExpertiseLevel,
    pub proficiency_score: f64,              // Proficiency score (0-1)
    pub certification_date: u64,             // Certification date
    pub expiry_date: Option<u64>,            // Expiry date
    pub certification_authority: CertificationAuthority,
    pub performance_evidence: Vec<PerformanceEvidence>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DomainExpertiseLevel {
    Novice,          // Novice (0-0.3)
    Intermediate,    // Intermediate (0.3-0.6)
    Advanced,        // Advanced (0.6-0.8)
    Expert,          // Expert (0.8-0.95)
    Master,          // Master (0.95-1.0)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum CertificationAuthority {
    SystemAutomatic,             // System automatic certification
    PeerReview,                 // Peer review certification
    CommunityValidation,        // Community validation certification
    ExternalAudit,              // External audit certification
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PerformanceEvidence {
    pub task_id: Hash,
    pub task_type: TaskType,
    pub quality_score: u8,
    pub completion_time: u64,
    pub innovation_score: Option<f64>,
    pub peer_ratings: Vec<f64>,
    pub validator_confirmations: Vec<Hash>,
}
```

## 2. AI Performance Evaluation System

### Core Evaluator Implementation

```rust
pub struct AIClassificationEngine {
    pub performance_analyzer: PerformanceAnalyzer,
    pub trend_detector: TrendDetector,
    pub behavior_monitor: BehaviorMonitor,
    pub peer_evaluation_system: PeerEvaluationSystem,
    pub milestone_tracker: MilestoneTracker,
    pub risk_assessor: RiskAssessor,
    pub improvement_advisor: ImprovementAdvisor,
}

impl AIClassificationEngine {
    pub async fn evaluate_ai_comprehensive(
        &self,
        ai_id: &str,
        evaluation_request: &EvaluationRequest,
    ) -> Result<ClassificationResult, ClassificationError> {

        // 1. Load AI historical data
        let ai_history = self.load_ai_performance_history(ai_id).await?;

        // 2. Performance analysis
        let performance_analysis = self.performance_analyzer
            .analyze(&ai_history, &evaluation_request.time_window).await?;

        // 3. Trend detection
        let trend_analysis = self.trend_detector
            .detect_trends(&ai_history.quality_timeline).await?;

        // 4. Behavior monitoring
        let behavior_analysis = self.behavior_monitor
            .analyze_patterns(&ai_history.behavior_data).await?;

        // 5. Peer evaluation analysis
        let peer_analysis = self.peer_evaluation_system
            .aggregate_peer_ratings(&ai_history.peer_ratings).await?;

        // 6. Milestone check
        let milestone_status = self.milestone_tracker
            .check_achievements(&ai_history.achievements).await?;

        // 7. Risk assessment
        let risk_profile = self.risk_assessor
            .assess_risks(&ai_history, &behavior_analysis).await?;

        // 8. Comprehensive score calculation
        let comprehensive_score = self.calculate_comprehensive_score(
            &performance_analysis,
            &trend_analysis,
            &behavior_analysis,
            &peer_analysis,
            &milestone_status,
        );

        // 9. Level determination
        let recommended_level = self.determine_classification(
            &comprehensive_score,
            &risk_profile,
            &ai_history.current_level,
        );

        // 10. Improvement recommendation generation
        let improvement_plan = self.improvement_advisor
            .generate_plan(&comprehensive_score, &risk_profile).await?;

        Ok(ClassificationResult {
            ai_id: ai_id.to_string(),
            evaluation_timestamp: chrono::Utc::now().timestamp() as u64,
            current_level: ai_history.current_level,
            recommended_level,
            comprehensive_score,
            performance_analysis,
            trend_analysis,
            behavior_analysis,
            peer_analysis,
            milestone_status,
            risk_profile,
            improvement_plan,
            confidence_level: self.calculate_confidence(&comprehensive_score),
            next_evaluation_date: self.schedule_next_evaluation(&recommended_level, &trend_analysis),
        })
    }

    fn calculate_comprehensive_score(
        &self,
        performance: &PerformanceAnalysis,
        trends: &TrendAnalysis,
        behavior: &BehaviorAnalysis,
        peer: &PeerAnalysis,
        milestones: &MilestoneStatus,
    ) -> ComprehensiveScore {

        let base_weights = ScoreWeights {
            accuracy: 0.25,         // 25% - Accuracy
            consistency: 0.20,      // 20% - Consistency
            innovation: 0.15,       // 15% - Innovation
            efficiency: 0.15,       // 15% - Efficiency
            peer_recognition: 0.10, // 10% - Peer recognition
            trend_factor: 0.10,     // 10% - Trend factor
            milestones: 0.05,       // 5% - Milestones
        };

        let raw_score =
            performance.accuracy_score * base_weights.accuracy +
            performance.consistency_score * base_weights.consistency +
            performance.innovation_score * base_weights.innovation +
            performance.efficiency_score * base_weights.efficiency +
            peer.average_rating * base_weights.peer_recognition +
            trends.impact_factor * base_weights.trend_factor +
            milestones.achievement_factor * base_weights.milestones;

        let adjusted_score = self.apply_behavioral_adjustments(raw_score, behavior);
        let final_score = self.apply_domain_specialization_bonus(adjusted_score, performance);

        ComprehensiveScore {
            raw_score,
            adjusted_score,
            final_score,
            contributing_factors: self.identify_strengths(performance, trends, peer),
            limiting_factors: self.identify_weaknesses(performance, behavior),
            score_breakdown: ScoreBreakdown {
                accuracy_contribution: performance.accuracy_score * base_weights.accuracy,
                consistency_contribution: performance.consistency_score * base_weights.consistency,
                innovation_contribution: performance.innovation_score * base_weights.innovation,
                efficiency_contribution: performance.efficiency_score * base_weights.efficiency,
                peer_contribution: peer.average_rating * base_weights.peer_recognition,
                trend_contribution: trends.impact_factor * base_weights.trend_factor,
                milestone_contribution: milestones.achievement_factor * base_weights.milestones,
            },
        }
    }

    fn apply_behavioral_adjustments(&self, base_score: f64, behavior: &BehaviorAnalysis) -> f64 {
        let mut adjusted_score = base_score;

        // Stability adjustment
        if behavior.stability_index > 0.9 {
            adjusted_score *= 1.05; // 5% stability bonus
        } else if behavior.stability_index < 0.5 {
            adjusted_score *= 0.95; // 5% instability penalty
        }

        // Anomalous behavior penalty
        if behavior.anomaly_count > 0 {
            let penalty = (behavior.anomaly_count as f64 * 0.02).min(0.1); // Maximum 10% penalty
            adjusted_score *= (1.0 - penalty);
        }

        // Response time consistency
        if behavior.response_time_variance < 0.1 {
            adjusted_score *= 1.02; // 2% consistency bonus
        }

        adjusted_score
    }

    fn apply_domain_specialization_bonus(&self, base_score: f64, performance: &PerformanceAnalysis) -> f64 {
        let mut final_score = base_score;

        // Domain specialization bonus
        for (domain, skill_level) in &performance.domain_skills {
            if skill_level.proficiency > 0.9 {
                final_score += 0.02; // +2% per high-level domain
            }
        }

        // Multi-domain capability bonus
        let high_proficiency_domains = performance.domain_skills.values()
            .filter(|skill| skill.proficiency > 0.8)
            .count();

        if high_proficiency_domains > 3 {
            final_score += 0.05; // +5% for multi-domain capability
        }

        final_score.min(1.0)
    }

    fn determine_classification(
        &self,
        score: &ComprehensiveScore,
        risk: &RiskProfile,
        current_level: AIExpertLevel,
    ) -> AIExpertLevel {

        // Initial classification based on score
        let score_based_level = AIExpertLevel::from_score(score.final_score);

        // Risk adjustment
        let risk_adjusted_level = if risk.overall_risk > 0.5 {
            // High risk, no promotion allowed
            if score_based_level > current_level {
                current_level
            } else {
                score_based_level
            }
        } else {
            score_based_level
        };

        // Stability requirements check
        if self.meets_stability_requirements(&risk_adjusted_level, score, risk) {
            risk_adjusted_level
        } else {
            // Does not meet stability requirements, maintain current level or demote
            self.get_stable_level(current_level, score)
        }
    }

    fn meets_stability_requirements(
        &self,
        target_level: &AIExpertLevel,
        score: &ComprehensiveScore,
        risk: &RiskProfile,
    ) -> bool {
        let requirements = match target_level {
            AIExpertLevel::Standard => StabilityRequirements {
                min_consistency: 0.6,
                max_risk: 0.7,
                min_performance_duration: 7, // 7 days
            },
            AIExpertLevel::Advanced => StabilityRequirements {
                min_consistency: 0.75,
                max_risk: 0.5,
                min_performance_duration: 14, // 14 days
            },
            AIExpertLevel::Specialist => StabilityRequirements {
                min_consistency: 0.85,
                max_risk: 0.3,
                min_performance_duration: 30, // 30 days
            },
            AIExpertLevel::Master => StabilityRequirements {
                min_consistency: 0.95,
                max_risk: 0.1,
                min_performance_duration: 60, // 60 days
            },
        };

        score.contributing_factors.contains(&ContributingFactor::ConsistentPerformance) &&
        risk.overall_risk <= requirements.max_risk &&
        self.check_performance_duration(target_level, requirements.min_performance_duration)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClassificationResult {
    pub ai_id: String,
    pub evaluation_timestamp: u64,
    pub current_level: AIExpertLevel,
    pub recommended_level: AIExpertLevel,
    pub comprehensive_score: ComprehensiveScore,
    pub performance_analysis: PerformanceAnalysis,
    pub trend_analysis: TrendAnalysis,
    pub behavior_analysis: BehaviorAnalysis,
    pub peer_analysis: PeerAnalysis,
    pub milestone_status: MilestoneStatus,
    pub risk_profile: RiskProfile,
    pub improvement_plan: ImprovementPlan,
    pub confidence_level: f64,
    pub next_evaluation_date: u64,
}
```

## 3. Promotion and Demotion Mechanism Implementation

### Automatic Promotion System

```rust
pub struct AutoPromotionSystem {
    pub promotion_rules: HashMap<AIExpertLevel, PromotionCriteria>,
    pub probation_tracker: ProbationTracker,
    pub performance_validator: PerformanceValidator,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromotionCriteria {
    pub min_score: f64,
    pub min_tasks_completed: u64,
    pub min_success_rate: f64,
    pub required_domains: Vec<TaskType>,
    pub peer_endorsements: u32,
    pub probation_period_days: u32,
    pub stability_requirements: StabilityRequirements,
}

impl AutoPromotionSystem {
    pub fn new() -> Self {
        let mut promotion_rules = HashMap::new();

        // Standard AI -> Advanced AI
        promotion_rules.insert(AIExpertLevel::Advanced, PromotionCriteria {
            min_score: 0.75,
            min_tasks_completed: 50,
            min_success_rate: 0.80,
            required_domains: vec![],
            peer_endorsements: 3,
            probation_period_days: 14,
            stability_requirements: StabilityRequirements {
                min_consistency: 0.75,
                max_risk: 0.5,
                min_performance_duration: 14,
            },
        });

        // Advanced AI -> Specialist AI
        promotion_rules.insert(AIExpertLevel::Specialist, PromotionCriteria {
            min_score: 0.85,
            min_tasks_completed: 100,
            min_success_rate: 0.90,
            required_domains: vec![], // At least proficient in one domain
            peer_endorsements: 5,
            probation_period_days: 30,
            stability_requirements: StabilityRequirements {
                min_consistency: 0.85,
                max_risk: 0.3,
                min_performance_duration: 30,
            },
        });

        // Specialist AI -> Master AI
        promotion_rules.insert(AIExpertLevel::Master, PromotionCriteria {
            min_score: 0.95,
            min_tasks_completed: 200,
            min_success_rate: 0.95,
            required_domains: vec![], // Requires multi-domain capability
            peer_endorsements: 10,
            probation_period_days: 60,
            stability_requirements: StabilityRequirements {
                min_consistency: 0.95,
                max_risk: 0.1,
                min_performance_duration: 60,
            },
        });

        Self {
            promotion_rules,
            probation_tracker: ProbationTracker::new(),
            performance_validator: PerformanceValidator::new(),
        }
    }

    pub async fn evaluate_promotion(
        &self,
        ai_id: &str,
        current_level: AIExpertLevel,
        classification_result: &ClassificationResult,
    ) -> PromotionDecision {

        let target_level = self.get_next_level(&current_level);
        if target_level.is_none() {
            return PromotionDecision::NotEligible("Already at highest level".to_string());
        }

        let target_level = target_level.unwrap();
        let criteria = &self.promotion_rules[&target_level];

        // Check basic conditions
        if !self.meets_basic_criteria(classification_result, criteria) {
            return PromotionDecision::NotEligible(
                self.generate_missing_criteria_message(classification_result, criteria)
            );
        }

        // Check probation status
        if self.probation_tracker.is_on_probation(ai_id) {
            return PromotionDecision::OnProbation(
                self.probation_tracker.get_remaining_time(ai_id)
            );
        }

        // Start probation period
        if !self.probation_tracker.has_completed_probation(ai_id, &target_level) {
            self.probation_tracker.start_probation(ai_id, target_level.clone(), criteria.probation_period_days);
            return PromotionDecision::ProbationStarted(target_level, criteria.probation_period_days);
        }

        // Validate probation performance
        let probation_performance = self.performance_validator
            .validate_probation_performance(ai_id, &target_level, criteria).await;

        match probation_performance {
            Ok(_) => PromotionDecision::Approved(target_level),
            Err(issues) => PromotionDecision::ProbationFailed(issues),
        }
    }

    fn meets_basic_criteria(
        &self,
        result: &ClassificationResult,
        criteria: &PromotionCriteria,
    ) -> bool {
        result.comprehensive_score.final_score >= criteria.min_score &&
        result.performance_analysis.tasks_completed >= criteria.min_tasks_completed &&
        result.performance_analysis.success_rate >= criteria.min_success_rate &&
        result.peer_analysis.endorsement_count >= criteria.peer_endorsements
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum PromotionDecision {
    NotEligible(String),
    OnProbation(u32), // Remaining days
    ProbationStarted(AIExpertLevel, u32),
    Approved(AIExpertLevel),
    ProbationFailed(Vec<String>),
}
```

### Automatic Demotion System

```rust
pub struct AutoDemotionSystem {
    pub demotion_triggers: HashMap<AIExpertLevel, DemotionTriggers>,
    pub grace_period_tracker: GracePeriodTracker,
    pub performance_monitor: PerformanceMonitor,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DemotionTriggers {
    pub consecutive_failures: u32,
    pub success_rate_threshold: f64,
    pub risk_score_threshold: f64,
    pub grace_period_days: u32,
    pub appeal_allowed: bool,
}

impl AutoDemotionSystem {
    pub fn new() -> Self {
        let mut demotion_triggers = HashMap::new();

        demotion_triggers.insert(AIExpertLevel::Master, DemotionTriggers {
            consecutive_failures: 3,
            success_rate_threshold: 0.90,
            risk_score_threshold: 0.2,
            grace_period_days: 7,
            appeal_allowed: true,
        });

        demotion_triggers.insert(AIExpertLevel::Specialist, DemotionTriggers {
            consecutive_failures: 5,
            success_rate_threshold: 0.85,
            risk_score_threshold: 0.4,
            grace_period_days: 14,
            appeal_allowed: true,
        });

        demotion_triggers.insert(AIExpertLevel::Advanced, DemotionTriggers {
            consecutive_failures: 7,
            success_rate_threshold: 0.75,
            risk_score_threshold: 0.6,
            grace_period_days: 21,
            appeal_allowed: true,
        });

        Self {
            demotion_triggers,
            grace_period_tracker: GracePeriodTracker::new(),
            performance_monitor: PerformanceMonitor::new(),
        }
    }

    pub async fn evaluate_demotion(
        &self,
        ai_id: &str,
        current_level: AIExpertLevel,
        classification_result: &ClassificationResult,
    ) -> DemotionDecision {

        if current_level == AIExpertLevel::Standard {
            return DemotionDecision::NotApplicable; // Already at lowest level
        }

        let triggers = &self.demotion_triggers[&current_level];
        let performance = &classification_result.performance_analysis;
        let risk = &classification_result.risk_profile;

        let mut demotion_reasons = Vec::new();

        // Check consecutive failures
        if performance.consecutive_failures >= triggers.consecutive_failures {
            demotion_reasons.push(format!(
                "Consecutive failures {} times, exceeding threshold {}",
                performance.consecutive_failures,
                triggers.consecutive_failures
            ));
        }

        // Check success rate
        if performance.success_rate < triggers.success_rate_threshold {
            demotion_reasons.push(format!(
                "Success rate {:.2}% below requirement {:.2}%",
                performance.success_rate * 100.0,
                triggers.success_rate_threshold * 100.0
            ));
        }

        // Check risk score
        if risk.overall_risk > triggers.risk_score_threshold {
            demotion_reasons.push(format!(
                "Risk score {:.2} exceeds threshold {:.2}",
                risk.overall_risk,
                triggers.risk_score_threshold
            ));
        }

        if demotion_reasons.is_empty() {
            return DemotionDecision::NotTriggered;
        }

        // Check grace period
        if !self.grace_period_tracker.is_in_grace_period(ai_id, &current_level) {
            self.grace_period_tracker.start_grace_period(
                ai_id,
                current_level.clone(),
                triggers.grace_period_days
            );
            return DemotionDecision::GracePeriodStarted(
                triggers.grace_period_days,
                demotion_reasons
            );
        }

        // Check improvement during grace period
        let improvement = self.performance_monitor
            .check_improvement_during_grace_period(ai_id, &current_level).await;

        if improvement.is_sufficient() {
            return DemotionDecision::ImprovedDuringGrace;
        }

        // Execute demotion
        let target_level = self.get_lower_level(&current_level);
        DemotionDecision::Approved(target_level, demotion_reasons)
    }

    fn get_lower_level(&self, current: &AIExpertLevel) -> AIExpertLevel {
        match current {
            AIExpertLevel::Master => AIExpertLevel::Specialist,
            AIExpertLevel::Specialist => AIExpertLevel::Advanced,
            AIExpertLevel::Advanced => AIExpertLevel::Standard,
            AIExpertLevel::Standard => AIExpertLevel::Standard, // Cannot demote further
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DemotionDecision {
    NotApplicable,
    NotTriggered,
    GracePeriodStarted(u32, Vec<String>), // Days, reasons
    ImprovedDuringGrace,
    Approved(AIExpertLevel, Vec<String>),
}
```

## 4. Validation System Integration

### Task Allocation Strategy

```rust
pub struct TaskAllocationEngine {
    pub level_requirements: HashMap<TaskType, AIExpertLevel>,
    pub difficulty_mapping: HashMap<DifficultyLevel, Vec<AIExpertLevel>>,
    pub specialization_matcher: SpecializationMatcher,
}

impl TaskAllocationEngine {
    pub fn get_eligible_ais(
        &self,
        task: &TaskState,
        available_ais: &[AIProfile],
    ) -> Vec<EligibleAI> {
        let mut eligible = Vec::new();

        for ai in available_ais {
            let eligibility = self.assess_eligibility(ai, task);
            if eligibility.is_eligible {
                eligible.push(EligibleAI {
                    ai_id: ai.ai_id.clone(),
                    ai_level: ai.current_level.clone(),
                    eligibility_score: eligibility.score,
                    required_stake: self.calculate_required_stake(ai, task),
                    expected_performance: eligibility.expected_performance,
                    specialization_match: eligibility.specialization_match,
                });
            }
        }

        // Sort by suitability
        eligible.sort_by(|a, b| b.eligibility_score.partial_cmp(&a.eligibility_score).unwrap());
        eligible
    }

    fn assess_eligibility(&self, ai: &AIProfile, task: &TaskState) -> EligibilityAssessment {
        let min_level = self.level_requirements.get(&task.task_data.task_type)
            .unwrap_or(&AIExpertLevel::Standard);

        if ai.current_level < *min_level {
            return EligibilityAssessment::not_eligible();
        }

        let specialization_match = self.specialization_matcher
            .calculate_match(&ai.specializations, &task.task_data.task_type);

        let experience_factor = self.calculate_experience_factor(ai, task);
        let reputation_factor = ai.reputation_score / 10000.0; // Normalize to 0-1

        let eligibility_score = (
            ai.current_level.validation_weight() * 0.4 +
            specialization_match * 0.3 +
            experience_factor * 0.2 +
            reputation_factor * 0.1
        );

        EligibilityAssessment {
            is_eligible: true,
            score: eligibility_score,
            expected_performance: self.predict_performance(ai, task),
            specialization_match,
        }
    }

    fn calculate_required_stake(&self, ai: &AIProfile, task: &TaskState) -> u64 {
        let base_stake = task.reward_pool.total_amount / 10; // 10% base stake
        let level_multiplier = ai.current_level.min_stake_multiplier();

        (base_stake as f64 * level_multiplier) as u64
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EligibleAI {
    pub ai_id: String,
    pub ai_level: AIExpertLevel,
    pub eligibility_score: f64,
    pub required_stake: u64,
    pub expected_performance: f64,
    pub specialization_match: f64,
}
```

### Validation Weight System

```rust
pub struct ValidationWeightingSystem {
    pub base_weights: HashMap<AIExpertLevel, f64>,
    pub reputation_adjustment: ReputationAdjustment,
    pub specialization_bonus: SpecializationBonus,
    pub consensus_calculator: ConsensusCalculator,
}

impl ValidationWeightingSystem {
    pub fn calculate_validation_weight(
        &self,
        validator: &AIProfile,
        task: &TaskState,
        validation_context: &ValidationContext,
    ) -> ValidationWeight {

        // Base weight
        let base_weight = self.base_weights[&validator.current_level];

        // Reputation adjustment
        let reputation_adjustment = self.reputation_adjustment
            .calculate_adjustment(validator.reputation_score);

        // Specialization bonus
        let specialization_bonus = self.specialization_bonus
            .calculate_bonus(&validator.specializations, &task.task_data.task_type);

        // Historical accuracy
        let accuracy_factor = validator.validation_accuracy;

        // Final weight
        let final_weight = base_weight *
            (1.0 + reputation_adjustment) *
            (1.0 + specialization_bonus) *
            accuracy_factor;

        ValidationWeight {
            base_weight,
            reputation_adjustment,
            specialization_bonus,
            accuracy_factor,
            final_weight,
            validator_level: validator.current_level.clone(),
        }
    }

    pub fn calculate_consensus(
        &self,
        validations: &[WeightedValidation],
        consensus_threshold: f64,
    ) -> ConsensusResult {

        let total_weight: f64 = validations.iter().map(|v| v.weight.final_weight).sum();
        let mut score_sum = 0.0;
        let mut weighted_scores = Vec::new();

        for validation in validations {
            let weighted_score = validation.score as f64 * validation.weight.final_weight;
            score_sum += weighted_score;
            weighted_scores.push(WeightedScore {
                validator_id: validation.validator_id.clone(),
                validator_level: validation.weight.validator_level.clone(),
                raw_score: validation.score,
                weight: validation.weight.final_weight,
                weighted_score,
            });
        }

        let consensus_score = (score_sum / total_weight) as u8;
        let consensus_confidence = self.calculate_consensus_confidence(&weighted_scores);

        ConsensusResult {
            consensus_score,
            confidence: consensus_confidence,
            achieved: consensus_confidence >= consensus_threshold,
            participating_validators: weighted_scores,
            total_weight,
        }
    }

    fn calculate_consensus_confidence(&self, scores: &[WeightedScore]) -> f64 {
        if scores.len() < 2 {
            return if scores.is_empty() { 0.0 } else { 0.5 };
        }

        // Calculate weighted variance
        let weighted_mean = scores.iter()
            .map(|s| s.weighted_score)
            .sum::<f64>() / scores.iter().map(|s| s.weight).sum::<f64>();

        let weighted_variance = scores.iter()
            .map(|s| s.weight * (s.raw_score as f64 - weighted_mean).powi(2))
            .sum::<f64>() / scores.iter().map(|s| s.weight).sum::<f64>();

        let standard_deviation = weighted_variance.sqrt();

        // Lower standard deviation means higher consensus
        (100.0 - standard_deviation.min(100.0)) / 100.0
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ValidationWeight {
    pub base_weight: f64,
    pub reputation_adjustment: f64,
    pub specialization_bonus: f64,
    pub accuracy_factor: f64,
    pub final_weight: f64,
    pub validator_level: AIExpertLevel,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WeightedValidation {
    pub validator_id: String,
    pub score: u8,
    pub weight: ValidationWeight,
    pub reasoning: String,
    pub confidence: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConsensusResult {
    pub consensus_score: u8,
    pub confidence: f64,
    pub achieved: bool,
    pub participating_validators: Vec<WeightedScore>,
    pub total_weight: f64,
}
```

This complete AI classification and certification system provides:

1. **Clear level definitions**: 4 levels, each with specific capability requirements and benefits
2. **Scientific evaluation algorithms**: Multi-dimensional comprehensive evaluation including performance, trends, behavior, peer evaluation, etc.
3. **Automated promotion/demotion**: Objective data-based automatic promotion/demotion mechanisms with probation and grace periods
4. **Validation system integration**: Complete implementation of task allocation, weight calculation, and consensus mechanisms
5. **Risk control mechanisms**: Multiple safety checks to ensure stable system operation

This system ensures that expert-level AIs truly possess corresponding capabilities while providing clear paths and incentive mechanisms for continuous AI improvement.