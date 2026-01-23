//! Network partition testing for multi-node clusters.
//!
//! Provides `PartitionController` for creating, managing, and healing
//! network partitions in a test cluster. Supports arbitrary partition
//! topologies and lifecycle callbacks for verification during partition.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;

use super::network::LocalTosNetwork;
use super::transport::{InterceptRule, LocalhostTransport, TransportAction};

/// Defines a network partition as a set of node groups.
///
/// Nodes within the same group can communicate, but nodes in different
/// groups cannot.
#[derive(Debug, Clone)]
pub struct Partition {
    /// Groups of nodes that can communicate with each other.
    /// Nodes in different groups are isolated.
    pub groups: Vec<HashSet<usize>>,
}

impl Partition {
    /// Create a two-way partition: nodes in `group_a` are isolated from `group_b`.
    pub fn two_way(group_a: Vec<usize>, group_b: Vec<usize>) -> Self {
        Self {
            groups: vec![group_a.into_iter().collect(), group_b.into_iter().collect()],
        }
    }

    /// Create a partition that isolates a single node from all others.
    pub fn isolate_node(node: usize, total_nodes: usize) -> Self {
        let isolated: HashSet<usize> = [node].into_iter().collect();
        let rest: HashSet<usize> = (0..total_nodes).filter(|&n| n != node).collect();
        Self {
            groups: vec![isolated, rest],
        }
    }

    /// Check if two nodes can communicate in this partition.
    pub fn can_communicate(&self, node_a: usize, node_b: usize) -> bool {
        for group in &self.groups {
            if group.contains(&node_a) && group.contains(&node_b) {
                return true;
            }
        }
        false
    }

    /// Get the group index for a specific node.
    pub fn group_of(&self, node: usize) -> Option<usize> {
        self.groups.iter().position(|g| g.contains(&node))
    }
}

/// Controls network partitions in a test cluster.
///
/// # Example
/// ```ignore
/// let controller = PartitionController::new(transport.clone());
///
/// // Create partition: [0, 1] vs [2]
/// controller.create_partition(Partition::two_way(vec![0, 1], vec![2])).await;
///
/// // Do operations during partition...
///
/// // Heal partition
/// controller.heal().await;
/// ```
pub struct PartitionController {
    /// Transport layer to apply rules to
    transport: LocalhostTransport,
    /// Currently active partition (None = fully connected)
    active_partition: Option<Partition>,
}

impl PartitionController {
    /// Create a new partition controller for the given transport.
    pub fn new(transport: LocalhostTransport) -> Self {
        Self {
            transport,
            active_partition: None,
        }
    }

    /// Create a network partition by adding drop rules to the transport.
    pub async fn create_partition(&mut self, partition: Partition) {
        // First heal any existing partition
        self.heal().await;

        // Add drop rules for cross-group communication
        for (i, group_a) in partition.groups.iter().enumerate() {
            for (j, group_b) in partition.groups.iter().enumerate() {
                if i == j {
                    continue;
                }
                for &from in group_a {
                    for &to in group_b {
                        self.transport
                            .add_rule(InterceptRule {
                                from: Some(from),
                                to: Some(to),
                                message_type: None,
                                action: TransportAction::Drop,
                            })
                            .await;
                    }
                }
            }
        }

        self.active_partition = Some(partition);
    }

    /// Heal the current partition (remove all partition rules).
    pub async fn heal(&mut self) {
        self.transport.clear_rules().await;
        self.active_partition = None;
    }

    /// Check if a partition is currently active.
    pub fn is_partitioned(&self) -> bool {
        self.active_partition.is_some()
    }

    /// Get the current partition configuration.
    pub fn current_partition(&self) -> Option<&Partition> {
        self.active_partition.as_ref()
    }

    /// Get the underlying transport.
    pub fn transport(&self) -> &LocalhostTransport {
        &self.transport
    }
}

/// Result of a partition test run.
#[derive(Debug)]
pub struct PartitionTestResult {
    /// Whether all assertions passed during the partition
    pub during_partition_ok: bool,
    /// Whether all assertions passed after healing
    pub after_heal_ok: bool,
    /// Errors encountered during the test
    pub errors: Vec<String>,
    /// Duration of the partition
    pub partition_duration: Duration,
}

/// Run a partition test with lifecycle callbacks.
///
/// # Lifecycle
/// 1. `before_partition` - Setup before partition is applied
/// 2. Create partition
/// 3. `during_partition` - Assertions during partition
/// 4. Wait for `partition_duration`
/// 5. Heal partition
/// 6. `after_heal` - Assertions after partition heals
///
/// # Example
/// ```ignore
/// let result = run_partition_test(
///     &network,
///     transport.clone(),
///     Partition::isolate_node(2, 3),
///     Duration::from_secs(5),
///     |net| Box::pin(async move {
///         // Mine blocks on partition A
///         net.node(0).mine_block().await?;
///         Ok(())
///     }),
///     |net| Box::pin(async move {
///         // Verify partition B is stale
///         let h0 = net.node(0).get_tip_height().await?;
///         let h2 = net.node(2).get_tip_height().await?;
///         assert!(h0 > h2);
///         Ok(())
///     }),
///     |net| Box::pin(async move {
///         // Verify all nodes converge
///         let h0 = net.node(0).get_tip_height().await?;
///         let h2 = net.node(2).get_tip_height().await?;
///         assert_eq!(h0, h2);
///         Ok(())
///     }),
/// ).await;
/// ```
pub async fn run_partition_test<'n, F1, F2, F3>(
    network: &'n LocalTosNetwork,
    transport: LocalhostTransport,
    partition: Partition,
    partition_duration: Duration,
    before_partition: F1,
    during_partition: F2,
    after_heal: F3,
) -> PartitionTestResult
where
    F1: AsyncFn<&'n LocalTosNetwork>,
    F2: AsyncFn<&'n LocalTosNetwork>,
    F3: AsyncFn<&'n LocalTosNetwork>,
{
    let mut errors = Vec::new();
    let start = std::time::Instant::now();

    // Phase 1: Before partition
    if let Err(e) = before_partition.call(network).await {
        errors.push(format!("before_partition: {}", e));
    }

    // Phase 2: Create partition
    let mut controller = PartitionController::new(transport);
    controller.create_partition(partition).await;

    // Phase 3: During partition
    let during_ok = match during_partition.call(network).await {
        Ok(_) => true,
        Err(e) => {
            errors.push(format!("during_partition: {}", e));
            false
        }
    };

    // Phase 4: Wait
    tokio::time::sleep(partition_duration).await;

    let partition_elapsed = start.elapsed();

    // Phase 5: Heal
    controller.heal().await;

    // Phase 6: After heal
    let after_ok = match after_heal.call(network).await {
        Ok(_) => true,
        Err(e) => {
            errors.push(format!("after_heal: {}", e));
            false
        }
    };

    PartitionTestResult {
        during_partition_ok: during_ok,
        after_heal_ok: after_ok,
        errors,
        partition_duration: partition_elapsed,
    }
}

/// Helper trait for async closures in partition tests.
/// This avoids Box<dyn Future> lifetime issues.
pub trait AsyncFn<A> {
    /// Call the async function.
    fn call(&self, arg: A) -> impl std::future::Future<Output = Result<()>> + Send;
}

impl<F, Fut, A> AsyncFn<A> for F
where
    F: Fn(A) -> Fut,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    fn call(&self, arg: A) -> impl std::future::Future<Output = Result<()>> + Send {
        (self)(arg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_two_way() {
        let p = Partition::two_way(vec![0, 1], vec![2, 3]);
        assert!(p.can_communicate(0, 1));
        assert!(p.can_communicate(2, 3));
        assert!(!p.can_communicate(0, 2));
        assert!(!p.can_communicate(1, 3));
    }

    #[test]
    fn test_partition_isolate() {
        let p = Partition::isolate_node(2, 5);
        assert!(!p.can_communicate(0, 2));
        assert!(!p.can_communicate(1, 2));
        assert!(p.can_communicate(0, 1));
        assert!(p.can_communicate(0, 3));
        assert!(p.can_communicate(3, 4));
    }

    #[test]
    fn test_partition_group_of() {
        let p = Partition::two_way(vec![0, 1], vec![2, 3]);
        assert_eq!(p.group_of(0), Some(0));
        assert_eq!(p.group_of(2), Some(1));
        assert_eq!(p.group_of(5), None);
    }

    #[tokio::test]
    async fn test_partition_controller() {
        let transport = LocalhostTransport::new();
        let mut controller = PartitionController::new(transport);

        assert!(!controller.is_partitioned());

        controller
            .create_partition(Partition::two_way(vec![0], vec![1]))
            .await;
        assert!(controller.is_partitioned());

        controller.heal().await;
        assert!(!controller.is_partitioned());
    }
}
