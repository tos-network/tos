#![allow(missing_docs)]

//! TestDaemon2 - DAG test daemon

use crate::orchestrator::Clock;
use crate::tier1_component_dag::{TestBlockDag, TestBlockchainDag};
use anyhow::Result;
use std::sync::Arc;

pub struct TestDaemonDag {
    blockchain: TestBlockchainDag,
    clock: Arc<dyn Clock>,
    is_running: bool,
}

impl TestDaemonDag {
    pub(crate) fn new(blockchain: TestBlockchainDag, clock: Arc<dyn Clock>) -> Self {
        Self {
            blockchain,
            clock,
            is_running: true,
        }
    }

    fn ensure_running(&self) -> Result<()> {
        if !self.is_running {
            anyhow::bail!("Daemon is not running");
        }
        Ok(())
    }

    pub fn blockchain(&self) -> &TestBlockchainDag {
        &self.blockchain
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub fn mine_block_on_tip(&self, tip_hash: &tos_common::crypto::Hash) -> Result<TestBlockDag> {
        self.ensure_running()?;
        self.blockchain.mine_block_on_tip(tip_hash)
    }

    pub fn receive_block(&self, block: TestBlockDag) -> Result<()> {
        self.ensure_running()?;
        self.blockchain.receive_block(block)
    }

    pub fn get_tips(&self) -> Vec<tos_common::crypto::Hash> {
        self.blockchain.get_tips()
    }

    pub fn get_block(&self, hash: &tos_common::crypto::Hash) -> Option<TestBlockDag> {
        self.blockchain.get_block(hash)
    }

    pub fn get_block_vrf_data_by_hash(
        &self,
        hash: &tos_common::crypto::Hash,
    ) -> Option<tos_common::block::BlockVrfData> {
        self.blockchain.get_block_vrf_data_by_hash(hash)
    }

    pub fn stop(&mut self) {
        self.is_running = false;
    }
}
