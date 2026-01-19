// Arbitration transaction verification module.

use crate::{
    arbitration::{
        canonical_hash_from_bytes, canonical_hash_without_signature, ArbiterStatus,
        ArbitrationOpen, JurorVote, VoteRequest,
    },
    config::{
        MAX_ARBITRATION_OPEN_BYTES, MAX_JUROR_VOTE_BYTES, MAX_SELECTION_COMMITMENT_BYTES,
        MAX_VOTE_REQUEST_BYTES, MIN_ARBITER_STAKE,
    },
    crypto::PublicKey,
    transaction::payload::{
        CommitArbitrationOpenPayload, CommitJurorVotePayload, CommitSelectionCommitmentPayload,
        CommitVoteRequestPayload, RegisterArbiterPayload, SlashArbiterPayload,
        UpdateArbiterPayload,
    },
};

use super::VerificationError;

/// Maximum arbiter display name length.
pub const MAX_ARBITER_NAME_LEN: usize = 128;

/// Maximum fee basis points.
pub const MAX_FEE_BPS: u16 = 10_000;

/// Verify RegisterArbiter payload.
pub fn verify_register_arbiter<E>(
    payload: &RegisterArbiterPayload,
) -> Result<(), VerificationError<E>> {
    let name_len = payload.get_name().len();
    if name_len == 0 || name_len > MAX_ARBITER_NAME_LEN {
        return Err(VerificationError::ArbiterNameLength {
            len: name_len,
            max: MAX_ARBITER_NAME_LEN,
        });
    }

    if payload.get_fee_basis_points() > MAX_FEE_BPS {
        return Err(VerificationError::ArbiterInvalidFee(
            payload.get_fee_basis_points(),
        ));
    }

    let stake_amount = payload.get_stake_amount();
    if stake_amount < MIN_ARBITER_STAKE {
        return Err(VerificationError::ArbiterStakeTooLow {
            required: MIN_ARBITER_STAKE,
            found: stake_amount,
        });
    }

    let min_value = payload.get_min_escrow_value();
    let max_value = payload.get_max_escrow_value();
    if min_value > max_value {
        return Err(VerificationError::ArbiterEscrowRangeInvalid {
            min: min_value,
            max: max_value,
        });
    }

    Ok(())
}

/// Verify UpdateArbiter payload.
pub fn verify_update_arbiter<E>(
    payload: &UpdateArbiterPayload,
) -> Result<(), VerificationError<E>> {
    if let Some(name) = payload.get_name() {
        let name_len = name.len();
        if name_len == 0 || name_len > MAX_ARBITER_NAME_LEN {
            return Err(VerificationError::ArbiterNameLength {
                len: name_len,
                max: MAX_ARBITER_NAME_LEN,
            });
        }
    }

    if let Some(fee) = payload.get_fee_basis_points() {
        if fee > MAX_FEE_BPS {
            return Err(VerificationError::ArbiterInvalidFee(fee));
        }
    }

    if let (Some(min_value), Some(max_value)) = (
        payload.get_min_escrow_value(),
        payload.get_max_escrow_value(),
    ) {
        if min_value > max_value {
            return Err(VerificationError::ArbiterEscrowRangeInvalid {
                min: min_value,
                max: max_value,
            });
        }
    }

    if let Some(status) = payload.get_status() {
        if status != ArbiterStatus::Suspended {
            return Err(VerificationError::ArbiterInvalidStatus);
        }
    }

    if payload.is_deactivate() {
        if let Some(add_stake) = payload.get_add_stake() {
            if add_stake > 0 {
                return Err(VerificationError::ArbiterDeactivateWithStake);
            }
        }
    }

    Ok(())
}

/// Verify SlashArbiter payload.
pub fn verify_slash_arbiter<E>(payload: &SlashArbiterPayload) -> Result<(), VerificationError<E>> {
    if payload.get_amount() == 0 {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Slash amount must be greater than 0"
        )));
    }

    if payload.get_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "SlashArbiter requires at least one approval"
        )));
    }

    if payload.get_reason_hash() == &crate::crypto::Hash::zero() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "SlashArbiter reason hash cannot be empty"
        )));
    }

    Ok(())
}

pub fn verify_commit_arbitration_open<E>(
    payload: &CommitArbitrationOpenPayload,
    signer: &PublicKey,
) -> Result<ArbitrationOpen, VerificationError<E>> {
    if payload.arbitration_open_payload.len() > MAX_ARBITRATION_OPEN_BYTES {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "arbitration_open_payload too large"
        )));
    }

    let open: ArbitrationOpen = serde_json::from_slice(&payload.arbitration_open_payload)
        .map_err(|_| VerificationError::InvalidFormat)?;

    if open.escrow_id != payload.escrow_id
        || open.dispute_id != payload.dispute_id
        || open.round != payload.round
        || open.request_id != payload.request_id
    {
        return Err(VerificationError::InvalidFormat);
    }

    if open.signature != payload.opener_signature {
        return Err(VerificationError::InvalidSignature);
    }

    let hash = canonical_hash_without_signature(&open, "signature")
        .map_err(|e| VerificationError::AnyError(anyhow::anyhow!(e)))?;
    if hash != payload.arbitration_open_hash {
        return Err(VerificationError::InvalidFormat);
    }

    if &open.coordinator_pubkey != signer {
        return Err(VerificationError::InvalidSignature);
    }

    let opener_pubkey = open
        .opener_pubkey
        .decompress()
        .map_err(|_| VerificationError::InvalidFormat)?;
    if !open.signature.verify(hash.as_bytes(), &opener_pubkey) {
        return Err(VerificationError::InvalidSignature);
    }

    Ok(open)
}

pub fn verify_commit_vote_request<E>(
    payload: &CommitVoteRequestPayload,
    signer: &PublicKey,
) -> Result<VoteRequest, VerificationError<E>> {
    if payload.vote_request_payload.len() > MAX_VOTE_REQUEST_BYTES {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "vote_request_payload too large"
        )));
    }

    let request: VoteRequest = serde_json::from_slice(&payload.vote_request_payload)
        .map_err(|_| VerificationError::InvalidFormat)?;

    if request.request_id != payload.request_id {
        return Err(VerificationError::InvalidFormat);
    }

    if request.signature != payload.coordinator_signature {
        return Err(VerificationError::InvalidSignature);
    }

    let hash = canonical_hash_without_signature(&request, "signature")
        .map_err(|e| VerificationError::AnyError(anyhow::anyhow!(e)))?;
    if hash != payload.vote_request_hash {
        return Err(VerificationError::InvalidFormat);
    }

    if &request.coordinator_pubkey != signer {
        return Err(VerificationError::InvalidSignature);
    }

    let coordinator_pubkey = request
        .coordinator_pubkey
        .decompress()
        .map_err(|_| VerificationError::InvalidFormat)?;
    if !request
        .signature
        .verify(hash.as_bytes(), &coordinator_pubkey)
    {
        return Err(VerificationError::InvalidSignature);
    }

    Ok(request)
}

pub fn verify_commit_selection_commitment<E>(
    payload: &CommitSelectionCommitmentPayload,
) -> Result<(), VerificationError<E>> {
    if payload.selection_commitment_payload.len() > MAX_SELECTION_COMMITMENT_BYTES {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "selection_commitment_payload too large"
        )));
    }

    let hash = canonical_hash_from_bytes(&payload.selection_commitment_payload);
    if hash != payload.selection_commitment_id {
        return Err(VerificationError::InvalidFormat);
    }

    Ok(())
}

pub fn verify_commit_juror_vote<E>(
    payload: &CommitJurorVotePayload,
) -> Result<JurorVote, VerificationError<E>> {
    if payload.vote_payload.len() > MAX_JUROR_VOTE_BYTES {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "vote_payload too large"
        )));
    }

    let vote: JurorVote = serde_json::from_slice(&payload.vote_payload)
        .map_err(|_| VerificationError::InvalidFormat)?;

    if vote.request_id != payload.request_id || vote.juror_pubkey != payload.juror_pubkey {
        return Err(VerificationError::InvalidFormat);
    }

    if vote.signature != payload.juror_signature {
        return Err(VerificationError::InvalidSignature);
    }

    let hash = canonical_hash_without_signature(&vote, "signature")
        .map_err(|e| VerificationError::AnyError(anyhow::anyhow!(e)))?;
    if hash != payload.vote_hash {
        return Err(VerificationError::InvalidFormat);
    }

    let juror_pubkey = vote
        .juror_pubkey
        .decompress()
        .map_err(|_| VerificationError::InvalidFormat)?;
    if !vote.signature.verify(hash.as_bytes(), &juror_pubkey) {
        return Err(VerificationError::InvalidSignature);
    }

    Ok(vote)
}
