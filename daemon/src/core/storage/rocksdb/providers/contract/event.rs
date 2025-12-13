// Contract Event Provider implementation for RocksDB
//
// Implements ContractEventProvider trait for storing and querying
// contract events emitted via LOG0-LOG4 syscalls.

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        ContractEventProvider, StoredContractEvent,
    },
};
use async_trait::async_trait;
use log::trace;
use rocksdb::Direction;
use tos_common::{block::TopoHeight, crypto::Hash};

// Key size constants
const CONTRACT_ID_SIZE: usize = 8;
const TOPOHEIGHT_SIZE: usize = 8;
const LOG_INDEX_SIZE: usize = 4;
const TOPIC_SIZE: usize = 32;

#[async_trait]
impl ContractEventProvider for RocksStorage {
    async fn store_contract_event(
        &mut self,
        event: StoredContractEvent,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "store contract event for contract {} tx {} at topoheight {} log_index {}",
                event.contract,
                event.tx_hash,
                event.topoheight,
                event.log_index
            );
        }

        // Get contract ID (or create one if needed)
        let contract_id = self.get_or_create_contract_id(&event.contract)?;

        // Store in ContractEvents: {contract_id}{topoheight}{log_index} => event
        let primary_key =
            Self::get_contract_event_key(contract_id, event.topoheight, event.log_index);
        self.insert_into_disk(Column::ContractEvents, &primary_key, &event)?;

        // Store in ContractEventsByTx: {tx_hash}{log_index} => event
        let tx_key = Self::get_event_by_tx_key(&event.tx_hash, event.log_index);
        self.insert_into_disk(Column::ContractEventsByTx, &tx_key, &event)?;

        // Store in ContractEventsByTopic if topic0 exists
        if let Some(topic0) = event.topic0() {
            let topic_key = Self::get_event_by_topic_key(
                contract_id,
                topic0,
                event.topoheight,
                event.log_index,
            );
            self.insert_into_disk(Column::ContractEventsByTopic, &topic_key, &event)?;
        }

        Ok(())
    }

    async fn get_events_by_contract(
        &self,
        contract: &Hash,
        from_topoheight: Option<TopoHeight>,
        to_topoheight: Option<TopoHeight>,
        limit: Option<usize>,
    ) -> Result<Vec<StoredContractEvent>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get events by contract {} from {:?} to {:?} limit {:?}",
                contract,
                from_topoheight,
                to_topoheight,
                limit
            );
        }

        let Some(contract_id) = self.get_optional_contract_id(contract)? else {
            return Ok(Vec::new());
        };

        let effective_limit = limit.unwrap_or(1000).min(1000);
        let mut events = Vec::with_capacity(effective_limit);

        // Build the prefix key for iteration
        let prefix = contract_id.to_be_bytes();
        let start_key = if let Some(from_topo) = from_topoheight {
            let mut key = Vec::with_capacity(CONTRACT_ID_SIZE + TOPOHEIGHT_SIZE);
            key.extend_from_slice(&prefix);
            key.extend_from_slice(&from_topo.to_be_bytes());
            key
        } else {
            prefix.to_vec()
        };

        let iter = self.iter::<Vec<u8>, StoredContractEvent>(
            Column::ContractEvents,
            IteratorMode::From(&start_key, Direction::Forward),
        )?;

        for result in iter {
            let (key, event) = result?;

            // Check if we're still in the same contract's events
            if key.len() < CONTRACT_ID_SIZE || &key[..CONTRACT_ID_SIZE] != prefix.as_slice() {
                break;
            }

            // Check topoheight range
            if let Some(to_topo) = to_topoheight {
                if event.topoheight > to_topo {
                    break;
                }
            }

            events.push(event);

            if events.len() >= effective_limit {
                break;
            }
        }

        Ok(events)
    }

    async fn get_events_by_topic(
        &self,
        contract: &Hash,
        topic0: &[u8; 32],
        from_topoheight: Option<TopoHeight>,
        to_topoheight: Option<TopoHeight>,
        limit: Option<usize>,
    ) -> Result<Vec<StoredContractEvent>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get events by topic for contract {} topic0 {} from {:?} to {:?} limit {:?}",
                contract,
                hex::encode(topic0),
                from_topoheight,
                to_topoheight,
                limit
            );
        }

        let Some(contract_id) = self.get_optional_contract_id(contract)? else {
            return Ok(Vec::new());
        };

        let effective_limit = limit.unwrap_or(1000).min(1000);
        let mut events = Vec::with_capacity(effective_limit);

        // Build prefix: contract_id + topic0
        let mut prefix = Vec::with_capacity(CONTRACT_ID_SIZE + TOPIC_SIZE);
        prefix.extend_from_slice(&contract_id.to_be_bytes());
        prefix.extend_from_slice(topic0);

        // Build start key with optional from_topoheight
        let start_key = if let Some(from_topo) = from_topoheight {
            let mut key = prefix.clone();
            key.extend_from_slice(&from_topo.to_be_bytes());
            key
        } else {
            prefix.clone()
        };

        let iter = self.iter::<Vec<u8>, StoredContractEvent>(
            Column::ContractEventsByTopic,
            IteratorMode::From(&start_key, Direction::Forward),
        )?;

        for result in iter {
            let (key, event) = result?;

            // Check if we're still in the same contract+topic prefix
            if key.len() < prefix.len() || &key[..prefix.len()] != prefix.as_slice() {
                break;
            }

            // Check topoheight range
            if let Some(to_topo) = to_topoheight {
                if event.topoheight > to_topo {
                    break;
                }
            }

            events.push(event);

            if events.len() >= effective_limit {
                break;
            }
        }

        Ok(events)
    }

    async fn get_events_by_tx(
        &self,
        tx_hash: &Hash,
    ) -> Result<Vec<StoredContractEvent>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get events by tx {}", tx_hash);
        }

        let mut events = Vec::new();

        let iter = self.iter::<Vec<u8>, StoredContractEvent>(
            Column::ContractEventsByTx,
            IteratorMode::WithPrefix(tx_hash.as_bytes(), Direction::Forward),
        )?;

        for result in iter {
            let (key, event) = result?;

            // Check if we're still in the same tx's events
            if key.len() < 32 || &key[..32] != tx_hash.as_bytes() {
                break;
            }

            events.push(event);
        }

        // Sort by log_index to ensure correct ordering
        events.sort_by_key(|e| e.log_index);

        Ok(events)
    }

    async fn delete_events_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete events at topoheight {}", topoheight);
        }

        // Get all events at this topoheight by scanning ContractEvents
        let events_to_delete: Vec<StoredContractEvent> = {
            let mut events = Vec::new();
            let iter = self.iter::<Vec<u8>, StoredContractEvent>(
                Column::ContractEvents,
                IteratorMode::Start,
            )?;

            for result in iter {
                let (_, event) = result?;
                if event.topoheight == topoheight {
                    events.push(event);
                }
            }
            events
        };

        // Delete each event from all indices
        for event in events_to_delete {
            if let Some(contract_id) = self.get_optional_contract_id(&event.contract)? {
                // Delete from ContractEvents
                let primary_key =
                    Self::get_contract_event_key(contract_id, event.topoheight, event.log_index);
                self.remove_from_disk(Column::ContractEvents, &primary_key)?;

                // Delete from ContractEventsByTx
                let tx_key = Self::get_event_by_tx_key(&event.tx_hash, event.log_index);
                self.remove_from_disk(Column::ContractEventsByTx, &tx_key)?;

                // Delete from ContractEventsByTopic if topic0 exists
                if let Some(topic0) = event.topic0() {
                    let topic_key = Self::get_event_by_topic_key(
                        contract_id,
                        topic0,
                        event.topoheight,
                        event.log_index,
                    );
                    self.remove_from_disk(Column::ContractEventsByTopic, &topic_key)?;
                }
            }
        }

        Ok(())
    }

    async fn count_events(&self) -> Result<u64, BlockchainError> {
        trace!("count contract events");
        let count = self.count_entries(Column::ContractEvents)?;
        Ok(count as u64)
    }
}

impl RocksStorage {
    // Key builders for contract events

    /// Build key for ContractEvents: {contract_id (8)}{topoheight (8)}{log_index (4)}
    fn get_contract_event_key(
        contract_id: u64,
        topoheight: TopoHeight,
        log_index: u32,
    ) -> [u8; 20] {
        let mut key = [0u8; CONTRACT_ID_SIZE + TOPOHEIGHT_SIZE + LOG_INDEX_SIZE];
        key[0..8].copy_from_slice(&contract_id.to_be_bytes());
        key[8..16].copy_from_slice(&topoheight.to_be_bytes());
        key[16..20].copy_from_slice(&log_index.to_be_bytes());
        key
    }

    /// Build key for ContractEventsByTx: {tx_hash (32)}{log_index (4)}
    fn get_event_by_tx_key(tx_hash: &Hash, log_index: u32) -> [u8; 36] {
        let mut key = [0u8; 32 + LOG_INDEX_SIZE];
        key[0..32].copy_from_slice(tx_hash.as_bytes());
        key[32..36].copy_from_slice(&log_index.to_be_bytes());
        key
    }

    /// Build key for ContractEventsByTopic: {contract_id (8)}{topic0 (32)}{topoheight (8)}{log_index (4)}
    fn get_event_by_topic_key(
        contract_id: u64,
        topic0: &[u8; 32],
        topoheight: TopoHeight,
        log_index: u32,
    ) -> [u8; 52] {
        let mut key = [0u8; CONTRACT_ID_SIZE + TOPIC_SIZE + TOPOHEIGHT_SIZE + LOG_INDEX_SIZE];
        key[0..8].copy_from_slice(&contract_id.to_be_bytes());
        key[8..40].copy_from_slice(topic0);
        key[40..48].copy_from_slice(&topoheight.to_be_bytes());
        key[48..52].copy_from_slice(&log_index.to_be_bytes());
        key
    }

    /// Get or create a contract ID for a contract hash
    fn get_or_create_contract_id(&mut self, contract: &Hash) -> Result<u64, BlockchainError> {
        // Try to get existing ID first
        if let Some(id) = self.get_optional_contract_id(contract)? {
            return Ok(id);
        }

        // Create a new ID - use hash as a deterministic ID source
        // This is a fallback; normally contracts should be registered via deploy
        let mut id_bytes = [0u8; 8];
        id_bytes.copy_from_slice(&contract.as_bytes()[..8]);
        let id = u64::from_be_bytes(id_bytes);

        // Store the mapping
        self.insert_into_disk(Column::Contracts, contract, contract)?;
        self.insert_into_disk(Column::ContractById, &id.to_be_bytes(), contract)?;

        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_key_construction() {
        let contract_id = 12345u64;
        let topoheight = 67890u64;
        let log_index = 42u32;
        let tx_hash = Hash::zero();
        let topic0 = [1u8; 32];

        // Test primary key
        let primary_key = RocksStorage::get_contract_event_key(contract_id, topoheight, log_index);
        assert_eq!(primary_key.len(), 20);
        assert_eq!(&primary_key[0..8], &contract_id.to_be_bytes());
        assert_eq!(&primary_key[8..16], &topoheight.to_be_bytes());
        assert_eq!(&primary_key[16..20], &log_index.to_be_bytes());

        // Test tx key
        let tx_key = RocksStorage::get_event_by_tx_key(&tx_hash, log_index);
        assert_eq!(tx_key.len(), 36);
        assert_eq!(&tx_key[0..32], tx_hash.as_bytes());
        assert_eq!(&tx_key[32..36], &log_index.to_be_bytes());

        // Test topic key
        let topic_key =
            RocksStorage::get_event_by_topic_key(contract_id, &topic0, topoheight, log_index);
        assert_eq!(topic_key.len(), 52);
        assert_eq!(&topic_key[0..8], &contract_id.to_be_bytes());
        assert_eq!(&topic_key[8..40], &topic0);
        assert_eq!(&topic_key[40..48], &topoheight.to_be_bytes());
        assert_eq!(&topic_key[48..52], &log_index.to_be_bytes());
    }
}
