// RocksDB Contract Event Storage Provider Implementation
//
// This module implements the ContractEventProvider trait for RocksDB storage.
// Events are indexed by:
// - Contract hash + topoheight for contract-based queries
// - Transaction hash for transaction-based lookups
// - (contract, topic0) for event signature filtering

use crate::core::error::BlockchainError;
use crate::core::storage::Direction;
use crate::core::storage::{
    rocksdb::{Column, IteratorMode},
    ContractEventProvider, RocksStorage, StoredContractEvent, MAX_EVENTS_PER_QUERY,
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::TopoHeight,
    crypto::Hash,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Key builder for ContractEvents column
/// Format: {topoheight:8}{contract_hash:32}{log_index:4}
fn build_event_key(topoheight: TopoHeight, contract: &Hash, log_index: u32) -> EventKey {
    EventKey {
        topoheight,
        contract: contract.clone(),
        log_index,
    }
}

/// Key builder for ContractEventsByTopic column
/// Format: {contract_hash:32}{topic0:32}{topoheight:8}{log_index:4}
fn build_topic_index_key(
    contract: &Hash,
    topic0: &[u8; 32],
    topoheight: TopoHeight,
    log_index: u32,
) -> TopicIndexKey {
    TopicIndexKey {
        contract: contract.clone(),
        topic0: *topic0,
        topoheight,
        log_index,
    }
}

/// Key builder for ContractEventsByTx column
/// Format: {tx_hash:32}
fn build_tx_index_key(tx_hash: &Hash) -> Hash {
    tx_hash.clone()
}

/// Structured key for ContractEvents column
#[derive(Clone)]
struct EventKey {
    topoheight: TopoHeight,
    contract: Hash,
    log_index: u32,
}

impl Serializer for EventKey {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(&self.topoheight.to_be_bytes());
        writer.write_bytes(self.contract.as_bytes());
        writer.write_bytes(&self.log_index.to_be_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let topoheight = reader.read_u64()?;
        let contract = reader.read_hash()?;
        let log_index = reader.read_u32()?;

        Ok(Self {
            topoheight,
            contract,
            log_index,
        })
    }

    fn size(&self) -> usize {
        8 + 32 + 4
    }
}

/// Structured key for ContractEventsByTopic column
#[derive(Clone)]
struct TopicIndexKey {
    contract: Hash,
    topic0: [u8; 32],
    topoheight: TopoHeight,
    log_index: u32,
}

impl Serializer for TopicIndexKey {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.contract.as_bytes());
        writer.write_bytes(&self.topic0);
        writer.write_bytes(&self.topoheight.to_be_bytes());
        writer.write_bytes(&self.log_index.to_be_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let contract = reader.read_hash()?;
        let topic0 = reader.read_bytes_32()?;
        let topoheight = reader.read_u64()?;
        let log_index = reader.read_u32()?;

        Ok(Self {
            contract,
            topic0,
            topoheight,
            log_index,
        })
    }

    fn size(&self) -> usize {
        32 + 32 + 8 + 4
    }
}

/// Event references stored in tx index
#[derive(Clone)]
struct EventRefs(Vec<EventRef>);

impl Serializer for EventRefs {
    fn write(&self, writer: &mut Writer) {
        for r in &self.0 {
            writer.write_bytes(&r.topoheight.to_be_bytes());
            writer.write_bytes(r.contract.as_bytes());
            writer.write_bytes(&r.log_index.to_be_bytes());
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let mut refs = Vec::new();
        while reader.total_size() - reader.total_read() >= 44 {
            let topoheight = reader.read_u64()?;
            let contract = reader.read_hash()?;
            let log_index = reader.read_u32()?;

            refs.push(EventRef {
                topoheight,
                contract,
                log_index,
            });
        }
        Ok(EventRefs(refs))
    }

    fn size(&self) -> usize {
        self.0.len() * 44
    }
}

/// Single event reference
#[derive(Clone)]
struct EventRef {
    topoheight: TopoHeight,
    contract: Hash,
    log_index: u32,
}

/// Empty marker value for topic index (just need the key)
struct EmptyMarker;

impl Serializer for EmptyMarker {
    fn write(&self, _writer: &mut Writer) {
        // Write nothing
    }

    fn read(_reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(EmptyMarker)
    }

    fn size(&self) -> usize {
        0
    }
}

#[async_trait]
impl ContractEventProvider for RocksStorage {
    async fn store_contract_event(
        &mut self,
        event: StoredContractEvent,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Storing contract event: contract={}, tx={}, topo={}, log_index={}",
                event.contract,
                event.tx_hash,
                event.topoheight,
                event.log_index
            );
        }

        // 1. Store the event in ContractEvents column
        let event_key = build_event_key(event.topoheight, &event.contract, event.log_index);
        self.insert_into_disk(Column::ContractEvents, &event_key.to_bytes(), &event)?;

        // 2. Store topic0 index if event has topics
        if let Some(topic0) = event.topic0() {
            let topic_key =
                build_topic_index_key(&event.contract, topic0, event.topoheight, event.log_index);
            // Value is empty marker - we just need the key for iteration
            self.insert_into_disk(
                Column::ContractEventsByTopic,
                &topic_key.to_bytes(),
                &EmptyMarker,
            )?;
        }

        // 3. Update tx index
        let tx_key = build_tx_index_key(&event.tx_hash);
        let event_ref = EventRef {
            topoheight: event.topoheight,
            contract: event.contract.clone(),
            log_index: event.log_index,
        };

        // Read existing refs and append
        let mut refs = if let Some(EventRefs(existing)) =
            self.load_optional_from_disk::<_, EventRefs>(Column::ContractEventsByTx, &tx_key)?
        {
            existing
        } else {
            Vec::new()
        };
        refs.push(event_ref);

        // Store updated refs
        self.insert_into_disk(Column::ContractEventsByTx, &tx_key, &EventRefs(refs))?;

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
                "Getting events by contract: {}, from={:?}, to={:?}",
                contract,
                from_topoheight,
                to_topoheight
            );
        }

        let mut events = Vec::new();
        let effective_limit = limit
            .unwrap_or(MAX_EVENTS_PER_QUERY)
            .min(MAX_EVENTS_PER_QUERY);
        let from_topo = from_topoheight.unwrap_or(0);
        let to_topo = to_topoheight.unwrap_or(u64::MAX);

        // Build start key
        let start_key = build_event_key(from_topo, contract, 0);

        // Iterate through ContractEvents column
        for result in self.iter::<EventKey, StoredContractEvent>(
            Column::ContractEvents,
            IteratorMode::From(&start_key.to_bytes(), Direction::Forward),
        )? {
            let (key, event) = result?;

            // Check if we've exceeded the topoheight range
            if key.topoheight > to_topo {
                break;
            }

            // Check if this is for our contract
            if &key.contract != contract {
                // Skip events for other contracts at this topoheight
                continue;
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
                "Getting events by topic: contract={}, topic0={:?}, from={:?}, to={:?}",
                contract,
                &topic0[..4],
                from_topoheight,
                to_topoheight
            );
        }

        let mut events = Vec::new();
        let effective_limit = limit
            .unwrap_or(MAX_EVENTS_PER_QUERY)
            .min(MAX_EVENTS_PER_QUERY);
        let from_topo = from_topoheight.unwrap_or(0);
        let to_topo = to_topoheight.unwrap_or(u64::MAX);

        // Build prefix for contract+topic0
        let start_key = build_topic_index_key(contract, topic0, from_topo, 0);

        // Iterate through topic index
        for result in self.iter::<TopicIndexKey, EmptyMarker>(
            Column::ContractEventsByTopic,
            IteratorMode::From(&start_key.to_bytes(), Direction::Forward),
        )? {
            let (key, _) = result?;

            // Check if this key still has our prefix (contract + topic0)
            if &key.contract != contract || key.topic0 != *topic0 {
                break;
            }

            // Check topoheight range
            if key.topoheight > to_topo {
                break;
            }

            // Load the actual event
            let event_key = build_event_key(key.topoheight, contract, key.log_index);
            if let Some(event) = self.load_optional_from_disk::<_, StoredContractEvent>(
                Column::ContractEvents,
                &event_key.to_bytes(),
            )? {
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
            trace!("Getting events by tx: {}", tx_hash);
        }

        let tx_key = build_tx_index_key(tx_hash);
        let Some(EventRefs(refs)) =
            self.load_optional_from_disk::<_, EventRefs>(Column::ContractEventsByTx, &tx_key)?
        else {
            return Ok(Vec::new());
        };

        let mut events = Vec::new();
        for event_ref in refs {
            let event_key = build_event_key(
                event_ref.topoheight,
                &event_ref.contract,
                event_ref.log_index,
            );
            if let Some(event) = self.load_optional_from_disk::<_, StoredContractEvent>(
                Column::ContractEvents,
                &event_key.to_bytes(),
            )? {
                events.push(event);
            }
        }

        Ok(events)
    }

    async fn delete_events_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Deleting events at topoheight: {}", topoheight);
        }

        // Build start key for this topoheight (with zero contract and log_index)
        let start_key = build_event_key(topoheight, &Hash::zero(), 0);

        // Collect keys to delete
        let mut keys_to_delete = Vec::new();
        let mut topic_keys_to_delete = Vec::new();

        for result in self.iter::<EventKey, StoredContractEvent>(
            Column::ContractEvents,
            IteratorMode::From(&start_key.to_bytes(), Direction::Forward),
        )? {
            let (key, event) = result?;

            // Check if this key is at our topoheight
            if key.topoheight != topoheight {
                break;
            }

            keys_to_delete.push(key.to_bytes());

            // Also build topic index key for deletion
            if let Some(topic0) = event.topic0() {
                let topic_key = build_topic_index_key(
                    &event.contract,
                    topic0,
                    event.topoheight,
                    event.log_index,
                );
                topic_keys_to_delete.push(topic_key.to_bytes());
            }
        }

        // Delete events and topic indices
        for key in keys_to_delete {
            self.remove_from_disk(Column::ContractEvents, &key)?;
        }
        for key in topic_keys_to_delete {
            self.remove_from_disk(Column::ContractEventsByTopic, &key)?;
        }

        // Note: We don't delete from ContractEventsByTx as it would require
        // parsing all tx indices. The event lookup will just return empty.

        Ok(())
    }

    async fn count_events(&self) -> Result<u64, BlockchainError> {
        Ok(self.count_entries(Column::ContractEvents)? as u64)
    }
}
