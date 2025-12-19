//! Serialization implementations for AI Mining types

use crate::{
    ai_mining::{
        AIMiner, AIMiningState, AIMiningStatistics, AIMiningTask, AccountReputation,
        SubmittedAnswer, TaskStatus, ValidationScore,
    },
    crypto::{elgamal::CompressedPublicKey, Hash},
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use std::collections::HashMap;

impl Serializer for AIMiningState {
    fn write(&self, writer: &mut Writer) {
        // Write the number of tasks
        (self.tasks.len() as u64).write(writer);
        // Write each task
        for (task_id, task) in &self.tasks {
            task_id.write(writer);
            task.write(writer);
        }

        // Write the number of miners
        (self.miners.len() as u64).write(writer);
        // Write each miner
        for (address, miner) in &self.miners {
            address.write(writer);
            miner.write(writer);
        }

        // Write account reputations
        (self.account_reputations.len() as u64).write(writer);
        for (address, reputation) in &self.account_reputations {
            address.write(writer);
            reputation.write(writer);
        }

        // Write statistics
        self.statistics.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // Read tasks
        let task_count = u64::read(reader)?;
        let mut tasks = HashMap::new();
        for _ in 0..task_count {
            let task_id = Hash::read(reader)?;
            let task = AIMiningTask::read(reader)?;
            tasks.insert(task_id, task);
        }

        // Read miners
        let miner_count = u64::read(reader)?;
        let mut miners = HashMap::new();
        for _ in 0..miner_count {
            let address = CompressedPublicKey::read(reader)?;
            let miner = AIMiner::read(reader)?;
            miners.insert(address, miner);
        }

        // Read reputations
        let reputation_count = u64::read(reader)?;
        let mut account_reputations = HashMap::new();
        for _ in 0..reputation_count {
            let address = CompressedPublicKey::read(reader)?;
            let reputation = AccountReputation::read(reader)?;
            account_reputations.insert(address, reputation);
        }

        // Read statistics
        let statistics = AIMiningStatistics::read(reader)?;

        Ok(AIMiningState {
            tasks,
            miners,
            account_reputations,
            statistics,
        })
    }

    fn size(&self) -> usize {
        8 + // task count
        self.tasks.iter().map(|(k, v)| k.size() + v.size()).sum::<usize>() +
        8 + // miner count
        self.miners.iter().map(|(k, v)| k.size() + v.size()).sum::<usize>() +
        8 + // reputation count
        self
            .account_reputations
            .iter()
            .map(|(k, v)| k.size() + v.size())
            .sum::<usize>() +
        self.statistics.size()
    }
}

impl Serializer for AIMiningStatistics {
    fn write(&self, writer: &mut Writer) {
        self.total_tasks.write(writer);
        self.active_tasks.write(writer);
        self.completed_tasks.write(writer);
        self.total_miners.write(writer);
        self.total_rewards_distributed.write(writer);
        self.total_staked.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(AIMiningStatistics {
            total_tasks: u64::read(reader)?,
            active_tasks: u64::read(reader)?,
            completed_tasks: u64::read(reader)?,
            total_miners: u64::read(reader)?,
            total_rewards_distributed: u64::read(reader)?,
            total_staked: u64::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        8 + 8 + 8 + 8 + 8 + 8 // 6 u64 fields
    }
}

impl Serializer for AIMiningTask {
    fn write(&self, writer: &mut Writer) {
        self.task_id.write(writer);
        self.publisher.write(writer);
        self.description.write(writer);
        self.reward_amount.write(writer);
        self.difficulty.write(writer);
        self.deadline.write(writer);
        self.status.write(writer);
        self.published_at.write(writer);

        // Write number of submitted answers
        (self.submitted_answers.len() as u64).write(writer);
        // Write each answer
        for answer in &self.submitted_answers {
            answer.write(writer);
        }

        self.rewards_processed.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let task_id = Hash::read(reader)?;
        let publisher = CompressedPublicKey::read(reader)?;
        let description = String::read(reader)?;
        let reward_amount = u64::read(reader)?;
        let difficulty = crate::ai_mining::DifficultyLevel::read(reader)?;
        let deadline = u64::read(reader)?;
        let status = TaskStatus::read(reader)?;
        let published_at = u64::read(reader)?;

        // Read submitted answers
        let answer_count = u64::read(reader)?;
        let mut submitted_answers = Vec::new();
        for _ in 0..answer_count {
            let answer = SubmittedAnswer::read(reader)?;
            submitted_answers.push(answer);
        }

        let rewards_processed = if reader.size() > 0 {
            bool::read(reader)?
        } else {
            false
        };

        Ok(AIMiningTask {
            task_id,
            publisher,
            description,
            reward_amount,
            difficulty,
            deadline,
            status,
            published_at,
            submitted_answers,
            rewards_processed,
        })
    }

    fn size(&self) -> usize {
        self.task_id.size() +
        self.publisher.size() +
        self.description.size() +
        8 + // reward_amount
        self.difficulty.size() +
        8 + // deadline
        self.status.size() +
        8 + // published_at
        8 + // answer count
        self.submitted_answers.iter().map(|v| v.size()).sum::<usize>() +
        1 // rewards_processed
    }
}

impl Serializer for AccountReputation {
    fn write(&self, writer: &mut Writer) {
        self.account.write(writer);
        self.created_at.write(writer);
        self.transaction_count.write(writer);
        self.stake_amount.write(writer);
        self.last_submission_time.write(writer);
        self.reputation_score.write(writer);
        self.total_rewards_earned.write(writer);
        self.successful_validations.write(writer);
        self.total_validations.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let account = CompressedPublicKey::read(reader)?;
        let created_at = u64::read(reader)?;
        let transaction_count = u64::read(reader)?;
        let stake_amount = u64::read(reader)?;
        let last_submission_time = u64::read(reader)?;
        let reputation_score = u64::read(reader)?;
        let total_rewards_earned = u64::read(reader)?;
        let successful_validations = u64::read(reader)?;
        let total_validations = u64::read(reader)?;

        Ok(AccountReputation {
            account,
            created_at,
            transaction_count,
            stake_amount,
            last_submission_time,
            reputation_score,
            total_rewards_earned,
            successful_validations,
            total_validations,
        })
    }

    fn size(&self) -> usize {
        self.account.size() + (8 * 8)
    }
}

impl Serializer for AIMiner {
    fn write(&self, writer: &mut Writer) {
        self.address.write(writer);
        self.registration_fee.write(writer);
        self.registered_at.write(writer);
        self.tasks_published.write(writer);
        self.answers_submitted.write(writer);
        self.validations_performed.write(writer);
        self.reputation.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(AIMiner {
            address: CompressedPublicKey::read(reader)?,
            registration_fee: u64::read(reader)?,
            registered_at: u64::read(reader)?,
            tasks_published: u32::read(reader)?,
            answers_submitted: u32::read(reader)?,
            validations_performed: u32::read(reader)?,
            reputation: u16::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.address.size() +
        8 + // registration_fee (u64)
        8 + // registered_at (u64)
        4 + // tasks_published (u32)
        4 + // answers_submitted (u32)
        4 + // validations_performed (u32)
        2 // reputation (u16)
    }
}

impl Serializer for TaskStatus {
    fn write(&self, writer: &mut Writer) {
        match self {
            TaskStatus::Active => 0u8.write(writer),
            TaskStatus::Expired => 1u8.write(writer),
            TaskStatus::Completed => 2u8.write(writer),
            TaskStatus::Cancelled => 3u8.write(writer),
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        match u8::read(reader)? {
            0 => Ok(TaskStatus::Active),
            1 => Ok(TaskStatus::Expired),
            2 => Ok(TaskStatus::Completed),
            3 => Ok(TaskStatus::Cancelled),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

impl Serializer for SubmittedAnswer {
    fn write(&self, writer: &mut Writer) {
        self.answer_id.write(writer);
        self.answer_content.write(writer);
        self.answer_hash.write(writer);
        self.submitter.write(writer);
        self.stake_amount.write(writer);

        // Write validation scores
        (self.validation_scores.len() as u64).write(writer);
        for score in &self.validation_scores {
            score.write(writer);
        }

        self.average_score.write(writer);
        self.submitted_at.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let answer_id = Hash::read(reader)?;
        let answer_content = String::read(reader)?;
        let answer_hash = Hash::read(reader)?;
        let submitter = CompressedPublicKey::read(reader)?;
        let stake_amount = u64::read(reader)?;

        // Read validation scores
        let score_count = u64::read(reader)?;
        let mut validation_scores = Vec::new();
        for _ in 0..score_count {
            let score = ValidationScore::read(reader)?;
            validation_scores.push(score);
        }

        let average_score = Option::<u8>::read(reader)?;
        let submitted_at = u64::read(reader)?;

        Ok(SubmittedAnswer {
            answer_id,
            answer_content,
            answer_hash,
            submitter,
            stake_amount,
            validation_scores,
            average_score,
            submitted_at,
        })
    }

    fn size(&self) -> usize {
        self.answer_id.size() +
        self.answer_content.size() +
        self.answer_hash.size() +
        self.submitter.size() +
        8 + // stake_amount
        8 + // score count
        self.validation_scores.iter().map(|v| v.size()).sum::<usize>() +
        self.average_score.size() +
        8 // submitted_at
    }
}

impl Serializer for ValidationScore {
    fn write(&self, writer: &mut Writer) {
        self.validator.write(writer);
        self.score.write(writer);
        self.validated_at.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(ValidationScore {
            validator: CompressedPublicKey::read(reader)?,
            score: u8::read(reader)?,
            validated_at: u64::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.validator.size() +
        1 + // score (u8)
        8 // validated_at (u64)
    }
}
