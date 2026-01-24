//! Feature gate testing system for protocol upgrades.
//!
//! Allows tests to control which protocol features are active at specific
//! topoheights, enabling testing of pre/post-upgrade behavior, activation
//! boundaries, and backward compatibility.

use std::collections::{HashMap, HashSet};

/// A protocol feature that can be activated at a specific topoheight.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Feature {
    /// Unique identifier for this feature
    pub id: &'static str,
    /// Human-readable description
    pub description: &'static str,
    /// Default activation topoheight on mainnet (None = not yet activated)
    pub activation_height: Option<u64>,
}

/// Registry of all known protocol features.
#[derive(Debug, Clone, Default)]
pub struct FeatureRegistry {
    features: HashMap<&'static str, Feature>,
}

impl FeatureRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a feature in the registry.
    pub fn register(&mut self, feature: Feature) -> &mut Self {
        self.features.insert(feature.id, feature);
        self
    }

    /// Get a feature by ID.
    pub fn get(&self, id: &str) -> Option<&Feature> {
        self.features.get(id)
    }

    /// Get all registered features.
    pub fn all(&self) -> impl Iterator<Item = &Feature> {
        self.features.values()
    }

    /// Get features that are active at a given topoheight.
    pub fn active_at(&self, topoheight: u64) -> Vec<&Feature> {
        self.features
            .values()
            .filter(|f| f.activation_height.is_some_and(|h| topoheight >= h))
            .collect()
    }

    /// Create the default TOS feature registry with known features.
    pub fn tos_defaults() -> Self {
        let mut registry = Self::new();
        registry
            .register(features::TAKO_V2_SYSCALLS)
            .register(features::EIP1153_TRANSIENT)
            .register(features::NFT_V2)
            .register(features::FEE_MODEL_V2)
            .register(features::BLOCKDAG_V2_ORDERING)
            .register(features::ENERGY_DELEGATION_V2)
            .register(features::P2P_COMPRESSION)
            .register(features::RPC_V2_RESPONSES)
            .register(features::MAX_BLOCK_SIZE_INCREASE)
            .register(features::CHECKED_ARITHMETIC_ENFORCE)
            .register(features::VRF_BLOCK_DATA);
        registry
    }
}

/// Known TOS protocol features.
#[allow(clippy::module_inception)]
pub mod features {
    use super::Feature;

    /// TAKO v2 syscall ABI with extended return data
    pub const TAKO_V2_SYSCALLS: Feature = Feature {
        id: "tako_v2_syscalls",
        description: "TAKO v2 syscall ABI with extended return data",
        activation_height: Some(500_000),
    };

    /// EIP-1153 transient storage (TLOAD/TSTORE)
    pub const EIP1153_TRANSIENT: Feature = Feature {
        id: "eip1153_transient",
        description: "EIP-1153 transient storage (TLOAD/TSTORE)",
        activation_height: Some(600_000),
    };

    /// NFT v2 with royalties support
    pub const NFT_V2: Feature = Feature {
        id: "nft_v2",
        description: "NFT v2 with royalties support",
        activation_height: Some(700_000),
    };

    /// New fee calculation model with dynamic base fee
    pub const FEE_MODEL_V2: Feature = Feature {
        id: "fee_model_v2",
        description: "New fee calculation with dynamic base fee",
        activation_height: None, // Not yet activated
    };

    /// Improved BlockDAG ordering algorithm
    pub const BLOCKDAG_V2_ORDERING: Feature = Feature {
        id: "blockdag_v2_ordering",
        description: "Improved DAG ordering algorithm",
        activation_height: None,
    };

    /// New delegation rules for energy system
    pub const ENERGY_DELEGATION_V2: Feature = Feature {
        id: "energy_delegation_v2",
        description: "New delegation rules for energy",
        activation_height: None,
    };

    /// Compressed P2P messages
    pub const P2P_COMPRESSION: Feature = Feature {
        id: "p2p_compression",
        description: "Compressed P2P messages",
        activation_height: None,
    };

    /// New RPC response format
    pub const RPC_V2_RESPONSES: Feature = Feature {
        id: "rpc_v2_responses",
        description: "New RPC response format",
        activation_height: None,
    };

    /// Increased block size limit
    pub const MAX_BLOCK_SIZE_INCREASE: Feature = Feature {
        id: "max_block_size_increase",
        description: "Increased block size limit",
        activation_height: None,
    };

    /// Enforce checked arithmetic in contracts
    pub const CHECKED_ARITHMETIC_ENFORCE: Feature = Feature {
        id: "checked_arithmetic_enforce",
        description: "Enforce checked math in contracts",
        activation_height: None,
    };

    /// VRF block data production and validation
    pub const VRF_BLOCK_DATA: Feature = Feature {
        id: "vrf_block_data",
        description: "VRF data in block headers for verifiable randomness",
        activation_height: Some(0),
    };
}

/// Base configuration for which features start active.
#[derive(Debug, Clone, Default)]
pub enum FeatureBase {
    /// Start with all mainnet-active features enabled (default)
    #[default]
    MainnetDefaults,
    /// Start with all features disabled (minimal testing)
    AllDisabled,
    /// Start with features as of a specific network height
    AsOfHeight(u64),
}

/// A configurable set of features for test environments.
///
/// Controls which protocol features are active at which topoheights,
/// enabling tests to verify behavior across protocol upgrades.
///
/// # Example
/// ```ignore
/// let features = FeatureSet::mainnet()
///     .deactivate("fee_model_v2")
///     .activate_at("nft_v2", 100);
/// ```
#[derive(Debug, Clone, Default)]
pub struct FeatureSet {
    /// Features explicitly deactivated (overrides defaults)
    deactivated: HashSet<String>,
    /// Features explicitly activated at specific heights
    activations: HashMap<String, u64>,
    /// Base feature configuration
    base: FeatureBase,
}

impl FeatureSet {
    /// Create with all mainnet features active at their default heights.
    pub fn mainnet() -> Self {
        Self {
            base: FeatureBase::MainnetDefaults,
            ..Default::default()
        }
    }

    /// Create with all features disabled.
    pub fn empty() -> Self {
        Self {
            base: FeatureBase::AllDisabled,
            ..Default::default()
        }
    }

    /// Create with features as active at a specific historical height.
    pub fn as_of_height(height: u64) -> Self {
        Self {
            base: FeatureBase::AsOfHeight(height),
            ..Default::default()
        }
    }

    /// Deactivate a specific feature (override its default activation).
    pub fn deactivate(mut self, feature_id: &str) -> Self {
        self.deactivated.insert(feature_id.to_string());
        self.activations.remove(feature_id);
        self
    }

    /// Activate a feature at a specific topoheight.
    pub fn activate_at(mut self, feature_id: &str, height: u64) -> Self {
        self.deactivated.remove(feature_id);
        self.activations.insert(feature_id.to_string(), height);
        self
    }

    /// Check if a feature is active at the given topoheight.
    pub fn is_active(&self, feature_id: &str, topoheight: u64) -> bool {
        // Explicitly deactivated features are never active
        if self.deactivated.contains(feature_id) {
            return false;
        }

        // Check explicit activation overrides
        if let Some(&activation_height) = self.activations.get(feature_id) {
            return topoheight >= activation_height;
        }

        // Fall back to base configuration
        let registry = FeatureRegistry::tos_defaults();
        match &self.base {
            FeatureBase::MainnetDefaults => registry
                .get(feature_id)
                .and_then(|f| f.activation_height)
                .is_some_and(|h| topoheight >= h),
            FeatureBase::AllDisabled => false,
            FeatureBase::AsOfHeight(base_height) => registry
                .get(feature_id)
                .and_then(|f| f.activation_height)
                .is_some_and(|h| h <= *base_height && topoheight >= h),
        }
    }

    /// Get all explicitly deactivated features.
    pub fn deactivated_features(&self) -> &HashSet<String> {
        &self.deactivated
    }

    /// Get all explicit activation overrides.
    pub fn activation_overrides(&self) -> &HashMap<String, u64> {
        &self.activations
    }

    /// Get the base configuration.
    pub fn base(&self) -> &FeatureBase {
        &self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_registry() {
        let registry = FeatureRegistry::tos_defaults();
        assert!(registry.get("tako_v2_syscalls").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_active_at() {
        let registry = FeatureRegistry::tos_defaults();

        // At height 0, only VRF_BLOCK_DATA is active (activation_height=0)
        let active = registry.active_at(0);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "vrf_block_data");

        // After TAKO v2 activation (500_000)
        let active = registry.active_at(500_001);
        assert!(active.iter().any(|f| f.id == "tako_v2_syscalls"));

        // After EIP1153 activation (600_000)
        let active = registry.active_at(600_001);
        assert!(active.iter().any(|f| f.id == "eip1153_transient"));
    }

    #[test]
    fn test_feature_set_mainnet() {
        let fs = FeatureSet::mainnet();

        // TAKO v2 activates at 500_000
        assert!(!fs.is_active("tako_v2_syscalls", 499_999));
        assert!(fs.is_active("tako_v2_syscalls", 500_000));
        assert!(fs.is_active("tako_v2_syscalls", 999_999));

        // fee_model_v2 has no activation height
        assert!(!fs.is_active("fee_model_v2", 999_999));
    }

    #[test]
    fn test_feature_set_empty() {
        let fs = FeatureSet::empty();

        // Nothing is active in empty mode
        assert!(!fs.is_active("tako_v2_syscalls", 999_999));
        assert!(!fs.is_active("eip1153_transient", 999_999));
    }

    #[test]
    fn test_feature_set_deactivate() {
        let fs = FeatureSet::mainnet().deactivate("tako_v2_syscalls");

        // Deactivated feature is never active
        assert!(!fs.is_active("tako_v2_syscalls", 999_999));

        // Other features unaffected
        assert!(fs.is_active("eip1153_transient", 600_000));
    }

    #[test]
    fn test_feature_set_activate_at() {
        let fs = FeatureSet::empty().activate_at("fee_model_v2", 100);

        // Active at specified height
        assert!(!fs.is_active("fee_model_v2", 99));
        assert!(fs.is_active("fee_model_v2", 100));
        assert!(fs.is_active("fee_model_v2", 101));

        // Other features still inactive
        assert!(!fs.is_active("tako_v2_syscalls", 999_999));
    }

    #[test]
    fn test_feature_set_as_of_height() {
        let fs = FeatureSet::as_of_height(550_000);

        // TAKO v2 (500_000) was active at height 550_000
        assert!(fs.is_active("tako_v2_syscalls", 500_000));

        // EIP1153 (600_000) was NOT active at height 550_000
        assert!(!fs.is_active("eip1153_transient", 600_000));
    }

    #[test]
    fn test_deactivate_then_activate() {
        // Deactivating then activating at new height replaces behavior
        let fs = FeatureSet::mainnet()
            .deactivate("tako_v2_syscalls")
            .activate_at("tako_v2_syscalls", 1_000_000);

        assert!(!fs.is_active("tako_v2_syscalls", 500_000));
        assert!(!fs.is_active("tako_v2_syscalls", 999_999));
        assert!(fs.is_active("tako_v2_syscalls", 1_000_000));
    }

    #[test]
    fn test_unknown_feature() {
        let fs = FeatureSet::mainnet();
        assert!(!fs.is_active("totally_unknown_feature", 999_999));
    }
}
