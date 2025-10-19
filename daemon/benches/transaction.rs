// TOS Transaction Verification Performance Benchmarks
// Phase 3: Performance Benchmarking Engineer
//
// Balance simplification: DEPRECATED BENCHMARK
// This benchmark is deprecated because:
// - ZK proof verification (removed)
// - ElGamal encryption/decryption (removed)
// - Bulletproofs verification (removed)
// - Parallel verification scaling (no longer applicable)
//
// Kept for historical reference only.
#![allow(dead_code)]

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, black_box};
use bulletproofs::RangeProof;
use merlin::Transcript;
use rayon::prelude::*;
use tos_common::crypto::{
    elgamal::{PedersenCommitment, PedersenOpening, Ciphertext, DecryptHandle},
    proofs::{
        BatchCollector,
        CiphertextValidityProof,
        CommitmentEqProof,
        BP_GENS,
        PC_GENS,
        BULLET_PROOF_SIZE,
    },
    KeyPair,
};

// ============================================================================
// Test Data Structures
// ============================================================================

/// Complete transaction proof data for benchmarking
struct TransactionProofs {
    // CommitmentEq proof data
    commitment_eq_proof: CommitmentEqProof,
    eq_keypair: KeyPair,
    eq_commitment: PedersenCommitment,
    eq_ciphertext: Ciphertext,

    // CiphertextValidity proof data
    ciphertext_validity_proof: CiphertextValidityProof,
    cv_commitment: PedersenCommitment,
    cv_keypair: KeyPair,
    cv_sender_keypair: KeyPair,
    cv_receiver_handle: DecryptHandle,
    cv_sender_handle: DecryptHandle,

    // RangeProof data
    range_proof: RangeProof,
    rp_commitment: PedersenCommitment,

    // Transaction value
    value: u64,
}

impl TransactionProofs {
    /// Create a new transaction proof set with the given value
    fn new(value: u64) -> Self {
        // 1. Generate CommitmentEqProof
        let eq_keypair = KeyPair::new();
        let eq_opening = PedersenOpening::generate_new();
        let eq_commitment = PedersenCommitment::new_with_opening(value, &eq_opening);
        let eq_ciphertext = eq_keypair.get_public_key().encrypt(value);

        let mut transcript = Transcript::new(b"commitment_eq");
        let commitment_eq_proof = CommitmentEqProof::new(
            &eq_keypair,
            &eq_ciphertext,
            &eq_opening,
            value,
            &mut transcript,
        );

        // 2. Generate CiphertextValidityProof
        let cv_keypair = KeyPair::new();
        let cv_sender_keypair = KeyPair::new();
        let cv_opening = PedersenOpening::generate_new();
        let cv_commitment = PedersenCommitment::new_with_opening(value, &cv_opening);
        let cv_receiver_handle = cv_keypair.get_public_key().decrypt_handle(&cv_opening);
        let cv_sender_handle = cv_sender_keypair.get_public_key().decrypt_handle(&cv_opening);

        let mut transcript = Transcript::new(b"ciphertext_validity");
        let ciphertext_validity_proof = CiphertextValidityProof::new(
            cv_keypair.get_public_key(),
            Some(cv_sender_keypair.get_public_key()),
            value,
            &cv_opening,
            &mut transcript,
        );

        // 3. Generate RangeProof
        let rp_opening = PedersenOpening::generate_new();
        let rp_commitment = PedersenCommitment::new_with_opening(value, &rp_opening);

        let mut transcript = Transcript::new(b"range_proof");
        let (range_proof, _) = RangeProof::prove_single(
            &BP_GENS,
            &PC_GENS,
            &mut transcript,
            value,
            &rp_opening.as_scalar(),
            BULLET_PROOF_SIZE,
        )
        .expect("Failed to generate range proof");

        Self {
            commitment_eq_proof,
            eq_keypair,
            eq_commitment,
            eq_ciphertext,
            ciphertext_validity_proof,
            cv_commitment,
            cv_keypair,
            cv_sender_keypair,
            cv_receiver_handle,
            cv_sender_handle,
            range_proof,
            rp_commitment,
            value,
        }
    }

    /// Verify all proofs individually (no batching)
    fn verify_individual(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Verify CommitmentEq
        self.commitment_eq_proof.verify(
            self.eq_keypair.get_public_key(),
            &self.eq_ciphertext,
            &self.eq_commitment,
            &mut Transcript::new(b"commitment_eq"),
        )?;

        // Verify CiphertextValidity
        self.ciphertext_validity_proof.verify(
            &self.cv_commitment,
            self.cv_keypair.get_public_key(),
            self.cv_sender_keypair.get_public_key(),
            &self.cv_receiver_handle,
            &self.cv_sender_handle,
            true,
            &mut Transcript::new(b"ciphertext_validity"),
        )?;

        // Verify RangeProof
        self.range_proof.verify_single(
            &BP_GENS,
            &PC_GENS,
            &mut Transcript::new(b"range_proof"),
            self.rp_commitment.as_point(),
            BULLET_PROOF_SIZE,
        )?;

        Ok(())
    }

    /// Pre-verify for batch verification
    fn pre_verify_batch(&self, batch_collector: &mut BatchCollector) -> Result<(), Box<dyn std::error::Error>> {
        self.commitment_eq_proof.pre_verify(
            self.eq_keypair.get_public_key(),
            &self.eq_ciphertext,
            &self.eq_commitment,
            &mut Transcript::new(b"commitment_eq"),
            batch_collector,
        )?;

        self.ciphertext_validity_proof.pre_verify(
            &self.cv_commitment,
            self.cv_keypair.get_public_key(),
            self.cv_sender_keypair.get_public_key(),
            &self.cv_receiver_handle,
            &self.cv_sender_handle,
            true,
            &mut Transcript::new(b"ciphertext_validity"),
            batch_collector,
        )?;

        Ok(())
    }
}

/// Create a batch of transaction proofs
fn create_transaction_batch(count: usize) -> Vec<TransactionProofs> {
    (0..count)
        .map(|i| TransactionProofs::new(1000 + i as u64))
        .collect()
}

// ============================================================================
// Benchmark Functions
// ============================================================================

/// Benchmark single transaction proof verification
fn bench_single_transaction_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_transaction_verification");

    let tx_proof = TransactionProofs::new(1000);

    group.bench_function("verify_commitment_eq", |b| {
        b.iter(|| {
            let result = tx_proof.commitment_eq_proof.verify(
                tx_proof.eq_keypair.get_public_key(),
                &tx_proof.eq_ciphertext,
                &tx_proof.eq_commitment,
                &mut Transcript::new(b"commitment_eq"),
            );
            black_box(result)
        });
    });

    group.bench_function("verify_ciphertext_validity", |b| {
        b.iter(|| {
            let result = tx_proof.ciphertext_validity_proof.verify(
                &tx_proof.cv_commitment,
                tx_proof.cv_keypair.get_public_key(),
                tx_proof.cv_sender_keypair.get_public_key(),
                &tx_proof.cv_receiver_handle,
                &tx_proof.cv_sender_handle,
                true,
                &mut Transcript::new(b"ciphertext_validity"),
            );
            black_box(result)
        });
    });

    group.bench_function("verify_range_proof", |b| {
        b.iter(|| {
            let result = tx_proof.range_proof.verify_single(
                &BP_GENS,
                &PC_GENS,
                &mut Transcript::new(b"range_proof"),
                tx_proof.rp_commitment.as_point(),
                BULLET_PROOF_SIZE,
            );
            black_box(result)
        });
    });

    group.bench_function("verify_all_proofs", |b| {
        b.iter(|| {
            let result = tx_proof.verify_individual();
            black_box(result)
        });
    });

    group.finish();
}

/// Benchmark ElGamal encryption and decryption
fn bench_elgamal_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("elgamal_operations");

    let keypair = KeyPair::new();
    let value = 1000u64;

    group.bench_function("encryption", |b| {
        b.iter(|| {
            let ciphertext = keypair.get_public_key().encrypt(black_box(value));
            black_box(ciphertext)
        });
    });

    group.bench_function("decryption", |b| {
        let ciphertext = keypair.get_public_key().encrypt(value);
        b.iter(|| {
            let decrypted = keypair.decrypt(&black_box(&ciphertext));
            black_box(decrypted)
        });
    });

    group.bench_function("pedersen_commitment", |b| {
        b.iter(|| {
            let opening = PedersenOpening::generate_new();
            let commitment = PedersenCommitment::new_with_opening(black_box(value), &opening);
            black_box(commitment)
        });
    });

    group.finish();
}

/// Benchmark batch verification vs individual verification
fn bench_batch_vs_individual(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_vs_individual_verification");
    group.sample_size(20);

    for batch_size in [10, 50, 100, 200, 500, 1000].iter() {
        let proofs = create_transaction_batch(*batch_size);

        // Individual verification
        group.bench_with_input(
            BenchmarkId::new("individual", batch_size),
            batch_size,
            |b, _| {
                b.iter(|| {
                    for proof in &proofs {
                        proof.verify_individual().expect("Verification failed");
                    }
                });
            },
        );

        // Batch verification
        group.bench_with_input(
            BenchmarkId::new("batch", batch_size),
            batch_size,
            |b, _| {
                b.iter(|| {
                    let mut batch_collector = BatchCollector::default();

                    // Pre-verify all proofs
                    for proof in &proofs {
                        proof.pre_verify_batch(&mut batch_collector)
                            .expect("Pre-verification failed");
                    }

                    // Batch verify
                    batch_collector.verify().expect("Batch verification failed");

                    // Verify range proofs individually (not batchable with current API)
                    for proof in &proofs {
                        proof.range_proof.verify_single(
                            &BP_GENS,
                            &PC_GENS,
                            &mut Transcript::new(b"range_proof"),
                            proof.rp_commitment.as_point(),
                            BULLET_PROOF_SIZE,
                        ).expect("Range proof verification failed");
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark parallel verification scaling (1-8 cores)
fn bench_parallel_verification_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_verification_scaling");
    group.sample_size(10);

    let proof_count = 1000;
    let proofs = create_transaction_batch(proof_count);

    for num_threads in [1, 2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("threads", num_threads),
            num_threads,
            |b, &threads| {
                b.iter(|| {
                    if threads == 1 {
                        // Sequential verification
                        for proof in &proofs {
                            proof.verify_individual().expect("Verification failed");
                        }
                    } else {
                        // Parallel verification
                        let pool = rayon::ThreadPoolBuilder::new()
                            .num_threads(threads)
                            .build()
                            .unwrap();

                        pool.install(|| {
                            proofs.par_iter().for_each(|proof| {
                                proof.verify_individual().expect("Verification failed");
                            });
                        });
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark parallel batch verification (combining batching + parallelism)
fn bench_parallel_batch_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_batch_verification");
    group.sample_size(10);

    let proof_count = 1000;
    let proofs = create_transaction_batch(proof_count);

    for num_threads in [1, 2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("threads", num_threads),
            num_threads,
            |b, &threads| {
                b.iter(|| {
                    let pool = rayon::ThreadPoolBuilder::new()
                        .num_threads(threads)
                        .build()
                        .unwrap();

                    pool.install(|| {
                        // Split proofs into chunks for parallel batch verification
                        let chunk_size = (proof_count + threads - 1) / threads;
                        let chunks: Vec<&[TransactionProofs]> = proofs.chunks(chunk_size).collect();

                        chunks.par_iter().try_for_each(|chunk| -> Result<(), String> {
                            let mut batch_collector = BatchCollector::default();

                            // Pre-verify chunk
                            for proof in *chunk {
                                proof.pre_verify_batch(&mut batch_collector)
                                    .map_err(|e| e.to_string())?;
                            }

                            // Batch verify
                            batch_collector.verify().map_err(|e| e.to_string())?;

                            // Verify range proofs
                            for proof in *chunk {
                                proof.range_proof.verify_single(
                                    &BP_GENS,
                                    &PC_GENS,
                                    &mut Transcript::new(b"range_proof"),
                                    proof.rp_commitment.as_point(),
                                    BULLET_PROOF_SIZE,
                                ).map_err(|e| e.to_string())?;
                            }

                            Ok(())
                        }).expect("Verification failed");
                    });
                });
            },
        );
    }

    group.finish();
}

/// Benchmark proof generation performance
fn bench_proof_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("proof_generation");

    let value = 1000u64;

    group.bench_function("generate_commitment_eq_proof", |b| {
        b.iter(|| {
            let keypair = KeyPair::new();
            let opening = PedersenOpening::generate_new();
            let commitment = PedersenCommitment::new_with_opening(value, &opening);
            let ciphertext = keypair.get_public_key().encrypt(value);

            let proof = CommitmentEqProof::new(
                &keypair,
                &ciphertext,
                &opening,
                value,
                &mut Transcript::new(b"commitment_eq"),
            );
            black_box(proof)
        });
    });

    group.bench_function("generate_ciphertext_validity_proof", |b| {
        b.iter(|| {
            let keypair = KeyPair::new();
            let sender_keypair = KeyPair::new();
            let opening = PedersenOpening::generate_new();

            let proof = CiphertextValidityProof::new(
                keypair.get_public_key(),
                Some(sender_keypair.get_public_key()),
                value,
                &opening,
                &mut Transcript::new(b"ciphertext_validity"),
            );
            black_box(proof)
        });
    });

    group.bench_function("generate_range_proof", |b| {
        b.iter(|| {
            let opening = PedersenOpening::generate_new();
            let (proof, _) = RangeProof::prove_single(
                &BP_GENS,
                &PC_GENS,
                &mut Transcript::new(b"range_proof"),
                value,
                &opening.as_scalar(),
                BULLET_PROOF_SIZE,
            )
            .expect("Failed to generate range proof");
            black_box(proof)
        });
    });

    group.bench_function("generate_all_proofs", |b| {
        b.iter(|| {
            let proof = TransactionProofs::new(black_box(value));
            black_box(proof)
        });
    });

    group.finish();
}

/// Benchmark different transaction value sizes
fn bench_different_value_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("different_value_sizes");

    for value in [100u64, 1_000, 10_000, 100_000, 1_000_000, 10_000_000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("value_{}", value)),
            value,
            |b, &val| {
                let proof = TransactionProofs::new(val);
                b.iter(|| {
                    let result = proof.verify_individual();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark speedup verification: measure actual speedup ratios
fn bench_speedup_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("speedup_verification");
    group.sample_size(10);

    let proof_count = 1000;
    let proofs = create_transaction_batch(proof_count);

    // 1. Baseline: Individual verification (no batching, no parallelism)
    group.bench_function("baseline_individual", |b| {
        b.iter(|| {
            for proof in &proofs {
                proof.verify_individual().expect("Verification failed");
            }
        });
    });

    // 2. Batch verification only (4x speedup claimed)
    group.bench_function("batch_only", |b| {
        b.iter(|| {
            let mut batch_collector = BatchCollector::default();
            for proof in &proofs {
                proof.pre_verify_batch(&mut batch_collector).expect("Pre-verification failed");
            }
            batch_collector.verify().expect("Batch verification failed");

            // Range proofs still need individual verification
            for proof in &proofs {
                proof.range_proof.verify_single(
                    &BP_GENS,
                    &PC_GENS,
                    &mut Transcript::new(b"range_proof"),
                    proof.rp_commitment.as_point(),
                    BULLET_PROOF_SIZE,
                ).expect("Range proof verification failed");
            }
        });
    });

    // 3. Parallel verification only (8x speedup claimed with 8 cores)
    group.bench_function("parallel_only_8_cores", |b| {
        b.iter(|| {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(8)
                .build()
                .unwrap();

            pool.install(|| {
                proofs.par_iter().for_each(|proof| {
                    proof.verify_individual().expect("Verification failed");
                });
            });
        });
    });

    // 4. Combined: Batch + Parallel (4x * 8x = 32x speedup claimed)
    group.bench_function("batch_and_parallel_8_cores", |b| {
        b.iter(|| {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(8)
                .build()
                .unwrap();

            pool.install(|| {
                let chunk_size = (proof_count + 7) / 8;
                let chunks: Vec<&[TransactionProofs]> = proofs.chunks(chunk_size).collect();

                chunks.par_iter().try_for_each(|chunk| -> Result<(), String> {
                    let mut batch_collector = BatchCollector::default();

                    for proof in *chunk {
                        proof.pre_verify_batch(&mut batch_collector).map_err(|e| e.to_string())?;
                    }

                    batch_collector.verify().map_err(|e| e.to_string())?;

                    for proof in *chunk {
                        proof.range_proof.verify_single(
                            &BP_GENS,
                            &PC_GENS,
                            &mut Transcript::new(b"range_proof"),
                            proof.rp_commitment.as_point(),
                            BULLET_PROOF_SIZE,
                        ).map_err(|e| e.to_string())?;
                    }

                    Ok(())
                }).expect("Verification failed");
            });
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    transaction_benches,
    bench_single_transaction_verification,
    bench_elgamal_operations,
    bench_batch_vs_individual,
    bench_parallel_verification_scaling,
    bench_parallel_batch_verification,
    bench_proof_generation,
    bench_different_value_sizes,
    bench_speedup_verification,
);

criterion_main!(transaction_benches);
