use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    crypto::{Hash, PublicKey},
    transaction::ArbiterSignature,
};

/// Dispute outcome encoded in verdict signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisputeOutcome {
    PayerWins,
    PayeeWins,
    Split,
}

impl DisputeOutcome {
    fn as_u8(self) -> u8 {
        match self {
            DisputeOutcome::PayerWins => 0,
            DisputeOutcome::PayeeWins => 1,
            DisputeOutcome::Split => 2,
        }
    }
}

/// Verdict artifact used for on-chain signature verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerdictArtifact {
    pub chain_id: u64,
    pub escrow_id: Hash,
    pub dispute_id: Hash,
    pub round: u32,
    pub outcome: DisputeOutcome,
    pub payer_amount: u64,
    pub payee_amount: u64,
    pub signatures: Vec<ArbiterSignature>,
}

impl VerdictArtifact {
    /// Build the domain-separated message that arbiters sign.
    pub fn message(&self) -> Vec<u8> {
        build_verdict_message(
            self.chain_id,
            &self.escrow_id,
            &self.dispute_id,
            self.round,
            self.outcome,
            self.payer_amount,
            self.payee_amount,
        )
    }
}

/// Registry interface used to validate arbiter status and stake.
pub trait ArbiterRegistry {
    type Error: std::fmt::Display;

    /// Returns true if the arbiter is active.
    fn is_active(&self, arbiter: &PublicKey) -> Result<bool, Self::Error>;
    /// Returns the arbiter's current stake.
    fn stake(&self, arbiter: &PublicKey) -> Result<u64, Self::Error>;
    /// Returns the minimum stake required to sign verdicts.
    fn min_stake(&self) -> Result<u64, Self::Error>;
    /// Returns true if the arbiter is assigned to the given escrow (in allowed_arbiters list).
    /// Default implementation returns true if allowed_arbiters is empty or arbiter is in the list.
    fn is_assigned(&self, arbiter: &PublicKey, allowed_arbiters: &[PublicKey]) -> bool {
        allowed_arbiters.is_empty() || allowed_arbiters.contains(arbiter)
    }
}

/// Verdict signature verification errors.
#[derive(Debug, Error)]
pub enum VerdictVerificationError {
    #[error("invalid verdict amounts")]
    InvalidVerdictAmounts,
    #[error("invalid verdict outcome")]
    InvalidOutcome,
    #[error("threshold not met: required {required}, found {found}")]
    ThresholdNotMet { required: u8, found: u8 },
    #[error("invalid signature")]
    InvalidSignature,
    #[error("arbiter not active")]
    ArbiterNotActive,
    #[error("arbiter stake too low: required {required}, found {found}")]
    ArbiterStakeTooLow { required: u64, found: u64 },
    #[error("arbiter not assigned to this escrow")]
    ArbiterNotAssigned,
    #[error("registry error: {0}")]
    Registry(String),
}

/// Derive a dispute outcome from verdict amounts.
pub fn derive_dispute_outcome(payer_amount: u64, payee_amount: u64) -> DisputeOutcome {
    if payer_amount == 0 && payee_amount > 0 {
        DisputeOutcome::PayeeWins
    } else if payee_amount == 0 && payer_amount > 0 {
        DisputeOutcome::PayerWins
    } else {
        DisputeOutcome::Split
    }
}

/// Build the domain-separated message that arbiters sign.
pub fn build_verdict_message(
    chain_id: u64,
    escrow_id: &Hash,
    dispute_id: &Hash,
    round: u32,
    outcome: DisputeOutcome,
    payer_amount: u64,
    payee_amount: u64,
) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(b"TOS_VERDICT_V1");
    message.extend_from_slice(&chain_id.to_le_bytes());
    message.extend_from_slice(escrow_id.as_bytes());
    message.extend_from_slice(dispute_id.as_bytes());
    message.extend_from_slice(&round.to_le_bytes());
    message.push(outcome.as_u8());
    message.extend_from_slice(&payer_amount.to_le_bytes());
    message.extend_from_slice(&payee_amount.to_le_bytes());
    message
}

/// Verify arbiter signatures against a verdict artifact.
///
/// This function checks:
/// 1. Verdict amounts are valid
/// 2. Outcome matches derived outcome from amounts
/// 3. Each signing arbiter is assigned to this escrow (if allowed_arbiters is non-empty)
/// 4. Each signing arbiter is active and has sufficient stake
/// 5. Signatures are cryptographically valid
/// 6. Threshold number of valid signatures is met
pub fn verify_verdict_signatures<R: ArbiterRegistry>(
    artifact: &VerdictArtifact,
    required_threshold: u8,
    registry: &R,
) -> Result<(), VerdictVerificationError> {
    // Call the extended version with empty allowed_arbiters for backwards compatibility
    verify_verdict_signatures_with_allowed(artifact, required_threshold, registry, &[])
}

/// Verify arbiter signatures against a verdict artifact with specific allowed arbiters.
///
/// If allowed_arbiters is non-empty, only arbiters in that list can sign the verdict.
/// This prevents unauthorized arbiters from resolving escrows they weren't assigned to.
pub fn verify_verdict_signatures_with_allowed<R: ArbiterRegistry>(
    artifact: &VerdictArtifact,
    required_threshold: u8,
    registry: &R,
    allowed_arbiters: &[PublicKey],
) -> Result<(), VerdictVerificationError> {
    let total = artifact
        .payer_amount
        .checked_add(artifact.payee_amount)
        .ok_or(VerdictVerificationError::InvalidVerdictAmounts)?;
    if total == 0 {
        return Err(VerdictVerificationError::InvalidVerdictAmounts);
    }

    let expected_outcome = derive_dispute_outcome(artifact.payer_amount, artifact.payee_amount);
    if artifact.outcome != expected_outcome {
        return Err(VerdictVerificationError::InvalidOutcome);
    }

    let min_stake = registry
        .min_stake()
        .map_err(|e| VerdictVerificationError::Registry(e.to_string()))?;

    let mut seen = HashSet::new();
    let mut valid = 0u8;

    let message = artifact.message();
    for sig in &artifact.signatures {
        if !seen.insert(sig.arbiter_pubkey.clone()) {
            continue;
        }
        // Check if arbiter is assigned to this escrow
        if !registry.is_assigned(&sig.arbiter_pubkey, allowed_arbiters) {
            return Err(VerdictVerificationError::ArbiterNotAssigned);
        }
        let is_active = registry
            .is_active(&sig.arbiter_pubkey)
            .map_err(|e| VerdictVerificationError::Registry(e.to_string()))?;
        if !is_active {
            return Err(VerdictVerificationError::ArbiterNotActive);
        }
        let stake = registry
            .stake(&sig.arbiter_pubkey)
            .map_err(|e| VerdictVerificationError::Registry(e.to_string()))?;
        if stake < min_stake {
            return Err(VerdictVerificationError::ArbiterStakeTooLow {
                required: min_stake,
                found: stake,
            });
        }
        verify_arbiter_signature(&message, sig)?;
        valid = valid.saturating_add(1);
    }

    if valid < required_threshold {
        return Err(VerdictVerificationError::ThresholdNotMet {
            required: required_threshold,
            found: valid,
        });
    }

    Ok(())
}

fn verify_arbiter_signature(
    message: &[u8],
    sig: &ArbiterSignature,
) -> Result<(), VerdictVerificationError> {
    let public = sig
        .arbiter_pubkey
        .decompress()
        .map_err(|_| VerdictVerificationError::InvalidSignature)?;
    if !sig.signature.verify(message, &public) {
        return Err(VerdictVerificationError::InvalidSignature);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::elgamal::KeyPair;

    struct TestRegistry {
        active: HashSet<PublicKey>,
        stake: u64,
        min_stake: u64,
    }

    impl ArbiterRegistry for TestRegistry {
        type Error = &'static str;

        fn is_active(&self, arbiter: &PublicKey) -> Result<bool, Self::Error> {
            Ok(self.active.contains(arbiter))
        }

        fn stake(&self, _arbiter: &PublicKey) -> Result<u64, Self::Error> {
            Ok(self.stake)
        }

        fn min_stake(&self) -> Result<u64, Self::Error> {
            Ok(self.min_stake)
        }
    }

    fn make_artifact(
        keypair: &KeyPair,
        escrow_id: Hash,
        dispute_id: Hash,
        round: u32,
        payer_amount: u64,
        payee_amount: u64,
    ) -> VerdictArtifact {
        let outcome = derive_dispute_outcome(payer_amount, payee_amount);
        let message = build_verdict_message(
            1,
            &escrow_id,
            &dispute_id,
            round,
            outcome,
            payer_amount,
            payee_amount,
        );
        let signature = keypair.sign(&message);
        VerdictArtifact {
            chain_id: 1,
            escrow_id,
            dispute_id,
            round,
            outcome,
            payer_amount,
            payee_amount,
            signatures: vec![ArbiterSignature {
                arbiter_pubkey: keypair.get_public_key().compress(),
                signature,
                timestamp: 1,
            }],
        }
    }

    #[test]
    fn verify_verdict_happy_path() -> Result<(), Box<dyn std::error::Error>> {
        let keypair = KeyPair::new();
        let artifact = make_artifact(&keypair, Hash::zero(), Hash::max(), 0, 40, 60);
        let mut active = HashSet::new();
        active.insert(keypair.get_public_key().compress());
        let registry = TestRegistry {
            active,
            stake: 1000,
            min_stake: 1,
        };
        verify_verdict_signatures(&artifact, 1, &registry)?;
        Ok(())
    }

    #[test]
    fn verify_verdict_rejects_inactive() -> Result<(), Box<dyn std::error::Error>> {
        let keypair = KeyPair::new();
        let artifact = make_artifact(&keypair, Hash::zero(), Hash::max(), 0, 40, 60);
        let registry = TestRegistry {
            active: HashSet::new(),
            stake: 1000,
            min_stake: 1,
        };
        let err = match verify_verdict_signatures(&artifact, 1, &registry) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, VerdictVerificationError::ArbiterNotActive));
        Ok(())
    }

    #[test]
    fn verify_verdict_rejects_low_stake() -> Result<(), Box<dyn std::error::Error>> {
        let keypair = KeyPair::new();
        let artifact = make_artifact(&keypair, Hash::zero(), Hash::max(), 0, 40, 60);
        let mut active = HashSet::new();
        active.insert(keypair.get_public_key().compress());
        let registry = TestRegistry {
            active,
            stake: 1,
            min_stake: 10,
        };
        let err = match verify_verdict_signatures(&artifact, 1, &registry) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            VerdictVerificationError::ArbiterStakeTooLow { .. }
        ));
        Ok(())
    }

    #[test]
    fn verify_verdict_rejects_bad_signature() -> Result<(), Box<dyn std::error::Error>> {
        let keypair = KeyPair::new();
        let mut artifact = make_artifact(&keypair, Hash::zero(), Hash::max(), 0, 40, 60);
        artifact.round = 1;
        let mut active = HashSet::new();
        active.insert(keypair.get_public_key().compress());
        let registry = TestRegistry {
            active,
            stake: 1000,
            min_stake: 1,
        };
        let err = match verify_verdict_signatures(&artifact, 1, &registry) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, VerdictVerificationError::InvalidSignature));
        Ok(())
    }

    #[test]
    fn verify_verdict_rejects_outcome_mismatch() -> Result<(), Box<dyn std::error::Error>> {
        let keypair = KeyPair::new();
        let mut artifact = make_artifact(&keypair, Hash::zero(), Hash::max(), 0, 40, 60);
        artifact.outcome = DisputeOutcome::PayerWins;
        let mut active = HashSet::new();
        active.insert(keypair.get_public_key().compress());
        let registry = TestRegistry {
            active,
            stake: 1000,
            min_stake: 1,
        };
        let err = match verify_verdict_signatures(&artifact, 1, &registry) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, VerdictVerificationError::InvalidOutcome));
        Ok(())
    }
}
