//! AI Mining module for TOS network
//!
//! This module implements the "Proof of Intelligent Work" mechanism where AI agents
//! can earn TOS rewards by solving real-world computational problems.

pub mod task;
pub mod state;
pub mod validation;
pub mod serializers;
pub mod reputation;

use serde::{Deserialize, Serialize};
pub use task::*;
pub use state::*;
pub use validation::*;
pub use reputation::*;
use crate::crypto::{Hash, elgamal::CompressedPublicKey};

/// Maximum description length to prevent spam attacks (2KB)
pub const MAX_TASK_DESCRIPTION_LENGTH: usize = 2048;

/// Minimum description length to be considered valid
pub const MIN_TASK_DESCRIPTION_LENGTH: usize = 10;

/// Maximum answer content length to prevent spam attacks (2KB)
pub const MAX_ANSWER_CONTENT_LENGTH: usize = 2048;

/// Minimum answer content length to be considered valid
pub const MIN_ANSWER_CONTENT_LENGTH: usize = 10;

// ====== New secure economic model constants ======

/// Minimum transaction cost to prevent spam attacks (0.00005 TOS)
pub const MIN_TRANSACTION_COST: u64 = 50_000;

/// Content pricing tier - short content (0-200 bytes): 0.0000005 TOS/byte
pub const SHORT_CONTENT_GAS_RATE: u64 = 500;

/// Content pricing tier - medium content (200-1000 bytes): 0.000001 TOS/byte
pub const MEDIUM_CONTENT_GAS_RATE: u64 = 1_000;

/// Content pricing tier - long content (1000+ bytes): 0.000002 TOS/byte
pub const LONG_CONTENT_GAS_RATE: u64 = 2_000;

/// Short content threshold (200 bytes)
pub const SHORT_CONTENT_THRESHOLD: usize = 200;

/// Medium content threshold (1000 bytes)
pub const MEDIUM_CONTENT_THRESHOLD: usize = 1000;

// ====== Difficulty-based base rewards ======

/// Basic task reward (0.05 TOS)
pub const BASIC_TASK_BASE_REWARD: u64 = 50_000_000;

/// Intermediate task reward (0.2 TOS)
pub const INTERMEDIATE_TASK_BASE_REWARD: u64 = 200_000_000;

/// Advanced task reward (0.5 TOS)
pub const ADVANCED_TASK_BASE_REWARD: u64 = 500_000_000;

/// Expert task reward (1.0 TOS)
pub const EXPERT_TASK_BASE_REWARD: u64 = 1_000_000_000;

// ====== Reputation and anti-Sybil constants ======

/// Minimum reputation requirement for basic tasks
pub const MIN_REPUTATION_FOR_BASIC: f64 = 0.1;

/// Minimum reputation requirement for intermediate tasks
pub const MIN_REPUTATION_FOR_INTERMEDIATE: f64 = 0.3;

/// Minimum reputation requirement for advanced tasks
pub const MIN_REPUTATION_FOR_ADVANCED: f64 = 0.5;

/// Minimum reputation requirement for expert tasks
pub const MIN_REPUTATION_FOR_EXPERT: f64 = 0.7;

/// Fee increase multiplier for low-stake users
pub const LOW_STAKE_PENALTY_MULTIPLIER: f64 = 5.0;

/// Fee increase multiplier for medium-stake users
pub const MEDIUM_STAKE_PENALTY_MULTIPLIER: f64 = 2.0;

/// Low stake threshold (0.00001 TOS)
pub const LOW_STAKE_THRESHOLD: u64 = 10_000;

/// Medium stake threshold (0.0001 TOS)
pub const MEDIUM_STAKE_THRESHOLD: u64 = 100_000;

/// Fee discount for high reputation users (50% discount)
pub const HIGH_REPUTATION_DISCOUNT: f64 = 0.5;

/// Fee discount for medium reputation users (30% discount)
pub const MEDIUM_REPUTATION_DISCOUNT: f64 = 0.7;

/// Fee increase multiplier for low reputation users
pub const LOW_REPUTATION_PENALTY: f64 = 2.0;

/// High reputation threshold
pub const HIGH_REPUTATION_THRESHOLD: f64 = 0.9;

/// Medium reputation threshold
pub const MEDIUM_REPUTATION_THRESHOLD: f64 = 0.7;

/// Low reputation threshold
pub const LOW_REPUTATION_THRESHOLD: f64 = 0.3;

// ====== Quality reward coefficients ======

/// High quality answer scarcity bonus (90%+ score)
pub const HIGH_QUALITY_SCARCITY_BONUS: f64 = 1.5;

/// Medium quality answer scarcity bonus (80%+ score)
pub const MEDIUM_QUALITY_SCARCITY_BONUS: f64 = 1.2;

/// High quality score threshold
pub const HIGH_QUALITY_SCORE_THRESHOLD: u8 = 90;

/// Medium quality score threshold
pub const MEDIUM_QUALITY_SCORE_THRESHOLD: u8 = 80;

/// AI Mining transaction payload types
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AIMiningPayload {
    /// Publish a new AI mining task
    PublishTask {
        task_id: Hash,
        reward_amount: u64,
        difficulty: DifficultyLevel,
        deadline: u64,
        /// Task description (limited to MAX_TASK_DESCRIPTION_LENGTH)
        description: String,
    },
    /// Submit a solution to a task
    SubmitAnswer {
        task_id: Hash,
        /// The actual answer content (limited to MAX_ANSWER_CONTENT_LENGTH)
        answer_content: String,
        answer_hash: Hash,
        stake_amount: u64,
    },
    /// Validate a submitted answer
    ValidateAnswer {
        task_id: Hash,
        answer_id: Hash,
        validation_score: u8,
    },
    /// Register as an AI miner
    RegisterMiner {
        miner_address: CompressedPublicKey,
        registration_fee: u64,
    },
}

/// Task difficulty levels
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DifficultyLevel {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

impl DifficultyLevel {
    /// Get the reward range for this difficulty level (in nanoTOS)
    pub fn reward_range(&self) -> (u64, u64) {
        match self {
            DifficultyLevel::Beginner => (5_000_000_000, 15_000_000_000),
            DifficultyLevel::Intermediate => (15_000_000_000, 50_000_000_000),
            DifficultyLevel::Advanced => (50_000_000_000, 200_000_000_000),
            DifficultyLevel::Expert => (200_000_000_000, 500_000_000_000),
        }
    }
}

/// AI Mining error types
#[derive(Debug, Clone, PartialEq)]
pub enum AIMiningError {
    InvalidTaskConfig(String),
    InsufficientStake { required: u64, available: u64 },
    TaskNotFound(Hash),
    MinerNotRegistered(CompressedPublicKey),
    ValidationFailed(String),
    SystemError(String),
}

impl std::fmt::Display for AIMiningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AIMiningError::InvalidTaskConfig(msg) => write!(f, "Invalid task configuration: {}", msg),
            AIMiningError::InsufficientStake { required, available } => {
                write!(f, "Insufficient stake: required {} nanoTOS, available {} nanoTOS", required, available)
            }
            AIMiningError::TaskNotFound(hash) => write!(f, "Task not found: {:?}", hash),
            AIMiningError::MinerNotRegistered(address) => write!(f, "Miner not registered: {:?}", address),
            AIMiningError::ValidationFailed(msg) => write!(f, "Validation failed: {}", msg),
            AIMiningError::SystemError(msg) => write!(f, "System error: {}", msg),
        }
    }
}

impl std::error::Error for AIMiningError {}

/// Result type for AI Mining operations
pub type AIMiningResult<T> = Result<T, AIMiningError>;

impl AIMiningPayload {
    /// Validate the AI mining payload
    pub fn validate(&self) -> AIMiningResult<()> {
        match self {
            AIMiningPayload::PublishTask { description, .. } => {
                // Validate description length
                if description.len() < MIN_TASK_DESCRIPTION_LENGTH {
                    return Err(AIMiningError::InvalidTaskConfig(
                        format!("Description too short: minimum {} characters required", MIN_TASK_DESCRIPTION_LENGTH)
                    ));
                }

                if description.len() > MAX_TASK_DESCRIPTION_LENGTH {
                    return Err(AIMiningError::InvalidTaskConfig(
                        format!("Description too long: maximum {} characters allowed", MAX_TASK_DESCRIPTION_LENGTH)
                    ));
                }

                // Validate UTF-8 encoding
                if !description.is_ascii() {
                    // Allow non-ASCII but ensure valid UTF-8
                    let _: &str = description; // This will panic if invalid UTF-8, but String guarantees valid UTF-8
                }

                Ok(())
            }
            AIMiningPayload::SubmitAnswer { answer_content, .. } => {
                // Validate answer content length
                if answer_content.len() < MIN_ANSWER_CONTENT_LENGTH {
                    return Err(AIMiningError::InvalidTaskConfig(
                        format!("Answer content too short: minimum {} characters required", MIN_ANSWER_CONTENT_LENGTH)
                    ));
                }

                if answer_content.len() > MAX_ANSWER_CONTENT_LENGTH {
                    return Err(AIMiningError::InvalidTaskConfig(
                        format!("Answer content too long: maximum {} characters allowed", MAX_ANSWER_CONTENT_LENGTH)
                    ));
                }

                // Validate UTF-8 encoding
                if !answer_content.is_ascii() {
                    // Allow non-ASCII but ensure valid UTF-8
                    let _: &str = answer_content; // This will panic if invalid UTF-8, but String guarantees valid UTF-8
                }

                Ok(())
            }
            _ => Ok(()) // Other payload types don't need validation
        }
    }

    /// Calculate gas cost based on new security model
    pub fn calculate_content_gas_cost(&self) -> u64 {
        match self {
            AIMiningPayload::PublishTask { description, .. } => {
                Self::calculate_tiered_gas_cost(description.len())
            }
            AIMiningPayload::SubmitAnswer { answer_content, .. } => {
                Self::calculate_tiered_gas_cost(answer_content.len())
            }
            _ => 0 // Other payload types don't have dynamic gas costs
        }
    }

    /// Calculate tiered gas cost
    fn calculate_tiered_gas_cost(content_length: usize) -> u64 {
        // Ensure minimum cost
        let content_cost = if content_length <= SHORT_CONTENT_THRESHOLD {
            // Short content: 0-200 bytes
            content_length as u64 * SHORT_CONTENT_GAS_RATE
        } else if content_length <= MEDIUM_CONTENT_THRESHOLD {
            // Medium content: 200-1000 bytes
            (SHORT_CONTENT_THRESHOLD as u64 * SHORT_CONTENT_GAS_RATE) +
            ((content_length - SHORT_CONTENT_THRESHOLD) as u64 * MEDIUM_CONTENT_GAS_RATE)
        } else {
            // Long content: 1000+ bytes
            (SHORT_CONTENT_THRESHOLD as u64 * SHORT_CONTENT_GAS_RATE) +
            ((MEDIUM_CONTENT_THRESHOLD - SHORT_CONTENT_THRESHOLD) as u64 * MEDIUM_CONTENT_GAS_RATE) +
            ((content_length - MEDIUM_CONTENT_THRESHOLD) as u64 * LONG_CONTENT_GAS_RATE)
        };

        // Ensure minimum cost
        content_cost.max(MIN_TRANSACTION_COST)
    }

    /// Calculate the gas cost for description content only (for backward compatibility)
    pub fn calculate_description_gas_cost(&self) -> u64 {
        match self {
            AIMiningPayload::PublishTask { description, .. } => {
                Self::calculate_tiered_gas_cost(description.len())
            }
            _ => 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_difficulty_level_ranges() {
        assert_eq!(DifficultyLevel::Beginner.reward_range(), (5_000_000_000, 15_000_000_000));
        assert_eq!(DifficultyLevel::Expert.reward_range(), (200_000_000_000, 500_000_000_000));
    }

    #[test]
    fn test_answer_content_validation() {
        // Test minimum length validation
        let short_answer = "Short"; // 5 bytes < 10 minimum
        let submit_short = AIMiningPayload::SubmitAnswer {
            task_id: Hash::new([1u8; 32]),
            answer_content: short_answer.to_string(),
            answer_hash: Hash::new([2u8; 32]),
            stake_amount: 1_000_000_000,
        };
        assert!(submit_short.validate().is_err());

        // Test maximum length validation
        let long_answer = "x".repeat(2049); // 2049 bytes > 2048 maximum
        let submit_long = AIMiningPayload::SubmitAnswer {
            task_id: Hash::new([1u8; 32]),
            answer_content: long_answer,
            answer_hash: Hash::new([2u8; 32]),
            stake_amount: 1_000_000_000,
        };
        assert!(submit_long.validate().is_err());

        // Test valid answer content
        let valid_answer = "This is a valid answer that meets the minimum length requirement for AI mining submission.";
        let submit_valid = AIMiningPayload::SubmitAnswer {
            task_id: Hash::new([1u8; 32]),
            answer_content: valid_answer.to_string(),
            answer_hash: Hash::new([2u8; 32]),
            stake_amount: 1_000_000_000,
        };
        assert!(submit_valid.validate().is_ok());
    }

    #[test]
    fn test_answer_content_gas_cost_calculation() {
        // Test gas cost calculation for different answer lengths
        let answer_10_chars = "Ten chars."; // Exactly 10 characters
        let payload_10 = AIMiningPayload::SubmitAnswer {
            task_id: Hash::new([1u8; 32]),
            answer_content: answer_10_chars.to_string(),
            answer_hash: Hash::new([2u8; 32]),
            stake_amount: 1_000_000_000,
        };
        assert_eq!(payload_10.calculate_content_gas_cost(), 10 * ANSWER_CONTENT_GAS_COST_PER_BYTE);

        let answer_100_chars = "a".repeat(100); // 100 characters
        let payload_100 = AIMiningPayload::SubmitAnswer {
            task_id: Hash::new([1u8; 32]),
            answer_content: answer_100_chars,
            answer_hash: Hash::new([2u8; 32]),
            stake_amount: 1_000_000_000,
        };
        assert_eq!(payload_100.calculate_content_gas_cost(), 100 * ANSWER_CONTENT_GAS_COST_PER_BYTE);

        // Test other payload types return 0
        let task_payload = AIMiningPayload::PublishTask {
            task_id: Hash::new([1u8; 32]),
            reward_amount: 10_000_000_000,
            difficulty: DifficultyLevel::Beginner,
            deadline: 1000,
            description: "Test description for gas cost calculation.".to_string(),
        };
        assert_eq!(task_payload.calculate_content_gas_cost(), 42 * DESCRIPTION_GAS_COST_PER_BYTE);
    }

    #[test]
    fn test_comprehensive_ai_mining_workflow() {
        // Test complete workflow from task creation to answer validation

        // 1. Create a detailed task
        let task_description = "Computer Vision Task: Analyze the provided image and identify all visible objects, their positions, and relationships. Provide a structured response with object names, bounding box coordinates (if applicable), and scene description.";
        let task_payload = AIMiningPayload::PublishTask {
            task_id: Hash::new([1u8; 32]),
            reward_amount: 10_000_000_000,
            difficulty: DifficultyLevel::Beginner,
            deadline: 1000,
            description: task_description.to_string(),
        };
        assert!(task_payload.validate().is_ok());

        // 2. Submit detailed answer
        let answer_content = "Analysis Results:\n\nDetected Objects:\n1. Cat (center-left): Persian breed, sitting position, coordinates approximately (120,80,300,250)\n2. Plant pot (background-right): Terra cotta, containing green foliage, coordinates (400,50,500,200)\n3. Wooden table (bottom): Oak surface, partial view, supports the cat, coordinates (0,200,600,400)\n\nScene Description: Indoor setting, natural lighting from left side, domestic environment with pet and decorative elements. The cat appears alert and is the primary subject of the image.";

        let answer_hash = Hash::new(blake3::hash(answer_content.as_bytes()).into());
        let submit_payload = AIMiningPayload::SubmitAnswer {
            task_id: Hash::new([1u8; 32]),
            answer_content: answer_content.to_string(),
            answer_hash,
            stake_amount: 2_000_000_000,
        };
        assert!(submit_payload.validate().is_ok());

        // 3. Validate the answer
        let validate_payload = AIMiningPayload::ValidateAnswer {
            task_id: Hash::new([1u8; 32]),
            answer_id: Hash::new([3u8; 32]),
            validation_score: 88,
        };
        assert!(validate_payload.validate().is_ok());

        // 4. Verify gas costs are calculated correctly
        let answer_gas_cost = submit_payload.calculate_content_gas_cost();
        let expected_gas_cost = answer_content.len() as u64 * ANSWER_CONTENT_GAS_COST_PER_BYTE;
        assert_eq!(answer_gas_cost, expected_gas_cost);

        let description_gas_cost = task_payload.calculate_content_gas_cost();
        let expected_description_cost = task_description.len() as u64 * DESCRIPTION_GAS_COST_PER_BYTE;
        assert_eq!(description_gas_cost, expected_description_cost);
    }

    #[test]
    fn test_real_world_task_examples() {
        // Test various real-world AI mining task scenarios

        // Mathematics problem
        let math_task = AIMiningPayload::PublishTask {
            task_id: Hash::new([1u8; 32]),
            reward_amount: 8_000_000_000,
            difficulty: DifficultyLevel::Beginner,
            deadline: 2000,
            description: "Calculate the compound interest for $5000 invested at 4% annual rate for 3 years. Show formula and step-by-step calculation.".to_string(),
        };
        assert!(math_task.validate().is_ok());

        let math_answer = "Compound Interest Calculation:\n\nFormula: A = P(1 + r)^t\nWhere: P = $5000, r = 0.04, t = 3\n\nCalculation:\nA = 5000(1 + 0.04)^3\nA = 5000(1.04)^3\nA = 5000 × 1.124864\nA = $5624.32\n\nCompound Interest = A - P = $5624.32 - $5000.00 = $624.32";

        let math_submission = AIMiningPayload::SubmitAnswer {
            task_id: Hash::new([1u8; 32]),
            answer_content: math_answer.to_string(),
            answer_hash: Hash::new(blake3::hash(math_answer.as_bytes()).into()),
            stake_amount: 1_500_000_000,
        };
        assert!(math_submission.validate().is_ok());

        // Natural Language Processing task
        let nlp_task = AIMiningPayload::PublishTask {
            task_id: Hash::new([2u8; 32]),
            reward_amount: 12_000_000_000,
            difficulty: DifficultyLevel::Intermediate,
            deadline: 3000,
            description: "Sentiment analysis task: Analyze the emotional tone of the given text and classify it as positive, negative, or neutral. Provide confidence scores and reasoning for your classification.".to_string(),
        };
        assert!(nlp_task.validate().is_ok());

        let nlp_answer = "Sentiment Analysis Results:\n\nClassification: Positive\nConfidence Score: 87%\n\nReasoning:\n- Identified positive keywords: 'excellent', 'wonderful', 'impressed'\n- Emotional indicators: excitement, satisfaction\n- Linguistic patterns: enthusiastic tone, use of superlatives\n- Context analysis: overall message conveys happiness and recommendation\n\nSupporting evidence: The text contains 3 strong positive sentiment markers and no negative indicators.";

        let nlp_submission = AIMiningPayload::SubmitAnswer {
            task_id: Hash::new([2u8; 32]),
            answer_content: nlp_answer.to_string(),
            answer_hash: Hash::new(blake3::hash(nlp_answer.as_bytes()).into()),
            stake_amount: 2_500_000_000,
        };
        assert!(nlp_submission.validate().is_ok());

        // Verify all content meets length requirements
        assert!(math_answer.len() >= MIN_ANSWER_CONTENT_LENGTH);
        assert!(math_answer.len() <= MAX_ANSWER_CONTENT_LENGTH);
        assert!(nlp_answer.len() >= MIN_ANSWER_CONTENT_LENGTH);
        assert!(nlp_answer.len() <= MAX_ANSWER_CONTENT_LENGTH);
    }
}