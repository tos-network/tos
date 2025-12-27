// Priority ordering for ScheduledExecution
// Implements hybrid priority model: offer amount first, then FIFO by registration time

use std::cmp::Ordering;

use super::ScheduledExecution;

/// Extension trait for priority comparison of scheduled executions
pub trait ScheduledExecutionPriority {
    /// Compare two scheduled executions by priority
    ///
    /// Priority order (highest first):
    /// 1. Higher offer_amount wins
    /// 2. If equal offers, earlier registration_topoheight wins (FIFO)
    /// 3. If still equal, lexicographic hash comparison (deterministic tiebreaker)
    fn priority_cmp(&self, other: &Self) -> Ordering;

    /// Calculate a numeric priority score for this execution
    /// Higher score = higher priority
    /// Score = (offer_amount × OFFER_WEIGHT) + (MAX_TOPO - registration_topoheight)
    fn priority_score(&self) -> u128;
}

/// Weight multiplier for offer amount in priority scoring
/// 1 native token = 1000 priority points
pub const OFFER_WEIGHT: u128 = 1000;

/// Maximum topoheight value for priority inversion
pub const MAX_TOPO_FOR_PRIORITY: u64 = u64::MAX;

impl ScheduledExecutionPriority for ScheduledExecution {
    fn priority_cmp(&self, other: &Self) -> Ordering {
        // 1. Higher offer = higher priority
        match self.offer_amount.cmp(&other.offer_amount) {
            Ordering::Equal => {
                // 2. Earlier registration = higher priority (FIFO)
                // Note: lower topoheight = earlier = higher priority
                match other
                    .registration_topoheight
                    .cmp(&self.registration_topoheight)
                {
                    Ordering::Equal => {
                        // 3. Deterministic tiebreaker by hash
                        self.hash.cmp(&other.hash)
                    }
                    ord => ord,
                }
            }
            ord => ord,
        }
    }

    fn priority_score(&self) -> u128 {
        let offer_component = (self.offer_amount as u128) * OFFER_WEIGHT;
        let fifo_component = MAX_TOPO_FOR_PRIORITY as u128 - self.registration_topoheight as u128;
        offer_component + fifo_component
    }
}

impl Ord for ScheduledExecution {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority_cmp(other)
    }
}

impl PartialOrd for ScheduledExecution {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::scheduled_execution::{ScheduledExecutionKind, ScheduledExecutionStatus};
    use crate::crypto::Hash;
    use indexmap::IndexMap;

    fn make_execution(offer: u64, reg_topo: u64, hash_byte: u8) -> ScheduledExecution {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0] = hash_byte;

        ScheduledExecution {
            hash: Hash::new(hash_bytes),
            contract: Hash::zero(),
            chunk_id: 0,
            params: vec![],
            max_gas: 100_000,
            kind: ScheduledExecutionKind::TopoHeight(1000),
            gas_sources: IndexMap::new(),
            offer_amount: offer,
            scheduler_contract: Hash::zero(),
            input_data: vec![],
            registration_topoheight: reg_topo,
            status: ScheduledExecutionStatus::Pending,
            defer_count: 0,
        }
    }

    #[test]
    fn test_higher_offer_wins() {
        let high_offer = make_execution(1000, 100, 0xAA);
        let low_offer = make_execution(100, 50, 0xBB);

        // Higher offer should have higher priority
        assert_eq!(high_offer.priority_cmp(&low_offer), Ordering::Greater);
        assert_eq!(low_offer.priority_cmp(&high_offer), Ordering::Less);
    }

    #[test]
    fn test_fifo_when_equal_offer() {
        let earlier = make_execution(500, 100, 0xAA);
        let later = make_execution(500, 200, 0xBB);

        // Earlier registration (lower topoheight) should have higher priority
        assert_eq!(earlier.priority_cmp(&later), Ordering::Greater);
        assert_eq!(later.priority_cmp(&earlier), Ordering::Less);
    }

    #[test]
    fn test_hash_tiebreaker() {
        let exec_a = make_execution(500, 100, 0x11);
        let exec_b = make_execution(500, 100, 0x22);

        // With same offer and registration, hash determines order
        let cmp = exec_a.priority_cmp(&exec_b);
        assert!(cmp != Ordering::Equal); // Must have deterministic order
    }

    #[test]
    fn test_priority_score() {
        let high_offer = make_execution(1000, 100, 0);
        let low_offer = make_execution(100, 100, 0);

        // Higher offer should have higher score
        assert!(high_offer.priority_score() > low_offer.priority_score());
    }

    #[test]
    fn test_zero_offer_fifo_only() {
        let first = make_execution(0, 100, 0xAA);
        let second = make_execution(0, 200, 0xBB);

        // With zero offers, pure FIFO ordering
        assert_eq!(first.priority_cmp(&second), Ordering::Greater);
    }

    #[test]
    fn test_sorting() {
        let mut executions = [
            make_execution(100, 300, 0x33),  // Low offer, late
            make_execution(1000, 100, 0x11), // High offer, early
            make_execution(500, 200, 0x22),  // Medium offer, medium time
            make_execution(1000, 200, 0x44), // High offer, late
        ];

        // Sort descending by priority (highest first)
        executions.sort_by(|a, b| b.cmp(a));

        // Expected order: high offer early, high offer late, medium, low
        assert_eq!(executions[0].offer_amount, 1000);
        assert_eq!(executions[0].registration_topoheight, 100);
        assert_eq!(executions[1].offer_amount, 1000);
        assert_eq!(executions[1].registration_topoheight, 200);
        assert_eq!(executions[2].offer_amount, 500);
        assert_eq!(executions[3].offer_amount, 100);
    }
}
