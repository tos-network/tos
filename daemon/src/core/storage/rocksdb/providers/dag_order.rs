use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode},
        DagOrderProvider, RocksStorage,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{block::TopoHeight, crypto::Hash};

#[async_trait]
impl DagOrderProvider for RocksStorage {
    async fn get_topo_height_for_hash(&self, hash: &Hash) -> Result<TopoHeight, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get topo height for hash {}", hash);
        }
        let use_cache = self
            .snapshot
            .as_ref()
            .and_then(|s| s.contains(Column::TopoByHash, hash.as_bytes()))
            .is_none();

        if use_cache {
            if let Some(objects) = &self.cache().objects {
                if let Some(value) = objects.topo_by_hash_cache.lock().await.get(hash) {
                    return Ok(*value);
                }
            }
        }

        let topoheight: TopoHeight = self.load_from_disk(Column::TopoByHash, hash)?;
        if use_cache {
            if let Some(objects) = &self.cache().objects {
                objects
                    .topo_by_hash_cache
                    .lock()
                    .await
                    .put(hash.clone(), topoheight);
            }
        }

        Ok(topoheight)
    }

    async fn set_topo_height_for_block(
        &mut self,
        hash: &Hash,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set topo height for block {} to {}", hash, topoheight);
        }
        self.insert_into_disk(Column::TopoByHash, hash, &topoheight)?;
        self.insert_into_disk(Column::HashAtTopo, topoheight.to_be_bytes(), hash)?;
        if let Some(objects) = &self.cache().objects {
            objects
                .topo_by_hash_cache
                .lock()
                .await
                .put(hash.clone(), topoheight);
            objects
                .hash_at_topo_cache
                .lock()
                .await
                .put(topoheight, hash.clone());
        }
        Ok(())
    }

    async fn is_block_topological_ordered(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is block topological ordered {}", hash);
        }
        let Some(topo_by_hash) =
            self.load_optional_from_disk::<_, TopoHeight>(Column::TopoByHash, hash)?
        else {
            return Ok(false);
        };

        let Some(hash_at_topo) = self
            .load_optional_from_disk::<_, Hash>(Column::HashAtTopo, &topo_by_hash.to_be_bytes())?
        else {
            return Ok(false);
        };

        Ok(hash_at_topo == *hash)
    }

    async fn get_hash_at_topo_height(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Hash, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get hash at topo height {}", topoheight);
        }
        let key = topoheight.to_be_bytes();
        let use_cache = self
            .snapshot
            .as_ref()
            .and_then(|s| s.contains(Column::HashAtTopo, &key))
            .is_none();

        if use_cache {
            if let Some(objects) = &self.cache().objects {
                if let Some(value) = objects.hash_at_topo_cache.lock().await.get(&topoheight) {
                    return Ok(value.clone());
                }
            }
        }

        let hash: Hash = self.load_from_disk(Column::HashAtTopo, &key)?;
        if use_cache {
            if let Some(objects) = &self.cache().objects {
                objects
                    .hash_at_topo_cache
                    .lock()
                    .await
                    .put(topoheight, hash.clone());
            }
        }

        Ok(hash)
    }

    async fn has_hash_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has hash at topo height {}", topoheight);
        }
        self.contains_data(Column::HashAtTopo, &topoheight.to_be_bytes())
    }

    // Fetch all the blocks orphaned in the DB
    async fn get_orphaned_blocks<'a>(
        &'a self,
    ) -> Result<impl Iterator<Item = Result<Hash, BlockchainError>> + 'a, BlockchainError> {
        trace!("get orphaned blocks");

        let iter = self.iter_keys(Column::Blocks, IteratorMode::Start)?;
        Ok(iter
            .map(|key| {
                let hash = key?;
                if self.contains_data(Column::TopoByHash, &hash)? {
                    return Ok(None);
                }

                Ok(Some(hash))
            })
            .filter_map(Result::transpose))
    }
}
