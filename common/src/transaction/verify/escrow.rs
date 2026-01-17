use thiserror::Error;

use crate::{
    arbitration::verdict::{
        derive_dispute_outcome, verify_verdict_signatures, ArbiterRegistry, VerdictArtifact,
        VerdictVerificationError,
    },
    crypto::PublicKey,
    escrow::{EscrowAccount, EscrowState},
    transaction::payload::{
        AppealEscrowPayload, ChallengeEscrowPayload, CreateEscrowPayload, DepositEscrowPayload,
        DisputeEscrowPayload, RefundEscrowPayload, ReleaseEscrowPayload, SubmitVerdictPayload,
    },
};

const MAX_TASK_ID_LEN: usize = 256;
const MAX_REASON_LEN: usize = 1024;
const MAX_REFUND_REASON_LEN: usize = 1024;
const MAX_BPS: u16 = 10_000;
const MIN_TIMEOUT_BLOCKS: u64 = 10;
const MAX_TIMEOUT_BLOCKS: u64 = 525_600;
const MIN_APPEAL_DEPOSIT_BPS: u16 = 500;

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
    #[error("appeal not allowed")]
    AppealNotAllowed,
    #[error("appeal deposit too low: required {required}, got {provided}")]
    AppealDepositTooLow { required: u64, provided: u64 },
    #[error("appeal window expired")]
    AppealWindowExpired,
    #[error("invalid verdict amounts")]
    InvalidVerdictAmounts,
    #[error("invalid verdict round")]
    InvalidVerdictRound,
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
    #[error("insufficient escrow balance: required {required}, available {available}")]
    InsufficientEscrowBalance { required: u64, available: u64 },
}

/// Verify create escrow payload (stateless).
pub fn verify_create_escrow(
    payload: &CreateEscrowPayload,
    payer: &PublicKey,
) -> Result<(), EscrowVerificationError> {
    if payload.amount == 0 {
        return Err(EscrowVerificationError::InvalidAmount);
    }
    if payload.task_id.is_empty() || payload.task_id.len() > MAX_TASK_ID_LEN {
        return Err(EscrowVerificationError::InvalidTaskId);
    }
    if payload.timeout_blocks < MIN_TIMEOUT_BLOCKS || payload.timeout_blocks > MAX_TIMEOUT_BLOCKS {
        return Err(EscrowVerificationError::InvalidTimeoutBlocks);
    }
    if payload.challenge_window == 0 {
        return Err(EscrowVerificationError::InvalidChallengeWindow);
    }
    if payload.challenge_deposit_bps > MAX_BPS {
        return Err(EscrowVerificationError::InvalidChallengeDepositBps);
    }
    if &payload.provider == payer {
        return Err(EscrowVerificationError::Unauthorized);
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
    if caller != &escrow.payer {
        return Err(EscrowVerificationError::Unauthorized);
    }
    if escrow.state != EscrowState::Funded {
        return Err(EscrowVerificationError::InvalidState);
    }
    Ok(())
}

/// Verify release escrow payload (stateful, includes escrow balance).
pub fn verify_release_escrow_with_balance(
    payload: &ReleaseEscrowPayload,
    escrow: &EscrowAccount,
    caller: &PublicKey,
    escrow_balance: u64,
) -> Result<(), EscrowVerificationError> {
    verify_release_escrow(payload, escrow, caller)?;
    if escrow_balance < payload.amount {
        return Err(EscrowVerificationError::InsufficientEscrowBalance {
            required: payload.amount,
            available: escrow_balance,
        });
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

    if !timeout_reached {
        if caller == &escrow.payee {
            if !matches!(
                escrow.state,
                EscrowState::Funded | EscrowState::PendingRelease
            ) {
                return Err(EscrowVerificationError::InvalidState);
            }
            return Ok(());
        }
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

/// Verify refund escrow payload (stateful, includes escrow balance).
pub fn verify_refund_escrow_with_balance(
    payload: &RefundEscrowPayload,
    escrow: &EscrowAccount,
    caller: &PublicKey,
    current_height: u64,
    escrow_balance: u64,
) -> Result<(), EscrowVerificationError> {
    verify_refund_escrow(payload, escrow, caller, current_height)?;
    if escrow_balance < payload.amount {
        return Err(EscrowVerificationError::InsufficientEscrowBalance {
            required: payload.amount,
            available: escrow_balance,
        });
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

/// Verify dispute escrow payload (read-only).
pub fn verify_dispute_escrow(
    payload: &DisputeEscrowPayload,
    escrow: &EscrowAccount,
    caller: &PublicKey,
) -> Result<(), EscrowVerificationError> {
    if payload.reason.is_empty() || payload.reason.len() > MAX_REASON_LEN {
        return Err(EscrowVerificationError::InvalidReasonLength);
    }
    if caller != &escrow.payer && caller != &escrow.payee {
        return Err(EscrowVerificationError::Unauthorized);
    }
    if !matches!(
        escrow.state,
        EscrowState::Funded | EscrowState::PendingRelease
    ) {
        return Err(EscrowVerificationError::InvalidState);
    }
    if escrow.dispute.is_some() {
        return Err(EscrowVerificationError::InvalidState);
    }
    if escrow.arbitration_config.is_none() {
        return Err(EscrowVerificationError::AppealNotAllowed);
    }
    Ok(())
}

/// Verify appeal escrow payload (read-only).
pub fn verify_appeal_escrow(
    payload: &AppealEscrowPayload,
    escrow: &EscrowAccount,
    caller: &PublicKey,
    current_height: u64,
) -> Result<(), EscrowVerificationError> {
    if payload.reason.is_empty() || payload.reason.len() > MAX_REASON_LEN {
        return Err(EscrowVerificationError::InvalidReasonLength);
    }
    if caller != &escrow.payer && caller != &escrow.payee {
        return Err(EscrowVerificationError::Unauthorized);
    }
    if escrow.state != EscrowState::Resolved {
        return Err(EscrowVerificationError::InvalidState);
    }
    if escrow.dispute.is_none() {
        return Err(EscrowVerificationError::InvalidState);
    }
    if escrow.appeal.is_some() {
        return Err(EscrowVerificationError::InvalidState);
    }
    let Some(config) = escrow.arbitration_config.as_ref() else {
        return Err(EscrowVerificationError::AppealNotAllowed);
    };
    if !config.allow_appeal {
        return Err(EscrowVerificationError::AppealNotAllowed);
    }
    if current_height >= escrow.timeout_at {
        return Err(EscrowVerificationError::AppealWindowExpired);
    }
    if payload.appeal_deposit == 0 {
        return Err(EscrowVerificationError::InvalidAmount);
    }
    let required = escrow
        .total_amount
        .checked_mul(u64::from(MIN_APPEAL_DEPOSIT_BPS))
        .and_then(|value| value.checked_div(u64::from(MAX_BPS)))
        .ok_or(EscrowVerificationError::InvalidChallengeDepositBps)?;
    if payload.appeal_deposit < required {
        return Err(EscrowVerificationError::AppealDepositTooLow {
            required,
            provided: payload.appeal_deposit,
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
    if let Some(dispute_id) = escrow.dispute_id.as_ref() {
        if dispute_id != &payload.dispute_id {
            return Err(EscrowVerificationError::InvalidVerdictRound);
        }
    }
    if let Some(dispute_round) = escrow.dispute_round {
        if payload.round <= dispute_round {
            return Err(EscrowVerificationError::InvalidVerdictRound);
        }
    } else if payload.round != 0 {
        return Err(EscrowVerificationError::InvalidVerdictRound);
    }
    let total = payload
        .payer_amount
        .checked_add(payload.payee_amount)
        .ok_or(EscrowVerificationError::InvalidVerdictAmounts)?;
    if total != escrow.amount {
        return Err(EscrowVerificationError::InvalidVerdictAmounts);
    }

    let outcome = derive_dispute_outcome(payload.payer_amount, payload.payee_amount);
    let artifact = VerdictArtifact {
        chain_id,
        escrow_id: payload.escrow_id.clone(),
        dispute_id: payload.dispute_id.clone(),
        round: payload.round,
        outcome,
        payer_amount: payload.payer_amount,
        payee_amount: payload.payee_amount,
        signatures: payload.signatures.clone(),
    };

    verify_verdict_signatures(&artifact, required_threshold, registry)
        .map_err(map_verdict_error)?;
    Ok(())
}

/// Verify submit verdict payload (stateful, includes escrow balance).
pub fn verify_submit_verdict_with_balance<R: ArbiterRegistry>(
    payload: &SubmitVerdictPayload,
    escrow: &EscrowAccount,
    chain_id: u64,
    required_threshold: u8,
    registry: &R,
    escrow_balance: u64,
) -> Result<(), EscrowVerificationError> {
    verify_submit_verdict(payload, escrow, chain_id, required_threshold, registry)?;
    let total = payload
        .payer_amount
        .checked_add(payload.payee_amount)
        .ok_or(EscrowVerificationError::InvalidVerdictAmounts)?;
    if escrow_balance < total {
        return Err(EscrowVerificationError::InsufficientEscrowBalance {
            required: total,
            available: escrow_balance,
        });
    }
    Ok(())
}

fn map_verdict_error(err: VerdictVerificationError) -> EscrowVerificationError {
    match err {
        VerdictVerificationError::InvalidVerdictAmounts
        | VerdictVerificationError::InvalidOutcome => {
            EscrowVerificationError::InvalidVerdictAmounts
        }
        VerdictVerificationError::ThresholdNotMet { required, found } => {
            EscrowVerificationError::ThresholdNotMet { required, found }
        }
        VerdictVerificationError::InvalidSignature => EscrowVerificationError::InvalidSignature,
        VerdictVerificationError::ArbiterNotActive => EscrowVerificationError::ArbiterNotActive,
        VerdictVerificationError::ArbiterStakeTooLow { required, found } => {
            EscrowVerificationError::ArbiterStakeTooLow { required, found }
        }
        VerdictVerificationError::Registry(message) => EscrowVerificationError::Registry(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitration::verdict::{build_verdict_message, DisputeOutcome};
    use crate::crypto::elgamal::KeyPair;
    use crate::crypto::Hash;
    use crate::escrow::{ArbitrationConfig, ArbitrationMode, DisputeInfo};
    use crate::serializer::Serializer;
    use crate::transaction::ArbiterSignature;
    use std::collections::HashSet;

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
            total_amount: 100,
            released_amount: 0,
            refunded_amount: 0,
            pending_release_amount: None,
            challenge_deposit: 0,
            asset: Hash::max(),
            state,
            dispute_id: None,
            dispute_round: None,
            challenge_window: 10,
            challenge_deposit_bps: 500,
            optimistic_release: true,
            release_requested_at: Some(5),
            created_at: 1,
            updated_at: 1,
            timeout_at: 11,
            timeout_blocks: 10,
            arbitration_config: None,
            dispute: None,
            appeal: None,
            resolutions: Vec::new(),
        })
    }

    fn sample_escrow_with_arbitration(
        state: EscrowState,
        allow_appeal: bool,
    ) -> Result<EscrowAccount, Box<dyn std::error::Error>> {
        let mut escrow = sample_escrow(state)?;
        escrow.arbitration_config = Some(ArbitrationConfig {
            mode: ArbitrationMode::Single,
            arbiters: vec![PublicKey::from_bytes(&[4u8; 32])?],
            threshold: None,
            fee_amount: 5,
            allow_appeal,
        });
        Ok(escrow)
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
        let payer = PublicKey::from_bytes(&[9u8; 32])?;
        let err = match verify_create_escrow(&payload, &payer) {
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
    fn refund_requires_timeout_or_payee() -> Result<(), Box<dyn std::error::Error>> {
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
    fn dispute_requires_arbitration_config() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Funded)?;
        let payload = DisputeEscrowPayload {
            escrow_id: escrow.id.clone(),
            reason: "dispute".to_string(),
            evidence_hash: None,
        };
        let err = match verify_dispute_escrow(&payload, &escrow, &escrow.payer) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, EscrowVerificationError::AppealNotAllowed));
        Ok(())
    }

    #[test]
    fn appeal_allows_valid_request() -> Result<(), Box<dyn std::error::Error>> {
        let mut escrow = sample_escrow_with_arbitration(EscrowState::Resolved, true)?;
        escrow.dispute = Some(DisputeInfo {
            initiator: escrow.payer.clone(),
            reason: "dispute".to_string(),
            evidence_hash: None,
            disputed_at: 1,
            deadline: escrow.timeout_at,
        });
        let payload = AppealEscrowPayload {
            escrow_id: escrow.id.clone(),
            reason: "appeal".to_string(),
            new_evidence_hash: None,
            appeal_deposit: 5,
            appeal_mode: crate::transaction::payload::AppealMode::Committee,
        };
        verify_appeal_escrow(&payload, &escrow, &escrow.payee, 5)?;
        Ok(())
    }

    #[test]
    fn dispute_allows_valid_request() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow_with_arbitration(EscrowState::Funded, true)?;
        let payload = DisputeEscrowPayload {
            escrow_id: escrow.id.clone(),
            reason: "dispute".to_string(),
            evidence_hash: None,
        };
        verify_dispute_escrow(&payload, &escrow, &escrow.payer)?;
        Ok(())
    }

    #[test]
    fn appeal_requires_minimum_deposit() -> Result<(), Box<dyn std::error::Error>> {
        let mut escrow = sample_escrow_with_arbitration(EscrowState::Resolved, true)?;
        escrow.total_amount = 1000;
        escrow.dispute = Some(DisputeInfo {
            initiator: escrow.payer.clone(),
            reason: "dispute".to_string(),
            evidence_hash: None,
            disputed_at: 1,
            deadline: escrow.timeout_at,
        });
        let payload = AppealEscrowPayload {
            escrow_id: escrow.id.clone(),
            reason: "appeal".to_string(),
            new_evidence_hash: None,
            appeal_deposit: 1,
            appeal_mode: crate::transaction::payload::AppealMode::Committee,
        };
        let err = match verify_appeal_escrow(&payload, &escrow, &escrow.payer, 5) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            EscrowVerificationError::AppealDepositTooLow { .. }
        ));
        Ok(())
    }

    #[test]
    fn submit_verdict_happy_path() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Challenged)?;
        let keypair = KeyPair::new();
        let arbiter_pubkey = keypair.get_public_key().compress();
        let message = build_verdict_message(
            1,
            &escrow.id,
            &Hash::max(),
            0,
            DisputeOutcome::Split,
            50,
            50,
        );
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

    #[test]
    fn submit_verdict_requires_round_zero_on_first() -> Result<(), Box<dyn std::error::Error>> {
        let mut escrow = sample_escrow(EscrowState::Challenged)?;
        escrow.dispute_round = None;
        let keypair = KeyPair::new();
        let arbiter_pubkey = keypair.get_public_key().compress();
        let message = build_verdict_message(
            1,
            &escrow.id,
            &Hash::max(),
            1,
            DisputeOutcome::Split,
            50,
            50,
        );
        let signature = keypair.sign(&message);

        let payload = SubmitVerdictPayload {
            escrow_id: escrow.id.clone(),
            dispute_id: Hash::max(),
            round: 1,
            payer_amount: 50,
            payee_amount: 50,
            signatures: vec![ArbiterSignature {
                arbiter_pubkey,
                signature,
                timestamp: 1,
            }],
        };

        let mut active = HashSet::new();
        active.insert(payload.signatures[0].arbiter_pubkey.clone());
        let registry = TestRegistry {
            active,
            stake: 1000,
            min_stake: 1,
        };

        let err = match verify_submit_verdict(&payload, &escrow, 1, 1, &registry) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, EscrowVerificationError::InvalidVerdictRound));
        Ok(())
    }

    #[test]
    fn submit_verdict_rejects_inactive_arbiter() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Challenged)?;
        let keypair = KeyPair::new();
        let arbiter_pubkey = keypair.get_public_key().compress();
        let message = build_verdict_message(
            1,
            &escrow.id,
            &Hash::max(),
            0,
            DisputeOutcome::Split,
            50,
            50,
        );
        let signature = keypair.sign(&message);

        let payload = SubmitVerdictPayload {
            escrow_id: escrow.id.clone(),
            dispute_id: Hash::max(),
            round: 0,
            payer_amount: 50,
            payee_amount: 50,
            signatures: vec![ArbiterSignature {
                arbiter_pubkey,
                signature,
                timestamp: 1,
            }],
        };

        let registry = TestRegistry {
            active: HashSet::new(),
            stake: 1000,
            min_stake: 1,
        };

        let err = match verify_submit_verdict(&payload, &escrow, 1, 1, &registry) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, EscrowVerificationError::ArbiterNotActive));
        Ok(())
    }

    #[test]
    fn submit_verdict_rejects_low_stake() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Challenged)?;
        let keypair = KeyPair::new();
        let arbiter_pubkey = keypair.get_public_key().compress();
        let message = build_verdict_message(
            1,
            &escrow.id,
            &Hash::max(),
            0,
            DisputeOutcome::Split,
            50,
            50,
        );
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
            stake: 1,
            min_stake: 10,
        };

        let err = match verify_submit_verdict(&payload, &escrow, 1, 1, &registry) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            EscrowVerificationError::ArbiterStakeTooLow { .. }
        ));
        Ok(())
    }

    #[test]
    fn submit_verdict_rejects_bad_signature() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Challenged)?;
        let keypair = KeyPair::new();
        let arbiter_pubkey = keypair.get_public_key().compress();
        let message = build_verdict_message(
            1,
            &escrow.id,
            &Hash::max(),
            1,
            DisputeOutcome::Split,
            50,
            50,
        );
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

        let err = match verify_submit_verdict(&payload, &escrow, 1, 1, &registry) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(err, EscrowVerificationError::InvalidSignature));
        Ok(())
    }

    #[test]
    fn release_rejects_insufficient_balance() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Funded)?;
        let payload = ReleaseEscrowPayload {
            escrow_id: escrow.id.clone(),
            amount: 50,
            completion_proof: None,
        };
        let err = match verify_release_escrow_with_balance(&payload, &escrow, &escrow.payer, 10) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            EscrowVerificationError::InsufficientEscrowBalance { .. }
        ));
        Ok(())
    }

    #[test]
    fn refund_rejects_insufficient_balance() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Funded)?;
        let payload = RefundEscrowPayload {
            escrow_id: escrow.id.clone(),
            amount: 50,
            reason: None,
        };
        let err = match verify_refund_escrow_with_balance(&payload, &escrow, &escrow.payee, 5, 10) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            EscrowVerificationError::InsufficientEscrowBalance { .. }
        ));
        Ok(())
    }

    #[test]
    fn verdict_rejects_insufficient_balance() -> Result<(), Box<dyn std::error::Error>> {
        let escrow = sample_escrow(EscrowState::Challenged)?;
        let keypair = KeyPair::new();
        let arbiter_pubkey = keypair.get_public_key().compress();
        let message = build_verdict_message(
            1,
            &escrow.id,
            &Hash::max(),
            0,
            DisputeOutcome::Split,
            50,
            50,
        );
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

        let err = match verify_submit_verdict_with_balance(&payload, &escrow, 1, 1, &registry, 10) {
            Ok(_) => return Err("expected error".into()),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            EscrowVerificationError::InsufficientEscrowBalance { .. }
        ));
        Ok(())
    }
}
