//! Arbitration-related data structures.

pub mod arbiter;
pub mod verdict;

pub use arbiter::{ArbiterAccount, ArbiterStatus, ExpertiseDomain};
pub use verdict::{DisputeOutcome, VerdictArtifact};
