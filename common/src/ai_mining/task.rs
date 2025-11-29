//! AI Mining task state management

use crate::{
    ai_mining::{AIMiningError, AIMiningResult, DifficultyLevel},
    crypto::{elgamal::CompressedPublicKey, Hash},
};
use serde::{Deserialize, Serialize};

/// Status of an AI mining task
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is active and accepting submissions
    Active,
    /// Task has expired (deadline passed)
    Expired,
    /// Task has been completed and validated
    Completed,
    /// Task has been cancelled by publisher
    Cancelled,
}

/// Represents an AI mining task in the blockchain state
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AIMiningTask {
    /// Unique identifier for this task
    pub task_id: Hash,
    /// Address of the task publisher
    pub publisher: CompressedPublicKey,
    /// Description or specification of the task
    pub description: String,
    /// Reward amount in nanoTOS
    pub reward_amount: u64,
    /// Task difficulty level
    pub difficulty: DifficultyLevel,
    /// Deadline for task completion (timestamp)
    pub deadline: u64,
    /// Current status of the task
    pub status: TaskStatus,
    /// Block height when task was published
    pub published_at: u64,
    /// List of submitted answers
    pub submitted_answers: Vec<SubmittedAnswer>,
    /// Whether rewards/reputation have been processed for this task
    #[serde(default)]
    pub rewards_processed: bool,
}

/// Represents a submitted answer to an AI mining task
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubmittedAnswer {
    /// Unique identifier for this answer
    pub answer_id: Hash,
    /// The actual answer content
    pub answer_content: String,
    /// Hash of the actual answer content
    pub answer_hash: Hash,
    /// Address of the submitter
    pub submitter: CompressedPublicKey,
    /// Stake amount in nanoTOS
    pub stake_amount: u64,
    /// Validation scores (0-100)
    pub validation_scores: Vec<ValidationScore>,
    /// Average validation score
    pub average_score: Option<u8>,
    /// Block height when answer was submitted
    pub submitted_at: u64,
}

/// Represents a validation score for a submitted answer
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValidationScore {
    /// Address of the validator
    pub validator: CompressedPublicKey,
    /// Score (0-100)
    pub score: u8,
    /// Block height when validation was submitted
    pub validated_at: u64,
}

/// Represents a registered AI miner
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AIMiner {
    /// Miner's address
    pub address: CompressedPublicKey,
    /// Registration fee paid
    pub registration_fee: u64,
    /// Block height when registered
    pub registered_at: u64,
    /// Total tasks published
    pub tasks_published: u32,
    /// Total answers submitted
    pub answers_submitted: u32,
    /// Total validations performed
    pub validations_performed: u32,
    /// Reputation score (0-1000)
    pub reputation: u16,
}

impl AIMiningTask {
    /// Create a new AI mining task
    pub fn new(
        task_id: Hash,
        publisher: CompressedPublicKey,
        description: String,
        reward_amount: u64,
        difficulty: DifficultyLevel,
        deadline: u64,
        published_at: u64,
    ) -> AIMiningResult<Self> {
        // Validate reward amount is within difficulty range
        let (min_reward, max_reward) = difficulty.reward_range();
        if reward_amount < min_reward || reward_amount > max_reward {
            return Err(AIMiningError::InvalidTaskConfig(format!(
                "Reward amount {reward_amount} is outside valid range [{min_reward}, {max_reward}] for difficulty {difficulty:?}"
            )));
        }

        Ok(Self {
            task_id,
            publisher,
            description,
            reward_amount,
            difficulty,
            deadline,
            status: TaskStatus::Active,
            published_at,
            submitted_answers: Vec::new(),
            rewards_processed: false,
        })
    }

    /// Check if task is expired based on current timestamp
    pub fn is_expired(&self, current_time: u64) -> bool {
        current_time > self.deadline
    }

    /// Add a submitted answer to this task
    pub fn add_answer(&mut self, answer: SubmittedAnswer) -> AIMiningResult<()> {
        if self.status != TaskStatus::Active {
            return Err(AIMiningError::ValidationFailed(
                "Task is not active".to_string(),
            ));
        }

        // Check for duplicate answer from same submitter
        if self
            .submitted_answers
            .iter()
            .any(|a| a.submitter == answer.submitter)
        {
            return Err(AIMiningError::ValidationFailed(
                "Submitter has already submitted an answer for this task".to_string(),
            ));
        }

        self.submitted_answers.push(answer);
        Ok(())
    }

    /// Get the best answer based on validation scores
    pub fn get_best_answer(&self) -> Option<&SubmittedAnswer> {
        // Use filter_map to avoid .unwrap() - semantically equivalent but safer
        self.submitted_answers
            .iter()
            .filter_map(|answer| answer.average_score.map(|score| (score, answer)))
            .max_by_key(|(score, _)| *score)
            .map(|(_, answer)| answer)
    }

    /// Mark task as completed if deadline has passed or best answer found
    pub fn update_status(&mut self, current_time: u64) {
        match self.status {
            TaskStatus::Active => {
                if self.is_expired(current_time) {
                    self.status = TaskStatus::Expired;
                } else if let Some(best_answer) = self.get_best_answer() {
                    // Mark as completed if we have a validated answer with good score
                    if best_answer.average_score.unwrap_or(0) >= 70 {
                        self.status = TaskStatus::Completed;
                    }
                }
            }
            _ => {} // No status change for other states
        }
    }
}

impl SubmittedAnswer {
    /// Create a new submitted answer
    pub fn new(
        answer_id: Hash,
        answer_content: String,
        answer_hash: Hash,
        submitter: CompressedPublicKey,
        stake_amount: u64,
        submitted_at: u64,
    ) -> Self {
        Self {
            answer_id,
            answer_content,
            answer_hash,
            submitter,
            stake_amount,
            validation_scores: Vec::new(),
            average_score: None,
            submitted_at,
        }
    }

    /// Add a validation score and recalculate average
    pub fn add_validation(&mut self, validation: ValidationScore) -> AIMiningResult<()> {
        // Check for duplicate validation from same validator
        if self
            .validation_scores
            .iter()
            .any(|v| v.validator == validation.validator)
        {
            return Err(AIMiningError::ValidationFailed(
                "Validator has already scored this answer".to_string(),
            ));
        }

        self.validation_scores.push(validation);
        self.recalculate_average();
        Ok(())
    }

    /// Recalculate the average validation score
    fn recalculate_average(&mut self) {
        if self.validation_scores.is_empty() {
            self.average_score = None;
        } else {
            let sum: u32 = self.validation_scores.iter().map(|v| v.score as u32).sum();
            self.average_score = Some((sum / self.validation_scores.len() as u32) as u8);
        }
    }
}

impl AIMiner {
    /// Create a new AI miner
    pub fn new(address: CompressedPublicKey, registration_fee: u64, registered_at: u64) -> Self {
        Self {
            address,
            registration_fee,
            registered_at,
            tasks_published: 0,
            answers_submitted: 0,
            validations_performed: 0,
            reputation: 500, // Start with neutral reputation
        }
    }

    /// Update reputation based on activity
    pub fn update_reputation(&mut self, activity_type: ReputationActivity, success: bool) {
        let change = match (activity_type, success) {
            (ReputationActivity::TaskPublish, true) => 5,
            (ReputationActivity::TaskPublish, false) => -10,
            (ReputationActivity::AnswerSubmit, true) => 10,
            (ReputationActivity::AnswerSubmit, false) => -5,
            (ReputationActivity::Validation, true) => 3,
            (ReputationActivity::Validation, false) => -2,
        };

        // Apply reputation change with bounds
        if change > 0 {
            self.reputation = (self.reputation + change as u16).min(1000);
        } else {
            self.reputation = (self.reputation as i32 + change).max(0) as u16;
        }
    }
}

/// Types of reputation-affecting activities
#[derive(Clone, Debug, PartialEq)]
pub enum ReputationActivity {
    TaskPublish,
    AnswerSubmit,
    Validation,
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let task_id = Hash::new([1u8; 32]);
        let publisher = create_test_pubkey([2u8; 32]);
        let task = AIMiningTask::new(
            task_id.clone(),
            publisher,
            "Test task".to_string(),
            10_000_000_000, // 10 TOS (within Beginner range)
            DifficultyLevel::Beginner,
            1000,
            100,
        )
        .unwrap();

        assert_eq!(task.task_id, task_id);
        assert_eq!(task.status, TaskStatus::Active);
        assert!(task.submitted_answers.is_empty());
    }

    #[test]
    fn test_invalid_reward_amount() {
        let task_id = Hash::new([1u8; 32]);
        let publisher = create_test_pubkey([2u8; 32]);
        let result = AIMiningTask::new(
            task_id,
            publisher,
            "Test task".to_string(),
            1_000_000, // Too low for any difficulty
            DifficultyLevel::Beginner,
            1000,
            100,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_answer_submission() {
        let mut task = create_test_task();
        let answer = create_test_answer();

        assert!(task.add_answer(answer).is_ok());
        assert_eq!(task.submitted_answers.len(), 1);
    }

    #[test]
    fn test_duplicate_answer_rejection() {
        let mut task = create_test_task();
        let answer1 = create_test_answer();
        let answer2 = create_test_answer(); // Same submitter

        assert!(task.add_answer(answer1).is_ok());
        assert!(task.add_answer(answer2).is_err());
    }

    #[test]
    fn test_task_expiration() {
        let mut task = create_test_task();
        assert!(!task.is_expired(500)); // Before deadline
        assert!(task.is_expired(1500)); // After deadline

        task.update_status(1500);
        assert_eq!(task.status, TaskStatus::Expired);
    }

    #[test]
    fn test_validation_scoring() {
        let mut answer = create_test_answer();
        let validation1 = ValidationScore {
            validator: create_test_pubkey([10u8; 32]),
            score: 80,
            validated_at: 200,
        };
        let validation2 = ValidationScore {
            validator: create_test_pubkey([11u8; 32]),
            score: 90,
            validated_at: 210,
        };

        assert!(answer.add_validation(validation1).is_ok());
        assert!(answer.add_validation(validation2).is_ok());
        assert_eq!(answer.average_score, Some(85)); // (80 + 90) / 2
    }

    #[test]
    fn test_miner_reputation() {
        let mut miner = AIMiner::new(create_test_pubkey([1u8; 32]), 1_000_000_000, 100);

        assert_eq!(miner.reputation, 500); // Initial neutral reputation

        miner.update_reputation(ReputationActivity::AnswerSubmit, true);
        assert_eq!(miner.reputation, 510);

        miner.update_reputation(ReputationActivity::AnswerSubmit, false);
        assert_eq!(miner.reputation, 505);
    }

    #[test]
    fn test_comprehensive_validation_workflow() {
        // Create a detailed task for image classification
        let task_description = "AI Image Classification Task: Classify the provided image as either 'cat' or 'dog'. Provide reasoning based on visible features such as ear shape, facial structure, body proportions, and distinctive characteristics. Minimum 50 characters required.";
        let mut task = create_test_task_with_description(task_description);

        // Submit a good answer
        let good_answer_content = "Classification: Cat. Reasoning: The image shows a feline with characteristic pointed triangular ears, almond-shaped eyes, prominent whiskers, compact facial structure, and typical cat body proportions. The fur pattern and overall morphology are distinctly feline.";
        let good_answer = create_test_answer_for_task(good_answer_content);

        // Submit a poor answer
        let poor_answer_content =
            "It's definitely a dog because I said so without any real analysis of the features.";
        let mut poor_answer = create_test_answer_for_task(poor_answer_content);
        poor_answer.submitter = create_test_pubkey([6u8; 32]); // Different submitter

        // Add both answers to task
        assert!(task.add_answer(good_answer.clone()).is_ok());
        assert!(task.add_answer(poor_answer.clone()).is_ok());

        // Multiple validators score the good answer highly
        let validator1 = create_test_pubkey([10u8; 32]);
        let validator2 = create_test_pubkey([11u8; 32]);
        let validator3 = create_test_pubkey([12u8; 32]);

        let good_validations = vec![
            ValidationScore {
                validator: validator1.clone(),
                score: 95,
                validated_at: 200,
            },
            ValidationScore {
                validator: validator2.clone(),
                score: 88,
                validated_at: 210,
            },
            ValidationScore {
                validator: validator3.clone(),
                score: 92,
                validated_at: 220,
            },
        ];

        // Apply validations to good answer
        for validation in good_validations {
            assert!(task.submitted_answers[0].add_validation(validation).is_ok());
        }

        // Multiple validators score the poor answer lowly
        let poor_validations = vec![
            ValidationScore {
                validator: validator1,
                score: 25,
                validated_at: 230,
            },
            ValidationScore {
                validator: validator2,
                score: 30,
                validated_at: 240,
            },
            ValidationScore {
                validator: validator3,
                score: 20,
                validated_at: 250,
            },
        ];

        // Apply validations to poor answer
        for validation in poor_validations {
            assert!(task.submitted_answers[1].add_validation(validation).is_ok());
        }

        // Verify scoring
        assert_eq!(task.submitted_answers[0].average_score, Some(91)); // (95+88+92)/3 = 91
        assert_eq!(task.submitted_answers[1].average_score, Some(25)); // (25+30+20)/3 = 25

        // Check best answer selection
        let best_answer = task.get_best_answer().unwrap();
        assert_eq!(best_answer.answer_content, good_answer_content);
        assert_eq!(best_answer.average_score, Some(91));

        // Update task status - should mark as completed due to high score
        task.update_status(500); // Before deadline
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[test]
    fn test_answer_validation_edge_cases() {
        let mut answer = create_test_answer();
        let validator = create_test_pubkey([10u8; 32]);

        // Test duplicate validation rejection
        let validation1 = ValidationScore {
            validator: validator.clone(),
            score: 80,
            validated_at: 200,
        };
        let validation2 = ValidationScore {
            validator,
            score: 90,
            validated_at: 210,
        };

        assert!(answer.add_validation(validation1).is_ok());
        assert!(answer.add_validation(validation2).is_err()); // Should fail - duplicate validator
    }

    #[test]
    fn test_task_description_and_answer_content_relationship() {
        // Test mathematical problem
        let math_task_desc = "Solve the following equation and show your work: What is the value of x in the equation 2x + 5 = 17? Provide step-by-step solution with explanations.";
        let math_answer = "Step 1: 2x + 5 = 17\nStep 2: 2x = 17 - 5\nStep 3: 2x = 12\nStep 4: x = 12/2\nStep 5: x = 6\nVerification: 2(6) + 5 = 12 + 5 = 17 ✓";

        let mut math_task = create_test_task_with_description(math_task_desc);
        let math_answer_submission = create_test_answer_for_task(math_answer);
        assert!(math_task.add_answer(math_answer_submission).is_ok());

        // Test creative writing task
        let creative_task_desc = "Write a short story (minimum 100 words) about a robot learning to paint. Include themes of creativity, learning, and artistic expression.";
        let creative_answer = "The small robot named Canvas had spent months observing human painters in the art studio. Its sensors recorded every brushstroke, every color mixture, every emotional expression translated to canvas. But when Canvas finally held a brush in its mechanical fingers, something unexpected happened. The programmed techniques felt hollow. Instead, Canvas began to paint its own interpretation of 'learning' - swirls of blue confusion mixed with golden moments of understanding, creating something entirely new and uniquely its own.";

        let mut creative_task = create_test_task_with_description(creative_task_desc);
        let creative_answer_submission = create_test_answer_for_task(creative_answer);
        assert!(creative_task.add_answer(creative_answer_submission).is_ok());

        // Verify content length requirements are met
        assert!(math_answer.len() >= 10); // Minimum answer length
        assert!(math_answer.len() <= 2048); // Maximum answer length
        assert!(creative_answer.len() >= 10);
        assert!(creative_answer.len() <= 2048);
    }

    #[test]
    fn test_validation_score_calculation_precision() {
        let mut answer = create_test_answer();

        // Test edge case with scores that don't divide evenly
        let validations = vec![
            ValidationScore {
                validator: create_test_pubkey([10u8; 32]),
                score: 84,
                validated_at: 200,
            },
            ValidationScore {
                validator: create_test_pubkey([11u8; 32]),
                score: 87,
                validated_at: 210,
            },
            ValidationScore {
                validator: create_test_pubkey([12u8; 32]),
                score: 89,
                validated_at: 220,
            },
        ];

        for validation in validations {
            assert!(answer.add_validation(validation).is_ok());
        }

        // (84 + 87 + 89) / 3 = 260 / 3 = 86.666... -> 86 (integer division)
        assert_eq!(answer.average_score, Some(86));
    }

    #[test]
    fn test_task_completion_threshold() {
        let mut task = create_test_task();
        let mut answer = create_test_answer();

        // Test threshold at exactly 70
        let validation = ValidationScore {
            validator: create_test_pubkey([10u8; 32]),
            score: 70,
            validated_at: 200,
        };
        assert!(answer.add_validation(validation).is_ok());
        assert!(task.add_answer(answer).is_ok());

        task.update_status(500); // Before deadline
        assert_eq!(task.status, TaskStatus::Completed); // Should complete at score 70

        // Test just below threshold
        let mut task2 = create_test_task();
        let mut answer2 = create_test_answer();
        answer2.submitter = create_test_pubkey([6u8; 32]); // Different submitter

        let validation2 = ValidationScore {
            validator: create_test_pubkey([11u8; 32]),
            score: 69,
            validated_at: 200,
        };
        assert!(answer2.add_validation(validation2).is_ok());
        assert!(task2.add_answer(answer2).is_ok());

        task2.update_status(500); // Before deadline
        assert_eq!(task2.status, TaskStatus::Active); // Should remain active at score 69
    }

    // Helper functions for tests
    fn create_test_pubkey(bytes: [u8; 32]) -> CompressedPublicKey {
        use curve25519_dalek::ristretto::CompressedRistretto;
        CompressedPublicKey::new(CompressedRistretto::from_slice(&bytes).unwrap())
    }

    fn create_test_task() -> AIMiningTask {
        AIMiningTask::new(
            Hash::new([1u8; 32]),
            create_test_pubkey([2u8; 32]),
            "Test task".to_string(),
            10_000_000_000,
            DifficultyLevel::Beginner,
            1000, // deadline
            100,  // published_at
        )
        .unwrap()
    }

    fn create_test_answer() -> SubmittedAnswer {
        let answer_content = "This is a test answer for image classification: The image shows a cat with distinctive features including whiskers, pointed ears, and feline body structure.";
        SubmittedAnswer::new(
            Hash::new([3u8; 32]),
            answer_content.to_string(),
            Hash::new(blake3::hash(answer_content.as_bytes()).into()),
            create_test_pubkey([5u8; 32]),
            1_000_000_000,
            150,
        )
    }

    fn create_test_task_with_description(description: &str) -> AIMiningTask {
        AIMiningTask::new(
            Hash::new([1u8; 32]),
            create_test_pubkey([2u8; 32]),
            description.to_string(),
            10_000_000_000,
            DifficultyLevel::Beginner,
            1000, // deadline
            100,  // published_at
        )
        .unwrap()
    }

    fn create_test_answer_for_task(answer_content: &str) -> SubmittedAnswer {
        SubmittedAnswer::new(
            Hash::new([3u8; 32]),
            answer_content.to_string(),
            Hash::new(blake3::hash(answer_content.as_bytes()).into()),
            create_test_pubkey([5u8; 32]),
            1_000_000_000,
            150,
        )
    }
}
