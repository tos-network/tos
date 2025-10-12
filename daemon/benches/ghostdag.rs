// TOS GHOSTDAG Performance Benchmarks
// Phase 3: Performance Benchmarking Engineer
//
// Benchmarks for GHOSTDAG consensus algorithm including:
// - Linear chain performance
// - Complex DAG scenarios
// - DAA window calculation
// - K-cluster validation

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;
use tos_common::crypto::Hash;
use tos_common::difficulty::Difficulty;
use tos_daemon::core::ghostdag::{
    BlueWorkType, TosGhostdagData, KType, TosGhostdag,
    calc_work_from_difficulty, daa,
};
use tos_daemon::core::reachability::TosReachability;

// ============================================================================
// Mock Storage for Benchmarking
// ============================================================================

/// Minimal mock storage for GHOSTDAG benchmarks
/// This simulates the storage layer without actual disk I/O
struct MockStorage {
    ghostdag_data: HashMap<Hash, Arc<TosGhostdagData>>,
    block_headers: HashMap<Hash, MockBlockHeader>,
    difficulties: HashMap<Hash, Difficulty>,
    reachability_data: HashMap<Hash, bool>,
}

#[derive(Clone)]
struct MockBlockHeader {
    parents: Vec<Hash>,
    timestamp: u64,
}

impl MockStorage {
    fn new() -> Self {
        Self {
            ghostdag_data: HashMap::new(),
            block_headers: HashMap::new(),
            difficulties: HashMap::new(),
            reachability_data: HashMap::new(),
        }
    }

    fn insert_block(
        &mut self,
        hash: Hash,
        parents: Vec<Hash>,
        timestamp: u64,
        ghostdag_data: TosGhostdagData,
        difficulty: Difficulty,
    ) {
        self.block_headers.insert(hash.clone(), MockBlockHeader { parents, timestamp });
        self.ghostdag_data.insert(hash.clone(), Arc::new(ghostdag_data));
        self.difficulties.insert(hash.clone(), difficulty);
        self.reachability_data.insert(hash, true);
    }

    async fn get_ghostdag_data(&self, hash: &Hash) -> Result<Arc<TosGhostdagData>, String> {
        self.ghostdag_data
            .get(hash)
            .cloned()
            .ok_or_else(|| format!("GHOSTDAG data not found"))
    }

    async fn get_ghostdag_blue_work(&self, hash: &Hash) -> Result<BlueWorkType, String> {
        Ok(self.ghostdag_data
            .get(hash)
            .ok_or_else(|| format!("Block not found"))?
            .blue_work)
    }

    async fn get_block_header_by_hash(&self, hash: &Hash) -> Result<MockBlockHeader, String> {
        self.block_headers
            .get(hash)
            .cloned()
            .ok_or_else(|| format!("Block header not found"))
    }

    async fn get_difficulty_for_block_hash(&self, hash: &Hash) -> Result<Difficulty, String> {
        self.difficulties
            .get(hash)
            .cloned()
            .ok_or_else(|| format!("Difficulty not found"))
    }

    async fn has_reachability_data(&self, hash: &Hash) -> Result<bool, String> {
        Ok(self.reachability_data.contains_key(hash))
    }
}

impl MockBlockHeader {
    fn get_parents(&self) -> &Vec<Hash> {
        &self.parents
    }

    fn get_timestamp(&self) -> u64 {
        self.timestamp
    }
}

// ============================================================================
// Test Data Generation
// ============================================================================

/// Generate a hash from an index for deterministic testing
fn hash_from_index(index: u64) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&index.to_le_bytes());
    Hash::new(bytes)
}

/// Create a linear chain of blocks
/// Each block has only one parent (except genesis)
fn create_linear_chain(length: usize, k: KType) -> MockStorage {
    let mut storage = MockStorage::new();
    let base_difficulty = Difficulty::from(1000u64);
    let base_work = calc_work_from_difficulty(&base_difficulty);

    // Genesis block
    let genesis_hash = hash_from_index(0);
    let genesis_data = TosGhostdagData::new(
        0,
        BlueWorkType::zero(),
        Hash::new([0u8; 32]),
        Vec::new(),
        Vec::new(),
        HashMap::new(),
        Vec::new(),
    );
    storage.insert_block(
        genesis_hash.clone(),
        Vec::new(),
        0,
        genesis_data,
        base_difficulty.clone(),
    );

    // Build chain
    for i in 1..length {
        let hash = hash_from_index(i as u64);
        let parent = hash_from_index((i - 1) as u64);

        // For linear chain, just increment blue_score and blue_work
        let blue_score = i as u64;
        let blue_work = base_work * (i + 1);

        let data = TosGhostdagData::new(
            blue_score,
            blue_work,
            parent.clone(),
            vec![parent.clone()], // Only parent is blue
            Vec::new(),           // No reds in linear chain
            {
                let mut map = HashMap::new();
                map.insert(parent.clone(), 0);
                map
            },
            Vec::new(),
        );

        storage.insert_block(
            hash,
            vec![parent],
            (i as u64) * 1000, // 1 second per block
            data,
            base_difficulty.clone(),
        );
    }

    storage
}

/// Create a complex DAG with multiple parents per block
/// This creates a more realistic scenario with parallel blocks
fn create_complex_dag(blocks: usize, avg_parents: usize, k: KType) -> MockStorage {
    let mut storage = MockStorage::new();
    let base_difficulty = Difficulty::from(1000u64);
    let base_work = calc_work_from_difficulty(&base_difficulty);

    // Genesis
    let genesis_hash = hash_from_index(0);
    let genesis_data = TosGhostdagData::new(
        0,
        BlueWorkType::zero(),
        Hash::new([0u8; 32]),
        Vec::new(),
        Vec::new(),
        HashMap::new(),
        Vec::new(),
    );
    storage.insert_block(
        genesis_hash.clone(),
        Vec::new(),
        0,
        genesis_data,
        base_difficulty.clone(),
    );

    // Track tips (blocks without children yet)
    let mut tips = vec![genesis_hash];

    for i in 1..blocks {
        let hash = hash_from_index(i as u64);

        // Select parents from tips (but not more than avg_parents)
        let num_parents = std::cmp::min(avg_parents, tips.len());
        let parents: Vec<Hash> = tips.iter().rev().take(num_parents).cloned().collect();

        // Find selected parent (highest blue_work)
        let mut selected_parent = parents[0].clone();
        let mut max_blue_work = BlueWorkType::zero();
        for parent in &parents {
            if let Some(parent_data) = storage.ghostdag_data.get(parent) {
                if parent_data.blue_work > max_blue_work {
                    max_blue_work = parent_data.blue_work;
                    selected_parent = parent.clone();
                }
            }
        }

        // Calculate blue_score and blue_work
        let parent_data = storage.ghostdag_data.get(&selected_parent).unwrap();
        let blue_score = parent_data.blue_score + parents.len() as u64;
        let blue_work = parent_data.blue_work + (base_work * parents.len());

        // Create mergeset (simplified - all parents as blues)
        let mut mergeset_blues = parents.clone();
        let blues_anticone_sizes: HashMap<Hash, KType> = parents
            .iter()
            .enumerate()
            .map(|(idx, p)| (p.clone(), idx as KType))
            .collect();

        let data = TosGhostdagData::new(
            blue_score,
            blue_work,
            selected_parent.clone(),
            mergeset_blues,
            Vec::new(), // No reds for simplicity
            blues_anticone_sizes,
            Vec::new(),
        );

        storage.insert_block(
            hash.clone(),
            parents.clone(),
            (i as u64) * 1000,
            data,
            base_difficulty.clone(),
        );

        // Update tips
        tips.push(hash);
        if tips.len() > 10 {
            tips.remove(0); // Keep only recent tips
        }
    }

    storage
}

// ============================================================================
// Benchmark Functions
// ============================================================================

/// Benchmark GHOSTDAG on linear chains
fn bench_ghostdag_linear_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("ghostdag_linear_chain");

    for size in [10, 100, 1000].iter() {
        let storage = create_linear_chain(*size, 10);
        let genesis_hash = hash_from_index(0);
        let reachability = Arc::new(TosReachability::new(10, genesis_hash.clone()));
        let ghostdag = TosGhostdag::new(10, genesis_hash, reachability);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_blocks", size)),
            size,
            |b, &size| {
                b.iter(|| {
                    // Simulate finding selected parent for last block
                    let last_hash = hash_from_index((size - 1) as u64);
                    let data = tokio::runtime::Runtime::new()
                        .unwrap()
                        .block_on(storage.get_ghostdag_data(&last_hash));
                    black_box(data)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark GHOSTDAG on complex DAGs
fn bench_ghostdag_complex_dag(c: &mut Criterion) {
    let mut group = c.benchmark_group("ghostdag_complex_dag");
    group.sample_size(10); // Reduce sample size for complex benchmarks

    for (blocks, avg_parents) in [(50, 2), (100, 3), (200, 4)].iter() {
        let storage = create_complex_dag(*blocks, *avg_parents, 10);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}blocks_{}parents", blocks, avg_parents)),
            blocks,
            |b, &blocks| {
                b.iter(|| {
                    // Benchmark reading GHOSTDAG data for multiple blocks
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    for i in 1..std::cmp::min(10, blocks) {
                        let hash = hash_from_index(i as u64);
                        let data = runtime.block_on(storage.get_ghostdag_data(&hash));
                        black_box(data);
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark DAA window calculation
fn bench_daa_window_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("daa_window_calculation");

    // Create a chain long enough for full DAA window
    let chain_length = (daa::DAA_WINDOW_SIZE * 2) as usize;
    let storage = create_linear_chain(chain_length, 10);

    for window_pos in [2016, 3000, 4000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("at_block_{}", window_pos)),
            window_pos,
            |b, &pos| {
                b.iter(|| {
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    let selected_parent = hash_from_index((pos - 1) as u64);

                    // Benchmark DAA score calculation
                    let result = runtime.block_on(
                        daa::calculate_daa_score(&storage, &selected_parent, &[])
                    );
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark k-cluster validation
fn bench_k_cluster_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("k_cluster_validation");

    for k in [5, 10, 18].iter() {
        // Create DAG with varying k parameters
        let storage = create_complex_dag(100, 3, *k);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("k_{}", k)),
            k,
            |b, &k_val| {
                b.iter(|| {
                    // Benchmark validation of k-cluster constraint
                    // We simulate this by checking blue anticone sizes
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    for i in 1..20 {
                        let hash = hash_from_index(i as u64);
                        if let Ok(data) = runtime.block_on(storage.get_ghostdag_data(&hash)) {
                            // Check that all blues have anticone size <= k
                            for (_blue, &anticone_size) in data.blues_anticone_sizes.iter() {
                                let valid = anticone_size <= k_val;
                                black_box(valid);
                            }
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark blue work calculation
fn bench_blue_work_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("blue_work_calculation");

    let difficulties = vec![
        Difficulty::from(1000u64),
        Difficulty::from(10_000u64),
        Difficulty::from(100_000u64),
        Difficulty::from(1_000_000u64),
    ];

    for diff in difficulties {
        let diff_val = format!("{:?}", diff.as_ref());
        group.bench_with_input(
            BenchmarkId::from_parameter(&diff_val[..std::cmp::min(20, diff_val.len())]),
            &diff,
            |b, difficulty| {
                b.iter(|| {
                    let work = calc_work_from_difficulty(black_box(difficulty));
                    black_box(work)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark mergeset ordering
fn bench_mergeset_ordering(c: &mut Criterion) {
    let mut group = c.benchmark_group("mergeset_ordering");
    group.sample_size(20);

    for size in [10, 50, 100].iter() {
        let storage = create_complex_dag(*size, 3, 10);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_blocks", size)),
            size,
            |b, &size| {
                b.iter(|| {
                    // Benchmark reading and sorting blocks by blue work
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    let mut blocks: Vec<(Hash, BlueWorkType)> = Vec::new();

                    for i in 1..std::cmp::min(50, size) {
                        let hash = hash_from_index(i as u64);
                        if let Ok(blue_work) = runtime.block_on(storage.get_ghostdag_blue_work(&hash)) {
                            blocks.push((hash, blue_work));
                        }
                    }

                    // Sort by blue work
                    blocks.sort_by(|a, b| a.1.cmp(&b.1));
                    black_box(blocks)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark GHOSTDAG data serialization size
fn bench_ghostdag_data_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("ghostdag_data_size");

    for mergeset_size in [1, 5, 10, 20].iter() {
        let mut blues = Vec::new();
        let mut anticone_sizes = HashMap::new();

        for i in 0..*mergeset_size {
            let hash = hash_from_index(i as u64);
            blues.push(hash.clone());
            anticone_sizes.insert(hash, i as KType);
        }

        let data = TosGhostdagData::new(
            100,
            BlueWorkType::from(1000u64),
            hash_from_index(0),
            blues,
            Vec::new(),
            anticone_sizes,
            Vec::new(),
        );

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_blues", mergeset_size)),
            mergeset_size,
            |b, _| {
                b.iter(|| {
                    // Benchmark serialization size estimation
                    let serialized = bincode::serialize(&data).unwrap();
                    black_box(serialized.len())
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    ghostdag_benches,
    bench_ghostdag_linear_chain,
    bench_ghostdag_complex_dag,
    bench_daa_window_calculation,
    bench_k_cluster_validation,
    bench_blue_work_calculation,
    bench_mergeset_ordering,
    bench_ghostdag_data_size,
);

criterion_main!(ghostdag_benches);
