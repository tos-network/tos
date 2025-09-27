use crate::{
    crypto::{Hash, elgamal::CompressedPublicKey},
    serializer::{Serializer, Reader, ReaderError, Writer},
    ai_mining::{AIMiningPayload, DifficultyLevel}
};

// Implement Serializer for AIMiningPayload
impl Serializer for AIMiningPayload {
    fn write(&self, writer: &mut Writer) {
        match self {
            AIMiningPayload::PublishTask { task_id, reward_amount, difficulty, deadline, description } => {
                0u8.write(writer);
                task_id.write(writer);
                reward_amount.write(writer);
                difficulty.write(writer);
                deadline.write(writer);
                description.write(writer);
            }
            AIMiningPayload::SubmitAnswer { task_id, answer_content, answer_hash, stake_amount } => {
                1u8.write(writer);
                task_id.write(writer);
                answer_content.write(writer);
                answer_hash.write(writer);
                stake_amount.write(writer);
            }
            AIMiningPayload::ValidateAnswer { task_id, answer_id, validation_score } => {
                2u8.write(writer);
                task_id.write(writer);
                answer_id.write(writer);
                validation_score.write(writer);
            }
            AIMiningPayload::RegisterMiner { miner_address, registration_fee } => {
                3u8.write(writer);
                miner_address.write(writer);
                registration_fee.write(writer);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let variant = u8::read(reader)?;
        match variant {
            0 => Ok(AIMiningPayload::PublishTask {
                task_id: Hash::read(reader)?,
                reward_amount: u64::read(reader)?,
                difficulty: DifficultyLevel::read(reader)?,
                deadline: u64::read(reader)?,
                description: String::read(reader)?,
            }),
            1 => Ok(AIMiningPayload::SubmitAnswer {
                task_id: Hash::read(reader)?,
                answer_content: String::read(reader)?,
                answer_hash: Hash::read(reader)?,
                stake_amount: u64::read(reader)?,
            }),
            2 => Ok(AIMiningPayload::ValidateAnswer {
                task_id: Hash::read(reader)?,
                answer_id: Hash::read(reader)?,
                validation_score: u8::read(reader)?,
            }),
            3 => Ok(AIMiningPayload::RegisterMiner {
                miner_address: CompressedPublicKey::read(reader)?,
                registration_fee: u64::read(reader)?,
            }),
            _ => Err(ReaderError::InvalidValue)
        }
    }

    fn size(&self) -> usize {
        1 + match self {
            AIMiningPayload::PublishTask { task_id, reward_amount, difficulty, deadline, description } =>
                task_id.size() + reward_amount.size() + difficulty.size() + deadline.size() + description.size(),
            AIMiningPayload::SubmitAnswer { task_id, answer_content, answer_hash, stake_amount } =>
                task_id.size() + answer_content.size() + answer_hash.size() + stake_amount.size(),
            AIMiningPayload::ValidateAnswer { task_id, answer_id, validation_score } =>
                task_id.size() + answer_id.size() + validation_score.size(),
            AIMiningPayload::RegisterMiner { miner_address, registration_fee } =>
                miner_address.size() + registration_fee.size(),
        }
    }
}

impl Serializer for DifficultyLevel {
    fn write(&self, writer: &mut Writer) {
        match self {
            DifficultyLevel::Beginner => 0u8.write(writer),
            DifficultyLevel::Intermediate => 1u8.write(writer),
            DifficultyLevel::Advanced => 2u8.write(writer),
            DifficultyLevel::Expert => 3u8.write(writer),
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        match u8::read(reader)? {
            0 => Ok(DifficultyLevel::Beginner),
            1 => Ok(DifficultyLevel::Intermediate),
            2 => Ok(DifficultyLevel::Advanced),
            3 => Ok(DifficultyLevel::Expert),
            _ => Err(ReaderError::InvalidValue)
        }
    }

    fn size(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_mining_payload_serialization() {
        let payload = AIMiningPayload::PublishTask {
            task_id: Hash::new([1u8; 32]),
            reward_amount: 10_000_000_000,
            difficulty: DifficultyLevel::Beginner,
            deadline: 1000,
            description: "Test task description".to_string(),
        };

        let serialized = payload.to_bytes();
        let mut reader = Reader::new(&serialized);
        let deserialized = AIMiningPayload::read(&mut reader).unwrap();

        assert_eq!(payload, deserialized);
    }

    #[test]
    fn test_difficulty_serialization() {
        let levels = vec![
            DifficultyLevel::Beginner,
            DifficultyLevel::Intermediate,
            DifficultyLevel::Advanced,
            DifficultyLevel::Expert,
        ];

        for level in levels {
            let serialized = level.to_bytes();
            let mut reader = Reader::new(&serialized);
            let deserialized = DifficultyLevel::read(&mut reader).unwrap();

            assert_eq!(level, deserialized);
        }
    }
}