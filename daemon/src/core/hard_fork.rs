use crate::config::{get_chain_tips, get_hard_forks, MILLIS_PER_SECOND};
use anyhow::Result;
use tos_common::{
    api::daemon::{ForkCondition, HardFork, TosHardfork},
    block::{Algorithm, BlockVersion},
    network::Network,
    transaction::TxVersion,
};

// Get the hard fork at a given height (for Block-based conditions only)
// For forks with Timestamp/TCD conditions, use get_activated_hard_fork() instead
pub fn get_hard_fork_at_height(network: &Network, height: u64) -> Option<&HardFork> {
    let mut hardfork: Option<&HardFork> = None;
    for conf in get_hard_forks(network) {
        // Only consider Block-based conditions for height lookup
        if let Some(activation_height) = conf.activation_height() {
            if height >= activation_height {
                hardfork = Some(conf);
            } else {
                break;
            }
        }
    }

    hardfork
}

// Get the version of the hard fork at a given height
// and returns true if there is a hard fork (version change) at that height
pub fn has_hard_fork_at_height(network: &Network, height: u64) -> (bool, BlockVersion) {
    match get_hard_fork_at_height(network, height) {
        Some(hard_fork) => {
            let is_exact = hard_fork.activation_height() == Some(height);
            (is_exact, hard_fork.version)
        }
        None => (false, BlockVersion::Nobunaga),
    }
}

// This function returns the block version at a given height
pub fn get_version_at_height(network: &Network, height: u64) -> BlockVersion {
    has_hard_fork_at_height(network, height).1
}

// This function returns the PoW algorithm at a given version
// All versions now use Algorithm::V3 (GPU/ASIC-friendly)
pub const fn get_pow_algorithm_for_version(_version: BlockVersion) -> Algorithm {
    Algorithm::V3
}

// This function returns the block time target for a given version
// TIP-1: Unified block time of 3 seconds for all versions
//
// TPS Analysis:
// - Theoretical max: 3,333 TPS (MAX_TXS_PER_BLOCK=10,000 / 3s)
// - Realistic with optimizations: 1,000-2,000 TPS
// - Single-threaded baseline: 100-200 TPS
//
// Rationale:
// - 3 seconds balances throughput vs orphan rate (3-5%)
// - Lower orphan rate vs 2s blocks (8-10%) or 1s blocks (15-20%)
// - Better global miner participation and decentralization
// - Block reward automatically adjusts proportionally via get_block_reward()
pub const fn get_block_time_target_for_version(_version: BlockVersion) -> u64 {
    3 * MILLIS_PER_SECOND
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
// Only considers Block-based activation conditions
pub fn is_version_allowed_at_height(network: &Network, height: u64, version: &str) -> Result<bool> {
    for hard_fork in get_hard_forks(network) {
        // Check if this fork is activated at the given height
        let is_activated = hard_fork
            .activation_height()
            .map(|h| height >= h)
            .unwrap_or(false);

        if let Some(req) = hard_fork.version_requirement.filter(|_| is_activated) {
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
// Only considers Block-based activation conditions
pub fn is_version_enabled_at_height(network: &Network, height: u64, version: BlockVersion) -> bool {
    for hard_fork in get_hard_forks(network) {
        let is_activated = hard_fork
            .activation_height()
            .map(|h| height >= h)
            .unwrap_or(false);

        if is_activated && hard_fork.version == version {
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

// ============================================================================
// TIP Activation Mechanism - Extended Functions
// ============================================================================
// These functions support multiple activation conditions:
// - Block height (deterministic)
// - Timestamp (time-based)
// - TCD (Threshold Cumulative Difficulty, hashrate-dependent)
// - Never (disabled)

/// Get the activated hard fork given the full blockchain state
///
/// This function checks all activation conditions (Block, Timestamp, TCD, Never)
/// to determine the currently active hard fork.
///
/// # Arguments
/// * `network` - The network type (Mainnet, Testnet, Devnet)
/// * `height` - Current block height
/// * `timestamp` - Current block timestamp in milliseconds
/// * `cumulative_difficulty` - Current chain cumulative difficulty
///
/// # Returns
/// The activated `HardFork` if any conditions are met
pub fn get_activated_hard_fork(
    network: &Network,
    height: u64,
    timestamp: u64,
    cumulative_difficulty: u64,
) -> Option<&'static HardFork> {
    let mut activated: Option<&HardFork> = None;

    for hard_fork in get_hard_forks(network) {
        if hard_fork.is_activated(height, timestamp, cumulative_difficulty) {
            activated = Some(hard_fork);
        } else {
            // Hard forks are ordered by activation, so if one isn't activated,
            // later ones won't be either (unless they have different conditions)
            // We continue checking in case a later fork with different conditions is active
        }
    }

    activated
}

/// Get the block version based on full blockchain state
///
/// Unlike `get_version_at_height`, this function considers all activation
/// conditions including timestamp and cumulative difficulty.
///
/// # Arguments
/// * `network` - The network type
/// * `height` - Current block height
/// * `timestamp` - Current block timestamp in milliseconds
/// * `cumulative_difficulty` - Current chain cumulative difficulty
///
/// # Returns
/// The `BlockVersion` that should be used
pub fn get_version_for_state(
    network: &Network,
    height: u64,
    timestamp: u64,
    cumulative_difficulty: u64,
) -> BlockVersion {
    match get_activated_hard_fork(network, height, timestamp, cumulative_difficulty) {
        Some(hard_fork) => hard_fork.version,
        None => BlockVersion::Nobunaga, // Default to genesis version
    }
}

/// Check if a specific hard fork is activated given the blockchain state
///
/// # Arguments
/// * `hard_fork` - The hard fork to check
/// * `height` - Current block height
/// * `timestamp` - Current block timestamp in milliseconds
/// * `cumulative_difficulty` - Current chain cumulative difficulty
///
/// # Returns
/// `true` if the hard fork is activated, `false` otherwise
pub fn is_hard_fork_activated(
    hard_fork: &HardFork,
    height: u64,
    timestamp: u64,
    cumulative_difficulty: u64,
) -> bool {
    hard_fork.is_activated(height, timestamp, cumulative_difficulty)
}

/// Check if any hard fork activates at the exact given state
///
/// This is useful to detect when a hard fork transition occurs.
///
/// # Arguments
/// * `network` - The network type
/// * `height` - Current block height
/// * `timestamp` - Current block timestamp in milliseconds
/// * `cumulative_difficulty` - Current chain cumulative difficulty
///
/// # Returns
/// `(bool, BlockVersion)` - Whether a fork activates at this point and the version
pub fn has_hard_fork_at_state(
    network: &Network,
    height: u64,
    timestamp: u64,
    cumulative_difficulty: u64,
) -> (bool, BlockVersion) {
    for hard_fork in get_hard_forks(network) {
        let is_exact_activation = match hard_fork.condition {
            ForkCondition::Block(activation_height) => height == activation_height,
            ForkCondition::Timestamp(activation_ts) => {
                // For timestamp, check if this is the first block at or after the timestamp
                timestamp >= activation_ts
            }
            ForkCondition::TCD(threshold) => {
                // For TCD, check if we just crossed the threshold
                cumulative_difficulty >= threshold
            }
            ForkCondition::Never => false,
        };

        if is_exact_activation {
            return (true, hard_fork.version);
        }
    }

    (false, BlockVersion::Nobunaga)
}

/// Get all hard forks that would be activated by a given timestamp
///
/// Useful for predicting future fork activations based on time.
///
/// # Arguments
/// * `network` - The network type
/// * `timestamp` - Target timestamp in milliseconds
///
/// # Returns
/// Vector of hard forks that will be activated by the given timestamp
pub fn get_forks_by_timestamp(network: &Network, timestamp: u64) -> Vec<&'static HardFork> {
    get_hard_forks(network)
        .iter()
        .filter(|hf| matches!(hf.condition, ForkCondition::Timestamp(ts) if timestamp >= ts))
        .collect()
}

/// Get all hard forks that would be activated by a given cumulative difficulty
///
/// Useful for predicting fork activations based on network hashrate.
///
/// # Arguments
/// * `network` - The network type
/// * `cumulative_difficulty` - Target cumulative difficulty
///
/// # Returns
/// Vector of hard forks that will be activated by the given difficulty
pub fn get_forks_by_tcd(network: &Network, cumulative_difficulty: u64) -> Vec<&'static HardFork> {
    get_hard_forks(network)
        .iter()
        .filter(|hf| {
            matches!(hf.condition, ForkCondition::TCD(threshold) if cumulative_difficulty >= threshold)
        })
        .collect()
}

/// Get all disabled (Never) hard forks
///
/// Returns hard forks that are configured but disabled.
///
/// # Arguments
/// * `network` - The network type
///
/// # Returns
/// Vector of disabled hard forks
pub fn get_disabled_forks(network: &Network) -> Vec<&'static HardFork> {
    get_hard_forks(network)
        .iter()
        .filter(|hf| hf.is_disabled())
        .collect()
}

/// Describe the activation condition for a hard fork
///
/// Returns a human-readable description of when the fork will activate.
///
/// # Arguments
/// * `hard_fork` - The hard fork to describe
///
/// # Returns
/// A string describing the activation condition
pub fn describe_fork_activation(hard_fork: &HardFork) -> String {
    match hard_fork.condition {
        ForkCondition::Block(height) => {
            format!("Activates at block height {}", height)
        }
        ForkCondition::Timestamp(ts) => {
            // Convert milliseconds to human-readable format
            let seconds = ts / 1000;
            format!(
                "Activates at timestamp {} (Unix epoch: {} seconds)",
                ts, seconds
            )
        }
        ForkCondition::TCD(threshold) => {
            format!("Activates when cumulative difficulty reaches {}", threshold)
        }
        ForkCondition::Never => "Never activates (disabled)".to_string(),
    }
}

// ============================================================================
// TIP (TOS Improvement Proposal) Activation Functions
// ============================================================================
// These functions provide convenient access to TIP activation status.
// Each TIP can be independently activated using its own ForkCondition.

/// Check if a specific TIP is active at a given blockchain state
///
/// # Arguments
/// * `network` - The network type (Mainnet, Testnet, Devnet)
/// * `hardfork` - The TIP to check
/// * `height` - Current block height
/// * `timestamp` - Current block timestamp in milliseconds
/// * `cumulative_difficulty` - Current chain cumulative difficulty
///
/// # Returns
/// `true` if the TIP is active, `false` otherwise
pub fn is_tip_active(
    network: &Network,
    hardfork: TosHardfork,
    height: u64,
    timestamp: u64,
    cumulative_difficulty: u64,
) -> bool {
    get_chain_tips(network).is_active(hardfork, height, timestamp, cumulative_difficulty)
}

/// Check if a specific TIP is active at a given block height
///
/// This only works for Block-based activation conditions.
/// For Timestamp or TCD conditions, use `is_tip_active` instead.
///
/// # Arguments
/// * `network` - The network type
/// * `hardfork` - The TIP to check
/// * `height` - Current block height
///
/// # Returns
/// `true` if the TIP is active at the given height
pub fn is_tip_active_at_height(network: &Network, hardfork: TosHardfork, height: u64) -> bool {
    get_chain_tips(network).is_active_at_height(hardfork, height)
}

/// Get the activation height for a specific TIP
///
/// Only returns a value if the TIP uses Block-based activation.
///
/// # Arguments
/// * `network` - The network type
/// * `hardfork` - The TIP to query
///
/// # Returns
/// The activation height if Block-based, None otherwise
pub fn get_tip_activation_height(network: &Network, hardfork: TosHardfork) -> Option<u64> {
    get_chain_tips(network).activation_height(hardfork)
}

/// Get the ForkCondition for a specific TIP
///
/// # Arguments
/// * `network` - The network type
/// * `hardfork` - The TIP to query
///
/// # Returns
/// The ForkCondition for this TIP (Never if not configured)
pub fn get_tip_condition(network: &Network, hardfork: TosHardfork) -> ForkCondition {
    get_chain_tips(network).fork(hardfork)
}

/// Get all active TIPs at a given blockchain state
///
/// # Arguments
/// * `network` - The network type
/// * `height` - Current block height
/// * `timestamp` - Current block timestamp in milliseconds
/// * `cumulative_difficulty` - Current chain cumulative difficulty
///
/// # Returns
/// Vector of all TIPs that are currently active
pub fn get_active_tips(
    network: &Network,
    height: u64,
    timestamp: u64,
    cumulative_difficulty: u64,
) -> Vec<TosHardfork> {
    get_chain_tips(network).active_tips(height, timestamp, cumulative_difficulty)
}

/// Describe the activation condition for a TIP
///
/// Returns a human-readable description of when the TIP will activate.
///
/// # Arguments
/// * `network` - The network type
/// * `hardfork` - The TIP to describe
///
/// # Returns
/// A string describing the activation condition
pub fn describe_tip_activation(network: &Network, hardfork: TosHardfork) -> String {
    let condition = get_tip_condition(network, hardfork);
    match condition {
        ForkCondition::Block(height) => {
            format!("{} activates at block height {}", hardfork, height)
        }
        ForkCondition::Timestamp(ts) => {
            let seconds = ts / 1000;
            format!(
                "{} activates at timestamp {} (Unix epoch: {} seconds)",
                hardfork, ts, seconds
            )
        }
        ForkCondition::TCD(threshold) => {
            format!(
                "{} activates when cumulative difficulty reaches {}",
                hardfork, threshold
            )
        }
        ForkCondition::Never => format!("{} never activates (disabled)", hardfork),
    }
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
        // All networks now use Nobunaga from height 0
        let (hard_fork, version) = has_hard_fork_at_height(&Network::Mainnet, 0);
        assert_eq!(hard_fork, true);
        assert_eq!(version, BlockVersion::Nobunaga);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Mainnet, 1);
        assert_eq!(hard_fork, false);
        assert_eq!(version, BlockVersion::Nobunaga);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 0);
        assert_eq!(hard_fork, true);
        assert_eq!(version, BlockVersion::Nobunaga);

        let (hard_fork, version) = has_hard_fork_at_height(&Network::Testnet, 100);
        assert_eq!(hard_fork, false);
        assert_eq!(version, BlockVersion::Nobunaga);
    }

    #[test]
    fn test_get_version_at_height() {
        // All heights return Nobunaga
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 0),
            BlockVersion::Nobunaga
        );
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 435_000),
            BlockVersion::Nobunaga
        );
        assert_eq!(
            get_version_at_height(&Network::Mainnet, 2_000_000),
            BlockVersion::Nobunaga
        );

        assert_eq!(
            get_version_at_height(&Network::Testnet, 0),
            BlockVersion::Nobunaga
        );
        assert_eq!(
            get_version_at_height(&Network::Testnet, 100),
            BlockVersion::Nobunaga
        );
    }

    #[test]
    fn test_get_pow_algorithm_for_version() {
        // All versions use Algorithm::V3 (GPU/ASIC-friendly)
        assert_eq!(
            get_pow_algorithm_for_version(BlockVersion::Nobunaga),
            Algorithm::V3
        );
    }

    #[test]
    fn test_is_tx_version_allowed_in_block_version() {
        // Nobunaga supports T1
        assert!(is_tx_version_allowed_in_block_version(
            TxVersion::T1,
            BlockVersion::Nobunaga
        ));
    }

    #[test]
    fn test_version_enabled() {
        // Nobunaga is enabled from height 0 on all networks
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            0,
            BlockVersion::Nobunaga
        ));
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            435_000,
            BlockVersion::Nobunaga
        ));
        assert!(is_version_enabled_at_height(
            &Network::Mainnet,
            2_000_000,
            BlockVersion::Nobunaga
        ));

        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            0,
            BlockVersion::Nobunaga
        ));
        assert!(is_version_enabled_at_height(
            &Network::Testnet,
            100,
            BlockVersion::Nobunaga
        ));
    }

    #[test]
    fn test_get_block_time_target_for_version() {
        // TIP-1: All versions unified to 3 seconds
        assert_eq!(
            get_block_time_target_for_version(BlockVersion::Nobunaga),
            3 * MILLIS_PER_SECOND
        );
    }

    // ========================================================================
    // TIP Activation Mechanism Tests
    // ========================================================================

    #[test]
    fn test_fork_condition_block_activation() {
        // Test Block-based activation condition
        let condition = ForkCondition::Block(1_000_000);

        // Before activation height
        assert!(!condition.is_satisfied(999_999, 0, 0));

        // At exact activation height
        assert!(condition.is_satisfied(1_000_000, 0, 0));

        // After activation height
        assert!(condition.is_satisfied(1_000_001, 0, 0));

        // Verify helper methods
        assert_eq!(condition.activation_height(), Some(1_000_000));
        assert_eq!(condition.activation_timestamp(), None);
        assert_eq!(condition.tcd_threshold(), None);
        assert!(!condition.is_never());
        assert!(condition.is_block_based());
    }

    #[test]
    fn test_fork_condition_timestamp_activation() {
        // Test Timestamp-based activation condition
        // 2026-01-01 00:00:00 UTC in milliseconds
        let activation_timestamp = 1767225600000u64;
        let condition = ForkCondition::Timestamp(activation_timestamp);

        // Before activation timestamp
        assert!(!condition.is_satisfied(0, activation_timestamp - 1, 0));

        // At exact activation timestamp
        assert!(condition.is_satisfied(0, activation_timestamp, 0));

        // After activation timestamp
        assert!(condition.is_satisfied(0, activation_timestamp + 1000, 0));

        // Verify helper methods
        assert_eq!(condition.activation_height(), None);
        assert_eq!(condition.activation_timestamp(), Some(activation_timestamp));
        assert_eq!(condition.tcd_threshold(), None);
        assert!(!condition.is_never());
        assert!(!condition.is_block_based());
    }

    #[test]
    fn test_fork_condition_tcd_activation() {
        // Test TCD (Threshold Cumulative Difficulty) activation condition
        let threshold = 1_000_000_000u64; // 1 billion
        let condition = ForkCondition::TCD(threshold);

        // Before threshold reached
        assert!(!condition.is_satisfied(0, 0, threshold - 1));

        // At exact threshold
        assert!(condition.is_satisfied(0, 0, threshold));

        // After threshold exceeded
        assert!(condition.is_satisfied(0, 0, threshold + 1000));

        // Verify helper methods
        assert_eq!(condition.activation_height(), None);
        assert_eq!(condition.activation_timestamp(), None);
        assert_eq!(condition.tcd_threshold(), Some(threshold));
        assert!(!condition.is_never());
        assert!(!condition.is_block_based());
    }

    #[test]
    fn test_fork_condition_never() {
        // Test Never condition (disabled fork)
        let condition = ForkCondition::Never;

        // Never activates regardless of state
        assert!(!condition.is_satisfied(0, 0, 0));
        assert!(!condition.is_satisfied(u64::MAX, u64::MAX, u64::MAX));

        // Verify helper methods
        assert_eq!(condition.activation_height(), None);
        assert_eq!(condition.activation_timestamp(), None);
        assert_eq!(condition.tcd_threshold(), None);
        assert!(condition.is_never());
        assert!(!condition.is_block_based());
    }

    #[test]
    fn test_fork_condition_display() {
        // Test Display trait implementation
        assert_eq!(format!("{}", ForkCondition::Block(1000)), "Block(1000)");
        assert_eq!(
            format!("{}", ForkCondition::Timestamp(1767225600000)),
            "Timestamp(1767225600000)"
        );
        assert_eq!(format!("{}", ForkCondition::TCD(1_000_000)), "TCD(1000000)");
        assert_eq!(format!("{}", ForkCondition::Never), "Never");
    }

    #[test]
    fn test_hard_fork_condition() {
        use tos_common::api::daemon::HardFork;

        // Test Block condition
        let block_fork = HardFork {
            condition: ForkCondition::Block(1000),
            version: BlockVersion::Nobunaga,
            changelog: "Block fork",
            version_requirement: None,
        };
        assert_eq!(block_fork.condition(), ForkCondition::Block(1000));
        assert_eq!(block_fork.activation_height(), Some(1000));
        assert_eq!(block_fork.activation_timestamp(), None);
        assert_eq!(block_fork.tcd_threshold(), None);
        assert!(!block_fork.is_disabled());

        // Test Timestamp condition
        let timestamp_fork = HardFork {
            condition: ForkCondition::Timestamp(1767225600000),
            version: BlockVersion::Nobunaga,
            changelog: "Timestamp fork",
            version_requirement: None,
        };
        assert_eq!(
            timestamp_fork.condition(),
            ForkCondition::Timestamp(1767225600000)
        );
        assert_eq!(timestamp_fork.activation_height(), None);
        assert_eq!(timestamp_fork.activation_timestamp(), Some(1767225600000));
        assert!(!timestamp_fork.is_disabled());

        // Test TCD condition
        let tcd_fork = HardFork {
            condition: ForkCondition::TCD(1_000_000),
            version: BlockVersion::Nobunaga,
            changelog: "TCD fork",
            version_requirement: None,
        };
        assert_eq!(tcd_fork.tcd_threshold(), Some(1_000_000));
        assert!(!tcd_fork.is_disabled());

        // Test Never condition
        let never_fork = HardFork {
            condition: ForkCondition::Never,
            version: BlockVersion::Nobunaga,
            changelog: "Disabled fork",
            version_requirement: None,
        };
        assert!(never_fork.is_disabled());
    }

    #[test]
    fn test_hard_fork_is_activated() {
        use tos_common::api::daemon::HardFork;

        // Test Block-based activation
        let block_fork = HardFork {
            condition: ForkCondition::Block(1000),
            version: BlockVersion::Nobunaga,
            changelog: "Block fork",
            version_requirement: None,
        };
        assert!(!block_fork.is_activated(999, 0, 0));
        assert!(block_fork.is_activated(1000, 0, 0));
        assert!(block_fork.is_activated(1001, 0, 0));

        // Test Timestamp-based activation
        let timestamp_fork = HardFork {
            condition: ForkCondition::Timestamp(1767225600000),
            version: BlockVersion::Nobunaga,
            changelog: "Timestamp fork",
            version_requirement: None,
        };
        assert!(!timestamp_fork.is_activated(0, 1767225599999, 0));
        assert!(timestamp_fork.is_activated(0, 1767225600000, 0));
        assert!(timestamp_fork.is_activated(0, 1767225600001, 0));

        // Test TCD-based activation
        let tcd_fork = HardFork {
            condition: ForkCondition::TCD(1_000_000),
            version: BlockVersion::Nobunaga,
            changelog: "TCD fork",
            version_requirement: None,
        };
        assert!(!tcd_fork.is_activated(0, 0, 999_999));
        assert!(tcd_fork.is_activated(0, 0, 1_000_000));
        assert!(tcd_fork.is_activated(0, 0, 1_000_001));

        // Test Never condition
        let never_fork = HardFork {
            condition: ForkCondition::Never,
            version: BlockVersion::Nobunaga,
            changelog: "Disabled fork",
            version_requirement: None,
        };
        assert!(!never_fork.is_activated(u64::MAX, u64::MAX, u64::MAX));
    }

    #[test]
    fn test_hard_fork_is_activated_at_height() {
        use tos_common::api::daemon::HardFork;

        // Test Block-based (should work with height-only check)
        let block_fork = HardFork {
            condition: ForkCondition::Block(1000),
            version: BlockVersion::Nobunaga,
            changelog: "Block fork",
            version_requirement: None,
        };
        assert!(!block_fork.is_activated_at_height(999));
        assert!(block_fork.is_activated_at_height(1000));
        assert!(block_fork.is_activated_at_height(1001));

        // Test Timestamp condition (cannot check by height alone)
        let timestamp_fork = HardFork {
            condition: ForkCondition::Timestamp(1767225600000),
            version: BlockVersion::Nobunaga,
            changelog: "Timestamp fork",
            version_requirement: None,
        };
        assert!(!timestamp_fork.is_activated_at_height(u64::MAX));

        // Test Never condition (should never activate)
        let never_fork = HardFork {
            condition: ForkCondition::Never,
            version: BlockVersion::Nobunaga,
            changelog: "Disabled fork",
            version_requirement: None,
        };
        assert!(!never_fork.is_activated_at_height(u64::MAX));
    }

    #[test]
    fn test_get_activated_hard_fork() {
        // Test with current Nobunaga configuration
        let hard_fork = get_activated_hard_fork(&Network::Mainnet, 0, 0, 0);
        assert!(hard_fork.is_some());
        assert_eq!(hard_fork.unwrap().version, BlockVersion::Nobunaga);

        // Test at various heights - all should return Nobunaga
        let hard_fork = get_activated_hard_fork(&Network::Mainnet, 1_000_000, 0, 0);
        assert!(hard_fork.is_some());
        assert_eq!(hard_fork.unwrap().version, BlockVersion::Nobunaga);
    }

    #[test]
    fn test_get_version_for_state() {
        // Test that get_version_for_state returns correct version
        assert_eq!(
            get_version_for_state(&Network::Mainnet, 0, 0, 0),
            BlockVersion::Nobunaga
        );

        // Test at various states
        assert_eq!(
            get_version_for_state(&Network::Mainnet, 1_000_000, 1767225600000, 1_000_000_000),
            BlockVersion::Nobunaga
        );
    }

    #[test]
    fn test_describe_fork_activation() {
        use tos_common::api::daemon::HardFork;

        let block_fork = HardFork {
            condition: ForkCondition::Block(1000),
            version: BlockVersion::Nobunaga,
            changelog: "Test",
            version_requirement: None,
        };
        assert_eq!(
            describe_fork_activation(&block_fork),
            "Activates at block height 1000"
        );

        let timestamp_fork = HardFork {
            condition: ForkCondition::Timestamp(1767225600000),
            version: BlockVersion::Nobunaga,
            changelog: "Test",
            version_requirement: None,
        };
        assert_eq!(
            describe_fork_activation(&timestamp_fork),
            "Activates at timestamp 1767225600000 (Unix epoch: 1767225600 seconds)"
        );

        let tcd_fork = HardFork {
            condition: ForkCondition::TCD(1_000_000_000),
            version: BlockVersion::Nobunaga,
            changelog: "Test",
            version_requirement: None,
        };
        assert_eq!(
            describe_fork_activation(&tcd_fork),
            "Activates when cumulative difficulty reaches 1000000000"
        );

        let never_fork = HardFork {
            condition: ForkCondition::Never,
            version: BlockVersion::Nobunaga,
            changelog: "Test",
            version_requirement: None,
        };
        assert_eq!(
            describe_fork_activation(&never_fork),
            "Never activates (disabled)"
        );
    }

    #[test]
    fn test_get_disabled_forks() {
        // Current configuration has no disabled forks
        let disabled = get_disabled_forks(&Network::Mainnet);
        assert!(disabled.is_empty());

        let disabled = get_disabled_forks(&Network::Testnet);
        assert!(disabled.is_empty());
    }

    #[test]
    fn test_fork_condition_serde() {
        // Test serialization/deserialization
        let conditions = vec![
            ForkCondition::Block(1000),
            ForkCondition::Timestamp(1767225600000),
            ForkCondition::TCD(1_000_000),
            ForkCondition::Never,
        ];

        for condition in conditions {
            let serialized = serde_json::to_string(&condition).unwrap();
            let deserialized: ForkCondition = serde_json::from_str(&serialized).unwrap();
            assert_eq!(condition, deserialized);
        }
    }

    #[test]
    fn test_hard_fork_serde() {
        use tos_common::api::daemon::HardFork;

        // Test HardFork serialization/deserialization
        let json = r#"{
            "condition": {"Block": 1000},
            "version": 0,
            "changelog": "Test fork"
        }"#;

        let hard_fork: HardFork = serde_json::from_str(json).unwrap();
        assert_eq!(hard_fork.condition, ForkCondition::Block(1000));
        assert_eq!(hard_fork.version, BlockVersion::Nobunaga);
    }

    #[test]
    fn test_has_hard_fork_at_state() {
        // Test at genesis (height 0)
        let (is_fork, version) = has_hard_fork_at_state(&Network::Mainnet, 0, 0, 0);
        assert!(is_fork);
        assert_eq!(version, BlockVersion::Nobunaga);

        // Test at non-fork height
        let (is_fork, version) = has_hard_fork_at_state(&Network::Mainnet, 1, 0, 0);
        assert!(!is_fork);
        assert_eq!(version, BlockVersion::Nobunaga);
    }

    // ========================================================================
    // TIP (TOS Improvement Proposal) Tests
    // ========================================================================

    #[test]
    fn test_tos_hardfork_all_empty() {
        // Currently no TIPs defined - empty framework for future use
        let all = TosHardfork::all();
        assert!(all.is_empty());
    }

    #[test]
    fn test_get_active_tips_empty() {
        // No TIPs configured, so active list should be empty
        for network in &[Network::Mainnet, Network::Testnet, Network::Devnet] {
            let active = get_active_tips(network, 0, 0, 0);
            assert!(active.is_empty());
        }
    }

    #[test]
    fn test_chain_tips_empty_framework() {
        use tos_common::api::daemon::ChainTips;

        // Test empty ChainTips
        let tips = ChainTips::new(vec![]);
        assert!(tips.active_tips(0, 0, 0).is_empty());

        // Default ChainTips should also be empty
        let default_tips = ChainTips::default();
        assert!(default_tips.active_tips(0, 0, 0).is_empty());
    }
}
