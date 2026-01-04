//! Reputation System and Anti-Sybil Attack Mechanisms
//!
//! This module implements a reputation scoring system based on account age,
//! transaction history, and stake amount to prevent Sybil attacks and maintain network security.

use super::*;
use crate::crypto::elgamal::CompressedPublicKey;
use serde::{Deserialize, Serialize};

/// Account reputation information
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountReputation {
    /// Account public key
    pub account: CompressedPublicKey,
    /// Account creation timestamp
    pub created_at: u64,
    /// Historical transaction count
    pub transaction_count: u64,
    /// Current stake amount
    pub stake_amount: u64,
    /// Last submission time
    pub last_submission_time: u64,
    /// Calculated reputation score (0-SCALE, where SCALE=10000 represents 1.0)
    pub reputation_score: u64,
    /// Total rewards earned
    pub total_rewards_earned: u64,
    /// Successful validation count
    pub successful_validations: u64,
    /// Total validation attempts
    pub total_validations: u64,
}

impl AccountReputation {
    /// Create new account reputation record
    pub fn new(account: CompressedPublicKey, created_at: u64) -> Self {
        Self {
            account,
            created_at,
            transaction_count: 0,
            stake_amount: 0,
            last_submission_time: 0,
            reputation_score: 0,
            total_rewards_earned: 0,
            successful_validations: 0,
            total_validations: 0,
        }
    }

    /// Calculate reputation score using deterministic integer arithmetic
    /// Returns score in range 0-SCALE (where SCALE=10000 represents 1.0)
    pub fn calculate_reputation_score(&mut self, current_time: u64) -> u64 {
        // 1. Account age score (30% weight)
        // age_score = min(account_age_days / 30, 1.0) * SCALE
        let account_age_days = (current_time.saturating_sub(self.created_at)) / (24 * 3600);
        let age_score = (account_age_days * SCALE / 30).min(SCALE); // 30 days to reach max score

        // 2. Transaction history score (40% weight)
        // history_score = min(transaction_count / 100, 1.0) * SCALE
        let history_score = (self.transaction_count * SCALE / 100).min(SCALE); // 100 transactions to reach max score

        // 3. Stake score (30% weight)
        // stake_score = min(stake_amount / 1_000_000, 1.0) * SCALE
        let stake_score = (self.stake_amount * SCALE / 1_000_000).min(SCALE); // 0.001 TOS to reach max score

        // Calculate base reputation score
        // base_reputation = age_score * 0.3 + history_score * 0.4 + stake_score * 0.3
        // Using scaled arithmetic: (age_score * 3000 + history_score * 4000 + stake_score * 3000) / SCALE
        let base_reputation =
            (age_score * 3_000 + history_score * 4_000 + stake_score * 3_000) / SCALE;

        // 4. Validation accuracy bonus (up to +20% = 2000 scaled)
        // Bonus = (accuracy - 0.8) * 1.0 capped at 0.2
        // accuracy = successful_validations / total_validations
        let validation_bonus = if self.total_validations > 10 {
            // accuracy_scaled = (successful * SCALE) / total
            let accuracy_scaled = (self.successful_validations * SCALE) / self.total_validations;
            // Bonus starts at 80% accuracy (8000 scaled)
            // bonus = min((accuracy - 8000), 2000) if accuracy > 8000, else 0
            if accuracy_scaled > 8_000 {
                (accuracy_scaled - 8_000).min(2_000)
            } else {
                0
            }
        } else {
            0
        };

        // 5. Long-term participation bonus (account age > 90 days, extra +10% = 1000 scaled)
        let long_term_bonus = if account_age_days > 90 { 1_000 } else { 0 };

        self.reputation_score = (base_reputation + validation_bonus + long_term_bonus).min(SCALE);
        self.reputation_score
    }

    /// Check if eligible to participate in tasks of specified difficulty
    pub fn can_participate_in_difficulty(&self, difficulty: &DifficultyLevel) -> bool {
        match difficulty {
            DifficultyLevel::Beginner => self.reputation_score >= MIN_REPUTATION_FOR_BASIC,
            DifficultyLevel::Intermediate => {
                self.reputation_score >= MIN_REPUTATION_FOR_INTERMEDIATE
            }
            DifficultyLevel::Advanced => self.reputation_score >= MIN_REPUTATION_FOR_ADVANCED,
            DifficultyLevel::Expert => self.reputation_score >= MIN_REPUTATION_FOR_EXPERT,
        }
    }

    /// Get frequency limit cooldown period (seconds)
    pub fn get_cooldown_period(&self) -> u64 {
        if self.reputation_score >= HIGH_REPUTATION_THRESHOLD {
            300 // 5 minutes
        } else if self.reputation_score >= MEDIUM_REPUTATION_THRESHOLD {
            900 // 15 minutes
        } else if self.reputation_score >= LOW_REPUTATION_THRESHOLD {
            1800 // 30 minutes
        } else {
            3600 // 1 hour
        }
    }

    /// Check if can submit now (frequency limit)
    pub fn can_submit_now(&self, current_time: u64) -> bool {
        if self.transaction_count == 0 {
            return true;
        }

        let cooldown = self.get_cooldown_period();
        current_time.saturating_sub(self.last_submission_time) >= cooldown
    }

    /// Get remaining cooldown time
    pub fn get_remaining_cooldown(&self, current_time: u64) -> u64 {
        if self.transaction_count == 0 {
            return 0;
        }

        let cooldown = self.get_cooldown_period();
        let elapsed = current_time.saturating_sub(self.last_submission_time);
        cooldown.saturating_sub(elapsed)
    }

    /// Update submission time
    pub fn update_submission_time(&mut self, submission_time: u64) {
        self.last_submission_time = submission_time;
        self.transaction_count += 1;
    }

    /// Update stake amount
    pub fn update_stake(&mut self, new_stake: u64) {
        self.stake_amount = new_stake;
    }

    /// Record validation result
    pub fn record_validation(&mut self, is_successful: bool) {
        self.total_validations += 1;
        if is_successful {
            self.successful_validations += 1;
        }
    }

    /// Record received reward
    pub fn record_reward(&mut self, reward_amount: u64) {
        self.total_rewards_earned += reward_amount;
    }
}

/// Anti-Sybil attack detection result
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AntiSybilResult {
    /// Whether the detection is passed
    pub is_valid: bool,
    /// Risk level
    pub risk_level: RiskLevel,
    /// Detection details
    pub details: Vec<String>,
    /// Recommended action
    pub recommended_action: RecommendedAction,
}

/// Risk level
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Low risk
    Low,
    /// Medium risk
    Medium,
    /// High risk
    High,
    /// Critical risk
    Critical,
}

/// Recommended action
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RecommendedAction {
    /// Allow operation
    Allow,
    /// Increase fee (multiplier in SCALE units, e.g., 50000 = 5.0x)
    IncreaseFee(u64),
    /// Extend cooldown time
    ExtendCooldown(u64),
    /// Reject operation
    Reject,
    /// Require additional verification
    RequireAdditionalVerification,
}

/// Anti-Sybil attack detector
pub struct AntiSybilDetector;

impl AntiSybilDetector {
    /// Detect whether an account has Sybil attack risk
    /// Uses deterministic integer arithmetic for consensus safety
    pub fn detect_sybil_risk(reputation: &AccountReputation, current_time: u64) -> AntiSybilResult {
        let mut details = Vec::new();
        // risk_score is in SCALE units (0-10000)
        let mut risk_score: u64 = 0;

        // 1. Account age check
        let account_age_hours = (current_time.saturating_sub(reputation.created_at)) / 3600;
        if account_age_hours < 24 {
            risk_score += 4_000; // 0.4
            details.push("Account created less than 24 hours ago".to_string());
        } else if account_age_hours < 168 {
            // 1 week
            risk_score += 2_000; // 0.2
            details.push("Account created less than 1 week ago".to_string());
        }

        // 2. Transaction history check
        if reputation.transaction_count == 0 {
            risk_score += 3_000; // 0.3
            details.push("No transaction history".to_string());
        } else if reputation.transaction_count < 5 {
            risk_score += 1_500; // 0.15
            details.push("Limited transaction history".to_string());
        }

        // 3. Stake check
        if reputation.stake_amount == 0 {
            risk_score += 3_000; // 0.3
            details.push("No stake amount".to_string());
        } else if reputation.stake_amount < LOW_STAKE_THRESHOLD {
            risk_score += 2_000; // 0.2
            details.push("Stake amount too low".to_string());
        }

        // 4. Reputation score check (compare scaled values)
        // 0.1 = 1000, 0.3 = 3000
        if reputation.reputation_score < 1_000 {
            risk_score += 5_000; // 0.5
            details.push("Reputation score extremely low".to_string());
        } else if reputation.reputation_score < 3_000 {
            risk_score += 2_000; // 0.2
            details.push("Reputation score relatively low".to_string());
        }

        // 5. Frequency check
        if !reputation.can_submit_now(current_time) {
            risk_score += 1_000; // 0.1
            let remaining = reputation.get_remaining_cooldown(current_time);
            details.push(format!(
                "Still in cooldown period, {} seconds remaining",
                remaining
            ));
        }

        // Determine risk level and recommended action
        // Thresholds: 0.8 = 8000, 0.6 = 6000, 0.4 = 4000, 0.2 = 2000
        // Fee multipliers: 5.0 = 50000, 2.0 = 20000, 1.2 = 12000
        let (risk_level, recommended_action) = if risk_score >= 8_000 {
            (RiskLevel::Critical, RecommendedAction::Reject)
        } else if risk_score >= 6_000 {
            (RiskLevel::High, RecommendedAction::IncreaseFee(50_000)) // 5.0x
        } else if risk_score >= 4_000 {
            (RiskLevel::Medium, RecommendedAction::IncreaseFee(20_000)) // 2.0x
        } else if risk_score >= 2_000 {
            (RiskLevel::Low, RecommendedAction::IncreaseFee(12_000)) // 1.2x
        } else {
            (RiskLevel::Low, RecommendedAction::Allow)
        };

        AntiSybilResult {
            is_valid: risk_score < 8_000,
            risk_level,
            details,
            recommended_action,
        }
    }
}

/// Calculate secure gas cost using deterministic integer arithmetic
pub fn calculate_secure_gas_cost(
    content_length: usize,
    difficulty: &DifficultyLevel,
    reputation: &AccountReputation,
    stake_amount: u64,
) -> u64 {
    // 1. Base fee
    let base_fee = 2500u64;

    // 2. Content storage cost - tiered pricing
    let content_cost = if content_length <= SHORT_CONTENT_THRESHOLD {
        content_length as u64 * SHORT_CONTENT_GAS_RATE
    } else if content_length <= MEDIUM_CONTENT_THRESHOLD {
        (SHORT_CONTENT_THRESHOLD as u64 * SHORT_CONTENT_GAS_RATE)
            + ((content_length - SHORT_CONTENT_THRESHOLD) as u64 * MEDIUM_CONTENT_GAS_RATE)
    } else {
        (SHORT_CONTENT_THRESHOLD as u64 * SHORT_CONTENT_GAS_RATE)
            + ((MEDIUM_CONTENT_THRESHOLD - SHORT_CONTENT_THRESHOLD) as u64
                * MEDIUM_CONTENT_GAS_RATE)
            + ((content_length - MEDIUM_CONTENT_THRESHOLD) as u64 * LONG_CONTENT_GAS_RATE)
    };

    // 3. Difficulty multiplier (in SCALE units)
    // 1.0 = 10000, 1.2 = 12000, 1.5 = 15000, 2.0 = 20000
    let difficulty_multiplier: u64 = match difficulty {
        DifficultyLevel::Beginner => SCALE,      // 1.0
        DifficultyLevel::Intermediate => 12_000, // 1.2
        DifficultyLevel::Advanced => 15_000,     // 1.5
        DifficultyLevel::Expert => 20_000,       // 2.0
    };

    // 4. Stake multiplier (in SCALE units)
    let stake_multiplier = if stake_amount < LOW_STAKE_THRESHOLD {
        LOW_STAKE_PENALTY_MULTIPLIER // 50000 = 5.0x
    } else if stake_amount < MEDIUM_STAKE_THRESHOLD {
        MEDIUM_STAKE_PENALTY_MULTIPLIER // 20000 = 2.0x
    } else {
        SCALE // 10000 = 1.0x
    };

    // 5. Reputation discount/penalty (in SCALE units)
    let reputation_modifier = if reputation.reputation_score >= HIGH_REPUTATION_THRESHOLD {
        HIGH_REPUTATION_DISCOUNT // 5000 = 0.5x (50% discount)
    } else if reputation.reputation_score >= MEDIUM_REPUTATION_THRESHOLD {
        MEDIUM_REPUTATION_DISCOUNT // 7000 = 0.7x (30% discount)
    } else if reputation.reputation_score < LOW_REPUTATION_THRESHOLD {
        LOW_REPUTATION_PENALTY // 20000 = 2.0x penalty
    } else {
        SCALE // 10000 = 1.0x
    };

    // 6. Calculate final cost using scaled integer arithmetic
    // raw_cost = base_fee + (content_cost * difficulty_multiplier / SCALE)
    let raw_cost = base_fee + (content_cost * difficulty_multiplier / SCALE);

    // adjusted_cost = raw_cost * stake_multiplier * reputation_modifier / SCALE / SCALE
    // To avoid overflow, divide after each multiplication
    let adjusted_cost = (raw_cost * stake_multiplier / SCALE) * reputation_modifier / SCALE;

    // 7. Ensure minimum cost
    adjusted_cost.max(MIN_TRANSACTION_COST)
}

/// Calculate base reward based on difficulty
pub fn calculate_base_reward(difficulty: &DifficultyLevel) -> u64 {
    match difficulty {
        DifficultyLevel::Beginner => BASIC_TASK_BASE_REWARD,
        DifficultyLevel::Intermediate => INTERMEDIATE_TASK_BASE_REWARD,
        DifficultyLevel::Advanced => ADVANCED_TASK_BASE_REWARD,
        DifficultyLevel::Expert => EXPERT_TASK_BASE_REWARD,
    }
}

/// Calculate final reward (including quality bonus) using deterministic integer arithmetic
pub fn calculate_final_reward(
    base_reward: u64,
    validation_score: u8,
    reputation: &AccountReputation,
) -> u64 {
    // 1. Quality multiplier (score/100 in SCALE units)
    // validation_score is 0-100, so quality_multiplier = validation_score * SCALE / 100
    let quality_multiplier = validation_score as u64 * SCALE / 100;

    // 2. Scarcity bonus (in SCALE units)
    let scarcity_bonus = if validation_score >= HIGH_QUALITY_SCORE_THRESHOLD {
        HIGH_QUALITY_SCARCITY_BONUS // 15000 = 1.5x
    } else if validation_score >= MEDIUM_QUALITY_SCORE_THRESHOLD {
        MEDIUM_QUALITY_SCARCITY_BONUS // 12000 = 1.2x
    } else {
        SCALE // 10000 = 1.0x
    };

    // 3. Long-term contributor bonus (in SCALE units)
    // 1.1 = 11000
    let loyalty_bonus = if reputation.reputation_score >= HIGH_REPUTATION_THRESHOLD {
        11_000 // 1.1x = 10% additional reward
    } else {
        SCALE // 10000 = 1.0x
    };

    // Calculate final reward using scaled integer arithmetic
    // final_reward = base_reward * quality_multiplier * scarcity_bonus * loyalty_bonus / SCALE^3
    // To avoid overflow, divide after each multiplication
    let step1 = base_reward * quality_multiplier / SCALE;
    let step2 = step1 * scarcity_bonus / SCALE;
    step2 * loyalty_bonus / SCALE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::elgamal::CompressedPublicKey;
    use crate::serializer::Serializer;

    #[test]
    fn test_reputation_calculation() {
        let account =
            CompressedPublicKey::from_bytes(&[0u8; 32]).expect("deserialization should succeed");
        let created_at = 1000000;
        let current_time = 1000000 + 30 * 24 * 3600; // 30 days later

        let mut reputation = AccountReputation::new(account, created_at);
        reputation.transaction_count = 50;
        reputation.stake_amount = 500_000; // 0.0005 TOS

        let score = reputation.calculate_reputation_score(current_time);

        // Score should be reasonable (between 0.5 and 1.0 in scaled units: 5000-10000)
        // Based on: 30 days age + 50 transactions + 0.0005 TOS stake
        assert!(
            score > 5_000 && score <= SCALE,
            "Score should be reasonable (5000-10000): {}",
            score
        );
    }

    #[test]
    fn test_anti_sybil_detection() {
        let account =
            CompressedPublicKey::from_bytes(&[0u8; 32]).expect("deserialization should succeed");
        let current_time = 1000000;

        // Test new account (high risk)
        let reputation = AccountReputation::new(account, current_time - 3600); // Created 1 hour ago
        let result = AntiSybilDetector::detect_sybil_risk(&reputation, current_time);

        assert_eq!(result.risk_level, RiskLevel::Critical);
        assert!(!result.is_valid);
    }

    #[test]
    fn test_secure_gas_calculation() {
        let account =
            CompressedPublicKey::from_bytes(&[0u8; 32]).expect("deserialization should succeed");
        let mut reputation = AccountReputation::new(account, 1000000);
        reputation.reputation_score = 5_000; // Medium reputation (0.5 in SCALE units)

        let gas_cost = calculate_secure_gas_cost(
            300, // 300 bytes content
            &DifficultyLevel::Intermediate,
            &reputation,
            200_000, // 0.0002 TOS stake
        );

        assert!(gas_cost >= MIN_TRANSACTION_COST);
        assert!(gas_cost > 2500); // Should be higher than base fee
    }

    #[test]
    fn test_reward_calculation() {
        let account =
            CompressedPublicKey::from_bytes(&[0u8; 32]).expect("deserialization should succeed");
        let mut reputation = AccountReputation::new(account, 1000000);
        reputation.reputation_score = 9_500; // High reputation (0.95 in SCALE units)

        let base_reward = calculate_base_reward(&DifficultyLevel::Advanced);
        let final_reward = calculate_final_reward(base_reward, 95, &reputation);

        assert_eq!(base_reward, ADVANCED_TASK_BASE_REWARD);
        // With 95% validation score + 1.5x scarcity bonus + 1.1x loyalty bonus
        // final_reward should be meaningful portion of base_reward
        // 0.95 * 1.5 * 1.1 = 1.5675, so final should be > base
        assert!(
            final_reward > base_reward / 2,
            "Final reward {} should be significant",
            final_reward
        );
    }
}
