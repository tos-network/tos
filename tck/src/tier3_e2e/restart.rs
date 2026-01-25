//! Node restart and recovery testing for LocalCluster.
//!
//! Tests that nodes can be stopped and restarted without losing state,
//! and that they re-sync with the network after recovery.

use std::time::Duration;

use anyhow::{anyhow, Result};

use super::network::LocalTosNetwork;
use crate::tier2_integration::NodeRpc;

/// Node restart mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartMode {
    /// Graceful shutdown: flush storage, close connections
    Graceful,
    /// Crash: kill without flush (simulates power failure)
    Crash,
}

/// State captured before a node is stopped, for recovery verification.
#[derive(Debug, Clone)]
pub struct PreStopState {
    /// Node index
    pub node_index: usize,
    /// Topoheight at stop time
    pub topoheight: u64,
    /// Tips at stop time
    pub tips: Vec<tos_common::crypto::Hash>,
}

impl LocalTosNetwork {
    /// Capture pre-stop state for later verification.
    pub async fn capture_pre_stop_state(&self, node: usize) -> Result<PreStopState> {
        if node >= self.node_count() {
            return Err(anyhow!("Node index {} out of range", node));
        }
        let topoheight = self.node(node).get_tip_height().await?;
        let tips = self.node(node).get_tips().await?;

        Ok(PreStopState {
            node_index: node,
            topoheight,
            tips,
        })
    }

    /// Verify a restarted node recovered its state correctly.
    ///
    /// Checks that:
    /// 1. The node has at least the pre-stop topoheight
    /// 2. The node eventually syncs to the current network state
    pub async fn verify_node_recovery(
        &self,
        node: usize,
        pre_stop: &PreStopState,
        timeout: Duration,
    ) -> Result<()> {
        if node >= self.node_count() {
            return Err(anyhow!("Node index {} out of range", node));
        }

        // Check state was preserved
        let current_topo = self.node(node).get_tip_height().await?;
        if current_topo < pre_stop.topoheight {
            return Err(anyhow!(
                "Node {} lost state after restart: had {}, now {}",
                node,
                pre_stop.topoheight,
                current_topo
            ));
        }

        // Wait for node to sync up to network state
        let reference_topo = self.node(0).get_tip_height().await?;
        let start = tokio::time::Instant::now();

        loop {
            let synced_topo = self.node(node).get_tip_height().await?;
            if synced_topo >= reference_topo {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(anyhow!(
                    "Node {} did not re-sync after restart: at {}, network at {}",
                    node,
                    synced_topo,
                    reference_topo
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Wait for a specific node to be synced with the network.
    pub async fn wait_node_synced(&self, node: usize, timeout: Duration) -> Result<()> {
        let start = tokio::time::Instant::now();

        loop {
            let node_height = self.node(node).get_tip_height().await?;
            let ref_height = self.node(0).get_tip_height().await?;

            if node_height >= ref_height {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(anyhow!(
                    "Node {} sync timeout: at {}, network at {}",
                    node,
                    node_height,
                    ref_height
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_restart_mode() {
        assert_ne!(RestartMode::Graceful, RestartMode::Crash);
    }

    #[test]
    fn test_pre_stop_state() {
        let state = PreStopState {
            node_index: 1,
            topoheight: 50,
            tips: vec![],
        };
        assert_eq!(state.node_index, 1);
        assert_eq!(state.topoheight, 50);
    }
}
