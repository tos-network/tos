// Sled implementation for ContractScheduledExecutionProvider

use async_trait::async_trait;
use futures::{stream, Stream};
use log::trace;
use tos_common::{
    block::TopoHeight,
    contract::{ScheduledExecution, ScheduledExecutionKind},
    crypto::Hash,
    serializer::Serializer,
};

use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{ContractScheduledExecutionProvider, SledStorage},
};

#[async_trait]
impl ContractScheduledExecutionProvider for SledStorage {
    /// Set contract scheduled execution at provided topoheight.
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

        // Store the execution data
        let key = Self::get_scheduled_execution_key(contract, execution_topoheight);
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.scheduled_executions,
            &key,
            execution.to_bytes(),
        )?;

        // Store the registration entry for efficient range queries
        let reg_key = Self::get_scheduled_execution_registration_key(
            topoheight,
            contract,
            execution_topoheight,
        );
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.scheduled_execution_registrations,
            &reg_key,
            &[],
        )?;

        // Store the priority index entry for OFFERCALL ordering
        let priority_key = Self::get_scheduled_execution_priority_key(
            execution_topoheight,
            execution.offer_amount,
            execution.registration_topoheight,
            contract,
        );
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.scheduled_execution_priority,
            &priority_key,
            &[],
        )?;

        Ok(())
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

        let key = Self::get_scheduled_execution_key(contract, topoheight);
        self.contains_data(&self.scheduled_executions, &key)
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

        let key = Self::get_scheduled_execution_key(contract, topoheight);
        self.load_from_disk(
            &self.scheduled_executions,
            &key,
            DiskContext::ScheduledExecution,
        )
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
        Ok(Self::scan_prefix_kv(
            self.snapshot.as_ref(),
            &self.scheduled_execution_registrations,
            &prefix,
        )
        .map(move |el| {
            let (key, _) = el?;
            // Key format: [8 bytes reg_topo][32 bytes contract_hash][8 bytes exec_topo]
            if key.len() != 48 {
                return Err(BlockchainError::CorruptedData);
            }

            let mut contract_bytes = [0u8; 32];
            contract_bytes.copy_from_slice(&key[8..40]);
            let contract = Hash::from_bytes(&contract_bytes)?;

            let mut exec_topo_bytes = [0u8; 8];
            exec_topo_bytes.copy_from_slice(&key[40..48]);
            let exec_topoheight = TopoHeight::from_be_bytes(exec_topo_bytes);

            Ok((exec_topoheight, contract))
        }))
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
        Ok(
            Self::scan_prefix_kv(self.snapshot.as_ref(), &self.scheduled_executions, &prefix).map(
                |el| {
                    let (_, value) = el?;
                    ScheduledExecution::from_bytes(&value)
                        .map_err(|_| BlockchainError::CorruptedData)
                },
            ),
        )
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
        let min_prefix = minimum_topoheight.to_be_bytes();
        let iter = Self::iter_from(
            self.snapshot.as_ref(),
            &self.scheduled_execution_registrations,
            &min_prefix,
        )
        .take_while(move |res| {
            match res {
                Ok((key, _)) => {
                    if key.len() < 8 {
                        return true; // Let it fail in the map
                    }
                    let mut topo_bytes = [0u8; 8];
                    topo_bytes.copy_from_slice(&key[0..8]);
                    let reg_topo = TopoHeight::from_be_bytes(topo_bytes);
                    reg_topo <= maximum_topoheight
                }
                Err(_) => true, // Let errors pass through
            }
        })
        .filter_map(move |res| {
            match res {
                Ok((key, _)) => {
                    if key.len() != 48 {
                        return Some(Err(BlockchainError::CorruptedData));
                    }

                    let mut reg_topo_bytes = [0u8; 8];
                    reg_topo_bytes.copy_from_slice(&key[0..8]);
                    let registration_topo = TopoHeight::from_be_bytes(reg_topo_bytes);

                    if registration_topo < minimum_topoheight
                        || registration_topo > maximum_topoheight
                    {
                        return None;
                    }

                    let mut contract_bytes = [0u8; 32];
                    contract_bytes.copy_from_slice(&key[8..40]);
                    let contract = match Hash::from_bytes(&contract_bytes) {
                        Ok(h) => h,
                        Err(_) => return Some(Err(BlockchainError::CorruptedData)),
                    };

                    let mut exec_topo_bytes = [0u8; 8];
                    exec_topo_bytes.copy_from_slice(&key[40..48]);
                    let exec_topoheight = TopoHeight::from_be_bytes(exec_topo_bytes);

                    // Load the execution
                    let exec_key = Self::get_scheduled_execution_key(&contract, exec_topoheight);
                    match self.load_from_disk::<ScheduledExecution>(
                        &self.scheduled_executions,
                        &exec_key,
                        DiskContext::ScheduledExecution,
                    ) {
                        Ok(execution) => Some(Ok((exec_topoheight, registration_topo, execution))),
                        Err(e) => Some(Err(e)),
                    }
                }
                Err(e) => Some(Err(e)),
            }
        });

        Ok(stream::iter(iter))
    }

    /// Get scheduled executions at topoheight, sorted by priority (OFFERCALL ordering).
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
        Ok(Self::scan_prefix_kv(
            self.snapshot.as_ref(),
            &self.scheduled_execution_priority,
            &prefix,
        )
        .map(move |res| {
            let (key, _) = res?;
            // Key format: [8 bytes exec_topo][8 bytes inverted_offer][8 bytes reg_topo][32 bytes contract_hash]
            if key.len() != 56 {
                return Err(BlockchainError::CorruptedData);
            }

            let mut exec_topo_bytes = [0u8; 8];
            exec_topo_bytes.copy_from_slice(&key[0..8]);
            let exec_topo = TopoHeight::from_be_bytes(exec_topo_bytes);

            let mut contract_bytes = [0u8; 32];
            contract_bytes.copy_from_slice(&key[24..56]);
            let contract = Hash::from_bytes(&contract_bytes)?;

            // Load the actual execution data
            let exec_key = Self::get_scheduled_execution_key(&contract, exec_topo);
            self.load_from_disk::<ScheduledExecution>(
                &self.scheduled_executions,
                &exec_key,
                DiskContext::ScheduledExecution,
            )
        }))
    }

    /// Delete a scheduled execution and its priority index entry.
    async fn delete_contract_scheduled_execution(
        &mut self,
        contract: &Hash,
        execution: &ScheduledExecution,
    ) -> Result<(), BlockchainError> {
        let execution_topoheight = match execution.kind {
            ScheduledExecutionKind::TopoHeight(topo) => topo,
            ScheduledExecutionKind::BlockEnd => execution.registration_topoheight,
        };

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete contract {} scheduled execution at topoheight {}",
                contract,
                execution_topoheight
            );
        }

        // Delete from main storage
        let key = Self::get_scheduled_execution_key(contract, execution_topoheight);
        Self::remove_from_disk_without_reading(
            self.snapshot.as_mut(),
            &self.scheduled_executions,
            &key,
        )?;

        // Delete from registration index
        let reg_key = Self::get_scheduled_execution_registration_key(
            execution.registration_topoheight,
            contract,
            execution_topoheight,
        );
        Self::remove_from_disk_without_reading(
            self.snapshot.as_mut(),
            &self.scheduled_execution_registrations,
            &reg_key,
        )?;

        // Delete from priority index
        let priority_key = Self::get_scheduled_execution_priority_key(
            execution_topoheight,
            execution.offer_amount,
            execution.registration_topoheight,
            contract,
        );
        Self::remove_from_disk_without_reading(
            self.snapshot.as_mut(),
            &self.scheduled_execution_priority,
            &priority_key,
        )?;

        Ok(())
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

        // Use the registration index to count schedules by this contract
        // We scan from from_topoheight and count entries for this contract
        let mut count = 0u64;
        let min_prefix = from_topoheight.to_be_bytes();

        for res in Self::iter_from(
            self.snapshot.as_ref(),
            &self.scheduled_execution_registrations,
            &min_prefix,
        ) {
            let (key, _) = res?;
            if key.len() != 48 {
                continue;
            }

            // Parse registration topoheight
            let mut reg_topo_bytes = [0u8; 8];
            reg_topo_bytes.copy_from_slice(&key[0..8]);
            let reg_topo = TopoHeight::from_be_bytes(reg_topo_bytes);

            // Stop if we've passed the end of the window
            if reg_topo > to_topoheight {
                break;
            }

            // Check if this is the contract we're looking for
            let mut contract_bytes = [0u8; 32];
            contract_bytes.copy_from_slice(&key[8..40]);
            if contract_bytes == *contract.as_bytes() {
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
        // This is inefficient but handles are not frequently looked up
        let handle_bytes = handle.to_be_bytes();

        for res in self.scheduled_executions.iter() {
            let (_, value) = res.map_err(BlockchainError::DatabaseError)?;
            let execution = ScheduledExecution::from_bytes(&value)
                .map_err(|_| BlockchainError::CorruptedData)?;

            // Check if the first 8 bytes of the hash match the handle
            let hash_bytes = execution.hash.as_bytes();
            if hash_bytes[0..8] == handle_bytes {
                return Ok(Some(execution));
            }
        }

        Ok(None)
    }
}

impl SledStorage {
    /// Generate key for scheduled execution storage.
    /// Format: [8 bytes topoheight][32 bytes contract_hash]
    pub fn get_scheduled_execution_key(contract: &Hash, topoheight: TopoHeight) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..8].copy_from_slice(&topoheight.to_be_bytes());
        buf[8..].copy_from_slice(contract.as_bytes());
        buf
    }

    /// Generate key for scheduled execution registration.
    /// Format: [8 bytes registration_topoheight][32 bytes contract_hash][8 bytes execution_topoheight]
    pub fn get_scheduled_execution_registration_key(
        registration_topoheight: TopoHeight,
        contract: &Hash,
        execution_topoheight: TopoHeight,
    ) -> [u8; 48] {
        let mut buf = [0u8; 48];
        buf[0..8].copy_from_slice(&registration_topoheight.to_be_bytes());
        buf[8..40].copy_from_slice(contract.as_bytes());
        buf[40..48].copy_from_slice(&execution_topoheight.to_be_bytes());
        buf
    }

    /// Generate priority key for OFFERCALL ordering.
    /// Format: [8 bytes exec_topo][8 bytes inverted_offer][8 bytes reg_topo][32 bytes contract_hash]
    pub fn get_scheduled_execution_priority_key(
        execution_topoheight: TopoHeight,
        offer_amount: u64,
        registration_topoheight: TopoHeight,
        contract: &Hash,
    ) -> [u8; 56] {
        let mut buf = [0u8; 56];
        // Execution topoheight (ascending)
        buf[0..8].copy_from_slice(&execution_topoheight.to_be_bytes());
        // Inverted offer amount (u64::MAX - offer, so higher offers sort first)
        let inverted_offer = u64::MAX.saturating_sub(offer_amount);
        buf[8..16].copy_from_slice(&inverted_offer.to_be_bytes());
        // Registration topoheight (ascending - FIFO for equal offers)
        buf[16..24].copy_from_slice(&registration_topoheight.to_be_bytes());
        // Contract hash (deterministic tiebreaker)
        buf[24..56].copy_from_slice(contract.as_bytes());
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a deterministic hash from a byte value
    fn make_hash(byte: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = byte;
        Hash::new(bytes)
    }

    #[test]
    fn test_scheduled_execution_key_format() {
        let contract = make_hash(0xAB);
        let topoheight: TopoHeight = 12345;

        let key = SledStorage::get_scheduled_execution_key(&contract, topoheight);

        // Verify key length
        assert_eq!(key.len(), 40);

        // Verify topoheight is in big-endian at start
        let mut topo_bytes = [0u8; 8];
        topo_bytes.copy_from_slice(&key[0..8]);
        assert_eq!(TopoHeight::from_be_bytes(topo_bytes), topoheight);

        // Verify contract hash follows
        let mut contract_bytes = [0u8; 32];
        contract_bytes.copy_from_slice(&key[8..40]);
        assert_eq!(Hash::new(contract_bytes), contract);
    }

    #[test]
    fn test_scheduled_execution_key_ordering() {
        let contract = make_hash(0x01);

        // Keys should be ordered by topoheight first
        let key1 = SledStorage::get_scheduled_execution_key(&contract, 100);
        let key2 = SledStorage::get_scheduled_execution_key(&contract, 200);
        let key3 = SledStorage::get_scheduled_execution_key(&contract, 50);

        // Lexicographic comparison should order by topoheight
        assert!(key3 < key1);
        assert!(key1 < key2);
    }

    #[test]
    fn test_registration_key_format() {
        let contract = make_hash(0xCD);
        let reg_topo: TopoHeight = 1000;
        let exec_topo: TopoHeight = 2000;

        let key =
            SledStorage::get_scheduled_execution_registration_key(reg_topo, &contract, exec_topo);

        // Verify key length
        assert_eq!(key.len(), 48);

        // Verify registration topoheight
        let mut reg_bytes = [0u8; 8];
        reg_bytes.copy_from_slice(&key[0..8]);
        assert_eq!(TopoHeight::from_be_bytes(reg_bytes), reg_topo);

        // Verify contract hash
        let mut contract_bytes = [0u8; 32];
        contract_bytes.copy_from_slice(&key[8..40]);
        assert_eq!(Hash::new(contract_bytes), contract);

        // Verify execution topoheight
        let mut exec_bytes = [0u8; 8];
        exec_bytes.copy_from_slice(&key[40..48]);
        assert_eq!(TopoHeight::from_be_bytes(exec_bytes), exec_topo);
    }

    #[test]
    fn test_priority_key_format() {
        let contract = make_hash(0xEF);
        let exec_topo: TopoHeight = 500;
        let offer: u64 = 1_000_000;
        let reg_topo: TopoHeight = 100;

        let key = SledStorage::get_scheduled_execution_priority_key(
            exec_topo, offer, reg_topo, &contract,
        );

        // Verify key length
        assert_eq!(key.len(), 56);

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

        // Verify contract hash
        let mut contract_bytes = [0u8; 32];
        contract_bytes.copy_from_slice(&key[24..56]);
        assert_eq!(Hash::new(contract_bytes), contract);
    }

    #[test]
    fn test_priority_key_higher_offer_sorts_first() {
        let contract = make_hash(0x01);
        let exec_topo: TopoHeight = 100;
        let reg_topo: TopoHeight = 50;

        // Higher offer should have LOWER key value (due to inversion)
        let key_high_offer = SledStorage::get_scheduled_execution_priority_key(
            exec_topo, 1_000_000, // High offer
            reg_topo, &contract,
        );
        let key_low_offer = SledStorage::get_scheduled_execution_priority_key(
            exec_topo, 100, // Low offer
            reg_topo, &contract,
        );

        // Lexicographically, high offer key should come BEFORE low offer key
        assert!(key_high_offer < key_low_offer);
    }

    #[test]
    fn test_priority_key_fifo_for_equal_offers() {
        let contract = make_hash(0x02);
        let exec_topo: TopoHeight = 100;
        let offer: u64 = 500_000;

        // Earlier registration should sort first for equal offers
        let key_early_reg = SledStorage::get_scheduled_execution_priority_key(
            exec_topo, offer, 10, // Early registration
            &contract,
        );
        let key_late_reg = SledStorage::get_scheduled_execution_priority_key(
            exec_topo, offer, 50, // Late registration
            &contract,
        );

        // Earlier registration should come first
        assert!(key_early_reg < key_late_reg);
    }

    #[test]
    fn test_priority_key_contract_tiebreaker() {
        let exec_topo: TopoHeight = 100;
        let offer: u64 = 500_000;
        let reg_topo: TopoHeight = 50;

        // When offer and registration are equal, contract hash breaks tie
        let contract_a = make_hash(0x01);
        let contract_b = make_hash(0x02);

        let key_a = SledStorage::get_scheduled_execution_priority_key(
            exec_topo,
            offer,
            reg_topo,
            &contract_a,
        );
        let key_b = SledStorage::get_scheduled_execution_priority_key(
            exec_topo,
            offer,
            reg_topo,
            &contract_b,
        );

        // Lower contract hash should come first
        assert!(key_a < key_b);
    }

    #[test]
    fn test_priority_key_execution_topoheight_primary_sort() {
        let contract = make_hash(0x01);
        let offer: u64 = 1_000_000;
        let reg_topo: TopoHeight = 50;

        // Earlier execution topoheight should always sort first
        let key_early_exec = SledStorage::get_scheduled_execution_priority_key(
            100, // Early execution
            offer, reg_topo, &contract,
        );
        let key_late_exec = SledStorage::get_scheduled_execution_priority_key(
            200, // Late execution
            offer, reg_topo, &contract,
        );

        assert!(key_early_exec < key_late_exec);
    }

    #[test]
    fn test_priority_key_zero_offer() {
        let contract = make_hash(0x01);
        let exec_topo: TopoHeight = 100;
        let reg_topo: TopoHeight = 50;

        // Zero offer should work correctly
        let key = SledStorage::get_scheduled_execution_priority_key(
            exec_topo, 0, // Zero offer
            reg_topo, &contract,
        );

        // Verify inverted offer is u64::MAX
        let mut offer_bytes = [0u8; 8];
        offer_bytes.copy_from_slice(&key[8..16]);
        let inverted = u64::from_be_bytes(offer_bytes);
        assert_eq!(inverted, u64::MAX);
    }

    #[test]
    fn test_priority_key_max_offer() {
        let contract = make_hash(0x01);
        let exec_topo: TopoHeight = 100;
        let reg_topo: TopoHeight = 50;

        // Max offer should work correctly
        let key = SledStorage::get_scheduled_execution_priority_key(
            exec_topo,
            u64::MAX, // Max offer
            reg_topo,
            &contract,
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
        let contract_a = make_hash(0x01);
        let contract_b = make_hash(0x02);

        // Create keys with various priorities for execution at topoheight 100
        let mut keys = vec![
            // (description, key)
            (
                "high offer, early reg",
                SledStorage::get_scheduled_execution_priority_key(100, 1_000_000, 10, &contract_a),
            ),
            (
                "high offer, late reg",
                SledStorage::get_scheduled_execution_priority_key(100, 1_000_000, 20, &contract_a),
            ),
            (
                "low offer, early reg",
                SledStorage::get_scheduled_execution_priority_key(100, 100, 5, &contract_a),
            ),
            (
                "medium offer",
                SledStorage::get_scheduled_execution_priority_key(100, 500_000, 15, &contract_a),
            ),
            (
                "high offer, same reg, contract_b",
                SledStorage::get_scheduled_execution_priority_key(100, 1_000_000, 10, &contract_b),
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
}
