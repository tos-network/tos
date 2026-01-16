use std::collections::HashSet;

use thiserror::Error;

use crate::{
    crypto::{Hash, PublicKey},
    escrow::{EscrowAccount, EscrowState},
    transaction::payload::{
        ArbiterSignature, ChallengeEscrowPayload, CreateEscrowPayload, DepositEscrowPayload,
        RefundEscrowPayload, ReleaseEscrowPayload, SubmitVerdictPayload,
    },
};

const MAX_TASK_ID_LEN: usize = 1024;
const MAX_REASON_LEN: usize = 1024;
const MAX_REFUND_REASON_LEN: usize = 1024;
const MAX_BPS: u16 = 10_000;

#[derive(Debug, Error)]
pub enum EscrowVerificationError {
    #[error("invalid amount")]
    InvalidAmount,
    #[error("invalid task id")]
    InvalidTaskId,
    #[error("invalid challenge window")]
    InvalidChallengeWindow,
    #[error("invalid timeout blocks")]
    InvalidTimeoutBlocks,
    #[error("invalid challenge deposit bps")]
    InvalidChallengeDepositBps,
    #[error("invalid escrow state")]
    InvalidState,
    #[error("unauthorized caller")]
    Unauthorized,
    #[error("timeout not reached")]
    TimeoutNotReached,
    #[error("challenge window expired")]
    ChallengeWindowExpired,
    #[error("challenge deposit too low: required {required}, got {provided}")]
    ChallengeDepositTooLow { required: u64, provided: u64 },
    #[error("invalid verdict amounts")]
    InvalidVerdictAmounts,
    #[error("threshold not met: required {required}, found {found}")]
    ThresholdNotMet { required: u8, found: u8 },
    #[error("invalid signature")]
    InvalidSignature,
    #[error("arbiter not active")]
    ArbiterNotActive,
    #[error("arbiter stake too low: required {required}, found {found}")]
    ArbiterStakeTooLow { required: u64, found: u64 },
    #[error("registry error: {0}")]
    Registry(String),
    #[error("invalid reason length")]
    InvalidReasonLength,
}

pub trait ArbiterRegistry {
    type Error: std::fmt::Display;

    fn is_active(&self, arbiter: &PublicKey) -> Result<bool, Self::Error>;
    fn stake(&self, arbiter: &PublicKey) -> Result<u64, Self::Error>;
    fn min_stake(&self) -> Result<u64, Self::Error>;
}

/// Verify create escrow payload (stateless).
pub fn verify_create_escrow(payload: &CreateEscrowPayload) -> Result<(), EscrowVerificationError> {
    if payload.amount == 0 {
        return Err(EscrowVerificationError::InvalidAmount);
    }
    if payload.task_id.is_empty() || payload.task_id.len() > MAX_TASK_ID_LEN {
        return Err(EscrowVerificationError::InvalidTaskId);
    }
    if payload.timeout_blocks == 0 {
        return Err(EscrowVerificationError::InvalidTimeoutBlocks);
    }
    if payload.challenge_window == 0 {
        return Err(EscrowVerificationError::InvalidChallengeWindow);
    }
    if payload.challenge_deposit_bps > MAX_BPS {
        return Err(EscrowVerificationError::InvalidChallengeDepositBps);
    }
    Ok(())
}

/// Verify deposit escrow payload (read-only).
pub fn verify_deposit_escrow(
    payload: &DepositEscrowPayload,
    escrow: &EscrowAccount,
) -> Result<(), EscrowVerificationError> {
    if payload.amount == 0 {
        return Err(EscrowVerificationError::InvalidAmount);
    }
    if !matches!(escrow.state, EscrowState::Created | EscrowState::Funded) {
        return Err(EscrowVerificationError::InvalidState);
    }
    Ok(())
}

/// Verify release escrow payload (read-only).
pub fn verify_release_escrow(
    payload: &ReleaseEscrowPayload,
    escrow: &EscrowAccount,
    caller: &PublicKey,
) -> Result<(), EscrowVerificationError> {
    if payload.amount == 0 || payload.amount > escrow.amount {
        return Err(EscrowVerificationError::InvalidAmount);
    }
    if caller != &escrow.payee {
        return Err(EscrowVerificationError::Unauthorized);
    }
    if escrow.state != EscrowState::Funded {
        return Err(EscrowVerificationError::InvalidState);
    }
    Ok(())
}

/// Verify refund escrow payload (read-only).
pub fn verify_refund_escrow(
    payload: &RefundEscrowPayload,
    escrow: &EscrowAccount,
    caller: &PublicKey,
    current_height: u64,
) -> Result<(), EscrowVerificationError> {
    if payload.amount == 0 || payload.amount > escrow.amount {
        return Err(EscrowVerificationError::InvalidAmount);
    }
    if let Some(reason) = payload.reason.as_ref() {
        if reason.len() > MAX_REFUND_REASON_LEN {
            return Err(EscrowVerificationError::InvalidReasonLength);
        }
    }

    let timeout_height = escrow
        .created_at
        .checked_add(escrow.timeout_blocks)
        .ok_or(EscrowVerificationError::InvalidTimeoutBlocks)?;
    let timeout_reached = current_height >= timeout_height;

    if caller == &escrow.payer {
        if !matches!(
            escrow.state,
            EscrowState::Funded | EscrowState::PendingRelease
        ) {
            return Err(EscrowVerificationError::InvalidState);
        }
        return Ok(());
    }

    if !timeout_reached {
        return Err(EscrowVerificationError::TimeoutNotReached);
    }

    if matches!(
        escrow.state,
        EscrowState::Released | EscrowState::Refunded | EscrowState::Resolved
    ) {
        return Err(EscrowVerificationError::InvalidState);
    }

    Ok(())
}

/// Verify challenge escrow payload (read-only).
pub fn verify_challenge_escrow(
    payload: &ChallengeEscrowPayload,
    escrow: &EscrowAccount,
    caller: &PublicKey,
    current_height: u64,
) -> Result<(), EscrowVerificationError> {
    if payload.reason.is_empty() || payload.reason.len() > MAX_REASON_LEN {
        return Err(EscrowVerificationError::InvalidReasonLength);
    }
    if caller != &escrow.payer {
        return Err(EscrowVerificationError::Unauthorized);
    }
    if escrow.state != EscrowState::PendingRelease {
        return Err(EscrowVerificationError::InvalidState);
    }
    let release_at = escrow
        .release_requested_at
        .ok_or(EscrowVerificationError::InvalidState)?;
    let window_end = release_at
        .checked_add(escrow.challenge_window)
        .ok_or(EscrowVerificationError::InvalidChallengeWindow)?;
    if current_height > window_end {
        return Err(EscrowVerificationError::ChallengeWindowExpired);
    }

    let required = escrow
        .amount
        .checked_mul(u64::from(escrow.challenge_deposit_bps))
        .and_then(|value| value.checked_div(u64::from(MAX_BPS)))
        .ok_or(EscrowVerificationError::InvalidChallengeDepositBps)?;
    if payload.deposit < required {
        return Err(EscrowVerificationError::ChallengeDepositTooLow {
            required,
            provided: payload.deposit,
        });
    }
    Ok(())
}

/// Verify submit verdict payload (read-only).
pub fn verify_submit_verdict<R: ArbiterRegistry>(
    payload: &SubmitVerdictPayload,
    escrow: &EscrowAccount,
    chain_id: u64,
    required_threshold: u8,
    registry: &R,
) -> Result<(), EscrowVerificationError> {
    if escrow.state != EscrowState::Challenged {
        return Err(EscrowVerificationError::InvalidState);
    }
    let total = payload
        .payer_amount
        .checked_add(payload.payee_amount)
        .ok_or(EscrowVerificationError::InvalidVerdictAmounts)?;
    if total != escrow.amount {
        return Err(EscrowVerificationError::InvalidVerdictAmounts);
    }

    let message = build_verdict_message(
        chain_id,
        &payload.escrow_id,
        &payload.dispute_id,
        payload.round,
        payload.payer_amount,
        payload.payee_amount,
    );

    let min_stake = registry
        .min_stake()
        .map_err(|e| EscrowVerificationError::Registry(e.to_string()))?;

    let mut seen = HashSet::new();
    let mut valid = 0u8;

    for sig in &payload.signatures {
        if !seen.insert(sig.arbiter_pubkey.clone()) {
            continue;
        }
        let is_active = registry
            .is_active(&sig.arbiter_pubkey)
            .map_err(|e| EscrowVerificationError::Registry(e.to_string()))?;
        if !is_active {
            return Err(EscrowVerificationError::ArbiterNotActive);
        }
        let stake = registry
            .stake(&sig.arbiter_pubkey)
            .map_err(|e| EscrowVerificationError::Registry(e.to_string()))?;
        if stake < min_stake {
            return Err(EscrowVerificationError::ArbiterStakeTooLow {
                required: min_stake,
                found: stake,
            });
        }
        verify_arbiter_signature(&message, sig)?;
        valid = valid.saturating_add(1);
    }

    if valid < required_threshold {
        return Err(EscrowVerificationError::ThresholdNotMet {
            required: required_threshold,
            found: valid,
        });
    }

    Ok(())
}

fn verify_arbiter_signature(
    message: &[u8],
    sig: &ArbiterSignature,
) -> Result<(), EscrowVerificationError> {
    let public = sig
        .arbiter_pubkey
        .decompress()
        .map_err(|_| EscrowVerificationError::InvalidSignature)?;
    if !sig.signature.verify(message, &public) {
        return Err(EscrowVerificationError::InvalidSignature);
    }
    Ok(())
}

fn build_verdict_message(
    chain_id: u64,
    escrow_id: &Hash,
    dispute_id: &Hash,
    round: u32,
    payer_amount: u64,
    payee_amount: u64,
) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(b"TOS_VERDICT_V1");
    message.extend_from_slice(&chain_id.to_le_bytes());
    message.extend_from_slice(escrow_id.as_bytes());
    message.extend_from_slice(dispute_id.as_bytes());
    message.extend_from_slice(&round.to_le_bytes());
    message.extend_from_slice(&payer_amount.to_le_bytes());
    message.extend_from_slice(&payee_amount.to_le_bytes());
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::elgamal::KeyPair;
    use crate::serializer::Serializer;

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

    fn sample_escrow(state: EscrowState) -> Result<EscrowAccount, Box<dyn std::error::Error>> {
        Ok(EscrowAccount {
            id: Hash::zero(),
            task_id: "task".to_string(),
            payer: PublicKey::from_bytes(&[1u8; 32])?,
            payee: PublicKey::from_bytes(&[2u8; 32])?,
            amount: 100,
            asset: Hash::max(),
            state,
            challenge_window: 10,
            challenge_deposit_bps: 500,
            release_requested_at: Some(5),
            created_at: 1,
            timeout_blocks: 10,
            arbitration_config: None,
        })
    }

    #[test]
    fn create_escrow_rejects_zero_amount() -> Result<(), Box<dyn std::error::Error>> {
        let payload = CreateEscrowPayload {
            task_id: "task".to_string(),
            provider: PublicKey::from_bytes(&[3u8; 32])?,
            amount: 0,
            asset: Hash::zero(),
            timeout_blocks: 10,
            challenge_window: 5,
            challenge_deposit_bps: 100,
            optimistic_release: true,
            arbitration_config: None,
            metadata: None,
        };
        let err = match verify_create_escrow(&payload) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, EscrowVerificationError::InvalidAmount));
        Ok(())
    }

    #[test]
    fn release_requires_payee() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Funded)?;
        let payload = ReleaseEscrowPayload {
            escrow_id: escrow.id.clone(),
            amount: 10,
            completion_proof: None,
        };
        let caller = PublicKey::from_bytes(&[9u8; 32])?;
        let err = match verify_release_escrow(&payload, &escrow, &caller) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, EscrowVerificationError::Unauthorized));
        Ok(())
    }

    #[test]
    fn refund_requires_timeout_or_payer() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Funded)?;
        let payload = RefundEscrowPayload {
            escrow_id: escrow.id.clone(),
            amount: 10,
            reason: None,
        };
        let caller = PublicKey::from_bytes(&[9u8; 32])?;
        let err = match verify_refund_escrow(&payload, &escrow, &caller, 5) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, EscrowVerificationError::TimeoutNotReached));
        Ok(())
    }

    #[test]
    fn challenge_requires_window() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::PendingRelease)?;
        let payload = ChallengeEscrowPayload {
            escrow_id: escrow.id.clone(),
            reason: "test".to_string(),
            evidence_hash: None,
            deposit: 1,
        };
        let err = match verify_challenge_escrow(&payload, &escrow, &escrow.payer, 100) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            EscrowVerificationError::ChallengeWindowExpired
        ));
        Ok(())
    }

    #[test]
    fn submit_verdict_happy_path() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Challenged)?;
        let keypair = KeyPair::new();
        let arbiter_pubkey = keypair.get_public_key().compress();
        let message = build_verdict_message(1, &escrow.id, &Hash::max(), 0, 50, 50);
        let signature = keypair.sign(&message);

        let payload = SubmitVerdictPayload {
            escrow_id: escrow.id.clone(),
            dispute_id: Hash::max(),
            round: 0,
            payer_amount: 50,
            payee_amount: 50,
            signatures: vec![ArbiterSignature {
                arbiter_pubkey: arbiter_pubkey.clone(),
                signature,
                timestamp: 1,
            }],
        };

        let mut active = HashSet::new();
        active.insert(arbiter_pubkey);
        let registry = TestRegistry {
            active,
            stake: 1000,
            min_stake: 1,
        };

        verify_submit_verdict(&payload, &escrow, 1, 1, &registry)?;
        Ok(())
    }
}
