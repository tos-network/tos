use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::{Block, BlockHeader, TopoHeight},
    crypto::Hash,
    immutable::Immutable,
};

use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{
        sled::{TOP_HEIGHT, TOP_TOPO_HEIGHT},
        BlockProvider, DagOrderProvider, DifficultyProvider, SledStorage, StateProvider,
    },
};

#[async_trait]
impl StateProvider for SledStorage {
    async fn get_top_block_hash(&self) -> Result<Hash, BlockchainError> {
        trace!("get top block hash");
        self.get_hash_at_topo_height(self.get_top_topoheight().await?)
            .await
    }

    async fn get_top_topoheight(&self) -> Result<TopoHeight, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get top topoheight");
        }
        self.load_from_disk(&self.extra, TOP_TOPO_HEIGHT, DiskContext::TopTopoHeight)
    }

    async fn set_top_topoheight(&mut self, topoheight: TopoHeight) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set new top topoheight at {}", topoheight);
        }
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.extra,
            TOP_TOPO_HEIGHT,
            &topoheight.to_be_bytes(),
        )?;
        Ok(())
    }

    async fn get_top_height(&self) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get top height");
        }
        self.load_from_disk(&self.extra, TOP_HEIGHT, DiskContext::TopHeight)
    }

    async fn set_top_height(&mut self, height: u64) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set new top height at {}", height);
        }
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.extra,
            TOP_HEIGHT,
            &height.to_be_bytes(),
        )?;
        Ok(())
    }

    async fn get_top_block_header(
        &self,
    ) -> Result<(Immutable<BlockHeader>, Hash), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get top block header");
        }
        let hash = self.get_top_block_hash().await?;
        Ok((self.get_block_header_by_hash(&hash).await?, hash))
    }

    async fn get_top_block(&self) -> Result<Block, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get top block");
        }
        let hash = self.get_top_block_hash().await?;
        // TIP-2 Phase 1 fix: Use already-fixed get_block_by_hash
        self.get_block_by_hash(&hash).await
    }
}
