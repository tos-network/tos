#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
// File: testing-framework/tests/contract_event_storage_test.rs
//
// Contract Event Storage Integration Tests
//
// Tests for the contract event storage system (Limitation 3 fix):
// - StoredContractEvent serialization/deserialization
// - ContractEventProvider trait implementation for RocksDB
// - Event storage, retrieval, and filtering
// - EventFilter functionality
//
// These tests validate the event indexing system for LOG0-LOG4 syscalls.

use tos_common::crypto::Hash;
use tos_daemon::core::storage::{
    ContractEventProvider, EventFilter, StoredContractEvent, MAX_EVENTS_PER_QUERY,
};
use tos_tck::utilities::create_test_rocksdb_storage;

/// Test StoredContractEvent creation and field access
#[tokio::test]
async fn test_stored_contract_event_creation() {
    let contract = Hash::zero();
    let tx_hash = Hash::new([1u8; 32]);
    let block_hash = Hash::new([2u8; 32]);
    let topoheight = 100u64;
    let log_index = 0u32;
    let topics = vec![[3u8; 32], [4u8; 32]];
    let data = vec![5u8, 6u8, 7u8];

    let event = StoredContractEvent::new(
        contract.clone(),
        tx_hash.clone(),
        block_hash.clone(),
        topoheight,
        log_index,
        topics.clone(),
        data.clone(),
    );

    assert_eq!(event.contract, contract);
    assert_eq!(event.tx_hash, tx_hash);
    assert_eq!(event.block_hash, block_hash);
    assert_eq!(event.topoheight, topoheight);
    assert_eq!(event.log_index, log_index);
    assert_eq!(event.topics, topics);
    assert_eq!(event.data, data);

    if log::log_enabled!(log::Level::Info) {
        log::info!("StoredContractEvent creation test passed");
    }
}

/// Test topic0() helper method
#[tokio::test]
async fn test_stored_contract_event_topic0() {
    // Event with topics
    let event_with_topics = StoredContractEvent::new(
        Hash::zero(),
        Hash::zero(),
        Hash::zero(),
        100,
        0,
        vec![[1u8; 32], [2u8; 32]],
        vec![],
    );
    assert!(event_with_topics.topic0().is_some());
    assert_eq!(event_with_topics.topic0().unwrap(), &[1u8; 32]);

    // Event without topics
    let event_no_topics = StoredContractEvent::new(
        Hash::zero(),
        Hash::zero(),
        Hash::zero(),
        100,
        0,
        vec![],
        vec![],
    );
    assert!(event_no_topics.topic0().is_none());

    if log::log_enabled!(log::Level::Info) {
        log::info!("topic0() helper test passed");
    }
}

/// Test StoredContractEvent serialization and deserialization
#[tokio::test]
async fn test_stored_contract_event_serialization() {
    use tos_common::serializer::{Reader, Serializer, Writer};

    let event = StoredContractEvent::new(
        Hash::new([10u8; 32]),
        Hash::new([20u8; 32]),
        Hash::new([30u8; 32]),
        12345,
        42,
        vec![[1u8; 32], [2u8; 32], [3u8; 32]],
        vec![100, 101, 102, 103, 104],
    );

    // Serialize
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    event.write(&mut writer);
    let serialized_bytes = writer.as_bytes().to_vec();

    // Deserialize
    let mut reader = Reader::new(&serialized_bytes);
    let decoded = StoredContractEvent::read(&mut reader).unwrap();

    // Verify equality
    assert_eq!(event, decoded);
    assert_eq!(decoded.contract, Hash::new([10u8; 32]));
    assert_eq!(decoded.tx_hash, Hash::new([20u8; 32]));
    assert_eq!(decoded.block_hash, Hash::new([30u8; 32]));
    assert_eq!(decoded.topoheight, 12345);
    assert_eq!(decoded.log_index, 42);
    assert_eq!(decoded.topics.len(), 3);
    assert_eq!(decoded.data, vec![100, 101, 102, 103, 104]);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Serialization test passed");
        log::info!("  Serialized size: {} bytes", serialized_bytes.len());
    }
}

/// Test serialization size calculation
#[tokio::test]
async fn test_stored_contract_event_size() {
    use tos_common::serializer::{Serializer, Writer};

    let event = StoredContractEvent::new(
        Hash::zero(),
        Hash::zero(),
        Hash::zero(),
        100,
        0,
        vec![[1u8; 32], [2u8; 32]],
        vec![1, 2, 3, 4, 5],
    );

    let calculated_size = event.size();

    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    event.write(&mut writer);
    let actual_size = writer.as_bytes().len();

    assert_eq!(calculated_size, actual_size);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Size calculation test passed");
        log::info!("  Calculated: {} bytes", calculated_size);
        log::info!("  Actual: {} bytes", actual_size);
    }
}

/// Test EventFilter builder pattern
#[tokio::test]
async fn test_event_filter_builder() {
    let contract = Hash::new([1u8; 32]);
    let topic0 = [2u8; 32];

    let filter = EventFilter::for_contract(contract.clone())
        .with_topic0(topic0)
        .with_range(Some(10), Some(100))
        .with_limit(50);

    assert_eq!(filter.contract, Some(contract));
    assert_eq!(filter.topic0, Some(topic0));
    assert_eq!(filter.from_topoheight, Some(10));
    assert_eq!(filter.to_topoheight, Some(100));
    assert_eq!(filter.limit, Some(50));

    if log::log_enabled!(log::Level::Info) {
        log::info!("EventFilter builder test passed");
    }
}

/// Test EventFilter effective_limit with various scenarios
#[tokio::test]
async fn test_event_filter_effective_limit() {
    // No explicit limit -> use MAX_EVENTS_PER_QUERY
    let filter_default = EventFilter::default();
    assert_eq!(filter_default.effective_limit(), MAX_EVENTS_PER_QUERY);

    // Explicit limit under max
    let filter_under = EventFilter::default().with_limit(50);
    assert_eq!(filter_under.effective_limit(), 50);

    // Explicit limit over max -> capped at max
    let filter_over = EventFilter::default().with_limit(2000);
    assert_eq!(filter_over.effective_limit(), MAX_EVENTS_PER_QUERY);

    // Limit at exactly max
    let filter_exact = EventFilter::default().with_limit(MAX_EVENTS_PER_QUERY);
    assert_eq!(filter_exact.effective_limit(), MAX_EVENTS_PER_QUERY);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Effective limit test passed");
        log::info!("  MAX_EVENTS_PER_QUERY: {}", MAX_EVENTS_PER_QUERY);
    }
}

/// Test storing and retrieving events by contract
#[tokio::test]
async fn test_store_and_get_events_by_contract() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);

    // Create and store events
    let event1 = StoredContractEvent::new(
        contract.clone(),
        tx_hash.clone(),
        block_hash.clone(),
        100,
        0,
        vec![[10u8; 32]],
        vec![1, 2, 3],
    );

    let event2 = StoredContractEvent::new(
        contract.clone(),
        tx_hash.clone(),
        block_hash.clone(),
        101,
        0,
        vec![[20u8; 32]],
        vec![4, 5, 6],
    );

    {
        let mut storage_write = storage.write().await;
        storage_write
            .store_contract_event(event1.clone())
            .await
            .unwrap();
        storage_write
            .store_contract_event(event2.clone())
            .await
            .unwrap();
    }

    // Retrieve events
    let storage_read = storage.read().await;
    let events = storage_read
        .get_events_by_contract(&contract, None, None, None)
        .await
        .unwrap();

    assert_eq!(events.len(), 2);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Store and retrieve by contract test passed");
        log::info!("  Stored: 2 events");
        log::info!("  Retrieved: {} events", events.len());
    }
}

/// Test storing and retrieving events by transaction hash
#[tokio::test]
async fn test_get_events_by_tx() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash1 = Hash::new([2u8; 32]);
    let tx_hash2 = Hash::new([3u8; 32]);
    let block_hash = Hash::new([4u8; 32]);

    // Store events from different transactions
    let event1 = StoredContractEvent::new(
        contract.clone(),
        tx_hash1.clone(),
        block_hash.clone(),
        100,
        0,
        vec![[10u8; 32]],
        vec![1, 2, 3],
    );

    let event2 = StoredContractEvent::new(
        contract.clone(),
        tx_hash1.clone(),
        block_hash.clone(),
        100,
        1,
        vec![[20u8; 32]],
        vec![4, 5, 6],
    );

    let event3 = StoredContractEvent::new(
        contract.clone(),
        tx_hash2.clone(),
        block_hash.clone(),
        101,
        0,
        vec![[30u8; 32]],
        vec![7, 8, 9],
    );

    {
        let mut storage_write = storage.write().await;
        storage_write.store_contract_event(event1).await.unwrap();
        storage_write.store_contract_event(event2).await.unwrap();
        storage_write.store_contract_event(event3).await.unwrap();
    }

    // Query by tx_hash1 - should get 2 events
    let storage_read = storage.read().await;
    let events_tx1 = storage_read.get_events_by_tx(&tx_hash1).await.unwrap();
    assert_eq!(events_tx1.len(), 2);

    // Query by tx_hash2 - should get 1 event
    let events_tx2 = storage_read.get_events_by_tx(&tx_hash2).await.unwrap();
    assert_eq!(events_tx2.len(), 1);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Get events by tx test passed");
        log::info!("  TX1 events: {}", events_tx1.len());
        log::info!("  TX2 events: {}", events_tx2.len());
    }
}

/// Test topoheight range filtering
#[tokio::test]
async fn test_topoheight_range_filter() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);

    // Store events at different topoheights
    {
        let mut storage_write = storage.write().await;
        for i in 0..10u64 {
            let event = StoredContractEvent::new(
                contract.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                100 + i, // topoheights: 100, 101, ..., 109
                0,
                vec![[i as u8; 32]],
                vec![i as u8],
            );
            storage_write.store_contract_event(event).await.unwrap();
        }
    }

    let storage_read = storage.read().await;

    // Test: Get all events (no range)
    let all_events = storage_read
        .get_events_by_contract(&contract, None, None, None)
        .await
        .unwrap();
    assert_eq!(all_events.len(), 10);

    // Test: Get events from topoheight 105 onwards
    let from_105 = storage_read
        .get_events_by_contract(&contract, Some(105), None, None)
        .await
        .unwrap();
    assert_eq!(from_105.len(), 5); // 105, 106, 107, 108, 109

    // Test: Get events up to topoheight 103
    let to_103 = storage_read
        .get_events_by_contract(&contract, None, Some(103), None)
        .await
        .unwrap();
    assert_eq!(to_103.len(), 4); // 100, 101, 102, 103

    // Test: Get events in range 102-106
    let range_102_106 = storage_read
        .get_events_by_contract(&contract, Some(102), Some(106), None)
        .await
        .unwrap();
    assert_eq!(range_102_106.len(), 5); // 102, 103, 104, 105, 106

    if log::log_enabled!(log::Level::Info) {
        log::info!("Topoheight range filter test passed");
        log::info!("  All events: {}", all_events.len());
        log::info!("  From 105: {}", from_105.len());
        log::info!("  To 103: {}", to_103.len());
        log::info!("  Range 102-106: {}", range_102_106.len());
    }
}

/// Test event limit functionality
#[tokio::test]
async fn test_event_limit() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);

    // Store 20 events
    {
        let mut storage_write = storage.write().await;
        for i in 0..20u64 {
            let event = StoredContractEvent::new(
                contract.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                100 + i,
                0,
                vec![[i as u8; 32]],
                vec![i as u8],
            );
            storage_write.store_contract_event(event).await.unwrap();
        }
    }

    let storage_read = storage.read().await;

    // Query with limit 5
    let limited = storage_read
        .get_events_by_contract(&contract, None, None, Some(5))
        .await
        .unwrap();
    assert_eq!(limited.len(), 5);

    // Query with limit larger than actual count
    let all = storage_read
        .get_events_by_contract(&contract, None, None, Some(100))
        .await
        .unwrap();
    assert_eq!(all.len(), 20);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Event limit test passed");
        log::info!("  Limit 5: {} events", limited.len());
        log::info!("  Limit 100: {} events", all.len());
    }
}

/// Test storing multiple events in batch
#[tokio::test]
async fn test_store_contract_events_batch() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);

    let events: Vec<StoredContractEvent> = (0..5u32)
        .map(|i| {
            StoredContractEvent::new(
                contract.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                100,
                i,
                vec![[i as u8; 32]],
                vec![i as u8],
            )
        })
        .collect();

    {
        let mut storage_write = storage.write().await;
        storage_write.store_contract_events(events).await.unwrap();
    }

    let storage_read = storage.read().await;
    let retrieved = storage_read
        .get_events_by_contract(&contract, None, None, None)
        .await
        .unwrap();

    assert_eq!(retrieved.len(), 5);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Batch store test passed");
        log::info!("  Stored 5 events in batch");
        log::info!("  Retrieved: {} events", retrieved.len());
    }
}

/// Test event count functionality
#[tokio::test]
async fn test_count_events() {
    let storage = create_test_rocksdb_storage().await;

    // Initially should have 0 events
    {
        let storage_read = storage.read().await;
        let count = storage_read.count_events().await.unwrap();
        assert_eq!(count, 0);
    }

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);

    // Store some events
    {
        let mut storage_write = storage.write().await;
        for i in 0..5u64 {
            let event = StoredContractEvent::new(
                contract.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                100 + i,
                0,
                vec![[i as u8; 32]],
                vec![i as u8],
            );
            storage_write.store_contract_event(event).await.unwrap();
        }
    }

    // Should have 5 events now
    {
        let storage_read = storage.read().await;
        let count = storage_read.count_events().await.unwrap();
        assert_eq!(count, 5);
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!("Count events test passed");
    }
}

/// Test delete events at topoheight (for reorg handling)
#[tokio::test]
async fn test_delete_events_at_topoheight() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);

    // Store events at different topoheights
    {
        let mut storage_write = storage.write().await;
        for i in 0..5u64 {
            let event = StoredContractEvent::new(
                contract.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                100 + i, // topoheights: 100, 101, 102, 103, 104
                0,
                vec![[i as u8; 32]],
                vec![i as u8],
            );
            storage_write.store_contract_event(event).await.unwrap();
        }
    }

    // Delete events at topoheight 102
    {
        let mut storage_write = storage.write().await;
        storage_write
            .delete_events_at_topoheight(102)
            .await
            .unwrap();
    }

    // Should have 4 events now (100, 101, 103, 104)
    let storage_read = storage.read().await;
    let events = storage_read
        .get_events_by_contract(&contract, None, None, None)
        .await
        .unwrap();

    // Verify topoheight 102 is gone
    for event in &events {
        assert_ne!(event.topoheight, 102);
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!("Delete events at topoheight test passed");
        log::info!("  Remaining events: {}", events.len());
    }
}

/// Test getting events with EventFilter convenience method
#[tokio::test]
async fn test_get_events_with_filter() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);
    let topic0 = [10u8; 32];

    // Store events with specific topic0
    {
        let mut storage_write = storage.write().await;

        // Events with matching topic0
        for i in 0..3u64 {
            let event = StoredContractEvent::new(
                contract.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                100 + i,
                0,
                vec![topic0, [i as u8; 32]],
                vec![i as u8],
            );
            storage_write.store_contract_event(event).await.unwrap();
        }

        // Events with different topic0
        for i in 3..5u64 {
            let event = StoredContractEvent::new(
                contract.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                100 + i,
                0,
                vec![[99u8; 32]], // different topic0
                vec![i as u8],
            );
            storage_write.store_contract_event(event).await.unwrap();
        }
    }

    let storage_read = storage.read().await;

    // Test filter by contract only
    let filter_contract = EventFilter::for_contract(contract.clone());
    let by_contract = storage_read
        .get_events_with_filter(&filter_contract)
        .await
        .unwrap();
    assert_eq!(by_contract.len(), 5);

    // Test filter by contract + topic0
    let filter_topic = EventFilter::for_contract(contract.clone()).with_topic0(topic0);
    let by_topic = storage_read
        .get_events_with_filter(&filter_topic)
        .await
        .unwrap();
    assert_eq!(by_topic.len(), 3);

    // Test filter with limit
    let filter_limited = EventFilter::for_contract(contract.clone()).with_limit(2);
    let limited = storage_read
        .get_events_with_filter(&filter_limited)
        .await
        .unwrap();
    assert_eq!(limited.len(), 2);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Get events with filter test passed");
        log::info!("  By contract: {} events", by_contract.len());
        log::info!("  By topic0: {} events", by_topic.len());
        log::info!("  With limit 2: {} events", limited.len());
    }
}

/// Test filter without contract returns empty (contract is required)
#[tokio::test]
async fn test_filter_requires_contract() {
    let storage = create_test_rocksdb_storage().await;

    // Empty filter (no contract specified)
    let filter = EventFilter::default();

    let storage_read = storage.read().await;
    let result = storage_read.get_events_with_filter(&filter).await.unwrap();

    // Should return empty since contract filter is required
    assert!(result.is_empty());

    if log::log_enabled!(log::Level::Info) {
        log::info!("Filter requires contract test passed");
    }
}

/// Test multiple contracts isolation
#[tokio::test]
async fn test_multiple_contracts_isolation() {
    let storage = create_test_rocksdb_storage().await;

    let contract1 = Hash::new([1u8; 32]);
    let contract2 = Hash::new([2u8; 32]);
    let tx_hash = Hash::new([3u8; 32]);
    let block_hash = Hash::new([4u8; 32]);

    // Store events for contract1
    {
        let mut storage_write = storage.write().await;
        for i in 0..3u64 {
            let event = StoredContractEvent::new(
                contract1.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                100 + i,
                0,
                vec![[i as u8; 32]],
                vec![i as u8],
            );
            storage_write.store_contract_event(event).await.unwrap();
        }

        // Store events for contract2
        for i in 0..5u64 {
            let event = StoredContractEvent::new(
                contract2.clone(),
                tx_hash.clone(),
                block_hash.clone(),
                200 + i,
                0,
                vec![[i as u8; 32]],
                vec![i as u8],
            );
            storage_write.store_contract_event(event).await.unwrap();
        }
    }

    let storage_read = storage.read().await;

    // Query contract1 - should get 3 events
    let events1 = storage_read
        .get_events_by_contract(&contract1, None, None, None)
        .await
        .unwrap();
    assert_eq!(events1.len(), 3);

    // Query contract2 - should get 5 events
    let events2 = storage_read
        .get_events_by_contract(&contract2, None, None, None)
        .await
        .unwrap();
    assert_eq!(events2.len(), 5);

    // Total count
    let total = storage_read.count_events().await.unwrap();
    assert_eq!(total, 8);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Multiple contracts isolation test passed");
        log::info!("  Contract1 events: {}", events1.len());
        log::info!("  Contract2 events: {}", events2.len());
        log::info!("  Total events: {}", total);
    }
}

/// Test empty data and topics handling
#[tokio::test]
async fn test_empty_data_and_topics() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);

    // Event with empty topics (LOG0)
    let event_log0 = StoredContractEvent::new(
        contract.clone(),
        tx_hash.clone(),
        block_hash.clone(),
        100,
        0,
        vec![], // No topics
        vec![1, 2, 3],
    );

    // Event with empty data
    let event_empty_data = StoredContractEvent::new(
        contract.clone(),
        tx_hash.clone(),
        block_hash.clone(),
        101,
        0,
        vec![[1u8; 32]],
        vec![], // No data
    );

    // Event with both empty
    let event_both_empty = StoredContractEvent::new(
        contract.clone(),
        tx_hash.clone(),
        block_hash.clone(),
        102,
        0,
        vec![],
        vec![],
    );

    {
        let mut storage_write = storage.write().await;
        storage_write
            .store_contract_event(event_log0.clone())
            .await
            .unwrap();
        storage_write
            .store_contract_event(event_empty_data.clone())
            .await
            .unwrap();
        storage_write
            .store_contract_event(event_both_empty.clone())
            .await
            .unwrap();
    }

    let storage_read = storage.read().await;
    let events = storage_read
        .get_events_by_contract(&contract, None, None, None)
        .await
        .unwrap();

    assert_eq!(events.len(), 3);

    // Verify data integrity
    let e0 = events.iter().find(|e| e.topoheight == 100).unwrap();
    assert!(e0.topics.is_empty());
    assert_eq!(e0.data, vec![1, 2, 3]);

    let e1 = events.iter().find(|e| e.topoheight == 101).unwrap();
    assert_eq!(e1.topics.len(), 1);
    assert!(e1.data.is_empty());

    let e2 = events.iter().find(|e| e.topoheight == 102).unwrap();
    assert!(e2.topics.is_empty());
    assert!(e2.data.is_empty());

    if log::log_enabled!(log::Level::Info) {
        log::info!("Empty data and topics test passed");
    }
}

/// Test maximum topics (4 topics for LOG4)
#[tokio::test]
async fn test_max_topics() {
    let storage = create_test_rocksdb_storage().await;

    let contract = Hash::new([1u8; 32]);
    let tx_hash = Hash::new([2u8; 32]);
    let block_hash = Hash::new([3u8; 32]);

    // Event with 4 topics (maximum)
    let event_max = StoredContractEvent::new(
        contract.clone(),
        tx_hash.clone(),
        block_hash.clone(),
        100,
        0,
        vec![[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]],
        vec![1, 2, 3],
    );

    {
        let mut storage_write = storage.write().await;
        storage_write
            .store_contract_event(event_max.clone())
            .await
            .unwrap();
    }

    let storage_read = storage.read().await;
    let events = storage_read
        .get_events_by_contract(&contract, None, None, None)
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topics.len(), 4);
    assert_eq!(events[0].topics[0], [1u8; 32]);
    assert_eq!(events[0].topics[1], [2u8; 32]);
    assert_eq!(events[0].topics[2], [3u8; 32]);
    assert_eq!(events[0].topics[3], [4u8; 32]);

    if log::log_enabled!(log::Level::Info) {
        log::info!("Max topics test passed");
        log::info!("  Topics count: {}", events[0].topics.len());
    }
}
