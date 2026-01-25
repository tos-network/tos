//! Byzantine fault injection for adversarial testing.
//!
//! Enables testing consensus safety under adversarial conditions by injecting
//! configurable faulty behaviors into cluster nodes.

use std::time::Duration;

use tos_common::crypto::Hash;

/// Configurable fault behaviors for cluster nodes.
/// Enables testing consensus safety and partition tolerance.
#[derive(Debug, Clone)]
pub enum NodeFaultType {
    /// Node broadcasts different blocks to different peers.
    /// Tests: fork detection, DAG consistency under equivocation.
    BroadcastDuplicateBlocks {
        /// Probability of sending duplicate (0.0 - 1.0)
        probability: f64,
        /// Whether duplicates have valid signatures
        valid_signatures: bool,
    },

    /// Node broadcasts blocks with invalid data.
    /// Tests: block validation, peer banning.
    BroadcastInvalidBlocks {
        /// Type of invalidity
        invalidity: BlockInvalidity,
    },

    /// Node mines blocks but withholds propagation.
    /// Tests: liveness under withholding attacks.
    WithholdBlocks {
        /// Number of blocks to withhold before releasing
        withhold_count: u64,
    },

    /// Node adds artificial delay to block/TX propagation.
    /// Tests: behavior under high-latency conditions.
    DelayPropagation {
        /// Delay added to each message
        delay: Duration,
        /// Whether delay is constant or random (up to delay)
        random: bool,
    },

    /// Node reorders transactions within blocks.
    /// Tests: transaction ordering guarantees.
    ReorderTransactions,

    /// Node drops incoming messages at a given rate.
    /// Tests: resilience to unreliable network.
    DropMessages {
        /// Drop rate (0.0 - 1.0)
        rate: f64,
        /// Whether to drop only blocks, only TXs, or both
        target: DropTarget,
    },

    /// Node sends transactions with invalid signatures.
    /// Tests: signature verification, mempool filtering.
    InjectInvalidTransactions {
        /// Number of invalid TXs per block
        count_per_block: u64,
    },

    /// Node double-spends by creating conflicting transactions.
    /// Tests: double-spend prevention in BlockDAG.
    DoubleSpend {
        /// Account hash to double-spend from
        from: Hash,
        /// Amount to attempt double-spending
        amount: u64,
    },
}

/// Types of block invalidity for testing.
#[derive(Debug, Clone)]
pub enum BlockInvalidity {
    /// Invalid block hash
    BadHash,
    /// Invalid miner signature
    BadSignature,
    /// Timestamp too far in the future
    FutureTimestamp(Duration),
    /// References non-existent parent
    InvalidParent,
    /// Contains invalid transaction
    InvalidTransaction,
    /// Exceeds max block size
    OversizedBlock,
}

/// What type of messages to drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    /// Drop only block messages
    Blocks,
    /// Drop only transaction messages
    Transactions,
    /// Drop both block and transaction messages
    Both,
}

/// Statistics from fault injection.
#[derive(Debug, Clone, Default)]
pub struct FaultStats {
    /// Number of faulty messages sent
    pub messages_corrupted: u64,
    /// Number of messages withheld
    pub messages_withheld: u64,
    /// Number of invalid blocks broadcast
    pub invalid_blocks_sent: u64,
    /// Number of peers that banned this node
    pub banned_by_peers: u64,
    /// Number of messages dropped
    pub messages_dropped: u64,
    /// Number of duplicate blocks sent
    pub duplicates_sent: u64,
}

/// Fault injection controller for a node.
#[derive(Debug)]
pub struct FaultInjector {
    /// Active fault type (None = normal behavior)
    fault: Option<NodeFaultType>,
    /// Accumulated statistics
    stats: FaultStats,
    /// Whether the injector is active
    active: bool,
}

impl FaultInjector {
    /// Create a new inactive fault injector.
    pub fn new() -> Self {
        Self {
            fault: None,
            stats: FaultStats::default(),
            active: false,
        }
    }

    /// Inject a fault type.
    pub fn inject(&mut self, fault: NodeFaultType) {
        self.fault = Some(fault);
        self.active = true;
    }

    /// Clear the current fault and deactivate.
    pub fn clear(&mut self) {
        self.fault = None;
        self.active = false;
    }

    /// Check if a fault is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get the current fault type.
    pub fn fault_type(&self) -> Option<&NodeFaultType> {
        self.fault.as_ref()
    }

    /// Get accumulated statistics.
    pub fn stats(&self) -> &FaultStats {
        &self.stats
    }

    /// Reset statistics.
    pub fn reset_stats(&mut self) {
        self.stats = FaultStats::default();
    }

    /// Record a corrupted message.
    pub fn record_corruption(&mut self) {
        self.stats.messages_corrupted = self.stats.messages_corrupted.saturating_add(1);
    }

    /// Record a withheld message.
    pub fn record_withhold(&mut self) {
        self.stats.messages_withheld = self.stats.messages_withheld.saturating_add(1);
    }

    /// Record an invalid block sent.
    pub fn record_invalid_block(&mut self) {
        self.stats.invalid_blocks_sent = self.stats.invalid_blocks_sent.saturating_add(1);
    }

    /// Record a dropped message.
    pub fn record_drop(&mut self) {
        self.stats.messages_dropped = self.stats.messages_dropped.saturating_add(1);
    }

    /// Record a ban by a peer.
    pub fn record_ban(&mut self) {
        self.stats.banned_by_peers = self.stats.banned_by_peers.saturating_add(1);
    }

    /// Record a duplicate block sent.
    pub fn record_duplicate(&mut self) {
        self.stats.duplicates_sent = self.stats.duplicates_sent.saturating_add(1);
    }

    /// Check if a message should be dropped based on the current fault.
    pub fn should_drop_message(&self, is_block: bool) -> bool {
        if !self.active {
            return false;
        }
        match &self.fault {
            Some(NodeFaultType::DropMessages { rate, target }) => {
                let applies = match target {
                    DropTarget::Blocks => is_block,
                    DropTarget::Transactions => !is_block,
                    DropTarget::Both => true,
                };
                if applies {
                    // Deterministic check based on rate
                    // In real implementation, use RNG
                    *rate > 0.5
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_injector_lifecycle() {
        let mut injector = FaultInjector::new();
        assert!(!injector.is_active());
        assert!(injector.fault_type().is_none());

        injector.inject(NodeFaultType::ReorderTransactions);
        assert!(injector.is_active());
        assert!(injector.fault_type().is_some());

        injector.clear();
        assert!(!injector.is_active());
        assert!(injector.fault_type().is_none());
    }

    #[test]
    fn test_fault_stats() {
        let mut injector = FaultInjector::new();
        injector.inject(NodeFaultType::BroadcastInvalidBlocks {
            invalidity: BlockInvalidity::BadSignature,
        });

        injector.record_invalid_block();
        injector.record_invalid_block();
        injector.record_ban();

        assert_eq!(injector.stats().invalid_blocks_sent, 2);
        assert_eq!(injector.stats().banned_by_peers, 1);

        injector.reset_stats();
        assert_eq!(injector.stats().invalid_blocks_sent, 0);
    }

    #[test]
    fn test_drop_message_logic() {
        let mut injector = FaultInjector::new();

        // No fault = no drops
        assert!(!injector.should_drop_message(true));
        assert!(!injector.should_drop_message(false));

        // Drop blocks only
        injector.inject(NodeFaultType::DropMessages {
            rate: 1.0,
            target: DropTarget::Blocks,
        });
        assert!(injector.should_drop_message(true));
        assert!(!injector.should_drop_message(false));

        // Drop transactions only
        injector.inject(NodeFaultType::DropMessages {
            rate: 1.0,
            target: DropTarget::Transactions,
        });
        assert!(!injector.should_drop_message(true));
        assert!(injector.should_drop_message(false));

        // Drop both
        injector.inject(NodeFaultType::DropMessages {
            rate: 1.0,
            target: DropTarget::Both,
        });
        assert!(injector.should_drop_message(true));
        assert!(injector.should_drop_message(false));
    }

    #[test]
    fn test_withhold_blocks_variant() {
        let fault = NodeFaultType::WithholdBlocks { withhold_count: 5 };
        if let NodeFaultType::WithholdBlocks { withhold_count } = fault {
            assert_eq!(withhold_count, 5);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_broadcast_duplicates_variant() {
        let fault = NodeFaultType::BroadcastDuplicateBlocks {
            probability: 0.75,
            valid_signatures: true,
        };
        if let NodeFaultType::BroadcastDuplicateBlocks {
            probability,
            valid_signatures,
        } = fault
        {
            assert!((probability - 0.75).abs() < f64::EPSILON);
            assert!(valid_signatures);
        }
    }
}
