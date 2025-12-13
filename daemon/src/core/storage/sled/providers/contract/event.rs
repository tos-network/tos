// Contract Event Provider implementation for Sled storage
//
// Implements ContractEventProvider trait for storing and querying
// contract events emitted via LOG0-LOG4 syscalls.

use crate::core::{
    error::BlockchainError,
    storage::{ContractEventProvider, SledStorage, StoredContractEvent},
};
use async_trait::async_trait;
use log::trace;
use tos_common::{block::TopoHeight, crypto::Hash, serializer::Serializer};

// Key size constants
const HASH_SIZE: usize = 32;
const TOPOHEIGHT_SIZE: usize = 8;
const LOG_INDEX_SIZE: usize = 4;
const TOPIC_SIZE: usize = 32;

#[async_trait]
impl ContractEventProvider for SledStorage {
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

        let event_bytes = event.to_bytes();

        // Store in contract_events: {contract_hash}{topoheight}{log_index} => event
        let primary_key =
            Self::get_contract_event_key(&event.contract, event.topoheight, event.log_index);
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.contract_events,
            &primary_key,
            &event_bytes[..],
        )?;

        // Store in contract_events_by_tx: {tx_hash}{log_index} => event
        let tx_key = Self::get_event_by_tx_key(&event.tx_hash, event.log_index);
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.contract_events_by_tx,
            &tx_key,
            &event_bytes[..],
        )?;

        // Store in contract_events_by_topic if topic0 exists
        if let Some(topic0) = event.topic0() {
            let topic_key = Self::get_event_by_topic_key(
                &event.contract,
                topic0,
                event.topoheight,
                event.log_index,
            );
            Self::insert_into_disk(
                self.snapshot.as_mut(),
                &self.contract_events_by_topic,
                &topic_key,
                &event_bytes[..],
            )?;
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

        let effective_limit = limit.unwrap_or(1000).min(1000);
        let mut events = Vec::with_capacity(effective_limit);

        // Build prefix for iteration
        let prefix = contract.as_bytes();

        for result in Self::scan_prefix(self.snapshot.as_ref(), &self.contract_events, prefix) {
            let key = result?;

            // Check key length and prefix match
            if key.len() < HASH_SIZE + TOPOHEIGHT_SIZE + LOG_INDEX_SIZE {
                continue;
            }

            // Extract topoheight from key
            let mut topo_bytes = [0u8; 8];
            topo_bytes.copy_from_slice(&key[HASH_SIZE..HASH_SIZE + TOPOHEIGHT_SIZE]);
            let topoheight = TopoHeight::from_be_bytes(topo_bytes);

            // Check topoheight range
            if let Some(from) = from_topoheight {
                if topoheight < from {
                    continue;
                }
            }
            if let Some(to) = to_topoheight {
                if topoheight > to {
                    continue;
                }
            }

            // Load the event
            if let Some(event_bytes) = self.contract_events.get(&key)? {
                let event = StoredContractEvent::from_bytes(&event_bytes)?;
                events.push(event);

                if events.len() >= effective_limit {
                    break;
                }
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

        let effective_limit = limit.unwrap_or(1000).min(1000);
        let mut events = Vec::with_capacity(effective_limit);

        // Build prefix: contract_hash + topic0
        let mut prefix = Vec::with_capacity(HASH_SIZE + TOPIC_SIZE);
        prefix.extend_from_slice(contract.as_bytes());
        prefix.extend_from_slice(topic0);

        for result in Self::scan_prefix(
            self.snapshot.as_ref(),
            &self.contract_events_by_topic,
            &prefix,
        ) {
            let key = result?;

            // Check key length
            if key.len() < HASH_SIZE + TOPIC_SIZE + TOPOHEIGHT_SIZE + LOG_INDEX_SIZE {
                continue;
            }

            // Extract topoheight from key
            let mut topo_bytes = [0u8; 8];
            topo_bytes.copy_from_slice(
                &key[HASH_SIZE + TOPIC_SIZE..HASH_SIZE + TOPIC_SIZE + TOPOHEIGHT_SIZE],
            );
            let topoheight = TopoHeight::from_be_bytes(topo_bytes);

            // Check topoheight range
            if let Some(from) = from_topoheight {
                if topoheight < from {
                    continue;
                }
            }
            if let Some(to) = to_topoheight {
                if topoheight > to {
                    continue;
                }
            }

            // Load the event
            if let Some(event_bytes) = self.contract_events_by_topic.get(&key)? {
                let event = StoredContractEvent::from_bytes(&event_bytes)?;
                events.push(event);

                if events.len() >= effective_limit {
                    break;
                }
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

        for result in Self::scan_prefix(
            self.snapshot.as_ref(),
            &self.contract_events_by_tx,
            tx_hash.as_bytes(),
        ) {
            let key = result?;

            // Check key length
            if key.len() < HASH_SIZE + LOG_INDEX_SIZE {
                continue;
            }

            // Load the event
            if let Some(event_bytes) = self.contract_events_by_tx.get(&key)? {
                let event = StoredContractEvent::from_bytes(&event_bytes)?;
                events.push(event);
            }
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

        // Get all events at this topoheight by scanning contract_events
        let events_to_delete: Vec<StoredContractEvent> = {
            let mut events = Vec::new();
            for result in Self::iter(self.snapshot.as_ref(), &self.contract_events) {
                let (_, value) = result?;
                let event = StoredContractEvent::from_bytes(&value)?;
                if event.topoheight == topoheight {
                    events.push(event);
                }
            }
            events
        };

        // Delete each event from all indices
        for event in events_to_delete {
            // Delete from contract_events
            let primary_key =
                Self::get_contract_event_key(&event.contract, event.topoheight, event.log_index);
            Self::remove_from_disk_without_reading(
                self.snapshot.as_mut(),
                &self.contract_events,
                &primary_key,
            )?;

            // Delete from contract_events_by_tx
            let tx_key = Self::get_event_by_tx_key(&event.tx_hash, event.log_index);
            Self::remove_from_disk_without_reading(
                self.snapshot.as_mut(),
                &self.contract_events_by_tx,
                &tx_key,
            )?;

            // Delete from contract_events_by_topic if topic0 exists
            if let Some(topic0) = event.topic0() {
                let topic_key = Self::get_event_by_topic_key(
                    &event.contract,
                    topic0,
                    event.topoheight,
                    event.log_index,
                );
                Self::remove_from_disk_without_reading(
                    self.snapshot.as_mut(),
                    &self.contract_events_by_topic,
                    &topic_key,
                )?;
            }
        }

        Ok(())
    }

    async fn count_events(&self) -> Result<u64, BlockchainError> {
        trace!("count contract events");
        let count = self.contract_events.len();
        Ok(count as u64)
    }
}

impl SledStorage {
    // Key builders for contract events

    /// Build key for contract_events: {contract_hash (32)}{topoheight (8)}{log_index (4)}
    fn get_contract_event_key(contract: &Hash, topoheight: TopoHeight, log_index: u32) -> [u8; 44] {
        let mut key = [0u8; HASH_SIZE + TOPOHEIGHT_SIZE + LOG_INDEX_SIZE];
        key[0..32].copy_from_slice(contract.as_bytes());
        key[32..40].copy_from_slice(&topoheight.to_be_bytes());
        key[40..44].copy_from_slice(&log_index.to_be_bytes());
        key
    }

    /// Build key for contract_events_by_tx: {tx_hash (32)}{log_index (4)}
    fn get_event_by_tx_key(tx_hash: &Hash, log_index: u32) -> [u8; 36] {
        let mut key = [0u8; HASH_SIZE + LOG_INDEX_SIZE];
        key[0..32].copy_from_slice(tx_hash.as_bytes());
        key[32..36].copy_from_slice(&log_index.to_be_bytes());
        key
    }

    /// Build key for contract_events_by_topic: {contract_hash (32)}{topic0 (32)}{topoheight (8)}{log_index (4)}
    fn get_event_by_topic_key(
        contract: &Hash,
        topic0: &[u8; 32],
        topoheight: TopoHeight,
        log_index: u32,
    ) -> [u8; 76] {
        let mut key = [0u8; HASH_SIZE + TOPIC_SIZE + TOPOHEIGHT_SIZE + LOG_INDEX_SIZE];
        key[0..32].copy_from_slice(contract.as_bytes());
        key[32..64].copy_from_slice(topic0);
        key[64..72].copy_from_slice(&topoheight.to_be_bytes());
        key[72..76].copy_from_slice(&log_index.to_be_bytes());
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_key_construction() {
        let contract = Hash::zero();
        let topoheight = 67890u64;
        let log_index = 42u32;
        let tx_hash = Hash::zero();
        let topic0 = [1u8; 32];

        // Test primary key
        let primary_key = SledStorage::get_contract_event_key(&contract, topoheight, log_index);
        assert_eq!(primary_key.len(), 44);
        assert_eq!(&primary_key[0..32], contract.as_bytes());
        assert_eq!(&primary_key[32..40], &topoheight.to_be_bytes());
        assert_eq!(&primary_key[40..44], &log_index.to_be_bytes());

        // Test tx key
        let tx_key = SledStorage::get_event_by_tx_key(&tx_hash, log_index);
        assert_eq!(tx_key.len(), 36);
        assert_eq!(&tx_key[0..32], tx_hash.as_bytes());
        assert_eq!(&tx_key[32..36], &log_index.to_be_bytes());

        // Test topic key
        let topic_key =
            SledStorage::get_event_by_topic_key(&contract, &topic0, topoheight, log_index);
        assert_eq!(topic_key.len(), 76);
        assert_eq!(&topic_key[0..32], contract.as_bytes());
        assert_eq!(&topic_key[32..64], &topic0);
        assert_eq!(&topic_key[64..72], &topoheight.to_be_bytes());
        assert_eq!(&topic_key[72..76], &log_index.to_be_bytes());
    }
}
