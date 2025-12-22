// RocksDB implementation for VersionedScheduledExecutionProvider

use async_trait::async_trait;
use log::trace;
use rocksdb::Direction;
use tos_common::{block::TopoHeight, contract::ScheduledExecution};

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, ContractId, IteratorMode, RocksStorage},
        VersionedScheduledExecutionProvider,
    },
};

#[async_trait]
impl VersionedScheduledExecutionProvider for RocksStorage {
    /// Delete scheduled executions registered at the specified topoheight.
    async fn delete_scheduled_executions_registered_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete scheduled executions registered at topoheight {}",
                topoheight
            );
        }

        // Use the registration index to find all executions registered at this topoheight
        let prefix = topoheight.to_be_bytes();
        let entries: Vec<_> = self
            .iter_keys::<(TopoHeight, ContractId, TopoHeight)>(
                Column::DelayedExecutionRegistrations,
                IteratorMode::WithPrefix(&prefix, Direction::Forward),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        for (reg_topo, contract_id, exec_topo) in entries {
            // Only delete if registration topoheight matches exactly
            if reg_topo != topoheight {
                continue;
            }

            // Load the execution to get the offer amount for priority key deletion
            let exec_key = Self::get_contract_scheduled_execution_key(contract_id, exec_topo);
            let execution_result: Result<ScheduledExecution, _> =
                self.load_from_disk(Column::DelayedExecution, &exec_key);
            if let Ok(execution) = execution_result {
                // Delete from main storage
                self.remove_from_disk(Column::DelayedExecution, &exec_key)?;

                // Delete from registration index
                let reg_key = Self::get_contract_scheduled_execution_registration_key(
                    topoheight,
                    contract_id,
                    exec_topo,
                );
                self.remove_from_disk(Column::DelayedExecutionRegistrations, &reg_key)?;

                // Delete from priority index
                let priority_key = Self::get_scheduled_execution_priority_key(
                    exec_topo,
                    execution.offer_amount,
                    execution.registration_topoheight,
                    contract_id,
                );
                self.remove_from_disk(Column::DelayedExecutionPriority, &priority_key)?;
            }
        }

        Ok(())
    }

    /// Delete scheduled executions registered above the specified topoheight.
    async fn delete_scheduled_executions_registered_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete scheduled executions registered above topoheight {}",
                topoheight
            );
        }

        // Start from topoheight + 1
        let start = (topoheight + 1).to_be_bytes();
        let entries: Vec<_> = self
            .iter_keys::<(TopoHeight, ContractId, TopoHeight)>(
                Column::DelayedExecutionRegistrations,
                IteratorMode::From(&start, Direction::Forward),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        for (reg_topo, contract_id, exec_topo) in entries {
            // Only process if registration topoheight is above the threshold
            if reg_topo <= topoheight {
                continue;
            }

            // Load the execution to get the offer amount for priority key deletion
            let exec_key = Self::get_contract_scheduled_execution_key(contract_id, exec_topo);
            let execution_result: Result<ScheduledExecution, _> =
                self.load_from_disk(Column::DelayedExecution, &exec_key);
            if let Ok(execution) = execution_result {
                // Delete from main storage
                self.remove_from_disk(Column::DelayedExecution, &exec_key)?;

                // Delete from registration index
                let reg_key = Self::get_contract_scheduled_execution_registration_key(
                    reg_topo,
                    contract_id,
                    exec_topo,
                );
                self.remove_from_disk(Column::DelayedExecutionRegistrations, &reg_key)?;

                // Delete from priority index
                let priority_key = Self::get_scheduled_execution_priority_key(
                    exec_topo,
                    execution.offer_amount,
                    execution.registration_topoheight,
                    contract_id,
                );
                self.remove_from_disk(Column::DelayedExecutionPriority, &priority_key)?;
            }
        }

        Ok(())
    }

    /// Delete scheduled executions registered below the specified topoheight.
    async fn delete_scheduled_executions_registered_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete scheduled executions registered below topoheight {}",
                topoheight
            );
        }

        // Iterate from start to topoheight
        let entries: Vec<_> = self
            .iter_keys::<(TopoHeight, ContractId, TopoHeight)>(
                Column::DelayedExecutionRegistrations,
                IteratorMode::Start,
            )?
            .take_while(|res| match res {
                Ok((reg_topo, _, _)) => *reg_topo < topoheight,
                Err(_) => true,
            })
            .collect::<Result<Vec<_>, _>>()?;

        for (reg_topo, contract_id, exec_topo) in entries {
            // Load the execution to get the offer amount for priority key deletion
            let exec_key = Self::get_contract_scheduled_execution_key(contract_id, exec_topo);
            let execution_result: Result<ScheduledExecution, _> =
                self.load_from_disk(Column::DelayedExecution, &exec_key);
            if let Ok(execution) = execution_result {
                // Delete from main storage
                self.remove_from_disk(Column::DelayedExecution, &exec_key)?;

                // Delete from registration index
                let reg_key = Self::get_contract_scheduled_execution_registration_key(
                    reg_topo,
                    contract_id,
                    exec_topo,
                );
                self.remove_from_disk(Column::DelayedExecutionRegistrations, &reg_key)?;

                // Delete from priority index
                let priority_key = Self::get_scheduled_execution_priority_key(
                    exec_topo,
                    execution.offer_amount,
                    execution.registration_topoheight,
                    contract_id,
                );
                self.remove_from_disk(Column::DelayedExecutionPriority, &priority_key)?;
            }
        }

        Ok(())
    }
}
