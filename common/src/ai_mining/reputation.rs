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
    /// Calculated reputation score (0.0-1.0)
    pub reputation_score: f64,
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
            reputation_score: 0.0,
            total_rewards_earned: 0,
            successful_validations: 0,
            total_validations: 0,
        }
    }

    /// Calculate reputation score
    pub fn calculate_reputation_score(&mut self, current_time: u64) -> f64 {
        // 1. Account age score (30% weight)
        let account_age_days = (current_time.saturating_sub(self.created_at)) / (24 * 3600);
        let age_score = (account_age_days as f64 / 30.0).min(1.0); // 30 days to reach max score

        // 2. Transaction history score (40% weight)
        let history_score = (self.transaction_count as f64 / 100.0).min(1.0); // 100 transactions to reach max score

        // 3. Stake score (30% weight)
        let stake_score = (self.stake_amount as f64 / 1_000_000.0).min(1.0); // 0.001 TOS to reach max score

        // Calculate base reputation score
        let base_reputation = age_score * 0.3 + history_score * 0.4 + stake_score * 0.3;

        // 4. Validation accuracy bonus (up to +20%)
        let validation_bonus = if self.total_validations > 10 {
            let accuracy = self.successful_validations as f64 / self.total_validations as f64;
            ((accuracy - 0.8).max(0.0) * 1.0).min(0.2) // 80%+ accuracy gets bonus
        } else {
            0.0
        };

        // 5. Long-term participation bonus (account age > 90 days, extra +10%)
        let long_term_bonus = if account_age_days > 90 { 0.1 } else { 0.0 };

        self.reputation_score = (base_reputation + validation_bonus + long_term_bonus).min(1.0);
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
    /// Increase fee
    IncreaseFee(f64),
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
    pub fn detect_sybil_risk(reputation: &AccountReputation, current_time: u64) -> AntiSybilResult {
        let mut details = Vec::new();
        let mut risk_score = 0.0;

        // 1. Account age check
        let account_age_hours = (current_time.saturating_sub(reputation.created_at)) / 3600;
        if account_age_hours < 24 {
            risk_score += 0.4;
            details.push("Account created less than 24 hours ago".to_string());
        } else if account_age_hours < 168 {
            // 1 week
            risk_score += 0.2;
            details.push("Account created less than 1 week ago".to_string());
        }

        // 2. Transaction history check
        if reputation.transaction_count == 0 {
            risk_score += 0.3;
            details.push("No transaction history".to_string());
        } else if reputation.transaction_count < 5 {
            risk_score += 0.15;
            details.push("Limited transaction history".to_string());
        }

        // 3. Stake check
        if reputation.stake_amount == 0 {
            risk_score += 0.3;
            details.push("No stake amount".to_string());
        } else if reputation.stake_amount < LOW_STAKE_THRESHOLD {
            risk_score += 0.2;
            details.push("Stake amount too low".to_string());
        }

        // 4. Reputation score check
        if reputation.reputation_score < 0.1 {
            risk_score += 0.5;
            details.push("Reputation score extremely low".to_string());
        } else if reputation.reputation_score < 0.3 {
            risk_score += 0.2;
            details.push("Reputation score relatively low".to_string());
        }

        // 5. Frequency check
        if !reputation.can_submit_now(current_time) {
            risk_score += 0.1;
            let remaining = reputation.get_remaining_cooldown(current_time);
            details.push(format!(
                "Still in cooldown period, {} seconds remaining",
                remaining
            ));
        }

        // Determine risk level and recommended action
        let (risk_level, recommended_action) = if risk_score >= 0.8 {
            (RiskLevel::Critical, RecommendedAction::Reject)
        } else if risk_score >= 0.6 {
            (RiskLevel::High, RecommendedAction::IncreaseFee(5.0))
        } else if risk_score >= 0.4 {
            (RiskLevel::Medium, RecommendedAction::IncreaseFee(2.0))
        } else if risk_score >= 0.2 {
            (RiskLevel::Low, RecommendedAction::IncreaseFee(1.2))
        } else {
            (RiskLevel::Low, RecommendedAction::Allow)
        };

        AntiSybilResult {
            is_valid: risk_score < 0.8,
            risk_level,
            details,
            recommended_action,
        }
    }
}

/// Calculate secure gas cost
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

    // 3. Difficulty multiplier
    let difficulty_multiplier = match difficulty {
        DifficultyLevel::Beginner => 1.0,
        DifficultyLevel::Intermediate => 1.2,
        DifficultyLevel::Advanced => 1.5,
        DifficultyLevel::Expert => 2.0,
    };

    // 4. Stake multiplier
    let stake_multiplier = if stake_amount < LOW_STAKE_THRESHOLD {
        LOW_STAKE_PENALTY_MULTIPLIER
    } else if stake_amount < MEDIUM_STAKE_THRESHOLD {
        MEDIUM_STAKE_PENALTY_MULTIPLIER
    } else {
        1.0
    };

    // 5. Reputation discount/penalty
    let reputation_modifier = if reputation.reputation_score >= HIGH_REPUTATION_THRESHOLD {
        HIGH_REPUTATION_DISCOUNT
    } else if reputation.reputation_score >= MEDIUM_REPUTATION_THRESHOLD {
        MEDIUM_REPUTATION_DISCOUNT
    } else if reputation.reputation_score < LOW_REPUTATION_THRESHOLD {
        LOW_REPUTATION_PENALTY
    } else {
        1.0
    };

    // 6. Calculate final cost
    let raw_cost = base_fee + (content_cost as f64 * difficulty_multiplier) as u64;
    let adjusted_cost = (raw_cost as f64 * stake_multiplier * reputation_modifier) as u64;

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

/// Calculate final reward (including quality bonus)
pub fn calculate_final_reward(
    base_reward: u64,
    validation_score: u8,
    reputation: &AccountReputation,
) -> u64 {
    // 1. Quality multiplier
    let quality_multiplier = validation_score as f64 / 100.0;

    // 2. Scarcity bonus
    let scarcity_bonus = if validation_score >= HIGH_QUALITY_SCORE_THRESHOLD {
        HIGH_QUALITY_SCARCITY_BONUS
    } else if validation_score >= MEDIUM_QUALITY_SCORE_THRESHOLD {
        MEDIUM_QUALITY_SCARCITY_BONUS
    } else {
        1.0
    };

    // 3. Long-term contributor bonus
    let loyalty_bonus = if reputation.reputation_score >= HIGH_REPUTATION_THRESHOLD {
        1.1 // 10% additional reward
    } else {
        1.0
    };

    // Calculate final reward
    let final_reward = base_reward as f64 * quality_multiplier * scarcity_bonus * loyalty_bonus;
    final_reward as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::elgamal::CompressedPublicKey;
    use crate::serializer::Serializer;

    #[test]
    fn test_reputation_calculation() {
        let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();
        let created_at = 1000000;
        let current_time = 1000000 + 30 * 24 * 3600; // 30 days later

        let mut reputation = AccountReputation::new(account, created_at);
        reputation.transaction_count = 50;
        reputation.stake_amount = 500_000; // 0.0005 TOS

        let score = reputation.calculate_reputation_score(current_time);

        // Score should be reasonable (between 0.5 and 1.0)
        // Based on: 30 days age + 50 transactions + 0.0005 TOS stake
        assert!(
            score > 0.5 && score < 1.0,
            "Score should be reasonable: {}",
            score
        );
    }

    #[test]
    fn test_anti_sybil_detection() {
        let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();
        let current_time = 1000000;

        // Test new account (high risk)
        let reputation = AccountReputation::new(account, current_time - 3600); // Created 1 hour ago
        let result = AntiSybilDetector::detect_sybil_risk(&reputation, current_time);

        assert_eq!(result.risk_level, RiskLevel::Critical);
        assert!(!result.is_valid);
    }

    #[test]
    fn test_secure_gas_calculation() {
        let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();
        let mut reputation = AccountReputation::new(account, 1000000);
        reputation.reputation_score = 0.5; // Medium reputation

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
        let account = CompressedPublicKey::from_bytes(&[0u8; 32]).unwrap();
        let mut reputation = AccountReputation::new(account, 1000000);
        reputation.reputation_score = 0.95; // High reputation

        let base_reward = calculate_base_reward(&DifficultyLevel::Advanced);
        let final_reward = calculate_final_reward(base_reward, 95, &reputation);

        assert_eq!(base_reward, ADVANCED_TASK_BASE_REWARD);
        assert!(final_reward > base_reward); // Should have reward bonus
    }
}
