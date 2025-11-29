//! Reputation System and Anti-Sybil Attack Mechanisms
//!
//! This module implements a reputation scoring system using account age,
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
    /// Calculated reputation score (0-10000, scaled by 10000 to represent 0.0-1.0)
    /// SCALE=10000: 0 represents 0.0, 10000 represents 1.0, 9000 represents 0.9, etc.
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
            reputation_score: 0, // 0 represents 0.0
            total_rewards_earned: 0,
            successful_validations: 0,
            total_validations: 0,
        }
    }

    /// Calculate reputation score using u128 scaled arithmetic
    /// Returns scaled score (0-10000) where 10000 represents 1.0
    pub fn calculate_reputation_score(&mut self, current_time: u64) -> u64 {
        const SCALE: u128 = 10000;

        // 1. Account age score (30% weight)
        let account_age_days = (current_time.saturating_sub(self.created_at)) / (24 * 3600);
        // age_score = min(age_days / 30, 1.0) * SCALE
        let age_score_scaled = ((account_age_days as u128 * SCALE) / 30).min(SCALE);

        // 2. Transaction history score (40% weight)
        // history_score = min(tx_count / 100, 1.0) * SCALE
        let history_score_scaled = ((self.transaction_count as u128 * SCALE) / 100).min(SCALE);

        // 3. Stake score (30% weight)
        // stake_score = min(stake / 1_000_000, 1.0) * SCALE
        let stake_score_scaled = ((self.stake_amount as u128 * SCALE) / 1_000_000).min(SCALE);

        // Calculate base reputation score
        // base_reputation = age_score * 0.3 + history_score * 0.4 + stake_score * 0.3
        let base_reputation_scaled = (age_score_scaled * 3000) / SCALE +      // 30% weight
            (history_score_scaled * 4000) / SCALE +  // 40% weight
            (stake_score_scaled * 3000) / SCALE; // 30% weight

        // 4. Validation accuracy bonus (up to +20% = 2000 scaled)
        let validation_bonus_scaled = if self.total_validations > 10 {
            // accuracy = successful / total
            let accuracy_scaled =
                (self.successful_validations as u128 * SCALE) / self.total_validations as u128;
            // bonus = max(accuracy - 0.8, 0.0) * 1.0 = max(accuracy - 8000, 0) / 10000 * 10000
            // Simplified: max(accuracy_scaled - 8000, 0)
            // Then cap at 2000 (representing 0.2)
            if accuracy_scaled > 8000 {
                (accuracy_scaled - 8000).min(2000)
            } else {
                0
            }
        } else {
            0
        };

        // 5. Long-term participation bonus (account age > 90 days, extra +10% = 1000 scaled)
        let long_term_bonus_scaled = if account_age_days > 90 {
            1000u128
        } else {
            0u128
        };

        // Calculate final reputation score (capped at SCALE = 10000)
        self.reputation_score = ((base_reputation_scaled
            + validation_bonus_scaled
            + long_term_bonus_scaled)
            .min(SCALE)) as u64;
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
/// SAFE: f64 in IncreaseFee is for advisory/display only, not enforced by consensus
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RecommendedAction {
    /// Allow operation
    Allow,
    /// Increase fee (multiplier for suggestion only, not consensus-enforced)
    /// SAFE: Advisory only, actual fee validation uses u64
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

        // 4. Reputation score check (scaled: 10000 = 1.0, 1000 = 0.1, 3000 = 0.3)
        if reputation.reputation_score < 1000 {
            // < 0.1
            risk_score += 0.5;
            details.push("Reputation score extremely low".to_string());
        } else if reputation.reputation_score < 3000 {
            // < 0.3
            risk_score += 0.2;
            details.push("Reputation score relatively low".to_string());
        }

        // 5. Frequency check
        if !reputation.can_submit_now(current_time) {
            risk_score += 0.1;
            let remaining = reputation.get_remaining_cooldown(current_time);
            details.push(format!(
                "Still in cooldown period, {remaining} seconds remaining"
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
    // SCALE factor for fixed-point arithmetic (represents 1.0)
    const SCALE: u128 = 10000;

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

    // 3. Difficulty multiplier (scaled by SCALE)
    let difficulty_multiplier_scaled = match difficulty {
        DifficultyLevel::Beginner => 10000u128,     // 1.0 * SCALE
        DifficultyLevel::Intermediate => 12000u128, // 1.2 * SCALE
        DifficultyLevel::Advanced => 15000u128,     // 1.5 * SCALE
        DifficultyLevel::Expert => 20000u128,       // 2.0 * SCALE
    };

    // 4. Stake multiplier (scaled by SCALE)
    let stake_multiplier_scaled = if stake_amount < LOW_STAKE_THRESHOLD {
        50000u128 // 5.0 * SCALE (LOW_STAKE_PENALTY_MULTIPLIER)
    } else if stake_amount < MEDIUM_STAKE_THRESHOLD {
        20000u128 // 2.0 * SCALE (MEDIUM_STAKE_PENALTY_MULTIPLIER)
    } else {
        10000u128 // 1.0 * SCALE
    };

    // 5. Reputation discount/penalty (scaled by SCALE)
    let reputation_modifier_scaled = if reputation.reputation_score >= HIGH_REPUTATION_THRESHOLD {
        5000u128 // 0.5 * SCALE (HIGH_REPUTATION_DISCOUNT)
    } else if reputation.reputation_score >= MEDIUM_REPUTATION_THRESHOLD {
        7000u128 // 0.7 * SCALE (MEDIUM_REPUTATION_DISCOUNT)
    } else if reputation.reputation_score < LOW_REPUTATION_THRESHOLD {
        20000u128 // 2.0 * SCALE (LOW_REPUTATION_PENALTY)
    } else {
        10000u128 // 1.0 * SCALE
    };

    // 6. Calculate final cost using u128 scaled arithmetic
    // raw_cost = base_fee + (content_cost * difficulty_multiplier)
    let raw_cost_scaled = (base_fee as u128 * SCALE)
        + ((content_cost as u128 * difficulty_multiplier_scaled) / SCALE) * SCALE;
    let raw_cost = (raw_cost_scaled / SCALE) as u64;

    // adjusted_cost = raw_cost * stake_multiplier * reputation_modifier
    let temp = (raw_cost as u128 * stake_multiplier_scaled) / SCALE;
    let adjusted_cost = ((temp * reputation_modifier_scaled) / SCALE) as u64;

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
    // SCALE factor for fixed-point arithmetic (represents 1.0)
    const SCALE: u128 = 10000;

    // 1. Quality multiplier (scaled: validation_score/100 * SCALE)
    let quality_multiplier_scaled = (validation_score as u128 * SCALE) / 100;

    // 2. Scarcity bonus (scaled by SCALE)
    let scarcity_bonus_scaled = if validation_score >= HIGH_QUALITY_SCORE_THRESHOLD {
        15000u128 // 1.5 * SCALE (HIGH_QUALITY_SCARCITY_BONUS)
    } else if validation_score >= MEDIUM_QUALITY_SCORE_THRESHOLD {
        12000u128 // 1.2 * SCALE (MEDIUM_QUALITY_SCARCITY_BONUS)
    } else {
        10000u128 // 1.0 * SCALE
    };

    // 3. Long-term contributor bonus (scaled by SCALE)
    let loyalty_bonus_scaled = if reputation.reputation_score >= HIGH_REPUTATION_THRESHOLD {
        11000u128 // 1.1 * SCALE (10% additional reward)
    } else {
        10000u128 // 1.0 * SCALE
    };

    // Calculate final reward using u128 scaled arithmetic
    // final_reward = base_reward * quality_multiplier * scarcity_bonus * loyalty_bonus
    let temp1 = (base_reward as u128 * quality_multiplier_scaled) / SCALE;
    let temp2 = (temp1 * scarcity_bonus_scaled) / SCALE;
    let final_reward = (temp2 * loyalty_bonus_scaled) / SCALE;

    final_reward as u64
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
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

        // Score should be reasonable (between 5000 and 10000, representing 0.5-1.0)
        // Based on: 30 days age + 50 transactions + 0.0005 TOS stake
        assert!(
            score > 5000 && score <= 10000,
            "Score should be reasonable: {score} (expected 5000-10000)"
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
        reputation.reputation_score = 5000; // Medium reputation (0.5 * SCALE)

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
        reputation.reputation_score = 9500; // High reputation (0.95 * SCALE)

        let base_reward = calculate_base_reward(&DifficultyLevel::Advanced);
        let final_reward = calculate_final_reward(base_reward, 95, &reputation);

        assert_eq!(base_reward, ADVANCED_TASK_BASE_REWARD);
        assert!(final_reward > base_reward); // Should have reward bonus
    }
}
