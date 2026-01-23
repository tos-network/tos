//! Transport abstraction for multi-node communication.
//!
//! Provides a pluggable transport layer that supports message interception,
//! delay injection, and selective message dropping for testing network
//! fault scenarios.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

/// A message being transported between nodes.
#[derive(Debug, Clone)]
pub struct TransportMessage {
    /// Source node index
    pub from: usize,
    /// Destination node index
    pub to: usize,
    /// Message payload (serialized)
    pub payload: Vec<u8>,
    /// Message type identifier
    pub message_type: MessageType,
    /// Timestamp when message was sent
    pub sent_at: u64,
}

/// Type of P2P message being transported.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageType {
    /// Block propagation
    Block,
    /// Transaction propagation
    Transaction,
    /// Ping/pong keepalive
    Ping,
    /// Peer discovery
    PeerExchange,
    /// Chain sync request/response
    ChainSync,
    /// Bootstrap sync data
    BootstrapSync,
    /// Other/unknown message types
    Other(String),
}

/// Action to take on an intercepted message.
#[derive(Debug, Clone)]
pub enum TransportAction {
    /// Deliver the message normally
    Deliver,
    /// Drop the message (never delivered)
    Drop,
    /// Delay the message by the specified duration
    Delay(Duration),
    /// Modify the payload before delivery
    Modify(Vec<u8>),
}

/// A rule for intercepting and modifying transport messages.
#[derive(Debug, Clone)]
pub struct InterceptRule {
    /// Source node filter (None = any source)
    pub from: Option<usize>,
    /// Destination node filter (None = any destination)
    pub to: Option<usize>,
    /// Message type filter (None = any type)
    pub message_type: Option<MessageType>,
    /// Action to take when rule matches
    pub action: TransportAction,
}

/// Transport layer abstraction for multi-node testing.
///
/// Provides hooks for intercepting, delaying, and dropping messages
/// between nodes in a test cluster.
#[derive(Debug, Clone, Default)]
pub struct LocalhostTransport {
    /// Active intercept rules (evaluated in order, first match wins)
    rules: Arc<RwLock<Vec<InterceptRule>>>,
    /// Per-link delay overrides (from, to) -> delay
    link_delays: Arc<RwLock<HashMap<(usize, usize), Duration>>>,
    /// Whether the transport is active (messages are delivered when true)
    active: Arc<RwLock<bool>>,
    /// Message delivery statistics
    stats: Arc<RwLock<TransportStats>>,
}

/// Statistics about message delivery through the transport.
#[derive(Debug, Clone, Default)]
pub struct TransportStats {
    /// Total messages sent
    pub messages_sent: u64,
    /// Messages delivered successfully
    pub messages_delivered: u64,
    /// Messages dropped by rules
    pub messages_dropped: u64,
    /// Messages delayed by rules
    pub messages_delayed: u64,
    /// Messages modified by rules
    pub messages_modified: u64,
}

impl LocalhostTransport {
    /// Create a new transport with no interception rules.
    pub fn new() -> Self {
        Self {
            active: Arc::new(RwLock::new(true)),
            ..Default::default()
        }
    }

    /// Add an intercept rule.
    pub async fn add_rule(&self, rule: InterceptRule) {
        self.rules.write().await.push(rule);
    }

    /// Clear all intercept rules.
    pub async fn clear_rules(&self) {
        self.rules.write().await.clear();
    }

    /// Set a fixed delay on a specific link.
    pub async fn set_link_delay(&self, from: usize, to: usize, delay: Duration) {
        self.link_delays.write().await.insert((from, to), delay);
    }

    /// Remove delay from a specific link.
    pub async fn clear_link_delay(&self, from: usize, to: usize) {
        self.link_delays.write().await.remove(&(from, to));
    }

    /// Block all traffic (simulates total network failure).
    pub async fn block_all(&self) {
        *self.active.write().await = false;
    }

    /// Resume all traffic.
    pub async fn resume_all(&self) {
        *self.active.write().await = true;
    }

    /// Determine what action to take for a given message.
    pub async fn evaluate(&self, message: &TransportMessage) -> TransportAction {
        // Check if transport is active
        if !*self.active.read().await {
            let mut stats = self.stats.write().await;
            stats.messages_sent = stats.messages_sent.saturating_add(1);
            stats.messages_dropped = stats.messages_dropped.saturating_add(1);
            return TransportAction::Drop;
        }

        let mut stats = self.stats.write().await;
        stats.messages_sent = stats.messages_sent.saturating_add(1);

        // Check intercept rules (first match wins)
        let rules = self.rules.read().await;
        for rule in rules.iter() {
            if Self::rule_matches(rule, message) {
                match &rule.action {
                    TransportAction::Drop => {
                        stats.messages_dropped = stats.messages_dropped.saturating_add(1);
                    }
                    TransportAction::Delay(_) => {
                        stats.messages_delayed = stats.messages_delayed.saturating_add(1);
                    }
                    TransportAction::Modify(_) => {
                        stats.messages_modified = stats.messages_modified.saturating_add(1);
                    }
                    TransportAction::Deliver => {
                        stats.messages_delivered = stats.messages_delivered.saturating_add(1);
                    }
                }
                return rule.action.clone();
            }
        }

        // Check link delays
        let delays = self.link_delays.read().await;
        if let Some(delay) = delays.get(&(message.from, message.to)) {
            stats.messages_delayed = stats.messages_delayed.saturating_add(1);
            return TransportAction::Delay(*delay);
        }

        // Default: deliver
        stats.messages_delivered = stats.messages_delivered.saturating_add(1);
        TransportAction::Deliver
    }

    /// Get transport statistics.
    pub async fn stats(&self) -> TransportStats {
        self.stats.read().await.clone()
    }

    /// Reset transport statistics.
    pub async fn reset_stats(&self) {
        *self.stats.write().await = TransportStats::default();
    }

    /// Check if a rule matches a message.
    fn rule_matches(rule: &InterceptRule, message: &TransportMessage) -> bool {
        if let Some(from) = rule.from {
            if message.from != from {
                return false;
            }
        }
        if let Some(to) = rule.to {
            if message.to != to {
                return false;
            }
        }
        if let Some(ref msg_type) = rule.message_type {
            if message.message_type != *msg_type {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transport_default_deliver() {
        let transport = LocalhostTransport::new();
        let msg = TransportMessage {
            from: 0,
            to: 1,
            payload: vec![1, 2, 3],
            message_type: MessageType::Block,
            sent_at: 0,
        };

        let action = transport.evaluate(&msg).await;
        assert!(matches!(action, TransportAction::Deliver));
    }

    #[tokio::test]
    async fn test_transport_drop_rule() {
        let transport = LocalhostTransport::new();
        transport
            .add_rule(InterceptRule {
                from: Some(0),
                to: None,
                message_type: None,
                action: TransportAction::Drop,
            })
            .await;

        let msg = TransportMessage {
            from: 0,
            to: 1,
            payload: vec![],
            message_type: MessageType::Transaction,
            sent_at: 0,
        };

        let action = transport.evaluate(&msg).await;
        assert!(matches!(action, TransportAction::Drop));

        let stats = transport.stats().await;
        assert_eq!(stats.messages_dropped, 1);
    }

    #[tokio::test]
    async fn test_transport_block_all() {
        let transport = LocalhostTransport::new();
        transport.block_all().await;

        let msg = TransportMessage {
            from: 0,
            to: 1,
            payload: vec![],
            message_type: MessageType::Ping,
            sent_at: 0,
        };

        let action = transport.evaluate(&msg).await;
        assert!(matches!(action, TransportAction::Drop));

        transport.resume_all().await;
        let action = transport.evaluate(&msg).await;
        assert!(matches!(action, TransportAction::Deliver));
    }

    #[tokio::test]
    async fn test_transport_link_delay() {
        let transport = LocalhostTransport::new();
        transport
            .set_link_delay(0, 1, Duration::from_millis(500))
            .await;

        let msg = TransportMessage {
            from: 0,
            to: 1,
            payload: vec![],
            message_type: MessageType::Block,
            sent_at: 0,
        };

        let action = transport.evaluate(&msg).await;
        assert!(matches!(action, TransportAction::Delay(_)));

        // Different link should not be delayed
        let msg2 = TransportMessage {
            from: 1,
            to: 0,
            payload: vec![],
            message_type: MessageType::Block,
            sent_at: 0,
        };
        let action2 = transport.evaluate(&msg2).await;
        assert!(matches!(action2, TransportAction::Deliver));
    }

    #[tokio::test]
    async fn test_transport_stats() {
        let transport = LocalhostTransport::new();
        transport
            .add_rule(InterceptRule {
                from: Some(0),
                to: None,
                message_type: None,
                action: TransportAction::Drop,
            })
            .await;

        let msg_drop = TransportMessage {
            from: 0,
            to: 1,
            payload: vec![],
            message_type: MessageType::Block,
            sent_at: 0,
        };
        let msg_deliver = TransportMessage {
            from: 1,
            to: 0,
            payload: vec![],
            message_type: MessageType::Block,
            sent_at: 0,
        };

        transport.evaluate(&msg_drop).await;
        transport.evaluate(&msg_deliver).await;

        let stats = transport.stats().await;
        assert_eq!(stats.messages_sent, 2);
        assert_eq!(stats.messages_dropped, 1);
        assert_eq!(stats.messages_delivered, 1);

        transport.reset_stats().await;
        let stats = transport.stats().await;
        assert_eq!(stats.messages_sent, 0);
    }
}
