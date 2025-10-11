use bulletproofs::RangeProof;
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use merlin::Transcript;
use rayon::prelude::*;
use tos_common::crypto::{
    elgamal::{PedersenCommitment, PedersenOpening},
    proofs::{
        BatchCollector,
        CiphertextValidityProof,
        CommitmentEqProof,
        BP_GENS,
        PC_GENS,
        BULLET_PROOF_SIZE,
    },
    KeyPair
};

// Simplified proof collection
struct SimplifiedProofs {
    commitment_eq_proof: CommitmentEqProof,
    eq_keypair: KeyPair,
    eq_commitment: PedersenCommitment,
    eq_ciphertext: tos_common::crypto::elgamal::Ciphertext,

    ciphertext_validity_proof: CiphertextValidityProof,
    cv_commitment: PedersenCommitment,
    cv_keypair: KeyPair,
    cv_sender_keypair: KeyPair,
    cv_receiver_handle: tos_common::crypto::elgamal::DecryptHandle,
    cv_sender_handle: tos_common::crypto::elgamal::DecryptHandle,

    range_proof: RangeProof,
    rp_commitment: PedersenCommitment,
}

impl SimplifiedProofs {
    fn new(index: usize) -> Self {
        let value = 1000 + index as u64;

        // 1. CommitmentEqProof
        let eq_keypair = KeyPair::new();
        let eq_opening = PedersenOpening::generate_new();
        let eq_commitment = PedersenCommitment::new_with_opening(value, &eq_opening);
        let eq_ciphertext = eq_keypair.get_public_key().encrypt(value);

        let mut transcript = Transcript::new(b"test");
        let commitment_eq_proof = CommitmentEqProof::new(
            &eq_keypair,
            &eq_ciphertext,
            &eq_opening,
            value,
            &mut transcript
        );

        // 2. CiphertextValidityProof
        let cv_keypair = KeyPair::new();
        let cv_sender_keypair = KeyPair::new();
        let cv_opening = PedersenOpening::generate_new();
        let cv_commitment = PedersenCommitment::new_with_opening(value, &cv_opening);
        let cv_receiver_handle = cv_keypair.get_public_key().decrypt_handle(&cv_opening);
        let cv_sender_handle = cv_sender_keypair.get_public_key().decrypt_handle(&cv_opening);

        let mut transcript = Transcript::new(b"test");
        let ciphertext_validity_proof = CiphertextValidityProof::new(
            cv_keypair.get_public_key(),
            Some(cv_sender_keypair.get_public_key()),
            value,
            &cv_opening,
            &mut transcript
        );

        // 3. RangeProof
        let rp_opening = PedersenOpening::generate_new();
        let rp_commitment = PedersenCommitment::new_with_opening(value, &rp_opening);

        let mut transcript = Transcript::new(b"test");
        let (range_proof, _) = RangeProof::prove_single(
            &BP_GENS,
            &PC_GENS,
            &mut transcript,
            value,
            &rp_opening.as_scalar(),
            BULLET_PROOF_SIZE
        ).expect("Failed to generate range proof");

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
        }
    }

    fn verify_no_batch(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.commitment_eq_proof.verify(
            self.eq_keypair.get_public_key(),
            &self.eq_ciphertext,
            &self.eq_commitment,
            &mut Transcript::new(b"test"),
        )?;

        self.ciphertext_validity_proof.verify(
            &self.cv_commitment,
            self.cv_keypair.get_public_key(),
            self.cv_sender_keypair.get_public_key(),
            &self.cv_receiver_handle,
            &self.cv_sender_handle,
            true,
            &mut Transcript::new(b"test"),
        )?;

        self.range_proof.verify_single(
            &BP_GENS,
            &PC_GENS,
            &mut Transcript::new(b"test"),
            self.rp_commitment.as_point(),
            BULLET_PROOF_SIZE
        )?;

        Ok(())
    }

    fn pre_verify_batch(&self, batch_collector: &mut BatchCollector) -> Result<(), Box<dyn std::error::Error>> {
        self.commitment_eq_proof.pre_verify(
            self.eq_keypair.get_public_key(),
            &self.eq_ciphertext,
            &self.eq_commitment,
            &mut Transcript::new(b"test"),
            batch_collector
        )?;

        self.ciphertext_validity_proof.pre_verify(
            &self.cv_commitment,
            self.cv_keypair.get_public_key(),
            self.cv_sender_keypair.get_public_key(),
            &self.cv_receiver_handle,
            &self.cv_sender_handle,
            true,
            &mut Transcript::new(b"test"),
            batch_collector
        )?;

        Ok(())
    }
}

fn create_test_proofs(count: usize) -> Vec<SimplifiedProofs> {
    (0..count)
        .map(|i| SimplifiedProofs::new(i))
        .collect()
}

// Serial batch verification (single-core)
fn verify_serial_batch(proofs: &[SimplifiedProofs]) -> Result<(), Box<dyn std::error::Error>> {
    let mut batch_collector = BatchCollector::default();

    for proof in proofs {
        proof.pre_verify_batch(&mut batch_collector)?;
    }

    batch_collector.verify()?;

    for proof in proofs {
        proof.range_proof.verify_single(
            &BP_GENS,
            &PC_GENS,
            &mut Transcript::new(b"test"),
            proof.rp_commitment.as_point(),
            BULLET_PROOF_SIZE
        )?;
    }

    Ok(())
}

// Parallel batch verification (multi-core)
fn verify_parallel_batch(proofs: &[SimplifiedProofs], num_threads: usize) -> Result<(), String> {
    // Set up Rayon thread pool
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .unwrap();

    pool.install(|| {
        // Split proofs into multiple batches, each thread handles one batch
        let chunk_size = (proofs.len() + num_threads - 1) / num_threads;
        let chunks: Vec<&[SimplifiedProofs]> = proofs.chunks(chunk_size).collect();

        // Process each batch in parallel
        chunks.par_iter().try_for_each(|chunk| -> Result<(), String> {
            let mut batch_collector = BatchCollector::default();

            // Collect proofs within the batch
            for proof in *chunk {
                proof.pre_verify_batch(&mut batch_collector)
                    .map_err(|e| e.to_string())?;
            }

            // Batch verify CommitmentEq + CiphertextValidity
            batch_collector.verify()
                .map_err(|e| e.to_string())?;

            // Verify RangeProof
            for proof in *chunk {
                proof.range_proof.verify_single(
                    &BP_GENS,
                    &PC_GENS,
                    &mut Transcript::new(b"test"),
                    proof.rp_commitment.as_point(),
                    BULLET_PROOF_SIZE
                ).map_err(|e| e.to_string())?;
            }

            Ok(())
        })
    })
}

fn bench_1000_proofs_parallel(c: &mut Criterion) {
    let mut group = c.benchmark_group("1000_proofs_parallel");
    group.sample_size(10);

    let proofs = create_test_proofs(1000);

    // Single-core batch
    group.bench_function("1_thread", |b| {
        b.iter(|| {
            verify_serial_batch(&proofs).expect("Verification failed");
        })
    });

    // 2-core parallel
    group.bench_function("2_threads", |b| {
        b.iter(|| {
            verify_parallel_batch(&proofs, 2).expect("Verification failed");
        })
    });

    // 4-core parallel
    group.bench_function("4_threads", |b| {
        b.iter(|| {
            verify_parallel_batch(&proofs, 4).expect("Verification failed");
        })
    });

    // 8-core parallel
    group.bench_function("8_threads", |b| {
        b.iter(|| {
            verify_parallel_batch(&proofs, 8).expect("Verification failed");
        })
    });

    group.finish();
}

fn bench_parallel_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_scaling");

    for num_threads in [1, 2, 4, 8].iter() {
        let proofs = create_test_proofs(1000);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_threads", num_threads)),
            num_threads,
            |b, &threads| {
                b.iter(|| {
                    if threads == 1 {
                        verify_serial_batch(&proofs).expect("Verification failed");
                    } else {
                        verify_parallel_batch(&proofs, threads).expect("Verification failed");
                    }
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    parallel_benches,
    bench_1000_proofs_parallel,
    bench_parallel_scaling
);
criterion_main!(parallel_benches);
