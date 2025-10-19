// Balance simplification: DEPRECATED BENCHMARK
// This benchmark is deprecated because Sigma Proofs and Bulletproofs have been removed.
// Kept for historical reference only.
#![allow(dead_code)]

use bulletproofs::RangeProof;
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use merlin::Transcript;
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

// Simplified proof collection - each proof type is generated independently
struct SimplifiedProofs {
    // CommitmentEqProof related
    commitment_eq_proof: CommitmentEqProof,
    eq_keypair: KeyPair,
    eq_commitment: PedersenCommitment,
    eq_ciphertext: tos_common::crypto::elgamal::Ciphertext,
    eq_value: u64,

    // CiphertextValidityProof related
    ciphertext_validity_proof: CiphertextValidityProof,
    cv_commitment: PedersenCommitment,
    cv_keypair: KeyPair,
    cv_sender_keypair: KeyPair,
    cv_receiver_handle: tos_common::crypto::elgamal::DecryptHandle,
    cv_sender_handle: tos_common::crypto::elgamal::DecryptHandle,

    // RangeProof related
    range_proof: RangeProof,
    rp_commitment: PedersenCommitment,
}

impl SimplifiedProofs {
    fn new(index: usize) -> Self {
        let value = 1000 + index as u64;

        // 1. Generate CommitmentEqProof
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

        // 2. Generate CiphertextValidityProof
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

        // 3. Generate RangeProof
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
            eq_value: value,
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
        // 1. Verify CommitmentEqProof
        self.commitment_eq_proof.verify(
            self.eq_keypair.get_public_key(),
            &self.eq_ciphertext,
            &self.eq_commitment,
            &mut Transcript::new(b"test"),
        )?;

        // 2. Verify CiphertextValidityProof
        self.ciphertext_validity_proof.verify(
            &self.cv_commitment,
            self.cv_keypair.get_public_key(),
            self.cv_sender_keypair.get_public_key(),
            &self.cv_receiver_handle,
            &self.cv_sender_handle,
            true,
            &mut Transcript::new(b"test"),
        )?;

        // 3. Verify RangeProof
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
        // 1. Pre-verify CommitmentEqProof
        self.commitment_eq_proof.pre_verify(
            self.eq_keypair.get_public_key(),
            &self.eq_ciphertext,
            &self.eq_commitment,
            &mut Transcript::new(b"test"),
            batch_collector
        )?;

        // 2. Pre-verify CiphertextValidityProof
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

fn bench_100_proofs(c: &mut Criterion) {
    let mut group = c.benchmark_group("100_proofs");
    let proofs = create_test_proofs(100);

    group.bench_function("no_batch", |b| {
        b.iter(|| {
            for proof in &proofs {
                proof.verify_no_batch().expect("Verification failed");
            }
        })
    });

    group.bench_function("with_batch", |b| {
        b.iter(|| {
            let mut batch_collector = BatchCollector::default();

            // Collect all proof data
            for proof in &proofs {
                proof.pre_verify_batch(&mut batch_collector).expect("Pre-verify failed");
            }

            // Batch verify CommitmentEq + CiphertextValidity
            batch_collector.verify().expect("Batch verification failed");

            // Batch verify RangeProofs
            let mut transcripts_and_commitments: Vec<_> = proofs.iter().map(|proof| {
                (Transcript::new(b"test"), vec![proof.rp_commitment.as_point().clone()])
            }).collect();

            RangeProof::verify_batch(
                transcripts_and_commitments.iter_mut().zip(&proofs).map(|((transcript, commitments), proof)| {
                    proof.range_proof.verification_view(
                        transcript,
                        commitments,
                        BULLET_PROOF_SIZE
                    )
                }),
                &BP_GENS,
                &PC_GENS,
            ).expect("Batch range proof verification failed");
        })
    });

    group.finish();
}

fn bench_1000_proofs(c: &mut Criterion) {
    let mut group = c.benchmark_group("1000_proofs");
    group.sample_size(10); // Reduce sample size because tests are slow

    let proofs = create_test_proofs(1000);

    group.bench_function("no_batch", |b| {
        b.iter(|| {
            for proof in &proofs {
                proof.verify_no_batch().expect("Verification failed");
            }
        })
    });

    group.bench_function("with_batch", |b| {
        b.iter(|| {
            let mut batch_collector = BatchCollector::default();

            for proof in &proofs {
                proof.pre_verify_batch(&mut batch_collector).expect("Pre-verify failed");
            }

            batch_collector.verify().expect("Batch verification failed");

            // Batch verify RangeProofs
            let mut transcripts_and_commitments: Vec<_> = proofs.iter().map(|proof| {
                (Transcript::new(b"test"), vec![proof.rp_commitment.as_point().clone()])
            }).collect();

            RangeProof::verify_batch(
                transcripts_and_commitments.iter_mut().zip(&proofs).map(|((transcript, commitments), proof)| {
                    proof.range_proof.verification_view(
                        transcript,
                        commitments,
                        BULLET_PROOF_SIZE
                    )
                }),
                &BP_GENS,
                &PC_GENS,
            ).expect("Batch range proof verification failed");
        })
    });

    group.finish();
}

fn bench_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_scaling");

    for size in [10, 50, 100, 500, 1000].iter() {
        let proofs = create_test_proofs(*size);

        group.bench_with_input(BenchmarkId::new("no_batch", size), size, |b, _| {
            b.iter(|| {
                for proof in &proofs {
                    proof.verify_no_batch().expect("Verification failed");
                }
            })
        });

        group.bench_with_input(BenchmarkId::new("with_batch", size), size, |b, _| {
            b.iter(|| {
                let mut batch_collector = BatchCollector::default();

                for proof in &proofs {
                    proof.pre_verify_batch(&mut batch_collector).expect("Pre-verify failed");
                }

                batch_collector.verify().expect("Batch verification failed");

                // Batch verify RangeProofs
                let mut transcripts_and_commitments: Vec<_> = proofs.iter().map(|proof| {
                    (Transcript::new(b"test"), vec![proof.rp_commitment.as_point().clone()])
                }).collect();

                RangeProof::verify_batch(
                    transcripts_and_commitments.iter_mut().zip(&proofs).map(|((transcript, commitments), proof)| {
                        proof.range_proof.verification_view(
                            transcript,
                            commitments,
                            BULLET_PROOF_SIZE
                        )
                    }),
                    &BP_GENS,
                    &PC_GENS,
                ).expect("Batch range proof verification failed");
            })
        });
    }

    group.finish();
}

criterion_group!(
    batch_benches,
    bench_100_proofs,
    bench_1000_proofs,
    bench_scaling
);
criterion_main!(batch_benches);
