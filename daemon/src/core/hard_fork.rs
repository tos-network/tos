use super::bps::OneBps;
use crate::config::get_hard_forks;
use anyhow::Result;
use tos_common::{
    api::daemon::HardFork, block::BlockVersion, network::Network, transaction::TxVersion,
};

// Get the hard fork at a given height
// VERSION UNIFICATION: With single Baseline version, this always returns the same hard fork
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
// VERSION UNIFICATION: Always returns Baseline
pub fn has_hard_fork_at_height(network: &Network, height: u64) -> (bool, BlockVersion) {
    match get_hard_fork_at_height(network, height) {
        Some(hard_fork) => (hard_fork.height == height, hard_fork.version),
        None => (false, BlockVersion::Baseline),
    }
}

// This function returns the block version at a given height
// VERSION UNIFICATION: Always returns Baseline
pub fn get_version_at_height(network: &Network, height: u64) -> BlockVersion {
    has_hard_fork_at_height(network, height).1
}

// VERSION UNIFICATION: PoW algorithm function removed.
// V2 algorithm is now the only algorithm, hardcoded in pow_hash().
// Callers should call header.get_pow_hash() directly without algorithm parameter.

// This function returns the block time target for a given version
//
// TIP-BPS: Elegant BPS Configuration System
// Uses the BPS (Blocks Per Second) configuration system for type-safe,
// compile-time calculation of all BPS-dependent parameters.
//
// VERSION UNIFICATION: All blocks use 1-second target (OneBps)
//
// Rationale:
// - OneBps (1 BPS) provides good balance between throughput and network convergence
// - All GHOSTDAG parameters (K=10, finality depth, etc.) automatically calculated
// - Block reward automatically adjusts proportionally via get_block_reward()
// - Future BPS changes only require updating the type alias (e.g., TwoBps)
pub const fn get_block_time_target_for_version(_version: BlockVersion) -> u64 {
    OneBps::target_time_per_block() // 1000ms (1 BPS)
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
        // VERSION UNIFICATION: Only one hard fork at height 0 with Baseline version
        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 0);
        assert_eq!(hard_fork, true);
        assert_eq!(version, BlockVersion::Baseline);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 1);
        assert_eq!(hard_fork, false);
        assert_eq!(version, BlockVersion::Baseline);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 10);
        assert_eq!(hard_fork, false);
        assert_eq!(version, BlockVersion::Baseline);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 100);
        assert_eq!(hard_fork, false);
        assert_eq!(version, BlockVersion::Baseline);
    }

    #[test]
    fn test_get_version_at_height() {
        // VERSION UNIFICATION: All heights return Baseline for all networks
        // Mainnet
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 0),
            BlockVersion::Baseline
        );
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 435_000),
            BlockVersion::Baseline
        );
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 2_000_000),
            BlockVersion::Baseline
        );

        // Testnet
        assert_eq!(
            get_version_at_height(&Network::Testnet, 0),
            BlockVersion::Baseline
        );
        assert_eq!(
            get_version_at_height(&Network::Testnet, 9),
            BlockVersion::Baseline
        );
        assert_eq!(
            get_version_at_height(&Network::Testnet, 10),
            BlockVersion::Baseline
        );
        assert_eq!(
            get_version_at_height(&Network::Testnet, 15),
            BlockVersion::Baseline
        );
    }

    // VERSION UNIFICATION: test_get_pow_algorithm_for_version removed
    // PoW algorithm is now hardcoded to V2 in pow_hash(), no function to test

    #[test]
    fn test_is_tx_version_allowed_in_block_version() {
        // VERSION UNIFICATION: Only Baseline version exists, supports T0
        assert!(is_tx_version_allowed_in_block_version(
            TxVersion::T0,
            BlockVersion::Baseline
        ));
    }

    #[test]
    fn test_version_enabled() {
        // VERSION UNIFICATION: Only Baseline is enabled from height 0
        // Mainnet
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            0,
            BlockVersion::Baseline
        ));
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            435_000,
            BlockVersion::Baseline
        ));
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            2_000_000,
            BlockVersion::Baseline
        ));

        // Testnet
        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            0,
            BlockVersion::Baseline
        ));
        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            9,
            BlockVersion::Baseline
        ));
        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            10,
            BlockVersion::Baseline
        ));
    }

    #[test]
    fn test_get_block_time_target_for_version() {
        use super::super::bps::OneBps;
        use crate::config::MILLIS_PER_SECOND;

        // VERSION UNIFICATION: All versions use OneBps configuration (1 second blocks)
        assert_eq!(
            get_block_time_target_for_version(BlockVersion::Baseline),
            OneBps::target_time_per_block()
        );

        // Verify OneBps is 1000ms (1 second)
        assert_eq!(OneBps::target_time_per_block(), 1000);
        assert_eq!(OneBps::target_time_per_block(), MILLIS_PER_SECOND);
    }
}
