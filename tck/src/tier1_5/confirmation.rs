//! Confirmation depth types for transaction finality testing.
//!
//! In a BlockDAG, "confirmation" depends on how many subsequent blocks
//! reference a block. This module provides the `ConfirmationDepth` enum
//! analogous to Solana's `CommitmentLevel` (Processed/Confirmed/Finalized).

/// Confirmation depth for transaction queries.
///
/// Represents how "final" a transaction is, based on DAG structure:
/// - `Included`: TX is in a block but may be orphaned
/// - `Confirmed(n)`: N blocks have been built on top
/// - `Stable`: Block is in the stable chain (cannot be reorganized)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfirmationDepth {
    /// Transaction is included in a block (may be orphaned).
    /// Equivalent to Solana's "processed" commitment.
    Included,

    /// Transaction has N confirmations (N blocks built on top).
    /// Equivalent to Solana's "confirmed" commitment.
    Confirmed(u64),

    /// Transaction is in a stable block (cannot be reorganized).
    /// Equivalent to Solana's "finalized" commitment.
    Stable,
}

impl ConfirmationDepth {
    /// Default confirmation depth for testing (8 blocks).
    pub const DEFAULT_CONFIRMED: Self = Self::Confirmed(8);

    /// Minimum confirmation for "safe" transactions (4 blocks).
    pub const SAFE: Self = Self::Confirmed(4);

    /// Number of confirmations typically needed for stability.
    /// In TOS, this depends on network parameters.
    pub const STABILITY_THRESHOLD: u64 = 64;

    /// Returns the minimum number of confirmations needed.
    pub fn min_confirmations(&self) -> u64 {
        match self {
            Self::Included => 0,
            Self::Confirmed(n) => *n,
            Self::Stable => Self::STABILITY_THRESHOLD,
        }
    }

    /// Check if a given number of confirmations satisfies this depth.
    pub fn is_satisfied_by(&self, confirmations: u64) -> bool {
        confirmations >= self.min_confirmations()
    }

    /// Returns true if this represents finalized/stable state.
    pub fn is_stable(&self) -> bool {
        matches!(self, Self::Stable)
    }

    /// Returns true if this requires any confirmations beyond inclusion.
    pub fn requires_confirmations(&self) -> bool {
        !matches!(self, Self::Included)
    }
}

impl std::fmt::Display for ConfirmationDepth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Included => write!(f, "included"),
            Self::Confirmed(n) => write!(f, "confirmed({})", n),
            Self::Stable => write!(f, "stable"),
        }
    }
}

impl Default for ConfirmationDepth {
    fn default() -> Self {
        Self::DEFAULT_CONFIRMED
    }
}

impl PartialOrd for ConfirmationDepth {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ConfirmationDepth {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.min_confirmations().cmp(&other.min_confirmations())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_min_confirmations() {
        assert_eq!(ConfirmationDepth::Included.min_confirmations(), 0);
        assert_eq!(ConfirmationDepth::Confirmed(4).min_confirmations(), 4);
        assert_eq!(ConfirmationDepth::Confirmed(8).min_confirmations(), 8);
        assert_eq!(
            ConfirmationDepth::Stable.min_confirmations(),
            ConfirmationDepth::STABILITY_THRESHOLD
        );
    }

    #[test]
    fn test_is_satisfied_by() {
        assert!(ConfirmationDepth::Included.is_satisfied_by(0));
        assert!(ConfirmationDepth::Included.is_satisfied_by(100));

        assert!(!ConfirmationDepth::Confirmed(4).is_satisfied_by(3));
        assert!(ConfirmationDepth::Confirmed(4).is_satisfied_by(4));
        assert!(ConfirmationDepth::Confirmed(4).is_satisfied_by(5));

        assert!(!ConfirmationDepth::Stable.is_satisfied_by(63));
        assert!(ConfirmationDepth::Stable.is_satisfied_by(64));
    }

    #[test]
    fn test_ordering() {
        assert!(ConfirmationDepth::Included < ConfirmationDepth::Confirmed(1));
        assert!(ConfirmationDepth::Confirmed(1) < ConfirmationDepth::Confirmed(4));
        assert!(ConfirmationDepth::Confirmed(4) < ConfirmationDepth::Stable);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ConfirmationDepth::Included), "included");
        assert_eq!(
            format!("{}", ConfirmationDepth::Confirmed(8)),
            "confirmed(8)"
        );
        assert_eq!(format!("{}", ConfirmationDepth::Stable), "stable");
    }

    #[test]
    fn test_default() {
        let depth = ConfirmationDepth::default();
        assert_eq!(depth, ConfirmationDepth::DEFAULT_CONFIRMED);
        assert_eq!(depth, ConfirmationDepth::Confirmed(8));
    }

    #[test]
    fn test_is_stable() {
        assert!(!ConfirmationDepth::Included.is_stable());
        assert!(!ConfirmationDepth::Confirmed(8).is_stable());
        assert!(ConfirmationDepth::Stable.is_stable());
    }

    #[test]
    fn test_requires_confirmations() {
        assert!(!ConfirmationDepth::Included.requires_confirmations());
        assert!(ConfirmationDepth::Confirmed(1).requires_confirmations());
        assert!(ConfirmationDepth::Stable.requires_confirmations());
    }
}
