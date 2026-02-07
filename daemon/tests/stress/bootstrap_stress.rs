// Bootstrap Stress Tests
// Tests bootstrap protocol serialization throughput, apply simulation,
// and pagination stress under high-volume data sync conditions.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use indexmap::{IndexMap, IndexSet};
use tokio::sync::RwLock;
use tos_common::{
    crypto::{Hash, PublicKey},
    serializer::Serializer,
};
use tos_daemon::p2p::packet::{
    BlockId, BootstrapChainRequest, BootstrapChainResponse, CommonPoint, StepKind, StepRequest,
    StepResponse, MAX_ITEMS_PER_PAGE,
};

// =============================================================================
// Helper Functions
// =============================================================================

/// Generate a deterministic Hash from a u64 seed
fn make_hash(seed: u64) -> Hash {
    let mut bytes = [0u8; 32];
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let offset_seed = seed.wrapping_add(i as u64);
        chunk.copy_from_slice(&offset_seed.to_le_bytes());
    }
    Hash::new(bytes)
}

/// Generate a deterministic PublicKey from a u64 seed
/// Uses the Serializer trait to deserialize from raw bytes
fn make_pubkey(seed: u64) -> PublicKey {
    let mut bytes = [0u8; 32];
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let offset_seed = seed.wrapping_add(i as u64).wrapping_mul(0x9E3779B97F4A7C15);
        chunk.copy_from_slice(&offset_seed.to_le_bytes());
    }
    // PublicKey (CompressedPublicKey) wraps CompressedRistretto which is just 32 bytes
    // Use the Serializer trait to construct from raw bytes
    PublicKey::from_bytes(&bytes).unwrap()
}

/// Generate a BlockId from a seed
fn make_block_id(seed: u64) -> BlockId {
    BlockId::new(make_hash(seed), seed)
}

/// Build a ChainInfo StepRequest with `count` block IDs
fn build_chain_info_request(count: usize) -> StepRequest<'static> {
    let mut blocks = IndexSet::with_capacity(count);
    for i in 0..count {
        blocks.insert(make_block_id(i as u64));
    }
    StepRequest::ChainInfo(blocks)
}

/// Build a Keys StepResponse with `count` public keys and optional next page
fn build_keys_response(count: usize, next_page: Option<u64>) -> StepResponse {
    let mut keys = IndexSet::with_capacity(count);
    for i in 0..count {
        keys.insert(make_pubkey(i as u64));
    }
    StepResponse::Keys(keys, next_page)
}

/// Build an Assets StepRequest
fn build_assets_request(min_topo: u64, max_topo: u64, page: Option<u64>) -> StepRequest<'static> {
    StepRequest::Assets(min_topo, max_topo, page)
}

/// Build a Keys StepRequest
fn build_keys_request(min_topo: u64, max_topo: u64, page: Option<u64>) -> StepRequest<'static> {
    StepRequest::Keys(min_topo, max_topo, page)
}

/// Build an Accounts StepRequest with `count` public keys
fn build_accounts_request(min_topo: u64, max_topo: u64, count: usize) -> StepRequest<'static> {
    let mut keys = IndexSet::with_capacity(count);
    for i in 0..count {
        keys.insert(make_pubkey(i as u64));
    }
    StepRequest::Accounts(min_topo, max_topo, Cow::Owned(keys))
}

/// Build a TNS Names response with `count` entries
fn build_tns_names_response(count: usize, next_page: Option<u64>) -> StepResponse {
    let mut entries = IndexMap::with_capacity(count);
    for i in 0..count {
        entries.insert(make_hash(i as u64), make_pubkey(i as u64));
    }
    StepResponse::TnsNames(entries, next_page)
}

/// Build a ChainInfo StepResponse
fn build_chain_info_response() -> StepResponse {
    let common_point = Some(CommonPoint::new(make_hash(0), 100));
    StepResponse::ChainInfo(common_point, 1000, 500, make_hash(999))
}

/// Build a KeyBalances response with `count` entries
fn build_key_balances_response(count: usize, next_page: Option<u64>) -> StepResponse {
    let mut entries = IndexMap::with_capacity(count);
    for i in 0..count {
        entries.insert(make_hash(i as u64), None);
    }
    StepResponse::KeyBalances(entries, next_page)
}

/// Serialize and deserialize a StepRequest, verifying roundtrip
fn roundtrip_request(req: &StepRequest<'_>) -> bool {
    let bytes = req.to_bytes();
    StepRequest::from_bytes(&bytes).is_ok()
}

/// Serialize and deserialize a StepResponse, verifying roundtrip
fn roundtrip_response(resp: &StepResponse) -> bool {
    let bytes = resp.to_bytes();
    StepResponse::from_bytes(&bytes).is_ok()
}

/// Serialize and deserialize a BootstrapChainRequest, verifying roundtrip
fn roundtrip_bootstrap_request(req: &BootstrapChainRequest<'_>) -> bool {
    let bytes = req.to_bytes();
    BootstrapChainRequest::from_bytes(&bytes).is_ok()
}

/// Serialize and deserialize a BootstrapChainResponse, verifying roundtrip
fn roundtrip_bootstrap_response(resp: &BootstrapChainResponse) -> bool {
    let bytes = resp.to_bytes();
    BootstrapChainResponse::from_bytes(&bytes).is_ok()
}

/// Format throughput stats for printing
fn format_throughput(label: &str, ops: usize, elapsed: std::time::Duration) {
    let ops_per_sec = ops as f64 / elapsed.as_secs_f64();
    let ns_per_op = elapsed.as_nanos() as f64 / ops as f64;
    println!(
        "  {:<44} {:>12.0} ops/sec  ({:>8.1} ns/op)",
        label, ops_per_sec, ns_per_op
    );
}

// =============================================================================
// Section A: Wire Format Serialization Throughput (no RocksDB)
// =============================================================================

/// Test 1: ChainInfo request roundtrip throughput
/// 10,000 serialize/deserialize cycles of chain_info requests (64 block IDs each)
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_chain_info_request_throughput() {
    const ITERATIONS: usize = 10_000;
    const BLOCK_COUNT: usize = 64;

    let req = build_chain_info_request(BLOCK_COUNT);

    // Warmup
    for _ in 0..100 {
        let _ = roundtrip_request(&req);
    }

    let start = Instant::now();
    let mut success_count = 0usize;

    for _ in 0..ITERATIONS {
        if roundtrip_request(&req) {
            success_count += 1;
        }
    }

    let elapsed = start.elapsed();

    println!("Bootstrap ChainInfo request throughput:");
    println!("  Block IDs per request: {}", BLOCK_COUNT);
    println!("  Iterations: {}", ITERATIONS);
    println!("  Successful: {}", success_count);
    println!("  Duration: {:?}", elapsed);
    format_throughput("ChainInfo request roundtrip", success_count, elapsed);

    // Also measure the wrapped BootstrapChainRequest
    let wrapped = BootstrapChainRequest::new(42, build_chain_info_request(BLOCK_COUNT));
    let start2 = Instant::now();
    let mut wrapped_success = 0usize;
    for _ in 0..ITERATIONS {
        if roundtrip_bootstrap_request(&wrapped) {
            wrapped_success += 1;
        }
    }
    let elapsed2 = start2.elapsed();
    format_throughput("BootstrapChainRequest roundtrip", wrapped_success, elapsed2);

    assert_eq!(success_count, ITERATIONS);
    assert_eq!(wrapped_success, ITERATIONS);
}

/// Test 2: Keys response roundtrip throughput
/// 10,000 serialize/deserialize cycles of keys responses (1024 keys = full page)
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_keys_response_throughput() {
    const ITERATIONS: usize = 10_000;
    const KEYS_PER_PAGE: usize = MAX_ITEMS_PER_PAGE;

    let resp = build_keys_response(KEYS_PER_PAGE, Some(2));

    // Warmup
    for _ in 0..10 {
        let _ = roundtrip_response(&resp);
    }

    let start = Instant::now();
    let mut success_count = 0usize;

    for _ in 0..ITERATIONS {
        if roundtrip_response(&resp) {
            success_count += 1;
        }
    }

    let elapsed = start.elapsed();

    println!("Bootstrap Keys response throughput:");
    println!("  Keys per page: {}", KEYS_PER_PAGE);
    println!("  Iterations: {}", ITERATIONS);
    println!("  Successful: {}", success_count);
    println!("  Duration: {:?}", elapsed);
    format_throughput("Keys response roundtrip", success_count, elapsed);

    // Also test wrapped response
    let wrapped = BootstrapChainResponse::new(1, build_keys_response(KEYS_PER_PAGE, Some(2)));
    let start2 = Instant::now();
    let mut wrapped_success = 0usize;
    for _ in 0..ITERATIONS {
        if roundtrip_bootstrap_response(&wrapped) {
            wrapped_success += 1;
        }
    }
    let elapsed2 = start2.elapsed();
    format_throughput(
        "BootstrapChainResponse roundtrip",
        wrapped_success,
        elapsed2,
    );

    assert_eq!(success_count, ITERATIONS);
    assert_eq!(wrapped_success, ITERATIONS);
}

/// Test 3: TNS Names response roundtrip throughput
/// 5,000 serialize/deserialize cycles of tns names responses (100 entries with hash+pubkey)
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_tns_names_response_throughput() {
    const ITERATIONS: usize = 5_000;
    const ENTRIES: usize = 100;

    let resp = build_tns_names_response(ENTRIES, Some(5));

    // Warmup
    for _ in 0..10 {
        let _ = roundtrip_response(&resp);
    }

    let start = Instant::now();
    let mut success_count = 0usize;

    for _ in 0..ITERATIONS {
        if roundtrip_response(&resp) {
            success_count += 1;
        }
    }

    let elapsed = start.elapsed();

    println!("Bootstrap TNS Names response throughput:");
    println!("  Entries per response: {}", ENTRIES);
    println!("  Iterations: {}", ITERATIONS);
    println!("  Successful: {}", success_count);
    println!("  Duration: {:?}", elapsed);
    format_throughput("TNS Names response roundtrip", success_count, elapsed);

    assert_eq!(success_count, ITERATIONS);
}

/// Test 4: KeyBalances response roundtrip throughput
/// 5,000 serialize/deserialize cycles of key_balances responses (100 entries)
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_key_balances_response_throughput() {
    const ITERATIONS: usize = 5_000;
    const ENTRIES: usize = 100;

    let resp = build_key_balances_response(ENTRIES, Some(3));

    // Warmup
    for _ in 0..10 {
        let _ = roundtrip_response(&resp);
    }

    let start = Instant::now();
    let mut success_count = 0usize;

    for _ in 0..ITERATIONS {
        if roundtrip_response(&resp) {
            success_count += 1;
        }
    }

    let elapsed = start.elapsed();

    println!("Bootstrap KeyBalances response throughput:");
    println!("  Entries per response: {}", ENTRIES);
    println!("  Iterations: {}", ITERATIONS);
    println!("  Successful: {}", success_count);
    println!("  Duration: {:?}", elapsed);
    format_throughput("KeyBalances response roundtrip", success_count, elapsed);

    assert_eq!(success_count, ITERATIONS);
}

/// Test 5: All step request types roundtrip
/// Serialize/deserialize each step request type 1,000 times, verify all succeed
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_all_step_request_types() {
    const ITERATIONS_PER_TYPE: usize = 1_000;

    // Build one request per type
    let requests: Vec<(&str, StepRequest<'static>)> = vec![
        ("ChainInfo", build_chain_info_request(8)),
        ("Assets", build_assets_request(0, 100, None)),
        ("Assets(page)", build_assets_request(0, 100, Some(2))),
        ("Keys", build_keys_request(0, 100, None)),
        ("Keys(page)", build_keys_request(0, 100, Some(5))),
        (
            "KeyBalances",
            StepRequest::KeyBalances(Cow::Owned(make_pubkey(0)), 0, 100, None),
        ),
        ("Accounts", build_accounts_request(0, 100, 10)),
        ("Contracts", StepRequest::Contracts(0, 100, None)),
        (
            "ContractModule",
            StepRequest::ContractModule(0, 100, Cow::Owned(make_hash(0))),
        ),
        ("KycData", StepRequest::KycData(Some(1))),
        ("Committees", StepRequest::Committees(None)),
        ("GlobalCommittee", StepRequest::GlobalCommittee),
        ("EscrowAccounts", StepRequest::EscrowAccounts(Some(1))),
        ("ArbitrationData", StepRequest::ArbitrationData(None)),
        ("ArbiterAccounts", StepRequest::ArbiterAccounts(Some(3))),
        ("TnsNames", StepRequest::TnsNames(Some(1))),
        (
            "EnergyData",
            StepRequest::EnergyData(Cow::Owned(vec![make_pubkey(0), make_pubkey(1)]), 100),
        ),
        ("ReferralRecords", StepRequest::ReferralRecords(None)),
        ("AgentData", StepRequest::AgentData(Some(2))),
        ("A2aNonces", StepRequest::A2aNonces(None)),
        ("ContractAssets", StepRequest::ContractAssets(Some(1))),
        ("UnoBalanceKeys", StepRequest::UnoBalanceKeys(None)),
        ("BlocksMetadata", StepRequest::BlocksMetadata(50)),
    ];

    let start = Instant::now();
    let mut total_success = 0usize;
    let mut total_attempts = 0usize;

    println!(
        "Bootstrap step request types roundtrip ({} iterations each):",
        ITERATIONS_PER_TYPE
    );

    for (name, req) in &requests {
        let type_start = Instant::now();
        let mut success = 0usize;

        for _ in 0..ITERATIONS_PER_TYPE {
            if roundtrip_request(req) {
                success += 1;
            }
        }

        let type_elapsed = type_start.elapsed();
        format_throughput(name, success, type_elapsed);

        total_success += success;
        total_attempts += ITERATIONS_PER_TYPE;
    }

    let elapsed = start.elapsed();
    println!("  ---");
    println!(
        "  Total: {}/{} successful in {:?}",
        total_success, total_attempts, elapsed
    );
    format_throughput("All types combined", total_success, elapsed);

    assert_eq!(total_success, total_attempts);
}

/// Test 6: Max-size response pages
/// Serialize/deserialize keys response with MAX_ITEMS_PER_PAGE (1024) entries, 100 times
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_max_size_response_pages() {
    const ITERATIONS: usize = 100;
    const MAX_KEYS: usize = MAX_ITEMS_PER_PAGE;

    let resp = build_keys_response(MAX_KEYS, Some(99));

    // Measure serialized size
    let bytes = resp.to_bytes();
    let serialized_size = bytes.len();

    // Verify we can deserialize the max-size response
    let decoded = StepResponse::from_bytes(&bytes);
    assert!(
        decoded.is_ok(),
        "Failed to deserialize max-size keys response"
    );

    let start = Instant::now();
    let mut success_count = 0usize;

    for _ in 0..ITERATIONS {
        let encoded = resp.to_bytes();
        if let Ok(decoded) = StepResponse::from_bytes(&encoded) {
            // Verify the decoded response has the right kind
            assert_eq!(decoded.kind(), StepKind::Keys);
            success_count += 1;
        }
    }

    let elapsed = start.elapsed();

    println!("Bootstrap max-size response pages:");
    println!("  Keys per page: {} (MAX_ITEMS_PER_PAGE)", MAX_KEYS);
    println!(
        "  Serialized size: {} bytes ({:.1} KB)",
        serialized_size,
        serialized_size as f64 / 1024.0
    );
    println!("  Iterations: {}", ITERATIONS);
    println!("  Successful: {}", success_count);
    println!("  Duration: {:?}", elapsed);
    format_throughput("Max-size page roundtrip", success_count, elapsed);

    // Also test with ChainInfo response (different shape)
    let chain_info_resp = build_chain_info_response();
    let chain_bytes = chain_info_resp.to_bytes();
    println!("  ChainInfo response size: {} bytes", chain_bytes.len());
    assert!(StepResponse::from_bytes(&chain_bytes).is_ok());

    assert_eq!(success_count, ITERATIONS);
}

// =============================================================================
// Section B: Apply Throughput Simulation (with mock state)
// =============================================================================

/// Mock bootstrap state tracking applied items
struct MockBootstrapState {
    keys: RwLock<IndexSet<PublicKey>>,
    balances: RwLock<HashMap<PublicKey, Vec<Hash>>>,
    accounts: RwLock<HashMap<PublicKey, u64>>, // key -> nonce
    tns_names: RwLock<IndexMap<Hash, PublicKey>>,
    blocks_metadata: RwLock<Vec<Hash>>,
    progress: AtomicU64,
    batch_count: AtomicU64,
}

impl MockBootstrapState {
    fn new() -> Self {
        Self {
            keys: RwLock::new(IndexSet::new()),
            balances: RwLock::new(HashMap::new()),
            accounts: RwLock::new(HashMap::new()),
            tns_names: RwLock::new(IndexMap::new()),
            blocks_metadata: RwLock::new(Vec::new()),
            progress: AtomicU64::new(0),
            batch_count: AtomicU64::new(0),
        }
    }

    async fn apply_keys(&self, keys: &IndexSet<PublicKey>) -> usize {
        let mut state = self.keys.write().await;
        let count = keys.len();
        for key in keys {
            state.insert(key.clone());
        }
        self.progress.fetch_add(count as u64, Ordering::SeqCst);
        count
    }

    async fn apply_key_balances(&self, key: &PublicKey, assets: &[Hash]) -> usize {
        let mut state = self.balances.write().await;
        let count = assets.len();
        let entry = state.entry(key.clone()).or_default();
        for asset in assets {
            entry.push(asset.clone());
        }
        self.progress.fetch_add(count as u64, Ordering::SeqCst);
        count
    }

    async fn apply_accounts(&self, entries: &[(PublicKey, u64)]) -> usize {
        let mut state = self.accounts.write().await;
        let count = entries.len();
        for (key, nonce) in entries {
            state.insert(key.clone(), *nonce);
        }
        self.progress.fetch_add(count as u64, Ordering::SeqCst);
        count
    }

    async fn apply_tns_names(&self, entries: &IndexMap<Hash, PublicKey>) -> usize {
        let mut state = self.tns_names.write().await;
        let count = entries.len();
        for (hash, key) in entries {
            state.insert(hash.clone(), key.clone());
        }
        self.progress.fetch_add(count as u64, Ordering::SeqCst);
        count
    }

    async fn apply_blocks_metadata(&self, hashes: &[Hash]) -> usize {
        let mut state = self.blocks_metadata.write().await;
        let count = hashes.len();
        for hash in hashes {
            state.push(hash.clone());
        }
        self.progress.fetch_add(count as u64, Ordering::SeqCst);
        count
    }

    async fn commit_batch(&self) {
        self.batch_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn reset(&self) {
        self.keys.write().await.clear();
        self.balances.write().await.clear();
        self.accounts.write().await.clear();
        self.tns_names.write().await.clear();
        self.blocks_metadata.write().await.clear();
        self.progress.store(0, Ordering::SeqCst);
        self.batch_count.store(0, Ordering::SeqCst);
    }

    fn get_progress(&self) -> u64 {
        self.progress.load(Ordering::SeqCst)
    }

    fn get_batch_count(&self) -> u64 {
        self.batch_count.load(Ordering::SeqCst)
    }
}

/// Test 7: High-volume keys apply
/// Apply 10,000 keys across 10 pages (1024 per page), verify progress counters
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_high_volume_keys_apply() {
    const TOTAL_KEYS: usize = 10_240;
    const KEYS_PER_PAGE: usize = MAX_ITEMS_PER_PAGE;
    const PAGES: usize = TOTAL_KEYS / KEYS_PER_PAGE;

    let state = Arc::new(MockBootstrapState::new());
    let start = Instant::now();

    for page in 0..PAGES {
        // Build a page of keys
        let mut keys = IndexSet::with_capacity(KEYS_PER_PAGE);
        for i in 0..KEYS_PER_PAGE {
            let seed = (page * KEYS_PER_PAGE + i) as u64;
            keys.insert(make_pubkey(seed));
        }

        // Serialize the response, then deserialize and apply
        let resp = StepResponse::Keys(
            keys.clone(),
            if page < PAGES - 1 {
                Some((page + 2) as u64)
            } else {
                None
            },
        );
        let bytes = resp.to_bytes();
        let decoded = StepResponse::from_bytes(&bytes).unwrap();

        if let StepResponse::Keys(decoded_keys, _) = decoded {
            state.apply_keys(&decoded_keys).await;
        }

        state.commit_batch().await;
    }

    let elapsed = start.elapsed();
    let progress = state.get_progress();

    println!("Bootstrap high-volume keys apply:");
    println!("  Total keys: {}", TOTAL_KEYS);
    println!("  Pages: {} ({} keys/page)", PAGES, KEYS_PER_PAGE);
    println!("  Progress counter: {}", progress);
    println!("  Batches committed: {}", state.get_batch_count());
    println!("  Duration: {:?}", elapsed);
    format_throughput("Keys apply (serialize+apply)", TOTAL_KEYS, elapsed);

    assert_eq!(progress, TOTAL_KEYS as u64);
    assert_eq!(state.get_batch_count(), PAGES as u64);

    // Verify all keys are stored
    let stored_keys = state.keys.read().await;
    assert_eq!(stored_keys.len(), TOTAL_KEYS);
}

/// Test 8: High-volume key balances apply
/// Apply 5,000 balance entries (100 keys x 50 assets each), verify counters
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_high_volume_key_balances_apply() {
    const NUM_KEYS: usize = 100;
    const ASSETS_PER_KEY: usize = 50;
    const TOTAL_ENTRIES: usize = NUM_KEYS * ASSETS_PER_KEY;

    let state = Arc::new(MockBootstrapState::new());
    let start = Instant::now();

    for key_idx in 0..NUM_KEYS {
        let key = make_pubkey(key_idx as u64);
        let mut assets = Vec::with_capacity(ASSETS_PER_KEY);
        for asset_idx in 0..ASSETS_PER_KEY {
            assets.push(make_hash((key_idx * ASSETS_PER_KEY + asset_idx) as u64));
        }

        state.apply_key_balances(&key, &assets).await;
        state.commit_batch().await;
    }

    let elapsed = start.elapsed();
    let progress = state.get_progress();

    println!("Bootstrap high-volume key balances apply:");
    println!("  Keys: {}", NUM_KEYS);
    println!("  Assets per key: {}", ASSETS_PER_KEY);
    println!("  Total entries: {}", TOTAL_ENTRIES);
    println!("  Progress counter: {}", progress);
    println!("  Batches committed: {}", state.get_batch_count());
    println!("  Duration: {:?}", elapsed);
    format_throughput("Key balances apply", TOTAL_ENTRIES, elapsed);

    assert_eq!(progress, TOTAL_ENTRIES as u64);
    assert_eq!(state.get_batch_count(), NUM_KEYS as u64);

    // Verify balances stored
    let balances = state.balances.read().await;
    assert_eq!(balances.len(), NUM_KEYS);
    for (_, assets) in balances.iter() {
        assert_eq!(assets.len(), ASSETS_PER_KEY);
    }
}

/// Test 9: Batch size variation
/// Apply 1,000 items with batch_max = 10, 100, 1000, verify all commit correctly
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_batch_size_variation() {
    const TOTAL_ITEMS: usize = 1_000;
    let batch_sizes: &[usize] = &[10, 100, 1000];

    println!("Bootstrap batch size variation:");

    for &batch_max in batch_sizes {
        let state = Arc::new(MockBootstrapState::new());
        let start = Instant::now();

        let mut applied = 0usize;
        let mut batch_applied = 0usize;

        while applied < TOTAL_ITEMS {
            let remaining = TOTAL_ITEMS - applied;
            let this_batch = remaining.min(batch_max);

            // Build and apply a batch of TNS names
            let mut entries = IndexMap::with_capacity(this_batch);
            for i in 0..this_batch {
                let seed = (applied + i) as u64;
                entries.insert(make_hash(seed), make_pubkey(seed));
            }

            state.apply_tns_names(&entries).await;
            batch_applied += 1;

            if batch_applied >= batch_max / batch_max.max(1) || applied + this_batch >= TOTAL_ITEMS
            {
                state.commit_batch().await;
                batch_applied = 0;
            }

            applied += this_batch;
        }

        let elapsed = start.elapsed();
        let progress = state.get_progress();

        println!(
            "  batch_max={:>5}: progress={}, batches={}, duration={:?}",
            batch_max,
            progress,
            state.get_batch_count(),
            elapsed
        );
        format_throughput(
            &format!("  batch_max={} apply", batch_max),
            TOTAL_ITEMS,
            elapsed,
        );

        assert_eq!(progress, TOTAL_ITEMS as u64);

        // Verify data integrity
        let tns = state.tns_names.read().await;
        assert_eq!(tns.len(), TOTAL_ITEMS);
    }
}

/// Test 10: Apply + reset cycle
/// Init -> apply 1000 items -> reset -> apply 1000 more -> verify progress after each reset
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_apply_reset_cycle() {
    const ITEMS_PER_CYCLE: usize = 1_000;
    const RESET_CYCLES: usize = 5;

    let state = Arc::new(MockBootstrapState::new());
    let start = Instant::now();

    println!("Bootstrap apply + reset cycle:");

    for cycle in 0..RESET_CYCLES {
        // Apply items
        let mut keys = IndexSet::with_capacity(ITEMS_PER_CYCLE);
        for i in 0..ITEMS_PER_CYCLE {
            let seed = (cycle * ITEMS_PER_CYCLE + i) as u64;
            keys.insert(make_pubkey(seed));
        }

        state.apply_keys(&keys).await;
        state.commit_batch().await;

        let progress_before_reset = state.get_progress();
        let keys_count = state.keys.read().await.len();

        println!(
            "  Cycle {}: applied {} items, progress={}, keys_stored={}",
            cycle, ITEMS_PER_CYCLE, progress_before_reset, keys_count
        );

        assert_eq!(keys_count, ITEMS_PER_CYCLE);
        assert_eq!(progress_before_reset, ITEMS_PER_CYCLE as u64);

        // Reset
        state.reset().await;

        let progress_after_reset = state.get_progress();
        let keys_after_reset = state.keys.read().await.len();

        assert_eq!(progress_after_reset, 0);
        assert_eq!(keys_after_reset, 0);
    }

    let elapsed = start.elapsed();

    println!("  Total cycles: {}", RESET_CYCLES);
    println!("  Total duration: {:?}", elapsed);
    format_throughput(
        "Apply+reset cycles",
        RESET_CYCLES * ITEMS_PER_CYCLE,
        elapsed,
    );
}

/// Test 11: Full sync simulation
/// ChainInfo -> keys -> key_balances -> accounts -> tns_names -> blocks_metadata
/// in sequence, 100 items each, verify all counters correct
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_full_sync_simulation() {
    const ITEMS_PER_STEP: usize = 100;

    let state = Arc::new(MockBootstrapState::new());
    let start = Instant::now();
    let mut step_timings = Vec::new();

    println!(
        "Bootstrap full sync simulation ({} items per step):",
        ITEMS_PER_STEP
    );

    // Step 1: ChainInfo (just verify serialization)
    {
        let step_start = Instant::now();
        let req = build_chain_info_request(8);
        let req_bytes = req.to_bytes();
        let _ = StepRequest::from_bytes(&req_bytes).unwrap();

        let resp = build_chain_info_response();
        let resp_bytes = resp.to_bytes();
        let decoded_resp = StepResponse::from_bytes(&resp_bytes).unwrap();
        assert_eq!(decoded_resp.kind(), StepKind::ChainInfo);
        step_timings.push(("ChainInfo", step_start.elapsed()));
    }

    // Step 2: Keys
    {
        let step_start = Instant::now();
        let mut keys = IndexSet::with_capacity(ITEMS_PER_STEP);
        for i in 0..ITEMS_PER_STEP {
            keys.insert(make_pubkey(i as u64));
        }
        let resp = StepResponse::Keys(keys.clone(), None);
        let bytes = resp.to_bytes();
        let decoded = StepResponse::from_bytes(&bytes).unwrap();
        if let StepResponse::Keys(decoded_keys, _) = decoded {
            state.apply_keys(&decoded_keys).await;
        }
        state.commit_batch().await;
        step_timings.push(("Keys", step_start.elapsed()));
    }

    // Step 3: Key Balances
    {
        let step_start = Instant::now();
        for key_idx in 0..ITEMS_PER_STEP {
            let key = make_pubkey(key_idx as u64);
            let asset = make_hash(key_idx as u64);
            state.apply_key_balances(&key, &[asset]).await;
        }
        state.commit_batch().await;
        step_timings.push(("KeyBalances", step_start.elapsed()));
    }

    // Step 4: Accounts
    {
        let step_start = Instant::now();
        let mut entries = Vec::with_capacity(ITEMS_PER_STEP);
        for i in 0..ITEMS_PER_STEP {
            entries.push((make_pubkey(i as u64), i as u64));
        }
        state.apply_accounts(&entries).await;
        state.commit_batch().await;
        step_timings.push(("Accounts", step_start.elapsed()));
    }

    // Step 5: TNS Names
    {
        let step_start = Instant::now();
        let resp = build_tns_names_response(ITEMS_PER_STEP, None);
        let bytes = resp.to_bytes();
        let decoded = StepResponse::from_bytes(&bytes).unwrap();
        if let StepResponse::TnsNames(entries, _) = decoded {
            state.apply_tns_names(&entries).await;
        }
        state.commit_batch().await;
        step_timings.push(("TnsNames", step_start.elapsed()));
    }

    // Step 6: BlocksMetadata
    {
        let step_start = Instant::now();
        let mut hashes = Vec::with_capacity(ITEMS_PER_STEP);
        for i in 0..ITEMS_PER_STEP {
            hashes.push(make_hash(i as u64));
        }
        state.apply_blocks_metadata(&hashes).await;
        state.commit_batch().await;
        step_timings.push(("BlocksMetadata", step_start.elapsed()));
    }

    let elapsed = start.elapsed();
    let progress = state.get_progress();

    // Print step timings
    for (name, duration) in &step_timings {
        println!("  {:<20} {:?}", name, duration);
    }
    println!("  ---");
    println!("  Total progress: {} items applied", progress);
    println!("  Total batches: {}", state.get_batch_count());
    println!("  Total duration: {:?}", elapsed);

    // Verify counters: keys(100) + key_balances(100) + accounts(100) + tns(100) + blocks(100)
    let expected_progress = (ITEMS_PER_STEP * 5) as u64;
    assert_eq!(progress, expected_progress);
    assert_eq!(state.get_batch_count(), 5); // 5 commit_batch calls (keys, balances, accounts, tns, blocks)

    // Verify individual state
    assert_eq!(state.keys.read().await.len(), ITEMS_PER_STEP);
    assert_eq!(state.balances.read().await.len(), ITEMS_PER_STEP);
    assert_eq!(state.accounts.read().await.len(), ITEMS_PER_STEP);
    assert_eq!(state.tns_names.read().await.len(), ITEMS_PER_STEP);
    assert_eq!(state.blocks_metadata.read().await.len(), ITEMS_PER_STEP);
}

// =============================================================================
// Section C: Pagination Stress
// =============================================================================

/// Test 12: Multi-page serialization stress
/// Serialize 50 consecutive pages of 1024 keys each (51,200 total keys),
/// deserialize and verify page numbers are consecutive and all keys correct
#[tokio::test]
#[ignore = "Stress test - run with --ignored"]
async fn stress_bootstrap_multi_page_serialization() {
    const TOTAL_PAGES: usize = 50;
    const KEYS_PER_PAGE: usize = MAX_ITEMS_PER_PAGE;
    const TOTAL_KEYS: usize = TOTAL_PAGES * KEYS_PER_PAGE;

    let state = Arc::new(MockBootstrapState::new());
    let start = Instant::now();

    let mut total_serialized_bytes = 0usize;
    let mut all_decoded_keys = Vec::with_capacity(TOTAL_KEYS);

    for page in 0..TOTAL_PAGES {
        // Build page with unique keys
        let mut keys = IndexSet::with_capacity(KEYS_PER_PAGE);
        for i in 0..KEYS_PER_PAGE {
            let seed = (page * KEYS_PER_PAGE + i) as u64;
            keys.insert(make_pubkey(seed));
        }

        // Next page number (1-indexed, None for last page)
        let next_page = if page < TOTAL_PAGES - 1 {
            Some((page + 2) as u64) // Pages are 1-indexed in protocol
        } else {
            None
        };

        // Serialize
        let resp = StepResponse::Keys(keys, next_page);
        let bytes = resp.to_bytes();
        total_serialized_bytes += bytes.len();

        // Deserialize and verify
        let decoded = StepResponse::from_bytes(&bytes).unwrap();
        match decoded {
            StepResponse::Keys(decoded_keys, decoded_page) => {
                assert_eq!(decoded_keys.len(), KEYS_PER_PAGE);
                assert_eq!(decoded_page, next_page);

                // Apply to state
                state.apply_keys(&decoded_keys).await;

                // Collect for final verification
                for key in &decoded_keys {
                    all_decoded_keys.push(key.clone());
                }
            }
            _ => panic!("Expected Keys response, got {:?}", decoded.kind()),
        }

        state.commit_batch().await;
    }

    let elapsed = start.elapsed();
    let progress = state.get_progress();

    println!("Bootstrap multi-page serialization stress:");
    println!("  Total pages: {}", TOTAL_PAGES);
    println!("  Keys per page: {}", KEYS_PER_PAGE);
    println!("  Total keys: {}", TOTAL_KEYS);
    println!(
        "  Total serialized bytes: {} ({:.1} MB)",
        total_serialized_bytes,
        total_serialized_bytes as f64 / (1024.0 * 1024.0)
    );
    println!("  Progress counter: {}", progress);
    println!("  Duration: {:?}", elapsed);
    format_throughput("Multi-page serialize+apply", TOTAL_KEYS, elapsed);
    println!(
        "  Throughput (bytes): {:.1} MB/sec",
        total_serialized_bytes as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0)
    );

    // Verify all keys were applied
    assert_eq!(progress, TOTAL_KEYS as u64);
    assert_eq!(all_decoded_keys.len(), TOTAL_KEYS);
    assert_eq!(state.get_batch_count(), TOTAL_PAGES as u64);

    // Verify key uniqueness
    let stored_keys = state.keys.read().await;
    assert_eq!(stored_keys.len(), TOTAL_KEYS);

    // Verify each original key is present
    for i in 0..TOTAL_KEYS {
        let expected_key = make_pubkey(i as u64);
        assert!(
            stored_keys.contains(&expected_key),
            "Key {} not found in stored keys",
            i
        );
    }
}

// =============================================================================
// Unit Tests (non-stress)
// =============================================================================

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_make_hash_deterministic() {
        let h1 = make_hash(42);
        let h2 = make_hash(42);
        let h3 = make_hash(43);
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_make_pubkey_deterministic() {
        let k1 = make_pubkey(42);
        let k2 = make_pubkey(42);
        let k3 = make_pubkey(43);
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_chain_info_request_roundtrip() {
        let req = build_chain_info_request(8);
        assert!(roundtrip_request(&req));
    }

    #[test]
    fn test_keys_response_roundtrip() {
        let resp = build_keys_response(10, Some(2));
        assert!(roundtrip_response(&resp));
    }

    #[test]
    fn test_tns_names_response_roundtrip() {
        let resp = build_tns_names_response(10, None);
        assert!(roundtrip_response(&resp));
    }

    #[test]
    fn test_chain_info_response_roundtrip() {
        let resp = build_chain_info_response();
        assert!(roundtrip_response(&resp));
    }

    #[test]
    fn test_bootstrap_request_wrapper_roundtrip() {
        let req = BootstrapChainRequest::new(99, build_chain_info_request(4));
        assert!(roundtrip_bootstrap_request(&req));
    }

    #[test]
    fn test_bootstrap_response_wrapper_roundtrip() {
        let resp = BootstrapChainResponse::new(99, build_keys_response(10, Some(2)));
        assert!(roundtrip_bootstrap_response(&resp));
    }

    #[tokio::test]
    async fn test_mock_bootstrap_state_apply_and_reset() {
        let state = MockBootstrapState::new();

        // Apply some keys
        let mut keys = IndexSet::new();
        keys.insert(make_pubkey(0));
        keys.insert(make_pubkey(1));
        state.apply_keys(&keys).await;

        assert_eq!(state.get_progress(), 2);
        assert_eq!(state.keys.read().await.len(), 2);

        // Reset
        state.reset().await;
        assert_eq!(state.get_progress(), 0);
        assert_eq!(state.keys.read().await.len(), 0);
    }
}
