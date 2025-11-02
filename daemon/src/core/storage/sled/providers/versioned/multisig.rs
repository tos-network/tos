use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{SledStorage, VersionedMultiSigProvider},
};
use async_trait::async_trait;
use log::trace;
use tos_common::block::TopoHeight;

#[async_trait]
impl VersionedMultiSigProvider for SledStorage {
    async fn delete_versioned_multisigs_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned nonces at topoheight {}", topoheight);
        }
        Self::delete_versioned_tree_at_topoheight(
            &mut self.snapshot,
            &self.multisig,
            &self.versioned_multisigs,
            topoheight,
        )
    }

    async fn delete_versioned_multisigs_above_topoheight(
        &mut self,
        topoheight: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned multisigs above topoheight {}!",
                topoheight
            );
        }
        Self::delete_versioned_tree_above_topoheight(
            &mut self.snapshot,
            &self.multisig,
            &self.versioned_multisigs,
            topoheight,
            DiskContext::VersionedMultisig,
        )
    }

    async fn delete_versioned_multisigs_below_topoheight(
        &mut self,
        topoheight: u64,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned multisigs below topoheight {}!",
                topoheight
            );
        }
        Self::delete_versioned_tree_below_topoheight(
            &mut self.snapshot,
            &self.multisig,
            &self.versioned_multisigs,
            topoheight,
            keep_last,
            DiskContext::VersionedMultisig,
        )
    }
}
