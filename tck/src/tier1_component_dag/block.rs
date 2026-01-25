#![allow(missing_docs)]

use tos_common::block::BlockVrfData;
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_common::crypto::Hash;

#[derive(Debug, Clone)]
pub struct TestBlockDag {
    pub hash: Hash,
    pub height: u64,
    pub topoheight: u64,
    pub parents: Vec<Hash>,
    pub selected_parent: Hash,
    pub vrf_data: Option<BlockVrfData>,
    pub miner: CompressedPublicKey,
}
