//! Shared test helpers for migrated parallel execution tests

use std::sync::Arc;
use tokio::sync::RwLock;

use tos_common::{
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    crypto::{Hash, Hashable, PublicKey, elgamal::CompressedPublicKey},
    immutable::Immutable,
    serializer::{Writer, Reader, Serializer},
};
use tos_environment::Environment;
use tos_daemon::core::state::parallel_chain_state::ParallelChainState;
use tos_testing_integration::MockStorage;

/// Create a dummy block for testing
pub fn create_dummy_block() -> (Block, Hash) {
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&[0u8; 32]);
    let data = writer.as_bytes();

    let mut reader = Reader::new(data);
    let miner = CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey");

    let header = BlockHeader::new_simple(
        BlockVersion::V0,
        vec![],
        0,
        [0u8; EXTRA_NONCE_SIZE],
        miner,
        Hash::zero(),
    );

    let block = Block::new(Immutable::Owned(header), vec![]);
    let hash = block.hash();
    (block, hash)
}

/// Create a ParallelChainState for testing
pub async fn create_parallel_state(
    storage: MockStorage,
) -> Result<Arc<ParallelChainState<MockStorage>>, Box<dyn std::error::Error>> {
    let (block, block_hash) = create_dummy_block();
    let storage_arc = Arc::new(RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let parallel_state = ParallelChainState::new(
        storage_arc,
        environment,
        0,  // stable_topoheight
        1,  // topoheight
        BlockVersion::V0,
        block,
        block_hash,
    ).await;

    Ok(parallel_state)
}
