// TOS Difficulty Adjustment Algorithm (DAA)
//
// TIP-BPS Integration:
// The DAA works in conjunction with the BPS (Blocks Per Second) system to maintain
// target block production rate:
//
// 1. BPS System: Defines target_time_per_block (e.g., 1000ms for OneBps)
// 2. DAA: Measures actual block times and adjusts difficulty
// 3. Feedback Loop: difficulty up -> blocks slower, difficulty down -> blocks faster
//
// Key Principle: BPS sets the *target*, DAA adjusts *difficulty* to achieve it.
// No minimum hashrate hack needed - DAA naturally converges to target BPS.

use log::trace;
use tos_common::{
    block::BlockVersion,
    difficulty::Difficulty,
    network::Network,
    time::TimestampMillis,
    varuint::VarUint
};

use crate::config::{
    MINIMUM_HASHRATE,
    MILLIS_PER_SECOND
};
use super::hard_fork::get_block_time_target_for_version;

mod v1;
mod v2;

// Kalman filter with unsigned integers only
// z: The observed value (latest hashrate calculated on current block time).
// x_est_prev: The previous hashrate estime.
// p_prev: The previous estimate covariance.
// Returns the new state estimate and covariance
fn kalman_filter(z: VarUint, x_est_prev: VarUint, p_prev: VarUint, shift: u64, left_shift: VarUint, process_noise_covar: VarUint) -> (VarUint, VarUint) {
    if log::log_enabled!(log::Level::Trace) {
        trace!("z: {}, x_est_prev: {}, p_prev: {}", z, x_est_prev, p_prev);
    }
    // Scale up
    let z = z * left_shift;
    let r = z * 2;
    let x_est_prev = x_est_prev * left_shift;

    // Prediction step
    let p_pred = ((x_est_prev * process_noise_covar) >> shift) + p_prev;

    // Update step
    let k = (p_pred << shift) / (p_pred + r + VarUint::one());

    // Ensure positive numbers only
    let mut x_est_new = if z >= x_est_prev {
        x_est_prev + ((k * (z - x_est_prev)) >> shift)
    } else {
        x_est_prev - ((k * (x_est_prev - z)) >> shift)
    };

    if log::log_enabled!(log::Level::Trace) {
        trace!("x_est_new: {}, p pred: {}, noise covar: {}, p_prev: {}, k: {}", x_est_new, p_pred, process_noise_covar, p_prev, k);
    }
    let p_new = ((left_shift - k) * p_pred) >> shift;

    // Scale down
    x_est_new >>= shift;

    (x_est_new, p_new)
}

// Calculate the required difficulty for the next block based on the solve time of the previous block
// We are using a Kalman filter to estimate the hashrate and adjust the difficulty
// This function will determine which algorithm to use based on the version
pub fn calculate_difficulty(parent_timestamp: TimestampMillis, timestamp: TimestampMillis, previous_difficulty: Difficulty, p: VarUint, minimum_difficulty: Difficulty, version: BlockVersion) -> (Difficulty, VarUint) {
    let solve_time = (timestamp - parent_timestamp).max(1);

    let block_time_target = get_block_time_target_for_version(version);
    match version {
        BlockVersion::V0 => v1::calculate_difficulty(solve_time, previous_difficulty, p, minimum_difficulty, block_time_target),
        _ => v2::calculate_difficulty(solve_time, previous_difficulty, p, minimum_difficulty, block_time_target),
    }
}

// Get the process noise covariance based on the version
// It is used by first blocks on a new version
pub fn get_covariance_p(version: BlockVersion) -> VarUint {
    match version {
        BlockVersion::V0 => v1::P,
        _ => v2::P
    }
}

// Get the difficulty based on the hashrate and block time target
// NOTE: The caller must ensure that the block time provided is in milliseconds
pub const fn get_difficulty_with_target(hashrate: u64, block_time_target: u64) -> Difficulty {
    Difficulty::from_u64(hashrate * block_time_target / MILLIS_PER_SECOND)
}

// Get minimum difficulty based on the network
// All networks use the same minimum hashrate (200 H/s) for consistency
// This allows solo mining on single-threaded CPUs and ensures test environments
// match production behavior
pub const fn get_minimum_difficulty(_network: &Network, version: BlockVersion) -> Difficulty {
    let block_time_target = get_block_time_target_for_version(version);
    get_difficulty_with_target(MINIMUM_HASHRATE, block_time_target)
}

// Get minimum difficulty at hard fork
// Only mainnet has hard fork difficulty adjustments
// All networks use the same MINIMUM_HASHRATE for consistency
pub const fn get_difficulty_at_hard_fork(network: &Network, version: BlockVersion) -> Option<Difficulty> {
    match network {
        Network::Mainnet => match version {
            BlockVersion::V0 | BlockVersion::V1 | BlockVersion::V2 => {
                let block_time_target = get_block_time_target_for_version(version);
                Some(get_difficulty_with_target(MINIMUM_HASHRATE, block_time_target))
            },
            BlockVersion::V3 => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use tos_common::utils::format_hashrate;
    use crate::config::{HASH, KILO_HASH, MEGA_HASH, GIGA_HASH};

    use super::*;

    #[test]
    fn test_difficulty_at_hard_fork() {
        // 200 H/s for V0 with 60s target = 200 * 60,000 / 1000 = 12,000
        assert_eq!(get_difficulty_at_hard_fork(&Network::Mainnet, BlockVersion::V0).unwrap(), Difficulty::from_u64(MINIMUM_HASHRATE * 60));

        // TIP-1 deprecated: V1/V2/V3 use 1s blocks
        // 200 H/s for V2 with 1s target = 200 * 1,000 / 1000 = 200
        assert_eq!(get_difficulty_at_hard_fork(&Network::Mainnet, BlockVersion::V2).unwrap(), Difficulty::from_u64(1 * MINIMUM_HASHRATE));

        // testnet returns None for all versions
        for version in [BlockVersion::V0, BlockVersion::V1, BlockVersion::V2, BlockVersion::V3] {
            assert!(get_difficulty_at_hard_fork(&Network::Testnet, version).is_none());
        }
    }

    #[test]
    fn test_const_hashrate_format() {
        assert_eq!(format_hashrate(HASH as f64), "1.00 H/s");
        assert_eq!(format_hashrate(KILO_HASH as f64), "1.00 KH/s");
        assert_eq!(format_hashrate(MEGA_HASH as f64), "1.00 MH/s");
        assert_eq!(format_hashrate(GIGA_HASH as f64), "1.00 GH/s");
    }
}