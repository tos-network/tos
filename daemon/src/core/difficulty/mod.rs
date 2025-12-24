use log::trace;
use tos_common::{
    block::BlockVersion, difficulty::Difficulty, network::Network, time::TimestampMillis,
    varuint::VarUint,
};

use super::hard_fork::get_block_time_target_for_version;
use crate::config::{
    DEVNET_MINIMUM_HASHRATE, MAINNET_MINIMUM_HASHRATE, MILLIS_PER_SECOND, TESTNET_MINIMUM_HASHRATE,
};

mod v1;
mod v2;

// Kalman filter with unsigned integers only
// z: The observed value (latest hashrate calculated on current block time).
// x_est_prev: The previous hashrate estime.
// p_prev: The previous estimate covariance.
// Returns the new state estimate and covariance
fn kalman_filter(
    z: VarUint,
    x_est_prev: VarUint,
    p_prev: VarUint,
    shift: u64,
    left_shift: VarUint,
    process_noise_covar: VarUint,
) -> (VarUint, VarUint) {
    trace!("z: {}, x_est_prev: {}, p_prev: {}", z, x_est_prev, p_prev);
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

    trace!(
        "x_est_new: {}, p pred: {}, noise covar: {}, p_prev: {}, k: {}",
        x_est_new,
        p_pred,
        process_noise_covar,
        p_prev,
        k
    );
    let p_new = ((left_shift - k) * p_pred) >> shift;

    // Scale down
    x_est_new >>= shift;

    (x_est_new, p_new)
}

// Calculate the required difficulty for the next block based on the solve time of the previous block
// We are using a Kalman filter to estimate the hashrate and adjust the difficulty
// Nobunaga and all future versions use V2 algorithm
pub fn calculate_difficulty(
    parent_timestamp: TimestampMillis,
    timestamp: TimestampMillis,
    previous_difficulty: Difficulty,
    p: VarUint,
    minimum_difficulty: Difficulty,
    version: BlockVersion,
) -> (Difficulty, VarUint) {
    let solve_time = (timestamp - parent_timestamp).max(1);

    let block_time_target = get_block_time_target_for_version(version);
    // All versions use V2 algorithm
    v2::calculate_difficulty(
        solve_time,
        previous_difficulty,
        p,
        minimum_difficulty,
        block_time_target,
    )
}

// Get the process noise covariance based on the version
// It is used by first blocks on a new version
// All versions use V2 covariance
pub fn get_covariance_p(_version: BlockVersion) -> VarUint {
    v2::P
}

// Get the difficulty based on the hashrate and block time target
// NOTE: The caller must ensure that the block time provided is in milliseconds
pub const fn get_difficulty_with_target(hashrate: u64, block_time_target: u64) -> Difficulty {
    Difficulty::from_u64(hashrate * block_time_target / MILLIS_PER_SECOND)
}

// Get minimum difficulty based on the network
// Each network has different minimum hashrate to balance security vs usability:
// - Mainnet: 100 KH/s (difficulty 300,000) - prevents block spam at launch
// - Testnet: 10 KH/s (difficulty 30,000) - balanced for multi-miner testing
// - Devnet: 1 KH/s (difficulty 3,000) - low for single developer testing
pub const fn get_minimum_difficulty(network: &Network, version: BlockVersion) -> Difficulty {
    let hashrate = match network {
        Network::Mainnet => MAINNET_MINIMUM_HASHRATE,
        Network::Testnet | Network::Stagenet => TESTNET_MINIMUM_HASHRATE,
        Network::Devnet => DEVNET_MINIMUM_HASHRATE,
    };

    let block_time_target = get_block_time_target_for_version(version);
    get_difficulty_with_target(hashrate, block_time_target)
}

// Get minimum difficulty at hard fork
// Nobunaga is the only version, returns appropriate difficulty
// Uses testnet hashrate for mainnet hard fork to allow gradual difficulty increase
pub const fn get_difficulty_at_hard_fork(
    network: &Network,
    version: BlockVersion,
) -> Option<Difficulty> {
    let hashrate = match network {
        Network::Mainnet => match version {
            // Use testnet hashrate for hard fork to allow gradual ramp-up
            BlockVersion::Nobunaga => TESTNET_MINIMUM_HASHRATE,
            // Future versions would be added here
        },
        _ => return None,
    };

    let block_time_target = get_block_time_target_for_version(version);
    Some(get_difficulty_with_target(hashrate, block_time_target))
}

#[cfg(test)]
mod tests {
    use crate::config::{GIGA_HASH, HASH, KILO_HASH, MEGA_HASH};
    use tos_common::utils::format_hashrate;

    use super::*;

    #[test]
    fn test_difficulty_at_hard_fork() {
        // Nobunaga uses 3s blocks
        // Hard fork uses testnet hashrate (10 KH/s) for gradual ramp-up
        // 10 KH/s * 3s = 10,000 * 3,000 / 1000 = 30,000
        assert_eq!(
            get_difficulty_at_hard_fork(&Network::Mainnet, BlockVersion::Nobunaga).unwrap(),
            Difficulty::from_u64(3 * TESTNET_MINIMUM_HASHRATE)
        );

        // testnet returns None for all versions
        assert!(get_difficulty_at_hard_fork(&Network::Testnet, BlockVersion::Nobunaga).is_none());
    }

    #[test]
    fn test_minimum_difficulty_per_network() {
        // Mainnet: 100 KH/s * 3s = 300,000
        assert_eq!(
            get_minimum_difficulty(&Network::Mainnet, BlockVersion::Nobunaga),
            Difficulty::from_u64(300_000)
        );

        // Testnet: 10 KH/s * 3s = 30,000
        assert_eq!(
            get_minimum_difficulty(&Network::Testnet, BlockVersion::Nobunaga),
            Difficulty::from_u64(30_000)
        );

        // Devnet: 1 KH/s * 3s = 3,000
        assert_eq!(
            get_minimum_difficulty(&Network::Devnet, BlockVersion::Nobunaga),
            Difficulty::from_u64(3_000)
        );
    }

    #[test]
    fn test_const_hashrate_format() {
        assert_eq!(format_hashrate(HASH as f64), "1.00 H/s");
        assert_eq!(format_hashrate(KILO_HASH as f64), "1.00 KH/s");
        assert_eq!(format_hashrate(MEGA_HASH as f64), "1.00 MH/s");
        assert_eq!(format_hashrate(GIGA_HASH as f64), "1.00 GH/s");
    }
}
