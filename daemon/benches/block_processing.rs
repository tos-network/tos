// TOS Block Processing Performance Benchmarks
// Phase 3: Performance Benchmarking Engineer
//
// Benchmarks for block processing pipeline including:
// - Time per block addition
// - GHOSTDAG computation time
// - Storage operation time
// - Memory usage tracking

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, black_box, measurement::WallTime};
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::System;
use tos_common::crypto::{Hash, Hashable};
use tos_common::difficulty::Difficulty;
use tos_common::block::{Block, BlockHeader};
use tos_daemon::core::ghostdag::{
    BlueWorkType, TosGhostdagData, KType, TosGhostdag,
    calc_work_from_difficulty,
};
use tos_daemon::core::reachability::TosReachability;

// ============================================================================
// Mock Storage for Block Processing Benchmarks
// ============================================================================

struct BlockProcessingStorage {
    blocks: HashMap<Hash, Block>,
    ghostdag_data: HashMap<Hash, Arc<TosGhostdagData>>,
    difficulties: HashMap<Hash, Difficulty>,
    block_count: usize,
    total_size_bytes: usize,
}

impl BlockProcessingStorage {
    fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            ghostdag_data: HashMap::new(),
            difficulties: HashMap::new(),
            block_count: 0,
            total_size_bytes: 0,
        }
    }

    fn insert_block(&mut self, block: Block, ghostdag_data: TosGhostdagData, difficulty: Difficulty) {
        let hash = block.hash();
        let block_size = std::mem::size_of_val(&block);

        self.blocks.insert(hash.clone(), block);
        self.ghostdag_data.insert(hash.clone(), Arc::new(ghostdag_data));
        self.difficulties.insert(hash, difficulty);

        self.block_count += 1;
        self.total_size_bytes += block_size;
    }

    fn get_block(&self, hash: &Hash) -> Option<&Block> {
        self.blocks.get(hash)
    }

    fn get_ghostdag_data(&self, hash: &Hash) -> Option<Arc<TosGhostdagData>> {
        self.ghostdag_data.get(hash).cloned()
    }

    fn memory_usage(&self) -> usize {
        self.total_size_bytes
            + self.ghostdag_data.len() * std::mem::size_of::<TosGhostdagData>()
            + self.difficulties.len() * std::mem::size_of::<Difficulty>()
    }
}

// ============================================================================
// Block Generation Utilities
// ============================================================================

fn hash_from_index(index: u64) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&index.to_le_bytes());
    Hash::new(bytes)
}

fn create_mock_block(index: u64, parents: Vec<Hash>, tx_count: usize) -> Block {
    use tos_common::block::BlockVersion;
    use tos_common::transaction::Transaction;
    use tos_common::crypto::PublicKey;

    let timestamp = 1000000 + (index * 1000);

    // Create mock transactions
    let mut transactions = Vec::new();
    for i in 0..tx_count {
        // Create minimal transaction for benchmarking
        let tx = Transaction::new_transfer(
            PublicKey::default(),
            tos_common::config::TOS_ASSET,
            1000 + i as u64,
        );
        transactions.push(tx);
    }

    let tips = parents.clone();

    Block::new(
        BlockVersion::V0,
        1 + index,
        timestamp,
        tos_common::config::GENESIS_BLOCK_DIFFICULTY,
        0,
        vec![],
        Hash::new([0u8; 32]),
        vec![],
        transactions,
        tips,
    )
}

fn create_chain_with_blocks(length: usize, tx_per_block: usize) -> BlockProcessingStorage {
    let mut storage = BlockProcessingStorage::new();
    let base_difficulty = Difficulty::from(1000u64);
    let base_work = calc_work_from_difficulty(&base_difficulty);

    // Genesis
    let genesis_hash = hash_from_index(0);
    let genesis_block = create_mock_block(0, Vec::new(), 0);
    let genesis_data = TosGhostdagData::new(
        0,
        BlueWorkType::zero(),
        Hash::new([0u8; 32]),
        Vec::new(),
        Vec::new(),
        HashMap::new(),
        Vec::new(),
    );
    storage.insert_block(genesis_block, genesis_data, base_difficulty.clone());

    // Build chain
    for i in 1..length {
        let parent = hash_from_index((i - 1) as u64);
        let block = create_mock_block(i as u64, vec![parent.clone()], tx_per_block);

        let blue_score = i as u64;
        let blue_work = base_work * (i + 1);

        let data = TosGhostdagData::new(
            blue_score,
            blue_work,
            parent.clone(),
            vec![parent.clone()],
            Vec::new(),
            {
                let mut map = HashMap::new();
                map.insert(parent, 0);
                map
            },
            Vec::new(),
        );

        storage.insert_block(block, data, base_difficulty.clone());
    }

    storage
}

// ============================================================================
// Benchmark Functions
// ============================================================================

/// Benchmark time per block addition
fn bench_block_addition(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_addition");

    for tx_count in [0, 10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_txs", tx_count)),
            tx_count,
            |b, &tx_count| {
                b.iter(|| {
                    let mut storage = BlockProcessingStorage::new();
                    let base_difficulty = Difficulty::from(1000u64);

                    // Add genesis
                    let genesis_block = create_mock_block(0, Vec::new(), 0);
                    let genesis_data = TosGhostdagData::new(
                        0,
                        BlueWorkType::zero(),
                        Hash::new([0u8; 32]),
                        Vec::new(),
                        Vec::new(),
                        HashMap::new(),
                        Vec::new(),
                    );
                    storage.insert_block(genesis_block, genesis_data, base_difficulty.clone());

                    // Add next block
                    let parent = hash_from_index(0);
                    let block = create_mock_block(1, vec![parent.clone()], tx_count);
                    let data = TosGhostdagData::new(
                        1,
                        calc_work_from_difficulty(&base_difficulty),
                        parent.clone(),
                        vec![parent],
                        Vec::new(),
                        HashMap::new(),
                        Vec::new(),
                    );

                    storage.insert_block(block, data, base_difficulty);
                    black_box(storage)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark GHOSTDAG computation time
fn bench_ghostdag_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("ghostdag_computation");
    group.sample_size(20);

    for chain_length in [10, 50, 100].iter() {
        let storage = create_chain_with_blocks(*chain_length, 10);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_blocks", chain_length)),
            chain_length,
            |b, &_length| {
                b.iter(|| {
                    // Benchmark GHOSTDAG data lookup and computation
                    for i in 0..*chain_length {
                        let hash = hash_from_index(i as u64);
                        if let Some(data) = storage.get_ghostdag_data(&hash) {
                            // Simulate GHOSTDAG computation steps
                            let _blue_score = data.blue_score;
                            let _blue_work = data.blue_work;
                            let _mergeset_size = data.mergeset_blues.len() + data.mergeset_reds.len();
                            black_box((&_blue_score, &_blue_work, &_mergeset_size));
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark storage operations
fn bench_storage_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_operations");

    // Test different operation types
    let storage = create_chain_with_blocks(100, 10);

    // Read operations
    group.bench_function("read_block", |b| {
        b.iter(|| {
            let hash = hash_from_index(50);
            let block = storage.get_block(&hash);
            black_box(block)
        });
    });

    group.bench_function("read_ghostdag_data", |b| {
        b.iter(|| {
            let hash = hash_from_index(50);
            let data = storage.get_ghostdag_data(&hash);
            black_box(data)
        });
    });

    // Write operations (simulation)
    group.bench_function("write_block", |b| {
        b.iter(|| {
            let mut storage = BlockProcessingStorage::new();
            let block = create_mock_block(1, vec![hash_from_index(0)], 10);
            let data = TosGhostdagData::new(
                1,
                BlueWorkType::from(1000u64),
                hash_from_index(0),
                vec![hash_from_index(0)],
                Vec::new(),
                HashMap::new(),
                Vec::new(),
            );
            storage.insert_block(block, data, Difficulty::from(1000u64));
            black_box(storage)
        });
    });

    group.finish();
}

/// Benchmark memory usage during block processing
fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    group.sample_size(10);

    for block_count in [10, 50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_blocks", block_count)),
            block_count,
            |b, &count| {
                b.iter(|| {
                    let storage = create_chain_with_blocks(count, 10);
                    let mem_usage = storage.memory_usage();
                    black_box(mem_usage)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark full block processing pipeline
fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_block_processing_pipeline");
    group.sample_size(10);

    for block_count in [10, 20, 50].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_blocks", block_count)),
            block_count,
            |b, &count| {
                b.iter(|| {
                    let storage = create_chain_with_blocks(count, 10);

                    // Simulate full processing: read block, compute GHOSTDAG, validate
                    for i in 0..count {
                        let hash = hash_from_index(i as u64);

                        // 1. Read block
                        let _block = storage.get_block(&hash);

                        // 2. Get GHOSTDAG data
                        if let Some(data) = storage.get_ghostdag_data(&hash) {
                            // 3. Validate (loosely - in DAG, blue_score can jump by up to TIPS_LIMIT)
                            // This is just to ensure the compiler doesn't optimize away the read
                            let _is_valid = data.blue_score <= i as u64 + 32; // TIPS_LIMIT = 32
                            black_box(_is_valid);
                        }
                    }

                    black_box(storage)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark block validation time
fn bench_block_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_validation");

    let storage = create_chain_with_blocks(100, 50);

    group.bench_function("validate_block_structure", |b| {
        b.iter(|| {
            let hash = hash_from_index(50);
            if let Some(block) = storage.get_block(&hash) {
                // Simulate basic validation
                let _has_valid_version = true;
                let _has_valid_timestamp = block.get_timestamp() > 0;
                let _has_valid_height = block.get_height() > 0;
                black_box((_has_valid_version, _has_valid_timestamp, _has_valid_height));
            }
        });
    });

    group.bench_function("validate_ghostdag_consistency", |b| {
        b.iter(|| {
            let hash = hash_from_index(50);
            if let Some(data) = storage.get_ghostdag_data(&hash) {
                // Validate GHOSTDAG consistency
                let _blues_count = data.mergeset_blues.len();
                let _reds_count = data.mergeset_reds.len();
                let _anticone_valid = data.blues_anticone_sizes.len() <= _blues_count;
                black_box((_blues_count, _reds_count, _anticone_valid));
            }
        });
    });

    group.finish();
}

/// Benchmark system resource usage
fn bench_system_resources(c: &mut Criterion) {
    let mut group = c.benchmark_group("system_resources");
    group.sample_size(10);

    group.bench_function("create_100_blocks_memory", |b| {
        let mut sys = System::new_all();

        b.iter(|| {
            sys.refresh_memory();
            let mem_before = sys.used_memory();

            let storage = create_chain_with_blocks(100, 10);
            black_box(&storage);

            sys.refresh_memory();
            let mem_after = sys.used_memory();

            let mem_delta = mem_after.saturating_sub(mem_before);
            black_box(mem_delta)
        });
    });

    group.finish();
}

/// Benchmark parallel block processing capability
fn bench_parallel_block_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_block_processing");
    group.sample_size(10);

    use rayon::prelude::*;

    for block_count in [10, 50, 100].iter() {
        let storage = create_chain_with_blocks(*block_count, 10);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_blocks_sequential", block_count)),
            block_count,
            |b, &count| {
                b.iter(|| {
                    for i in 0..count {
                        let hash = hash_from_index(i as u64);
                        if let Some(data) = storage.get_ghostdag_data(&hash) {
                            let _score = data.blue_score;
                            black_box(_score);
                        }
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_blocks_parallel", block_count)),
            block_count,
            |b, &count| {
                b.iter(|| {
                    (0..count).into_par_iter().for_each(|i| {
                        let hash = hash_from_index(i as u64);
                        if let Some(data) = storage.get_ghostdag_data(&hash) {
                            let _score = data.blue_score;
                            black_box(_score);
                        }
                    });
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
    block_processing_benches,
    bench_block_addition,
    bench_ghostdag_computation,
    bench_storage_operations,
    bench_memory_usage,
    bench_full_pipeline,
    bench_block_validation,
    bench_system_resources,
    bench_parallel_block_processing,
);

criterion_main!(block_processing_benches);
