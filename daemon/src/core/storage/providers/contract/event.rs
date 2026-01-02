// Contract Event Storage Provider
//
// This module provides trait definitions and types for storing and querying
// contract events emitted via LOG0-LOG4 syscalls in the TAKO VM.

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tos_common::{
    block::TopoHeight,
    crypto::Hash,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Maximum number of events to return in a single query (pagination limit)
pub const MAX_EVENTS_PER_QUERY: usize = 1000;

/// Stored contract event structure
///
/// This represents a contract event that has been persisted to storage.
/// Events are Ethereum-compatible with indexed topics (up to 4) and arbitrary data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredContractEvent {
    /// Contract address that emitted the event
    pub contract: Hash,
    /// Transaction hash that triggered the event
    pub tx_hash: Hash,
    /// Block hash where the transaction was executed
    pub block_hash: Hash,
    /// Topoheight when the event was stored
    pub topoheight: TopoHeight,
    /// Event index within the transaction (0-based)
    pub log_index: u32,
    /// Indexed topics (max 4, each 32 bytes) - Ethereum compatible
    /// topic[0] is typically the event signature hash
    pub topics: Vec<[u8; 32]>,
    /// Non-indexed event data (ABI-encoded parameters)
    pub data: Vec<u8>,
}

impl StoredContractEvent {
    /// Create a new stored contract event
    pub fn new(
        contract: Hash,
        tx_hash: Hash,
        block_hash: Hash,
        topoheight: TopoHeight,
        log_index: u32,
        topics: Vec<[u8; 32]>,
        data: Vec<u8>,
    ) -> Self {
        Self {
            contract,
            tx_hash,
            block_hash,
            topoheight,
            log_index,
            topics,
            data,
        }
    }

    /// Get the first topic (event signature) if present
    pub fn topic0(&self) -> Option<&[u8; 32]> {
        self.topics.first()
    }
}

impl Serializer for StoredContractEvent {
    fn write(&self, writer: &mut Writer) {
        self.contract.write(writer);
        self.tx_hash.write(writer);
        self.block_hash.write(writer);
        self.topoheight.write(writer);
        self.log_index.write(writer);
        // Write topics count and each topic
        (self.topics.len() as u8).write(writer);
        for topic in &self.topics {
            writer.write_bytes(topic);
        }
        // Write data
        self.data.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let contract = Hash::read(reader)?;
        let tx_hash = Hash::read(reader)?;
        let block_hash = Hash::read(reader)?;
        let topoheight = TopoHeight::read(reader)?;
        let log_index = u32::read(reader)?;
        // Read topics
        let topic_count = u8::read(reader)?;
        if topic_count > 4 {
            return Err(ReaderError::InvalidValue);
        }
        let mut topics = Vec::with_capacity(topic_count as usize);
        for _ in 0..topic_count {
            let topic = reader.read_bytes_32()?;
            topics.push(topic);
        }
        // Read data
        let data = Vec::<u8>::read(reader)?;

        Ok(Self {
            contract,
            tx_hash,
            block_hash,
            topoheight,
            log_index,
            topics,
            data,
        })
    }

    fn size(&self) -> usize {
        self.contract.size()
            + self.tx_hash.size()
            + self.block_hash.size()
            + self.topoheight.size()
            + self.log_index.size()
            + 1 // topic count
            + self.topics.len() * 32
            + self.data.size()
    }
}

/// Event query filter parameters
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Filter by contract address
    pub contract: Option<Hash>,
    /// Filter by topic0 (event signature)
    pub topic0: Option<[u8; 32]>,
    /// Minimum topoheight (inclusive)
    pub from_topoheight: Option<TopoHeight>,
    /// Maximum topoheight (inclusive)
    pub to_topoheight: Option<TopoHeight>,
    /// Maximum number of events to return
    pub limit: Option<usize>,
}

impl EventFilter {
    /// Create a new event filter for a specific contract
    pub fn for_contract(contract: Hash) -> Self {
        Self {
            contract: Some(contract),
            ..Default::default()
        }
    }

    /// Add topic0 filter (event signature)
    pub fn with_topic0(mut self, topic0: [u8; 32]) -> Self {
        self.topic0 = Some(topic0);
        self
    }

    /// Add topoheight range filter
    pub fn with_range(mut self, from: Option<TopoHeight>, to: Option<TopoHeight>) -> Self {
        self.from_topoheight = from;
        self.to_topoheight = to;
        self
    }

    /// Add result limit
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Get effective limit (capped at MAX_EVENTS_PER_QUERY)
    pub fn effective_limit(&self) -> usize {
        self.limit
            .unwrap_or(MAX_EVENTS_PER_QUERY)
            .min(MAX_EVENTS_PER_QUERY)
    }
}

/// Contract event storage provider trait
///
/// Implementations must support both RocksDB and Sled storage backends.
#[async_trait]
pub trait ContractEventProvider {
    /// Store a contract event
    ///
    /// Events are indexed by:
    /// - contract hash + topoheight for contract-based queries
    /// - (contract, topic0) for event signature filtering
    /// - tx_hash for transaction-based lookups
    async fn store_contract_event(
        &mut self,
        event: StoredContractEvent,
    ) -> Result<(), BlockchainError>;

    /// Store multiple events from a single transaction
    ///
    /// This is more efficient than storing events one by one.
    async fn store_contract_events(
        &mut self,
        events: Vec<StoredContractEvent>,
    ) -> Result<(), BlockchainError> {
        for event in events {
            self.store_contract_event(event).await?;
        }
        Ok(())
    }

    /// Get events by contract address
    ///
    /// Returns events for a specific contract, optionally filtered by topoheight range.
    async fn get_events_by_contract(
        &self,
        contract: &Hash,
        from_topoheight: Option<TopoHeight>,
        to_topoheight: Option<TopoHeight>,
        limit: Option<usize>,
    ) -> Result<Vec<StoredContractEvent>, BlockchainError>;

    /// Get events by contract and topic0 (event signature)
    ///
    /// This is the most common query pattern for filtering specific event types.
    async fn get_events_by_topic(
        &self,
        contract: &Hash,
        topic0: &[u8; 32],
        from_topoheight: Option<TopoHeight>,
        to_topoheight: Option<TopoHeight>,
        limit: Option<usize>,
    ) -> Result<Vec<StoredContractEvent>, BlockchainError>;

    /// Get all events for a specific transaction
    async fn get_events_by_tx(
        &self,
        tx_hash: &Hash,
    ) -> Result<Vec<StoredContractEvent>, BlockchainError>;

    /// Get events using a filter
    ///
    /// This is a convenience method that dispatches to the appropriate query method.
    async fn get_events_with_filter(
        &self,
        filter: &EventFilter,
    ) -> Result<Vec<StoredContractEvent>, BlockchainError> {
        let limit = filter.effective_limit();

        match (&filter.contract, &filter.topic0) {
            (Some(contract), Some(topic0)) => {
                self.get_events_by_topic(
                    contract,
                    topic0,
                    filter.from_topoheight,
                    filter.to_topoheight,
                    Some(limit),
                )
                .await
            }
            (Some(contract), None) => {
                self.get_events_by_contract(
                    contract,
                    filter.from_topoheight,
                    filter.to_topoheight,
                    Some(limit),
                )
                .await
            }
            _ => Ok(Vec::new()), // Contract filter is required
        }
    }

    /// Delete events for a specific topoheight (for reorg handling)
    async fn delete_events_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Count total events stored (for statistics)
    async fn count_events(&self) -> Result<u64, BlockchainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stored_event_serialization() {
        let event = StoredContractEvent::new(
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
            100,
            0,
            vec![[1u8; 32], [2u8; 32]],
            vec![3, 4, 5],
        );

        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        event.write(&mut writer);

        let bytes = writer.as_bytes();
        let mut reader = Reader::new(bytes);
        let decoded = StoredContractEvent::read(&mut reader).expect("test");

        assert_eq!(event, decoded);
    }

    #[test]
    fn test_event_filter_builder() {
        let filter = EventFilter::for_contract(Hash::zero())
            .with_topic0([1u8; 32])
            .with_range(Some(10), Some(100))
            .with_limit(50);

        assert!(filter.contract.is_some());
        assert!(filter.topic0.is_some());
        assert_eq!(filter.from_topoheight, Some(10));
        assert_eq!(filter.to_topoheight, Some(100));
        assert_eq!(filter.limit, Some(50));
    }

    #[test]
    fn test_effective_limit() {
        // With explicit limit under max
        let filter = EventFilter::default().with_limit(50);
        assert_eq!(filter.effective_limit(), 50);

        // With explicit limit over max
        let filter = EventFilter::default().with_limit(2000);
        assert_eq!(filter.effective_limit(), MAX_EVENTS_PER_QUERY);

        // Without explicit limit
        let filter = EventFilter::default();
        assert_eq!(filter.effective_limit(), MAX_EVENTS_PER_QUERY);
    }
}
