// RocksDB implementation for ContractScheduledExecutionProvider

use async_trait::async_trait;
use futures::{stream, Stream};
use log::trace;
use tos_common::{
    block::TopoHeight,
    contract::{ScheduledExecution, ScheduledExecutionKind},
    crypto::Hash,
};

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, ContractId, IteratorMode, RocksStorage},
        snapshot::Direction,
        ContractScheduledExecutionProvider,
    },
};

#[async_trait]
impl ContractScheduledExecutionProvider for RocksStorage {
    /// Set contract scheduled execution at provided topoheight.
    /// Caller must ensure that the topoheight configured is >= current topoheight
    /// and no other execution exists, otherwise it will be overwritten.
    async fn set_contract_scheduled_execution_at_topoheight(
        &mut self,
        contract: &Hash,
        topoheight: TopoHeight,
        execution: &ScheduledExecution,
        execution_topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        trace!(
            "set contract {} scheduled execution at topoheight {}",
            contract,
            execution_topoheight
        );

        let contract_id = self.get_contract_id(contract)?;

        // Store the execution data
        let key = Self::get_contract_scheduled_execution_key(contract_id, execution_topoheight);
        self.insert_into_disk(Column::DelayedExecution, &key, execution)?;

        // Store the registration entry for efficient range queries
        let key = Self::get_contract_scheduled_execution_registration_key(
            topoheight,
            contract_id,
            execution_topoheight,
        );
        self.insert_into_disk(Column::DelayedExecutionRegistrations, &key, &[])?;

        // Store the priority index entry for OFFERCALL ordering
        let priority_key = Self::get_scheduled_execution_priority_key(
            execution_topoheight,
            execution.offer_amount,
            execution.registration_topoheight,
            contract_id,
        );
        self.insert_into_disk(Column::DelayedExecutionPriority, &priority_key, &[])
    }

    /// Check if a contract has a scheduled execution registered at the provided topoheight.
    async fn has_contract_scheduled_execution_at_topoheight(
        &self,
        contract: &Hash,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError> {
        trace!(
            "has contract {} scheduled execution at topoheight {}",
            contract,
            topoheight
        );

        let Some(contract_id) = self.get_optional_contract_id(contract)? else {
            return Ok(false);
        };
        let key = Self::get_contract_scheduled_execution_key(contract_id, topoheight);

        self.contains_data(Column::DelayedExecution, &key)
    }

    /// Get the contract scheduled execution registered at the provided topoheight.
    async fn get_contract_scheduled_execution_at_topoheight(
        &self,
        contract: &Hash,
        topoheight: TopoHeight,
    ) -> Result<ScheduledExecution, BlockchainError> {
        trace!(
            "get contract {} scheduled execution at topoheight {}",
            contract,
            topoheight
        );

        let contract_id = self.get_contract_id(contract)?;
        let key = Self::get_contract_scheduled_execution_key(contract_id, topoheight);

        self.load_from_disk(Column::DelayedExecution, &key)
    }

    /// Get the registered scheduled executions at the provided topoheight.
    async fn get_registered_contract_scheduled_executions_at_topoheight<'a>(
        &'a self,
        topoheight: TopoHeight,
    ) -> Result<
        impl Iterator<Item = Result<(TopoHeight, Hash), BlockchainError>> + Send + 'a,
        BlockchainError,
    > {
        let prefix = topoheight.to_be_bytes();
        self.iter_keys::<(TopoHeight, ContractId, TopoHeight)>(
            Column::DelayedExecutionRegistrations,
            IteratorMode::WithPrefix(&prefix, Direction::Forward),
        )
        .map(|iter| {
            iter.map(move |res| {
                let (_, contract_id, topoheight) = res?;
                let contract = self.get_contract_from_id(contract_id)?;
                Ok((topoheight, contract))
            })
        })
    }

    /// Get the scheduled executions planned for execution at the provided topoheight.
    async fn get_contract_scheduled_executions_at_topoheight<'a>(
        &'a self,
        topoheight: TopoHeight,
    ) -> Result<
        impl Iterator<Item = Result<ScheduledExecution, BlockchainError>> + Send + 'a,
        BlockchainError,
    > {
        let prefix = topoheight.to_be_bytes();
        self.iter::<(), ScheduledExecution>(
            Column::DelayedExecution,
            IteratorMode::WithPrefix(&prefix, Direction::Forward),
        )
        .map(|iter| iter.map(|res| res.map(|(_, v)| v)))
    }

    /// Get the registered scheduled executions in a topoheight range (inclusive).
    async fn get_registered_contract_scheduled_executions_in_range<'a>(
        &'a self,
        minimum_topoheight: TopoHeight,
        maximum_topoheight: TopoHeight,
    ) -> Result<
        impl Stream<Item = Result<(TopoHeight, TopoHeight, ScheduledExecution), BlockchainError>>
            + Send
            + 'a,
        BlockchainError,
    > {
        let min = minimum_topoheight.to_be_bytes();
        let max = (maximum_topoheight + 1).to_be_bytes();
        let stream = self
            .iter_keys::<(TopoHeight, ContractId, TopoHeight)>(
                Column::DelayedExecutionRegistrations,
                IteratorMode::Range {
                    lower_bound: &min,
                    upper_bound: &max,
                    direction: Direction::Reverse,
                },
            )?
            .map(move |res| {
                let (registration, contract_id, execution_topoheight) = res?;
                if registration <= maximum_topoheight && registration >= minimum_topoheight {
                    let key = Self::get_contract_scheduled_execution_key(
                        contract_id,
                        execution_topoheight,
                    );
                    let execution = self.load_from_disk(Column::DelayedExecution, &key)?;

                    Ok(Some((execution_topoheight, registration, execution)))
                } else {
                    Ok(None)
                }
            })
            .filter_map(Result::transpose);

        Ok(stream::iter(stream))
    }

    /// Get scheduled executions at topoheight, sorted by priority (OFFERCALL ordering).
    /// Priority order: higher offer first, then FIFO by registration time, then by contract ID.
    async fn get_priority_sorted_scheduled_executions_at_topoheight<'a>(
        &'a self,
        topoheight: TopoHeight,
    ) -> Result<
        impl Iterator<Item = Result<ScheduledExecution, BlockchainError>> + Send + 'a,
        BlockchainError,
    > {
        trace!(
            "get priority sorted scheduled executions at topoheight {}",
            topoheight
        );

        let prefix = topoheight.to_be_bytes();

        // Iterate over priority index keys for this topoheight
        // Keys are already sorted: [exec_topo][inverted_offer][reg_topo][contract_id]
        // Since inverted_offer = u64::MAX - offer, higher offers sort first
        //
        // Key format: [8 bytes exec_topo][8 bytes inverted_offer][8 bytes reg_topo][8 bytes contract_id]
        // We parse the exec_topo (bytes 0-8) and contract_id (bytes 24-32) to load the execution
        self.iter_raw_keys(
            Column::DelayedExecutionPriority,
            IteratorMode::WithPrefix(&prefix, Direction::Forward),
        )
        .map(|iter| {
            iter.map(move |res| {
                let key_bytes = res?;
                if key_bytes.len() != 32 {
                    return Err(BlockchainError::Unknown);
                }
                // Parse exec_topo (bytes 0-8)
                let exec_topo = TopoHeight::from_be_bytes(
                    key_bytes[0..8]
                        .try_into()
                        .map_err(|_| BlockchainError::Unknown)?,
                );
                // Parse contract_id (bytes 24-32)
                let contract_id = ContractId::from_be_bytes(
                    key_bytes[24..32]
                        .try_into()
                        .map_err(|_| BlockchainError::Unknown)?,
                );
                // Load the actual execution data using the standard key
                let key = Self::get_contract_scheduled_execution_key(contract_id, exec_topo);
                self.load_from_disk(Column::DelayedExecution, &key)
            })
        })
    }

    /// Delete a scheduled execution and its priority index entry.
    async fn delete_contract_scheduled_execution(
        &mut self,
        contract: &Hash,
        execution: &ScheduledExecution,
    ) -> Result<(), BlockchainError> {
        // Get execution topoheight from kind
        let execution_topoheight = match execution.kind {
            ScheduledExecutionKind::TopoHeight(topo) => topo,
            ScheduledExecutionKind::BlockEnd => {
                // BlockEnd executions are handled differently - they run at current block
                // For now, we use registration_topoheight as a fallback
                execution.registration_topoheight
            }
        };

        trace!(
            "delete contract {} scheduled execution at topoheight {}",
            contract,
            execution_topoheight
        );

        let contract_id = self.get_contract_id(contract)?;

        // Delete from main storage
        let key = Self::get_contract_scheduled_execution_key(contract_id, execution_topoheight);
        self.remove_from_disk(Column::DelayedExecution, &key)?;

        // Delete from registration index
        let reg_key = Self::get_contract_scheduled_execution_registration_key(
            execution.registration_topoheight,
            contract_id,
            execution_topoheight,
        );
        self.remove_from_disk(Column::DelayedExecutionRegistrations, &reg_key)?;

        // Delete from priority index
        let priority_key = Self::get_scheduled_execution_priority_key(
            execution_topoheight,
            execution.offer_amount,
            execution.registration_topoheight,
            contract_id,
        );
        self.remove_from_disk(Column::DelayedExecutionPriority, &priority_key)
    }
}

impl RocksStorage {
    /// Generate key for scheduled execution storage.
    /// Format: [8 bytes topoheight][8 bytes contract_id]
    pub fn get_contract_scheduled_execution_key(
        contract: ContractId,
        topoheight: TopoHeight,
    ) -> [u8; 16] {
        let mut buf = [0; 16];
        buf[0..8].copy_from_slice(&topoheight.to_be_bytes());
        buf[8..].copy_from_slice(&contract.to_be_bytes());
        buf
    }

    /// Generate key for scheduled execution registration.
    /// Format: [8 bytes registration_topoheight][8 bytes contract_id][8 bytes execution_topoheight]
    pub fn get_contract_scheduled_execution_registration_key(
        topoheight: TopoHeight,
        contract: ContractId,
        execution_topoheight: TopoHeight,
    ) -> [u8; 24] {
        let mut buf = [0; 24];
        buf[0..8].copy_from_slice(&topoheight.to_be_bytes());
        buf[8..16].copy_from_slice(&contract.to_be_bytes());
        buf[16..].copy_from_slice(&execution_topoheight.to_be_bytes());
        buf
    }

    /// Generate priority key for OFFERCALL ordering.
    /// Format: [8 bytes exec_topo][8 bytes inverted_offer][8 bytes reg_topo][8 bytes contract_id]
    ///
    /// The key is designed so that RocksDB's lexicographic ordering produces:
    /// 1. Executions at earlier topoheights first (exec_topo ascending)
    /// 2. Higher offers first (inverted_offer = u64::MAX - offer, so higher offers sort first)
    /// 3. Earlier registrations first for equal offers (reg_topo ascending, FIFO)
    /// 4. Deterministic ordering by contract_id for identical priorities
    pub fn get_scheduled_execution_priority_key(
        execution_topoheight: TopoHeight,
        offer_amount: u64,
        registration_topoheight: TopoHeight,
        contract: ContractId,
    ) -> [u8; 32] {
        let mut buf = [0u8; 32];
        // Execution topoheight (ascending - process earlier blocks first)
        buf[0..8].copy_from_slice(&execution_topoheight.to_be_bytes());
        // Inverted offer amount (u64::MAX - offer, so higher offers sort first)
        let inverted_offer = u64::MAX.saturating_sub(offer_amount);
        buf[8..16].copy_from_slice(&inverted_offer.to_be_bytes());
        // Registration topoheight (ascending - FIFO for equal offers)
        buf[16..24].copy_from_slice(&registration_topoheight.to_be_bytes());
        // Contract ID (deterministic tiebreaker)
        buf[24..32].copy_from_slice(&contract.to_be_bytes());
        buf
    }
}
