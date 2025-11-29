use crate::{crypto::Hash, varuint::VarUint};
use primitive_types::U256;
use thiserror::Error;

// This type is used to easily switch between u64 and u128 as example
// And its easier to see where we use the block difficulty
// Difficulty is a value that represents the amount of work required to mine a block
// On tos, each difficulty point is a hash per second
pub type Difficulty = VarUint;
// Cumulative difficulty is the sum of all difficulties of all blocks in the chain
// It is used to determine which branch is the main chain in BlockDAG merging.
pub type CumulativeDifficulty = VarUint;

#[derive(Error, Debug)]
pub enum DifficultyError {
    #[error("Difficulty cannot be a value zero")]
    DifficultyCannotBeZero,
    #[error("Error while converting value to BigUint")]
    ErrorOnConversionBigUint,
}

// Verify the validity of a block difficulty against the current network difficulty
// All operations are done on U256 to avoid overflow
pub fn check_difficulty(hash: &Hash, difficulty: &Difficulty) -> Result<bool, DifficultyError> {
    let target = compute_difficulty_target(difficulty)?;
    Ok(check_difficulty_against_target(hash, &target))
}

// Compute the difficulty target from the difficulty value
// This can be used to keep the target in cache instead of recomputing it each time
pub fn compute_difficulty_target(difficulty: &Difficulty) -> Result<U256, DifficultyError> {
    let diff = difficulty.as_ref();
    if diff.is_zero() {
        return Err(DifficultyError::DifficultyCannotBeZero);
    }

    Ok(U256::max_value() / diff)
}

// Check if the hash is below the target difficulty
pub fn check_difficulty_against_target(hash: &Hash, target: &U256) -> bool {
    let hash_work = U256::from_big_endian(hash.as_bytes());
    hash_work <= *target
}

// Convert a hash to a difficulty value
// This is only used by miner
#[inline(always)]
pub fn difficulty_from_hash(hash: &Hash) -> Difficulty {
    (U256::max_value() / U256::from_big_endian(hash.as_bytes())).into()
}

/// Convert difficulty to compact bits representation
///
/// The bits field is a compact representation of the difficulty target.
/// Format: The first byte is the number of significant bytes, followed by
/// the first 3 significant bytes of the target (if applicable).
///
/// For simplicity, we store the leading zeros count (0-255) which represents
/// the difficulty level. Higher values = more leading zeros required = harder.
pub fn difficulty_to_bits(difficulty: &Difficulty) -> u32 {
    let diff_u256 = difficulty.as_ref();
    if diff_u256.is_zero() {
        return 0;
    }

    // Count leading zeros in the 256-bit difficulty
    // Each U256 limb is 64 bits, so we need to calculate carefully
    let leading_zeros = diff_u256.leading_zeros();

    // Store difficulty information in bits field:
    // - Upper 8 bits: leading zeros (0-255)
    // - Lower 24 bits: first 3 significant bytes of difficulty
    let shift = 232_u32.saturating_sub(leading_zeros);
    let significant = (*diff_u256 >> shift).low_u32() & 0x00FFFFFF;

    (leading_zeros << 24) | significant
}

/// Convert compact bits representation back to difficulty
///
/// Inverse of difficulty_to_bits. Reconstructs the approximate difficulty
/// from the compact representation.
pub fn bits_to_difficulty(bits: u32) -> Difficulty {
    if bits == 0 {
        return VarUint::from(0u64);
    }

    let leading_zeros = bits >> 24;
    let significant = bits & 0x00FFFFFF;

    // Reconstruct the U256 value
    let base = U256::from(significant);
    let shift = 232_u32.saturating_sub(leading_zeros);

    (base << shift).into()
}
