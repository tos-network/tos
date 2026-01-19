//! Arbitration-related data structures.

pub mod arbiter;
pub mod commit;
pub mod message;
pub mod messages;
pub mod verdict;

pub use arbiter::{
    expertise_domains_to_skill_tags, ArbiterAccount, ArbiterStatus, ArbiterWithdrawError,
    ExpertiseDomain, ARBITER_COOLDOWN_TOPOHEIGHT, CASE_COMPLETION_GRACE_TOPOHEIGHT,
    MAX_WITHDRAWAL_PER_TX, MIN_COOLDOWN_TOPOHEIGHT,
};
pub use commit::{ArbitrationJurorVoteKey, ArbitrationRequestKey, ArbitrationRoundKey};
pub use message::*;
pub use messages::*;
pub use verdict::{DisputeOutcome, VerdictArtifact};
