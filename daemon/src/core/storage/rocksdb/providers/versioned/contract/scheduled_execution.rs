// RocksDB implementation for VersionedScheduledExecutionsProvider

use async_trait::async_trait;
use log::trace;
use tos_common::block::TopoHeight;

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, ContractId, IteratorMode, RocksStorage},
        snapshot::Direction,
        VersionedScheduledExecutionsProvider,
    },
};

#[async_trait]
impl VersionedScheduledExecutionsProvider for RocksStorage {
    async fn delete_scheduled_executions_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        trace!("delete scheduled executions at topoheight {}", topoheight);
        let prefix = topoheight.to_be_bytes();
        self.delete_scheduled_executions_with_mode(IteratorMode::WithPrefix(
            &prefix,
            Direction::Forward,
        ))
        .await
    }

    async fn delete_scheduled_executions_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        trace!(
            "delete scheduled executions above topoheight {}",
            topoheight
        );
        let lower = (topoheight + 1).to_be_bytes();
        self.delete_scheduled_executions_with_mode(IteratorMode::Range {
            lower_bound: &lower,
            upper_bound: &[],
            direction: Direction::Forward,
        })
        .await
    }

    async fn delete_scheduled_executions_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        trace!(
            "delete scheduled executions below topoheight {}",
            topoheight
        );
        let upper = topoheight.to_be_bytes();
        self.delete_scheduled_executions_with_mode(IteratorMode::Range {
            lower_bound: &[],
            upper_bound: &upper,
            direction: Direction::Forward,
        })
        .await
    }
}

impl RocksStorage {
    async fn delete_scheduled_executions_with_mode(
        &mut self,
        mode: IteratorMode<'_>,
    ) -> Result<(), BlockchainError> {
        // First collect all keys to delete from the registrations table
        let keys_to_delete: Vec<(TopoHeight, ContractId, TopoHeight)> = self
            .iter_keys::<(TopoHeight, ContractId, TopoHeight)>(
                Column::DelayedExecutionRegistrations,
                mode,
            )?
            .collect::<Result<Vec<_>, _>>()?;

        // Delete from both tables
        for (registration_topo, contract_id, execution_topo) in keys_to_delete {
            // Delete from DelayedExecution
            let exec_key = Self::get_contract_scheduled_execution_key(contract_id, execution_topo);
            self.remove_from_disk(Column::DelayedExecution, &exec_key)?;

            // Delete from DelayedExecutionRegistrations
            let reg_key = Self::get_contract_scheduled_execution_registration_key(
                registration_topo,
                contract_id,
                execution_topo,
            );
            self.remove_from_disk(Column::DelayedExecutionRegistrations, &reg_key)?;
        }

        Ok(())
    }
}
