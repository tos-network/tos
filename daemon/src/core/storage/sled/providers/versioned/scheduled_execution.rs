// Sled implementation for VersionedScheduledExecutionProvider

use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::TopoHeight, contract::ScheduledExecution, crypto::Hash, serializer::Serializer,
};

use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{SledStorage, VersionedScheduledExecutionProvider},
};

#[async_trait]
impl VersionedScheduledExecutionProvider for SledStorage {
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
        let entries: Vec<_> = Self::scan_prefix_kv(
            self.snapshot.as_ref(),
            &self.scheduled_execution_registrations,
            &prefix,
        )
        .collect::<Result<Vec<_>, _>>()?;

        for (key, _) in entries {
            // Key format: [8 bytes reg_topo][32 bytes contract_hash][8 bytes exec_topo]
            if key.len() != 48 {
                continue;
            }

            let mut reg_topo_bytes = [0u8; 8];
            reg_topo_bytes.copy_from_slice(&key[0..8]);
            let reg_topo = TopoHeight::from_be_bytes(reg_topo_bytes);

            // Only delete if registration topoheight matches exactly
            if reg_topo != topoheight {
                continue;
            }

            let mut contract_bytes = [0u8; 32];
            contract_bytes.copy_from_slice(&key[8..40]);
            let contract = Hash::from_bytes(&contract_bytes)?;

            let mut exec_topo_bytes = [0u8; 8];
            exec_topo_bytes.copy_from_slice(&key[40..48]);
            let exec_topo = TopoHeight::from_be_bytes(exec_topo_bytes);

            // Load the execution to get the offer amount for priority key deletion
            let exec_key = Self::get_scheduled_execution_key(&contract, exec_topo);
            if let Ok(execution) = self.load_from_disk::<ScheduledExecution>(
                &self.scheduled_executions,
                &exec_key,
                DiskContext::ScheduledExecution,
            ) {
                // Delete from main storage
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_executions,
                    &exec_key,
                )?;

                // Delete from registration index
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_execution_registrations,
                    &key,
                )?;

                // Delete from priority index
                let priority_key = Self::get_scheduled_execution_priority_key(
                    exec_topo,
                    execution.offer_amount,
                    execution.registration_topoheight,
                    &contract,
                );
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_execution_priority,
                    &priority_key,
                )?;
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
        let entries: Vec<_> = Self::iter_from(
            self.snapshot.as_ref(),
            &self.scheduled_execution_registrations,
            &start,
        )
        .collect::<Result<Vec<_>, _>>()?;

        for (key, _) in entries {
            // Key format: [8 bytes reg_topo][32 bytes contract_hash][8 bytes exec_topo]
            if key.len() != 48 {
                continue;
            }

            let mut reg_topo_bytes = [0u8; 8];
            reg_topo_bytes.copy_from_slice(&key[0..8]);
            let reg_topo = TopoHeight::from_be_bytes(reg_topo_bytes);

            // Only process if registration topoheight is above the threshold
            if reg_topo <= topoheight {
                continue;
            }

            let mut contract_bytes = [0u8; 32];
            contract_bytes.copy_from_slice(&key[8..40]);
            let contract = Hash::from_bytes(&contract_bytes)?;

            let mut exec_topo_bytes = [0u8; 8];
            exec_topo_bytes.copy_from_slice(&key[40..48]);
            let exec_topo = TopoHeight::from_be_bytes(exec_topo_bytes);

            // Load the execution to get the offer amount for priority key deletion
            let exec_key = Self::get_scheduled_execution_key(&contract, exec_topo);
            if let Ok(execution) = self.load_from_disk::<ScheduledExecution>(
                &self.scheduled_executions,
                &exec_key,
                DiskContext::ScheduledExecution,
            ) {
                // Delete from main storage
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_executions,
                    &exec_key,
                )?;

                // Delete from registration index
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_execution_registrations,
                    &key,
                )?;

                // Delete from priority index
                let priority_key = Self::get_scheduled_execution_priority_key(
                    exec_topo,
                    execution.offer_amount,
                    execution.registration_topoheight,
                    &contract,
                );
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_execution_priority,
                    &priority_key,
                )?;
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

        // Iterate from start and take while below threshold
        let entries: Vec<_> = Self::iter(
            self.snapshot.as_ref(),
            &self.scheduled_execution_registrations,
        )
        .take_while(|res| match res {
            Ok((key, _)) => {
                if key.len() < 8 {
                    return true;
                }
                let mut topo_bytes = [0u8; 8];
                topo_bytes.copy_from_slice(&key[0..8]);
                TopoHeight::from_be_bytes(topo_bytes) < topoheight
            }
            Err(_) => true,
        })
        .collect::<Result<Vec<_>, _>>()?;

        for (key, _) in entries {
            // Key format: [8 bytes reg_topo][32 bytes contract_hash][8 bytes exec_topo]
            if key.len() != 48 {
                continue;
            }

            let mut contract_bytes = [0u8; 32];
            contract_bytes.copy_from_slice(&key[8..40]);
            let contract = Hash::from_bytes(&contract_bytes)?;

            let mut exec_topo_bytes = [0u8; 8];
            exec_topo_bytes.copy_from_slice(&key[40..48]);
            let exec_topo = TopoHeight::from_be_bytes(exec_topo_bytes);

            // Load the execution to get the offer amount for priority key deletion
            let exec_key = Self::get_scheduled_execution_key(&contract, exec_topo);
            if let Ok(execution) = self.load_from_disk::<ScheduledExecution>(
                &self.scheduled_executions,
                &exec_key,
                DiskContext::ScheduledExecution,
            ) {
                // Delete from main storage
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_executions,
                    &exec_key,
                )?;

                // Delete from registration index
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_execution_registrations,
                    &key,
                )?;

                // Delete from priority index
                let priority_key = Self::get_scheduled_execution_priority_key(
                    exec_topo,
                    execution.offer_amount,
                    execution.registration_topoheight,
                    &contract,
                );
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.scheduled_execution_priority,
                    &priority_key,
                )?;
            }
        }

        Ok(())
    }
}
