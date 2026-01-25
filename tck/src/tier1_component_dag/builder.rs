#![allow(missing_docs)]

use super::TestBlockchainDag;
use crate::orchestrator::{Clock, SystemClock};
use crate::tier1_component::VrfConfig;
use anyhow::Result;
use std::sync::Arc;

pub struct TestBlockchainDagBuilder {
    clock: Option<Arc<dyn Clock>>,
    vrf_config: Option<VrfConfig>,
}

impl TestBlockchainDagBuilder {
    pub fn new() -> Self {
        Self {
            clock: None,
            vrf_config: None,
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    pub fn with_vrf_config(mut self, config: VrfConfig) -> Self {
        self.vrf_config = Some(config);
        self
    }

    pub fn with_vrf_key(mut self, secret_hex: String) -> Self {
        self.vrf_config = Some(VrfConfig::new(secret_hex));
        self
    }

    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        let config = self.vrf_config.get_or_insert(VrfConfig {
            secret_key_hex: None,
            chain_id: 3,
        });
        config.chain_id = chain_id;
        self
    }

    pub fn with_random_vrf_key(mut self) -> Self {
        let secret_hex = tos_daemon::vrf::VrfKeyManager::new().secret_key_hex();
        self.vrf_config = Some(VrfConfig::new(secret_hex));
        self
    }

    pub fn build(self) -> Result<TestBlockchainDag> {
        let clock = self.clock.unwrap_or_else(|| Arc::new(SystemClock));
        TestBlockchainDag::new(clock, self.vrf_config)
    }
}

impl Default for TestBlockchainDagBuilder {
    fn default() -> Self {
        Self::new()
    }
}
