use tos_common::{difficulty::Difficulty, serializer::*, varuint::VarUint};

// All needed difficulty for a block
// Phase 2: Removed cumulative_difficulty field (use GHOSTDAG blue_work instead)
pub struct BlockDifficulty {
    pub difficulty: Difficulty,
    pub covariance: VarUint,
}

impl Serializer for BlockDifficulty {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let difficulty = Difficulty::read(reader)?;

        // Migration compatibility: Try to read cumulative_difficulty for V2 format
        // If present, skip it; if not, we're reading V3 format
        // We detect this by checking if there's enough data left for covariance

        // Try to read old cumulative_difficulty field if present (V2 format)
        // We do this by checking if there's enough remaining data for cumulative_difficulty + covariance
        // If so, skip the cumulative_difficulty (16 bytes) and read covariance
        let covariance = if reader.size() >= 16 + 1 {
            // 16 for u128, at least 1 for VarUint
            // Likely V2 format with cumulative_difficulty - skip it
            reader.skip(16)?;
            VarUint::read(reader)?
        } else {
            // V3 format without cumulative_difficulty
            VarUint::read(reader)?
        };

        Ok(Self {
            difficulty,
            covariance,
        })
    }

    fn write(&self, writer: &mut Writer) {
        // V3 format: Only write difficulty and covariance
        self.difficulty.write(writer);
        self.covariance.write(writer);
    }

    fn size(&self) -> usize {
        // V3 format size
        self.difficulty.size() + self.covariance.size()
    }
}
