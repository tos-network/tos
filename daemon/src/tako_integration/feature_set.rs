/// TOS SVM Feature Set - Aligns with Solana's SVMFeatureSet for TBPF version control
///
/// This module provides runtime feature flags that control which TBPF versions
/// are enabled for deployment and execution, matching Solana's approach.
///
/// # Architecture
///
/// ```text
/// Feature Flags (SVMFeatureSet)
///     ↓
/// enabled_tbpf_versions() → RangeInclusive<TBPFVersion>
///     ↓
/// Config.enabled_tbpf_versions
///     ↓
/// ELF Loader (validates e_flags against range)
/// ```
///
/// # TBPF Version History
///
/// | Version | e_flags | Features |
/// |---------|---------|----------|
/// | V0 | 0 | Legacy format (backward compatible) |
/// | V1 | 1 | SIMD-0166: Dynamic stack frames |
/// | V2 | 2 | SIMD-0174, SIMD-0173: Arithmetic improvements |
/// | V3 | 3 | SIMD-0178, SIMD-0189, SIMD-0377: Static syscalls, stricter ELF |
/// | V4 | 4 | SIMD-0177: Future enhancements |
///
/// # Solana Alignment
///
/// This implementation mirrors Solana's `agave/svm-feature-set/src/lib.rs`:
/// - `disable_tbpf_v0_execution`: Disable legacy V0 format
/// - `reenable_tbpf_v0_execution`: Override to re-enable V0
/// - `enable_tbpf_v1_deployment_and_execution`: Enable V1
/// - `enable_tbpf_v2_deployment_and_execution`: Enable V2
/// - `enable_tbpf_v3_deployment_and_execution`: Enable V3
use tos_tbpf::program::TBPFVersion;

/// TOS SVM Feature Set for controlling TBPF version behavior
///
/// This struct mirrors Solana's SVMFeatureSet and provides runtime control
/// over which TBPF versions are allowed for contract deployment and execution.
///
/// # Default Behavior
///
/// By default (all flags false):
/// - `min_tbpf_version = V0` (V0 execution enabled)
/// - `max_tbpf_version = V0` (only V0 deployment allowed)
///
/// This matches Solana's conservative default for backward compatibility.
///
/// # Production Configuration
///
/// For production TOS nodes, use `SVMFeatureSet::production()`:
/// - Supports V0-V3 contracts
/// - V0 enabled for backward compatibility
/// - V3 enabled for modern contracts
///
/// # Example
///
/// ```rust
/// use tos_daemon::tako_integration::SVMFeatureSet;
///
/// // Default: V0 only
/// let default_features = SVMFeatureSet::default();
///
/// // Production: V0-V3
/// let production_features = SVMFeatureSet::production();
///
/// // All versions (testing)
/// let all_features = SVMFeatureSet::all_enabled();
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct SVMFeatureSet {
    /// Disable execution of TBPF V0 programs
    /// Default: false (V0 enabled for backward compatibility)
    pub disable_tbpf_v0_execution: bool,

    /// Re-enable V0 execution even if `disable_tbpf_v0_execution` is true
    /// This allows emergency rollback to V0 support
    /// Default: false
    pub reenable_tbpf_v0_execution: bool,

    /// Enable deployment and execution of TBPF V1 programs (SIMD-0166)
    /// Default: false
    pub enable_tbpf_v1_deployment_and_execution: bool,

    /// Enable deployment and execution of TBPF V2 programs (SIMD-0174, SIMD-0173)
    /// Default: false
    pub enable_tbpf_v2_deployment_and_execution: bool,

    /// Enable deployment and execution of TBPF V3 programs (SIMD-0178, SIMD-0189, SIMD-0377)
    /// Default: false
    pub enable_tbpf_v3_deployment_and_execution: bool,

    /// Stricter ABI and runtime constraints (affects memory mapping)
    /// Default: false
    pub stricter_abi_and_runtime_constraints: bool,
}

impl SVMFeatureSet {
    /// Create a new feature set with all features disabled
    ///
    /// This is the most conservative configuration:
    /// - Only V0 contracts are supported
    /// - Maximum backward compatibility
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a feature set with all features enabled
    ///
    /// This configuration enables all TBPF versions (V0-V3):
    /// - Useful for testing
    /// - Matches Solana's test environment configuration
    ///
    /// Note: V4 is not included as it's reserved for future use
    pub fn all_enabled() -> Self {
        Self {
            disable_tbpf_v0_execution: false, // Keep V0 enabled
            reenable_tbpf_v0_execution: true,
            enable_tbpf_v1_deployment_and_execution: true,
            enable_tbpf_v2_deployment_and_execution: true,
            enable_tbpf_v3_deployment_and_execution: true,
            stricter_abi_and_runtime_constraints: false,
        }
    }

    /// Create a production feature set for TOS mainnet
    ///
    /// Production configuration:
    /// - V0-V3 supported (matching Solana's production environment)
    /// - V0 kept enabled for backward compatibility with existing contracts
    /// - V3 enabled for modern contracts with stricter ELF validation
    ///
    /// This matches Solana's `agave/svm/tests/mock_bank.rs:371`:
    /// `enabled_sbpf_versions: SBPFVersion::V0..=SBPFVersion::V3`
    pub fn production() -> Self {
        Self {
            disable_tbpf_v0_execution: false, // Keep V0 for backward compatibility
            reenable_tbpf_v0_execution: false,
            enable_tbpf_v1_deployment_and_execution: true,
            enable_tbpf_v2_deployment_and_execution: true,
            enable_tbpf_v3_deployment_and_execution: true,
            stricter_abi_and_runtime_constraints: false,
        }
    }

    /// Create a feature set for V3-only execution
    ///
    /// This configuration:
    /// - Disables V0 execution (legacy contracts rejected)
    /// - Enables V3 for modern contracts
    /// - Useful for new networks without legacy contract compatibility needs
    pub fn v3_only() -> Self {
        Self {
            disable_tbpf_v0_execution: true,
            reenable_tbpf_v0_execution: false,
            enable_tbpf_v1_deployment_and_execution: false,
            enable_tbpf_v2_deployment_and_execution: false,
            enable_tbpf_v3_deployment_and_execution: true,
            stricter_abi_and_runtime_constraints: true,
        }
    }

    /// Calculate the minimum TBPF version based on feature flags
    ///
    /// Logic (matching Solana's `agave/syscalls/src/lib.rs:302-307`):
    /// - If V0 execution is not disabled OR re-enabled: min = V0
    /// - Otherwise: min = V3 (skip V1/V2 which are transitional)
    pub fn min_tbpf_version(&self) -> TBPFVersion {
        if !self.disable_tbpf_v0_execution || self.reenable_tbpf_v0_execution {
            TBPFVersion::V0
        } else {
            TBPFVersion::V3
        }
    }

    /// Calculate the maximum TBPF version based on feature flags
    ///
    /// Logic (matching Solana's `agave/syscalls/src/lib.rs:308-316`):
    /// - Check in order: V3 → V2 → V1 → V0
    /// - Return the highest enabled version
    pub fn max_tbpf_version(&self) -> TBPFVersion {
        if self.enable_tbpf_v3_deployment_and_execution {
            TBPFVersion::V3
        } else if self.enable_tbpf_v2_deployment_and_execution {
            TBPFVersion::V2
        } else if self.enable_tbpf_v1_deployment_and_execution {
            TBPFVersion::V1
        } else {
            TBPFVersion::V0
        }
    }

    /// Get the enabled TBPF version range
    ///
    /// This is the main method used by the executor to configure the VM.
    /// Returns a RangeInclusive<TBPFVersion> that can be used directly
    /// in Config.enabled_tbpf_versions.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tos_daemon::tako_integration::SVMFeatureSet;
    /// use tos_tbpf::vm::Config;
    ///
    /// let features = SVMFeatureSet::production();
    /// let mut config = Config::default();
    /// config.enabled_tbpf_versions = features.enabled_tbpf_versions();
    /// // config.enabled_tbpf_versions is now V0..=V3
    /// ```
    pub fn enabled_tbpf_versions(&self) -> std::ops::RangeInclusive<TBPFVersion> {
        let min = self.min_tbpf_version();
        let max = self.max_tbpf_version();

        // Debug assertion: min should never be greater than max
        debug_assert!(
            min <= max,
            "Invalid TBPF version range: min {:?} > max {:?}",
            min,
            max
        );

        min..=max
    }

    /// Check if aligned memory mapping should be used
    ///
    /// Aligned memory mapping is disabled when stricter ABI constraints are enabled.
    /// This matches Solana's `agave/syscalls/src/lib.rs:333`:
    /// `aligned_memory_mapping: !feature_set.stricter_abi_and_runtime_constraints`
    pub fn use_aligned_memory_mapping(&self) -> bool {
        !self.stricter_abi_and_runtime_constraints
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_feature_set() {
        let features = SVMFeatureSet::default();

        // Default: V0 only
        assert_eq!(features.min_tbpf_version(), TBPFVersion::V0);
        assert_eq!(features.max_tbpf_version(), TBPFVersion::V0);

        let range = features.enabled_tbpf_versions();
        assert!(range.contains(&TBPFVersion::V0));
        assert!(!range.contains(&TBPFVersion::V1));
        assert!(!range.contains(&TBPFVersion::V2));
        assert!(!range.contains(&TBPFVersion::V3));
    }

    #[test]
    fn test_production_feature_set() {
        let features = SVMFeatureSet::production();

        // Production: V0-V3
        assert_eq!(features.min_tbpf_version(), TBPFVersion::V0);
        assert_eq!(features.max_tbpf_version(), TBPFVersion::V3);

        let range = features.enabled_tbpf_versions();
        assert!(range.contains(&TBPFVersion::V0));
        assert!(range.contains(&TBPFVersion::V1));
        assert!(range.contains(&TBPFVersion::V2));
        assert!(range.contains(&TBPFVersion::V3));
        assert!(!range.contains(&TBPFVersion::V4));
    }

    #[test]
    fn test_all_enabled_feature_set() {
        let features = SVMFeatureSet::all_enabled();

        // All enabled: V0-V3
        assert_eq!(features.min_tbpf_version(), TBPFVersion::V0);
        assert_eq!(features.max_tbpf_version(), TBPFVersion::V3);

        let range = features.enabled_tbpf_versions();
        assert!(range.contains(&TBPFVersion::V0));
        assert!(range.contains(&TBPFVersion::V3));
    }

    #[test]
    fn test_v3_only_feature_set() {
        let features = SVMFeatureSet::v3_only();

        // V3 only: V3-V3
        assert_eq!(features.min_tbpf_version(), TBPFVersion::V3);
        assert_eq!(features.max_tbpf_version(), TBPFVersion::V3);

        let range = features.enabled_tbpf_versions();
        assert!(!range.contains(&TBPFVersion::V0));
        assert!(!range.contains(&TBPFVersion::V1));
        assert!(!range.contains(&TBPFVersion::V2));
        assert!(range.contains(&TBPFVersion::V3));
    }

    #[test]
    fn test_reenable_v0_override() {
        let features = SVMFeatureSet {
            disable_tbpf_v0_execution: true,
            reenable_tbpf_v0_execution: true, // Override
            enable_tbpf_v3_deployment_and_execution: true,
            ..Default::default()
        };

        // reenable should override disable
        assert_eq!(features.min_tbpf_version(), TBPFVersion::V0);
        assert_eq!(features.max_tbpf_version(), TBPFVersion::V3);
    }

    #[test]
    fn test_aligned_memory_mapping() {
        let default_features = SVMFeatureSet::default();
        assert!(default_features.use_aligned_memory_mapping());

        let v3_only = SVMFeatureSet::v3_only();
        assert!(!v3_only.use_aligned_memory_mapping());
    }

    #[test]
    fn test_version_range_consistency() {
        // Test various combinations to ensure min <= max
        let combinations = vec![
            SVMFeatureSet::default(),
            SVMFeatureSet::production(),
            SVMFeatureSet::all_enabled(),
            SVMFeatureSet::v3_only(),
            SVMFeatureSet {
                enable_tbpf_v1_deployment_and_execution: true,
                ..Default::default()
            },
            SVMFeatureSet {
                enable_tbpf_v2_deployment_and_execution: true,
                ..Default::default()
            },
        ];

        for features in combinations {
            let min = features.min_tbpf_version();
            let max = features.max_tbpf_version();
            assert!(
                min <= max,
                "Invalid range for {:?}: min {:?} > max {:?}",
                features,
                min,
                max
            );
        }
    }
}
