// RocksDB implementation for ContractScheduledExecutionProvider

use async_trait::async_trait;
use futures::{stream, Stream};
use log::trace;
use rocksdb::Direction;
use tos_common::{
    block::TopoHeight,
    contract::{ScheduledExecution, ScheduledExecutionKind},
    crypto::Hash,
};

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, ContractId, IteratorMode, RocksStorage},
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
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set contract {} scheduled execution at topoheight {}",
                contract,
                execution_topoheight
            );
        }

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
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "has contract {} scheduled execution at topoheight {}",
                contract,
                topoheight
            );
        }

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
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get contract {} scheduled execution at topoheight {}",
                contract,
                topoheight
            );
        }

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
        // Start iteration from the minimum topoheight and filter within range
        let min = minimum_topoheight.to_be_bytes();
        let stream = self
            .iter_keys::<(TopoHeight, ContractId, TopoHeight)>(
                Column::DelayedExecutionRegistrations,
                IteratorMode::From(&min, Direction::Forward),
            )?
            .take_while(move |res| {
                // Stop when we exceed maximum_topoheight
                match res {
                    Ok((registration, _, _)) => *registration <= maximum_topoheight,
                    Err(_) => true, // Let errors pass through
                }
            })
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
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get priority sorted scheduled executions at topoheight {}",
                topoheight
            );
        }

        let prefix = topoheight.to_be_bytes();

        // Iterate over priority index keys for this topoheight
        // Keys are already sorted: [exec_topo][inverted_offer][reg_topo][contract_id]
        // Since inverted_offer = u64::MAX - offer, higher offers sort first
        //
        // Key format: [8 bytes exec_topo][8 bytes inverted_offer][8 bytes reg_topo][8 bytes contract_id]
        // Use nested tuples since Serializer only supports tuples up to 3 elements:
        // ((exec_topo, inverted_offer), (reg_topo, contract_id))
        self.iter_keys::<((TopoHeight, u64), (TopoHeight, ContractId))>(
            Column::DelayedExecutionPriority,
            IteratorMode::WithPrefix(&prefix, Direction::Forward),
        )
        .map(|iter| {
            iter.map(move |res| {
                let ((exec_topo, _inverted_offer), (_reg_topo, contract_id)) = res?;
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

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete contract {} scheduled execution at topoheight {}",
                contract,
                execution_topoheight
            );
        }

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

    /// Count scheduled executions by a contract within a topoheight window.
    async fn count_contract_scheduled_executions_in_window(
        &self,
        contract: &Hash,
        from_topoheight: TopoHeight,
        to_topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "count contract {} scheduled executions in window [{}, {}]",
                contract,
                from_topoheight,
                to_topoheight
            );
        }

        let Some(contract_id) = self.get_optional_contract_id(contract)? else {
            return Ok(0);
        };

        // Use the registration index to count schedules by this contract
        let mut count = 0u64;
        let min_prefix = from_topoheight.to_be_bytes();

        // Iterate from minimum topoheight
        for res in self.iter_keys::<(TopoHeight, ContractId, TopoHeight)>(
            Column::DelayedExecutionRegistrations,
            IteratorMode::From(&min_prefix, Direction::Forward),
        )? {
            let (reg_topo, id, _exec_topo) = res?;

            // Stop if we've passed the end of the window
            if reg_topo > to_topoheight {
                break;
            }

            // Count if this is the contract we're looking for
            if id == contract_id {
                count = count.saturating_add(1);
            }
        }

        Ok(count)
    }

    /// Get a scheduled execution by its handle.
    async fn get_scheduled_execution_by_handle(
        &self,
        handle: u64,
    ) -> Result<Option<ScheduledExecution>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get scheduled execution by handle {}", handle);
        }

        // Handle is the first 8 bytes of the execution hash
        // We need to scan through all executions to find a match
        let handle_bytes = handle.to_be_bytes();

        for res in
            self.iter::<(), ScheduledExecution>(Column::DelayedExecution, IteratorMode::Start)?
        {
            let (_, execution) = res?;

            // Check if the first 8 bytes of the hash match the handle
            let hash_bytes = execution.hash.as_bytes();
            if hash_bytes[0..8] == handle_bytes {
                return Ok(Some(execution));
            }
        }

        Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduled_execution_key_format() {
        let contract_id: ContractId = 12345;
        let topoheight: TopoHeight = 67890;

        let key = RocksStorage::get_contract_scheduled_execution_key(contract_id, topoheight);

        // Verify key length
        assert_eq!(key.len(), 16);

        // Verify topoheight is in big-endian at start
        let mut topo_bytes = [0u8; 8];
        topo_bytes.copy_from_slice(&key[0..8]);
        assert_eq!(TopoHeight::from_be_bytes(topo_bytes), topoheight);

        // Verify contract ID follows
        let mut id_bytes = [0u8; 8];
        id_bytes.copy_from_slice(&key[8..16]);
        assert_eq!(ContractId::from_be_bytes(id_bytes), contract_id);
    }

    #[test]
    fn test_scheduled_execution_key_ordering() {
        let contract_id: ContractId = 1;

        // Keys should be ordered by topoheight first
        let key1 = RocksStorage::get_contract_scheduled_execution_key(contract_id, 100);
        let key2 = RocksStorage::get_contract_scheduled_execution_key(contract_id, 200);
        let key3 = RocksStorage::get_contract_scheduled_execution_key(contract_id, 50);

        // Lexicographic comparison should order by topoheight
        assert!(key3 < key1);
        assert!(key1 < key2);
    }

    #[test]
    fn test_registration_key_format() {
        let contract_id: ContractId = 42;
        let reg_topo: TopoHeight = 1000;
        let exec_topo: TopoHeight = 2000;

        let key = RocksStorage::get_contract_scheduled_execution_registration_key(
            reg_topo,
            contract_id,
            exec_topo,
        );

        // Verify key length
        assert_eq!(key.len(), 24);

        // Verify registration topoheight
        let mut reg_bytes = [0u8; 8];
        reg_bytes.copy_from_slice(&key[0..8]);
        assert_eq!(TopoHeight::from_be_bytes(reg_bytes), reg_topo);

        // Verify contract ID
        let mut id_bytes = [0u8; 8];
        id_bytes.copy_from_slice(&key[8..16]);
        assert_eq!(ContractId::from_be_bytes(id_bytes), contract_id);

        // Verify execution topoheight
        let mut exec_bytes = [0u8; 8];
        exec_bytes.copy_from_slice(&key[16..24]);
        assert_eq!(TopoHeight::from_be_bytes(exec_bytes), exec_topo);
    }

    #[test]
    fn test_priority_key_format() {
        let contract_id: ContractId = 99;
        let exec_topo: TopoHeight = 500;
        let offer: u64 = 1_000_000;
        let reg_topo: TopoHeight = 100;

        let key = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo,
            offer,
            reg_topo,
            contract_id,
        );

        // Verify key length
        assert_eq!(key.len(), 32);

        // Verify execution topoheight
        let mut exec_bytes = [0u8; 8];
        exec_bytes.copy_from_slice(&key[0..8]);
        assert_eq!(TopoHeight::from_be_bytes(exec_bytes), exec_topo);

        // Verify inverted offer
        let mut offer_bytes = [0u8; 8];
        offer_bytes.copy_from_slice(&key[8..16]);
        let inverted = u64::from_be_bytes(offer_bytes);
        assert_eq!(inverted, u64::MAX - offer);

        // Verify registration topoheight
        let mut reg_bytes = [0u8; 8];
        reg_bytes.copy_from_slice(&key[16..24]);
        assert_eq!(TopoHeight::from_be_bytes(reg_bytes), reg_topo);

        // Verify contract ID
        let mut id_bytes = [0u8; 8];
        id_bytes.copy_from_slice(&key[24..32]);
        assert_eq!(ContractId::from_be_bytes(id_bytes), contract_id);
    }

    #[test]
    fn test_priority_key_higher_offer_sorts_first() {
        let contract_id: ContractId = 1;
        let exec_topo: TopoHeight = 100;
        let reg_topo: TopoHeight = 50;

        // Higher offer should have LOWER key value (due to inversion)
        let key_high_offer = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo,
            1_000_000, // High offer
            reg_topo,
            contract_id,
        );
        let key_low_offer = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo,
            100, // Low offer
            reg_topo,
            contract_id,
        );

        // Lexicographically, high offer key should come BEFORE low offer key
        assert!(key_high_offer < key_low_offer);
    }

    #[test]
    fn test_priority_key_fifo_for_equal_offers() {
        let contract_id: ContractId = 2;
        let exec_topo: TopoHeight = 100;
        let offer: u64 = 500_000;

        // Earlier registration should sort first for equal offers
        let key_early_reg = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo,
            offer,
            10, // Early registration
            contract_id,
        );
        let key_late_reg = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo,
            offer,
            50, // Late registration
            contract_id,
        );

        // Earlier registration should come first
        assert!(key_early_reg < key_late_reg);
    }

    #[test]
    fn test_priority_key_contract_id_tiebreaker() {
        let exec_topo: TopoHeight = 100;
        let offer: u64 = 500_000;
        let reg_topo: TopoHeight = 50;

        // When offer and registration are equal, contract ID breaks tie
        let contract_a: ContractId = 1;
        let contract_b: ContractId = 2;

        let key_a = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo, offer, reg_topo, contract_a,
        );
        let key_b = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo, offer, reg_topo, contract_b,
        );

        // Lower contract ID should come first
        assert!(key_a < key_b);
    }

    #[test]
    fn test_priority_key_execution_topoheight_primary_sort() {
        let contract_id: ContractId = 1;
        let offer: u64 = 1_000_000;
        let reg_topo: TopoHeight = 50;

        // Earlier execution topoheight should always sort first
        let key_early_exec = RocksStorage::get_scheduled_execution_priority_key(
            100, // Early execution
            offer,
            reg_topo,
            contract_id,
        );
        let key_late_exec = RocksStorage::get_scheduled_execution_priority_key(
            200, // Late execution
            offer,
            reg_topo,
            contract_id,
        );

        assert!(key_early_exec < key_late_exec);
    }

    #[test]
    fn test_priority_key_zero_offer() {
        let contract_id: ContractId = 1;
        let exec_topo: TopoHeight = 100;
        let reg_topo: TopoHeight = 50;

        // Zero offer should work correctly
        let key = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo,
            0, // Zero offer
            reg_topo,
            contract_id,
        );

        // Verify inverted offer is u64::MAX
        let mut offer_bytes = [0u8; 8];
        offer_bytes.copy_from_slice(&key[8..16]);
        let inverted = u64::from_be_bytes(offer_bytes);
        assert_eq!(inverted, u64::MAX);
    }

    #[test]
    fn test_priority_key_max_offer() {
        let contract_id: ContractId = 1;
        let exec_topo: TopoHeight = 100;
        let reg_topo: TopoHeight = 50;

        // Max offer should work correctly
        let key = RocksStorage::get_scheduled_execution_priority_key(
            exec_topo,
            u64::MAX, // Max offer
            reg_topo,
            contract_id,
        );

        // Verify inverted offer is 0
        let mut offer_bytes = [0u8; 8];
        offer_bytes.copy_from_slice(&key[8..16]);
        let inverted = u64::from_be_bytes(offer_bytes);
        assert_eq!(inverted, 0);
    }

    #[test]
    fn test_priority_ordering_comprehensive() {
        // Test full priority ordering with multiple executions
        let contract_a: ContractId = 1;
        let contract_b: ContractId = 2;

        // Create keys with various priorities for execution at topoheight 100
        let mut keys = vec![
            // (description, key)
            (
                "high offer, early reg",
                RocksStorage::get_scheduled_execution_priority_key(100, 1_000_000, 10, contract_a),
            ),
            (
                "high offer, late reg",
                RocksStorage::get_scheduled_execution_priority_key(100, 1_000_000, 20, contract_a),
            ),
            (
                "low offer, early reg",
                RocksStorage::get_scheduled_execution_priority_key(100, 100, 5, contract_a),
            ),
            (
                "medium offer",
                RocksStorage::get_scheduled_execution_priority_key(100, 500_000, 15, contract_a),
            ),
            (
                "high offer, same reg, contract_b",
                RocksStorage::get_scheduled_execution_priority_key(100, 1_000_000, 10, contract_b),
            ),
        ];

        // Sort by key
        keys.sort_by(|a, b| a.1.cmp(&b.1));

        // Expected order:
        // 1. high offer, early reg, contract_a (highest offer, earliest reg, lower contract)
        // 2. high offer, early reg, contract_b (highest offer, earliest reg, higher contract)
        // 3. high offer, late reg (highest offer, later reg)
        // 4. medium offer
        // 5. low offer (lowest priority despite early reg)
        assert_eq!(keys[0].0, "high offer, early reg");
        assert_eq!(keys[1].0, "high offer, same reg, contract_b");
        assert_eq!(keys[2].0, "high offer, late reg");
        assert_eq!(keys[3].0, "medium offer");
        assert_eq!(keys[4].0, "low offer, early reg");
    }

    #[test]
    fn test_key_sizes_match_specification() {
        // Verify all key sizes match expected values
        let key1 = RocksStorage::get_contract_scheduled_execution_key(1, 100);
        assert_eq!(
            key1.len(),
            16,
            "Execution key should be 16 bytes: [8 topo][8 contract_id]"
        );

        let key2 = RocksStorage::get_contract_scheduled_execution_registration_key(100, 1, 200);
        assert_eq!(
            key2.len(),
            24,
            "Registration key should be 24 bytes: [8 reg_topo][8 contract_id][8 exec_topo]"
        );

        let key3 = RocksStorage::get_scheduled_execution_priority_key(100, 1000, 50, 1);
        assert_eq!(
            key3.len(),
            32,
            "Priority key should be 32 bytes: [8 exec_topo][8 inv_offer][8 reg_topo][8 contract_id]"
        );
    }
}
