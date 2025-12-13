use crate::config::{get_hard_forks, MILLIS_PER_SECOND};
use anyhow::Result;
use tos_common::{
    api::daemon::HardFork,
    block::{Algorithm, BlockVersion},
    network::Network,
    transaction::TxVersion,
};

// Get the hard fork at a given height
pub fn get_hard_fork_at_height(network: &Network, height: u64) -> Option<&HardFork> {
    let mut hardfork: Option<&HardFork> = None;
    for conf in get_hard_forks(network) {
        if height >= conf.height {
            hardfork = Some(conf);
        } else {
            break;
        }
    }

    hardfork
}

// Get the version of the hard fork at a given height
// and returns true if there is a hard fork (version change) at that height
pub fn has_hard_fork_at_height(network: &Network, height: u64) -> (bool, BlockVersion) {
    match get_hard_fork_at_height(network, height) {
        Some(hard_fork) => (hard_fork.height == height, hard_fork.version),
        None => (false, BlockVersion::V0),
    }
}

// This function returns the block version at a given height
pub fn get_version_at_height(network: &Network, height: u64) -> BlockVersion {
    has_hard_fork_at_height(network, height).1
}

// This function returns the PoW algorithm at a given version
pub const fn get_pow_algorithm_for_version(version: BlockVersion) -> Algorithm {
    match version {
        BlockVersion::V0 => Algorithm::V1,
        _ => Algorithm::V2,
    }
}

// This function returns the block time target for a given version
// TIP-1: Unified block time of 3 seconds for all versions (except V0)
// V0: 60 seconds (kept for backward compatibility with genesis)
// V1/V2/V3: 3 seconds (unified target for optimal performance)
//
// Rationale:
// - 3 seconds provides optimal balance: 326 TPS, 78.3% mining window
// - Lower orphan rate (3-5%) vs 2s blocks (8-10%)
// - Better global miner participation and decentralization
// - Block reward automatically adjusts proportionally via get_block_reward()
pub const fn get_block_time_target_for_version(version: BlockVersion) -> u64 {
    match version {
        BlockVersion::V0 => 60 * MILLIS_PER_SECOND,
        BlockVersion::V1 | BlockVersion::V2 | BlockVersion::V3 => 3 * MILLIS_PER_SECOND, // TIP-1: Unified to 3 seconds
    }
}

// This function checks if a version is matching the requirements
// it split the version if it contains a `-` and only takes the first part
// to support our git commit hash
pub fn is_version_matching_requirement(version: &str, req: &str) -> Result<bool> {
    let r = semver::VersionReq::parse(req)?;
    let str_version = match version.split_once('-') {
        Some((v, _)) => v,
        None => version,
    };

    let v = semver::Version::parse(str_version)?;

    Ok(r.matches(&v))
}

// This function checks if a version is allowed at a given height
pub fn is_version_allowed_at_height(network: &Network, height: u64, version: &str) -> Result<bool> {
    for hard_fork in get_hard_forks(network) {
        if let Some(req) = hard_fork
            .version_requirement
            .filter(|_| hard_fork.height <= height)
        {
            let matches = is_version_matching_requirement(version, req)?;
            if !matches {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

// Verify if the BlockVersion is/was enabled at a given height
// Even if we are any version above the one requested, this function returns true
pub fn is_version_enabled_at_height(network: &Network, height: u64, version: BlockVersion) -> bool {
    for hard_fork in get_hard_forks(network) {
        if hard_fork.height <= height && hard_fork.version == version {
            return true;
        }
    }

    false
}

// This function checks if a transaction version is allowed in a block version
#[inline(always)]
pub const fn is_tx_version_allowed_in_block_version(
    tx_version: TxVersion,
    block_version: BlockVersion,
) -> bool {
    block_version.is_tx_version_allowed(tx_version)
}

#[cfg(test)]
mod tests {
    use tos_common::{block::BlockVersion, transaction::TxVersion};

    use super::*;

    #[test]
    fn test_version_matching_requirement() {
        assert_eq!(
            is_version_matching_requirement("1.0.0-abcdef", ">=1.0.0").unwrap(),
            true
        );
        assert_eq!(
            is_version_matching_requirement("1.0.0-999", ">=1.0.0").unwrap(),
            true
        );
        assert_eq!(
            is_version_matching_requirement("1.0.0-abcdef999", ">=1.0.0").unwrap(),
            true
        );
        assert_eq!(
            is_version_matching_requirement("1.0.0", ">=1.0.1").unwrap(),
            false
        );
        assert_eq!(
            is_version_matching_requirement("1.0.0", "<1.0.1").unwrap(),
            true
        );
        assert_eq!(
            is_version_matching_requirement("1.0.0", "<1.0.0").unwrap(),
            false
        );

        // Debug: Test specific versions
        println!("Testing version matching:");
        println!(
            "1.13.0 >= 1.13.0: {}",
            is_version_matching_requirement("1.13.0", ">=1.13.0").unwrap()
        );
        println!(
            "1.18.0- >= 1.13.0: {}",
            is_version_matching_requirement("1.18.0-", ">=1.13.0").unwrap()
        );
        println!(
            "1.18.0- >= 1.16.0: {}",
            is_version_matching_requirement("1.18.0-", ">=1.16.0").unwrap()
        );
    }

    #[test]
    fn test_current_software_version_hard_forks_requirements() {
        const VERSIONS: [&str; 3] = ["1.0.0", "1.0.0-abcdef", "1.0.0-abcdef999"];

        // At height 0, No version requires
        for version in VERSIONS {
            let allowed = is_version_allowed_at_height(&Network::Mainnet, 0, version).unwrap();
            println!("Version {} allowed at height 0: {}", version, allowed);
            assert!(allowed);
        }
    }

    #[test]
    fn test_has_hard_fork_at_height() {
        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 0);
        assert_eq!(hard_fork, true);
        assert_eq!(version, BlockVersion::V0);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 1);
        assert_eq!(hard_fork, false);
        assert_eq!(version, BlockVersion::V0);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 5);
        assert_eq!(hard_fork, true);
        assert_eq!(version, BlockVersion::V1);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 6);
        assert_eq!(hard_fork, false);
        assert_eq!(version, BlockVersion::V1);
    }

    #[test]
    fn test_get_version_at_height() {
        // Mainnet
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 0),
            BlockVersion::V2
        );
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 435_000),
            BlockVersion::V2
        );
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 2_000_000),
            BlockVersion::V2
        );

        // Testnet
        assert_eq!(
            get_version_at_height(&Network::Testnet, 0),
            BlockVersion::V0
        );
        assert_eq!(
            get_version_at_height(&Network::Testnet, 6),
            BlockVersion::V1
        );
        assert_eq!(
            get_version_at_height(&Network::Testnet, 10),
            BlockVersion::V2
        );
        assert_eq!(
            get_version_at_height(&Network::Testnet, 50),
            BlockVersion::V3
        );
    }

    #[test]
    fn test_get_pow_algorithm_for_version() {
        assert_eq!(
            get_pow_algorithm_for_version(BlockVersion::V0),
            Algorithm::V1
        );
        assert_eq!(
            get_pow_algorithm_for_version(BlockVersion::V1),
            Algorithm::V2
        );
    }

    #[test]
    fn test_is_tx_version_allowed_in_block_version() {
        // All block versions now support T0
        assert!(is_tx_version_allowed_in_block_version(
            TxVersion::T0,
            BlockVersion::V0
        ));
        assert!(is_tx_version_allowed_in_block_version(
            TxVersion::T0,
            BlockVersion::V1
        ));
        assert!(is_tx_version_allowed_in_block_version(
            TxVersion::T0,
            BlockVersion::V2
        ));
        assert!(is_tx_version_allowed_in_block_version(
            TxVersion::T0,
            BlockVersion::V3
        ));
    }

    #[test]
    fn test_version_enabled() {
        // Mainnet - V1 and V2 are enabled from height 0
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            0,
            BlockVersion::V0
        ));
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            0,
            BlockVersion::V1
        ));
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            0,
            BlockVersion::V2
        ));

        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            435_000,
            BlockVersion::V1
        ));
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            435_000,
            BlockVersion::V2
        ));
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            2_000_000,
            BlockVersion::V2
        ));

        // V3 is not yet enabled
        assert!(!is_version_enabled_at_height(
            &Network::Mainnet,
            2_000_000,
            BlockVersion::V3
        ));

        // Testnet
        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            0,
            BlockVersion::V0
        ));
        assert!(!is_version_enabled_at_height(
            &Network::Testnet,
            0,
            BlockVersion::V1
        ));
        assert!(!is_version_enabled_at_height(
            &Network::Testnet,
            0,
            BlockVersion::V2
        ));

        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            5,
            BlockVersion::V0
        ));
        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            5,
            BlockVersion::V1
        ));
        assert!(!is_version_enabled_at_height(
            &Network::Testnet,
            5,
            BlockVersion::V2
        ));

        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            10,
            BlockVersion::V0
        ));
        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            10,
            BlockVersion::V1
        ));
        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            10,
            BlockVersion::V2
        ));
    }

    #[test]
    fn test_get_block_time_target_for_version() {
        // V0 kept at 60s for genesis compatibility
        assert_eq!(
            get_block_time_target_for_version(BlockVersion::V0),
            60 * MILLIS_PER_SECOND
        );

        // TIP-1: All subsequent versions unified to 3 seconds
        assert_eq!(
            get_block_time_target_for_version(BlockVersion::V1),
            3 * MILLIS_PER_SECOND
        );
        assert_eq!(
            get_block_time_target_for_version(BlockVersion::V2),
            3 * MILLIS_PER_SECOND
        );
        assert_eq!(
            get_block_time_target_for_version(BlockVersion::V3),
            3 * MILLIS_PER_SECOND
        );
    }
}
