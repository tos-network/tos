//! Arbitration-related data structures.

pub mod arbiter;
pub mod verdict;

pub use arbiter::{
    expertise_domains_to_skill_tags, ArbiterAccount, ArbiterStatus, ExpertiseDomain,
};
pub use verdict::{DisputeOutcome, VerdictArtifact};
