// Sled Contract Event Storage Provider Implementation
//
// This module implements the ContractEventProvider trait for Sled storage.
// Events are indexed by:
// - Topoheight + contract hash + log_index for contract-based queries
// - Transaction hash for transaction-based lookups
// - Contract + topic0 for event signature filtering

use crate::core::{
    error::BlockchainError,
    storage::{ContractEventProvider, SledStorage, StoredContractEvent, MAX_EVENTS_PER_QUERY},
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::TopoHeight,
    crypto::Hash,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Key builder for contract_events tree
/// Format: {topoheight:8}{contract_hash:32}{log_index:4}
fn build_event_key(topoheight: TopoHeight, contract: &Hash, log_index: u32) -> [u8; 44] {
    let mut key = [0u8; 44];
    key[..8].copy_from_slice(&topoheight.to_be_bytes());
    key[8..40].copy_from_slice(contract.as_bytes());
    key[40..44].copy_from_slice(&log_index.to_be_bytes());
    key
}

/// Parse event key back to components
fn parse_event_key(key: &[u8]) -> Option<(TopoHeight, Hash, u32)> {
    if key.len() != 44 {
        return None;
    }
    let topoheight = TopoHeight::from_be_bytes(key[..8].try_into().ok()?);
    let contract = Hash::new(key[8..40].try_into().ok()?);
    let log_index = u32::from_be_bytes(key[40..44].try_into().ok()?);
    Some((topoheight, contract, log_index))
}

/// Key builder for contract_events_by_topic tree
/// Format: {contract_hash:32}{topic0:32}{topoheight:8}{log_index:4}
fn build_topic_index_key(
    contract: &Hash,
    topic0: &[u8; 32],
    topoheight: TopoHeight,
    log_index: u32,
) -> [u8; 76] {
    let mut key = [0u8; 76];
    key[..32].copy_from_slice(contract.as_bytes());
    key[32..64].copy_from_slice(topic0);
    key[64..72].copy_from_slice(&topoheight.to_be_bytes());
    key[72..76].copy_from_slice(&log_index.to_be_bytes());
    key
}

/// Parse topic index key back to components
fn parse_topic_index_key(key: &[u8]) -> Option<(Hash, [u8; 32], TopoHeight, u32)> {
    if key.len() != 76 {
        return None;
    }
    let contract = Hash::new(key[..32].try_into().ok()?);
    let mut topic0 = [0u8; 32];
    topic0.copy_from_slice(&key[32..64]);
    let topoheight = TopoHeight::from_be_bytes(key[64..72].try_into().ok()?);
    let log_index = u32::from_be_bytes(key[72..76].try_into().ok()?);
    Some((contract, topic0, topoheight, log_index))
}

/// Event references stored in tx index
/// Each reference is 44 bytes: {topoheight:8}{contract_hash:32}{log_index:4}
struct EventRefs(Vec<EventRef>);

struct EventRef {
    topoheight: TopoHeight,
    contract: Hash,
    log_index: u32,
}

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

#[async_trait]
impl ContractEventProvider for SledStorage {
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

        // 1. Store the event in contract_events tree
        let event_key = build_event_key(event.topoheight, &event.contract, event.log_index);
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.contract_events,
            &event_key,
            event.to_bytes(),
        )?;

        // 2. Store topic0 index if event has topics
        if let Some(topic0) = event.topic0() {
            let topic_key =
                build_topic_index_key(&event.contract, topic0, event.topoheight, event.log_index);
            // Value is empty - we just need the key for iteration
            Self::insert_into_disk(
                self.snapshot.as_mut(),
                &self.contract_events_by_topic,
                &topic_key,
                &[],
            )?;
        }

        // 3. Update tx index
        let event_ref = EventRef {
            topoheight: event.topoheight,
            contract: event.contract.clone(),
            log_index: event.log_index,
        };

        // Read existing refs and append
        let mut refs = if let Some(existing) = self.load_optional_from_disk::<EventRefs>(
            &self.contract_events_by_tx,
            event.tx_hash.as_bytes(),
        )? {
            existing.0
        } else {
            Vec::new()
        };
        refs.push(event_ref);

        // Store updated refs
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.contract_events_by_tx,
            event.tx_hash.as_bytes(),
            EventRefs(refs).to_bytes(),
        )?;

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

        // Iterate through contract_events tree
        for result in Self::iter(self.snapshot.as_ref(), &self.contract_events) {
            let (key, value) = result?;

            // Parse the key
            let Some((topoheight, event_contract, _log_index)) = parse_event_key(&key) else {
                continue;
            };

            // Check topoheight range
            if topoheight < from_topo {
                continue;
            }
            if topoheight > to_topo {
                break;
            }

            // Check if this is for our contract
            if &event_contract != contract {
                continue;
            }

            // Deserialize the event
            let event = StoredContractEvent::from_bytes(&value)?;
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

        // Build prefix for contract+topic0 (64 bytes)
        let mut prefix = [0u8; 64];
        prefix[..32].copy_from_slice(contract.as_bytes());
        prefix[32..64].copy_from_slice(topic0);

        // Iterate through topic index
        for result in Self::iter(self.snapshot.as_ref(), &self.contract_events_by_topic) {
            let (key, _) = result?;

            // Check if key starts with our prefix
            if key.len() < 64 || &key[..64] != prefix {
                // If we've passed our prefix, we're done
                if key.len() >= 64 && &key[..64] > &prefix[..] {
                    break;
                }
                continue;
            }

            // Parse the full key
            let Some((_, _, topoheight, log_index)) = parse_topic_index_key(&key) else {
                continue;
            };

            // Check topoheight range
            if topoheight < from_topo {
                continue;
            }
            if topoheight > to_topo {
                break;
            }

            // Load the actual event
            let event_key = build_event_key(topoheight, contract, log_index);
            if let Some(event) = self
                .load_optional_from_disk::<StoredContractEvent>(&self.contract_events, &event_key)?
            {
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

        let Some(EventRefs(refs)) = self.load_optional_from_disk::<EventRefs>(
            &self.contract_events_by_tx,
            tx_hash.as_bytes(),
        )?
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
            if let Some(event) = self
                .load_optional_from_disk::<StoredContractEvent>(&self.contract_events, &event_key)?
            {
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

        // Build start key for this topoheight (used for iteration range)
        let _start_key = build_event_key(topoheight, &Hash::zero(), 0);

        // Collect keys to delete
        let mut keys_to_delete = Vec::new();
        let mut topic_keys_to_delete = Vec::new();

        for result in Self::iter(self.snapshot.as_ref(), &self.contract_events) {
            let (key, value) = result?;

            // Parse the key
            let Some((key_topo, _, _)) = parse_event_key(&key) else {
                continue;
            };

            // Check if this key is at our topoheight
            if key_topo < topoheight {
                continue;
            }
            if key_topo > topoheight {
                break;
            }

            keys_to_delete.push(key.to_vec());

            // Also build topic index key for deletion
            let event = StoredContractEvent::from_bytes(&value)?;
            if let Some(topic0) = event.topic0() {
                let topic_key = build_topic_index_key(
                    &event.contract,
                    topic0,
                    event.topoheight,
                    event.log_index,
                );
                topic_keys_to_delete.push(topic_key.to_vec());
            }
        }

        // Delete events and topic indices
        for key in keys_to_delete {
            Self::remove_from_disk_without_reading(
                self.snapshot.as_mut(),
                &self.contract_events,
                &key,
            )?;
        }
        for key in topic_keys_to_delete {
            Self::remove_from_disk_without_reading(
                self.snapshot.as_mut(),
                &self.contract_events_by_topic,
                &key,
            )?;
        }

        // Note: We don't delete from contract_events_by_tx as it would require
        // parsing all tx indices. The event lookup will just return empty.

        Ok(())
    }

    async fn count_events(&self) -> Result<u64, BlockchainError> {
        let mut count = 0u64;
        for _ in Self::iter(self.snapshot.as_ref(), &self.contract_events) {
            count += 1;
        }
        Ok(count)
    }
}
