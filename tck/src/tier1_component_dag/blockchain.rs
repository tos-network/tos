#![allow(missing_docs)]

use super::block::TestBlockDag;
use crate::orchestrator::Clock;
use crate::tier1_component::VrfConfig;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tos_common::block::BlockVrfData;
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_common::crypto::{hash, Hash, KeyPair};
use tos_daemon::vrf::VrfKeyManager;

pub struct TestBlockchainDag {
    _clock: Arc<dyn Clock>,
    blocks_by_hash: RwLock<HashMap<Hash, TestBlockDag>>,
    tips: RwLock<HashSet<Hash>>,
    topoheight: RwLock<u64>,
    vrf_key_manager: Option<VrfKeyManager>,
    chain_id: u64,
    miner_keypair: KeyPair,
    _genesis_hash: Hash,
}

impl TestBlockchainDag {
    pub(crate) fn new(clock: Arc<dyn Clock>, vrf_config: Option<VrfConfig>) -> Result<Self> {
        let (vrf_key_manager, chain_id) = if let Some(config) = vrf_config {
            let manager = if let Some(ref secret_hex) = config.secret_key_hex {
                Some(VrfKeyManager::from_hex(secret_hex).context("Invalid VRF secret key")?)
            } else {
                None
            };
            (manager, config.chain_id)
        } else {
            (None, 3)
        };

        let miner_keypair = KeyPair::new();
        let miner_pk = miner_keypair.get_public_key().compress();
        let genesis_hash = Hash::zero();

        let genesis = TestBlockDag {
            hash: genesis_hash.clone(),
            height: 0,
            topoheight: 0,
            parents: Vec::new(),
            selected_parent: genesis_hash.clone(),
            vrf_data: None,
            miner: miner_pk,
        };

        let mut blocks_by_hash = HashMap::new();
        blocks_by_hash.insert(genesis_hash.clone(), genesis);

        let mut tips = HashSet::new();
        tips.insert(genesis_hash.clone());

        Ok(Self {
            _clock: clock,
            blocks_by_hash: RwLock::new(blocks_by_hash),
            tips: RwLock::new(tips),
            topoheight: RwLock::new(0),
            vrf_key_manager,
            chain_id,
            miner_keypair,
            _genesis_hash: genesis_hash,
        })
    }

    pub fn get_tips(&self) -> Vec<Hash> {
        self.tips.read().iter().cloned().collect()
    }

    pub fn get_block(&self, hash: &Hash) -> Option<TestBlockDag> {
        self.blocks_by_hash.read().get(hash).cloned()
    }

    pub fn get_block_vrf_data_by_hash(&self, hash: &Hash) -> Option<BlockVrfData> {
        self.blocks_by_hash
            .read()
            .get(hash)
            .and_then(|b| b.vrf_data.clone())
    }

    pub fn miner_public_key(&self) -> CompressedPublicKey {
        self.miner_keypair.get_public_key().compress()
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn mine_block_on_tip(&self, tip_hash: &Hash) -> Result<TestBlockDag> {
        self.mine_block_with_parents(vec![tip_hash.clone()], tip_hash.clone())
    }

    pub fn mine_block_with_parents(
        &self,
        mut parents: Vec<Hash>,
        selected_parent: Hash,
    ) -> Result<TestBlockDag> {
        if parents.is_empty() {
            anyhow::bail!("DAG block must have at least one parent");
        }

        let blocks_by_hash = self.blocks_by_hash.read();
        for parent in &parents {
            if !blocks_by_hash.contains_key(parent) {
                anyhow::bail!("Parent block not found: {}", parent);
            }
        }
        drop(blocks_by_hash);

        parents.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));

        let height = parents
            .iter()
            .filter_map(|parent| self.blocks_by_hash.read().get(parent).map(|b| b.height))
            .max()
            .unwrap_or(0)
            .saturating_add(1);

        let mut topoheight_guard = self.topoheight.write();
        *topoheight_guard = topoheight_guard.saturating_add(1);
        let topoheight = *topoheight_guard;
        drop(topoheight_guard);

        let miner_pk = self.miner_keypair.get_public_key().compress();
        let miner_pk_bytes: [u8; 32] = *miner_pk.as_bytes();

        let block_hash = compute_block_hash(height, topoheight, &parents, &miner_pk_bytes);

        let vrf_data = if let Some(ref vrf_mgr) = self.vrf_key_manager {
            let block_hash_bytes: [u8; 32] = *block_hash.as_bytes();
            match vrf_mgr.sign(
                self.chain_id,
                &block_hash_bytes,
                &miner_pk,
                &self.miner_keypair,
            ) {
                Ok(vrf_result) => Some(BlockVrfData::new(
                    vrf_result.public_key.to_bytes(),
                    vrf_result.output.to_bytes(),
                    vrf_result.proof.to_bytes(),
                    vrf_result.binding_signature.to_bytes(),
                )),
                Err(e) => {
                    if log::log_enabled!(log::Level::Warn) {
                        log::warn!("VRF signing failed for DAG block {}: {:?}", height, e);
                    }
                    None
                }
            }
        } else {
            None
        };

        let block = TestBlockDag {
            hash: block_hash.clone(),
            height,
            topoheight,
            parents: parents.clone(),
            selected_parent,
            vrf_data,
            miner: miner_pk,
        };

        let mut blocks_by_hash = self.blocks_by_hash.write();
        if blocks_by_hash.contains_key(&block_hash) {
            anyhow::bail!("Duplicate block hash: {}", block_hash);
        }
        blocks_by_hash.insert(block_hash.clone(), block.clone());
        drop(blocks_by_hash);

        let mut tips = self.tips.write();
        for parent in &parents {
            tips.remove(parent);
        }
        tips.insert(block_hash);
        drop(tips);

        Ok(block)
    }

    pub fn receive_block(&self, block: TestBlockDag) -> Result<()> {
        let mut blocks_by_hash = self.blocks_by_hash.write();
        if blocks_by_hash.contains_key(&block.hash) {
            return Ok(());
        }

        for parent in &block.parents {
            if !blocks_by_hash.contains_key(parent) {
                anyhow::bail!("Parent block not found: {}", parent);
            }
        }

        blocks_by_hash.insert(block.hash.clone(), block.clone());
        drop(blocks_by_hash);

        let mut topoheight = self.topoheight.write();
        if block.topoheight > *topoheight {
            *topoheight = block.topoheight;
        }
        drop(topoheight);

        let mut tips = self.tips.write();
        for parent in &block.parents {
            tips.remove(parent);
        }
        tips.insert(block.hash);
        Ok(())
    }
}

fn compute_block_hash(height: u64, topoheight: u64, parents: &[Hash], miner_pk: &[u8; 32]) -> Hash {
    let mut input = Vec::with_capacity(16 + parents.len() * 32 + miner_pk.len());
    input.extend_from_slice(&height.to_le_bytes());
    input.extend_from_slice(&topoheight.to_le_bytes());
    for parent in parents {
        input.extend_from_slice(parent.as_bytes());
    }
    input.extend_from_slice(miner_pk);
    hash(&input)
}
