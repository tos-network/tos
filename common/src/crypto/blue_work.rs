// BlueWorkType - Cumulative blue work for GHOSTDAG fork choice
//
// Based on Kaspa's BlueWorkType (Uint192), but using U256 for simplicity.
// U256 provides more than enough range for cumulative work calculations.

use primitive_types::U256;
use crate::serializer::{Reader, ReaderError, Serializer, Writer};

/// Blue work type - cumulative proof-of-work for blue (selected) chain
///
/// This is a 256-bit unsigned integer used to track cumulative work
/// in the GHOSTDAG protocol. The chain tip with highest blue_work
/// is considered the "heaviest" and selected as the main chain.
///
/// Formula: blue_work = parent_blue_work + block_work
///
/// where block_work is derived from block difficulty.
pub type BlueWorkType = U256;

/// Trait extension for writing BlueWorkType
pub trait BlueWorkWriter {
    /// Write BlueWorkType to bytes (big-endian, 32 bytes)
    fn write_blue_work(&mut self, value: &BlueWorkType);
}

/// Trait extension for reading BlueWorkType
pub trait BlueWorkReader {
    /// Read BlueWorkType from bytes (big-endian, 32 bytes)
    fn read_blue_work(&mut self) -> Result<BlueWorkType, ReaderError>;
}

impl<'a> BlueWorkWriter for Writer<'a> {
    fn write_blue_work(&mut self, value: &BlueWorkType) {
        // U256 consists of 4 x u64 limbs in little-endian order
        // We need to convert to big-endian bytes for serialization
        let limbs = value.0;
        for limb in limbs.iter().rev() {
            self.write_u64(limb);
        }
    }
}

impl<'a> BlueWorkReader for Reader<'a> {
    fn read_blue_work(&mut self) -> Result<BlueWorkType, ReaderError> {
        // Read 4 x u64 limbs in big-endian order
        let mut limbs = [0u64; 4];
        for limb in limbs.iter_mut().rev() {
            *limb = self.read_u64()?;
        }
        Ok(U256(limbs))
    }
}

/// Implement Serializer trait for BlueWorkType
impl Serializer for BlueWorkType {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        reader.read_blue_work()
    }

    fn write(&self, writer: &mut Writer) {
        writer.write_blue_work(self);
    }

    fn size(&self) -> usize {
        32 // 4 x u64 = 32 bytes
    }
}

/// Calculate block work from difficulty bits
///
/// This converts the compact difficulty representation (bits)
/// into the actual work contribution for this block.
///
/// Formula: work = 2^256 / (target + 1)
///
/// where target is derived from bits (compact difficulty format)
pub fn calculate_block_work_from_bits(bits: u32) -> BlueWorkType {
    // Extract exponent and mantissa from compact bits format
    // Format: 0xMMMMMMEE where MM is mantissa, EE is exponent
    let exponent = (bits >> 24) as usize;
    let mantissa = bits & 0x00FFFFFF;

    if mantissa == 0 {
        return BlueWorkType::zero();
    }

    // Calculate target = mantissa * 2^(8 * (exponent - 3))
    let target = if exponent <= 3 {
        BlueWorkType::from(mantissa >> (8 * (3 - exponent)))
    } else {
        BlueWorkType::from(mantissa) << (8 * (exponent - 3))
    };

    // work = 2^256 / (target + 1)
    // Since we can't do 2^256 directly, we use max value
    if target.is_zero() {
        return BlueWorkType::max_value();
    }

    // Calculate: work = (2^256 - 1) / (target + 1) + 1
    let target_plus_one = target.saturating_add(BlueWorkType::one());
    BlueWorkType::max_value() / target_plus_one
}

/// Calculate block work from difficulty (floating point)
///
/// This is a simpler interface for calculating work from
/// difficulty expressed as a floating point number.
///
/// difficulty = max_target / current_target
/// work = 2^256 / (max_target / difficulty)
pub fn calculate_block_work_from_difficulty(difficulty: u64) -> BlueWorkType {
    if difficulty == 0 {
        return BlueWorkType::zero();
    }

    // Simplified: work ≈ difficulty
    // For accurate calculation, we'd need the max_target constant
    BlueWorkType::from(difficulty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blue_work_zero() {
        let work = BlueWorkType::zero();
        assert_eq!(work, BlueWorkType::from(0));
    }

    #[test]
    fn test_blue_work_addition() {
        let work1 = BlueWorkType::from(1000);
        let work2 = BlueWorkType::from(2000);
        let total = work1 + work2;
        assert_eq!(total, BlueWorkType::from(3000));
    }

    #[test]
    fn test_blue_work_comparison() {
        let work1 = BlueWorkType::from(1000);
        let work2 = BlueWorkType::from(2000);
        assert!(work1 < work2);
        assert!(work2 > work1);
    }

    #[test]
    fn test_blue_work_serialization() {
        use crate::serializer::Serializer;

        let work = BlueWorkType::from(0x123456789ABCDEF0u64);
        let bytes = work.to_bytes();
        let decoded = BlueWorkType::from_bytes(&bytes).unwrap();
        assert_eq!(work, decoded);
    }

    #[test]
    fn test_blue_work_large_value() {
        use crate::serializer::Serializer;

        // Test with a large value
        let work = BlueWorkType::from(u128::MAX);
        let bytes = work.to_bytes();

        assert_eq!(bytes.len(), 32); // 4 x u64 = 32 bytes

        let decoded = BlueWorkType::from_bytes(&bytes).unwrap();
        assert_eq!(work, decoded);
    }

    #[test]
    fn test_block_work_from_bits() {
        // Test with some example bits values
        let bits = 0x1d00ffff; // Bitcoin genesis block difficulty
        let work = calculate_block_work_from_bits(bits);
        assert!(work > BlueWorkType::zero());
    }

    #[test]
    fn test_block_work_from_difficulty() {
        let difficulty = 1000u64;
        let work = calculate_block_work_from_difficulty(difficulty);
        assert_eq!(work, BlueWorkType::from(1000));
    }
}
