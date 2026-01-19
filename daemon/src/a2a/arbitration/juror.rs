use tos_common::arbitration::{
    canonical_hash_without_signature, JurorVote, VoteChoice, VoteRequest,
};
use tos_common::time::get_current_time_in_seconds;

use super::evidence::fetch_evidence;
use super::persistence::{save_juror_case, JurorCase};
use super::replay::ReplayCache;
use super::{ArbitrationError, MAX_CLOCK_DRIFT_SECS};

pub struct JurorService;

impl JurorService {
    pub fn new() -> Self {
        Self
    }

    pub async fn handle_vote_request(
        &self,
        request: VoteRequest,
        vote_choice: VoteChoice,
        juror_pubkey: tos_common::crypto::PublicKey,
        juror_account: String,
        juror_signature: tos_common::crypto::Signature,
    ) -> Result<JurorVote, ArbitrationError> {
        validate_timestamps(request.issued_at, request.vote_deadline)?;
        verify_signed(&request, &request.signature, &request.coordinator_pubkey)?;

        let mut replay = ReplayCache::load("juror")?;
        let now = get_current_time_in_seconds();
        if replay.check_and_insert(&request.request_id.to_hex(), request.vote_deadline, now)? {
            return Err(ArbitrationError::Replay);
        }

        let _artifact = fetch_evidence(&request.evidence_uri, &request.evidence_hash).await?;

        let vote_request_hash = canonical_hash_without_signature(&request, "signature")
            .map_err(|e| ArbitrationError::InvalidMessage(e.to_string()))?;

        let vote = JurorVote {
            message_type: "JurorVote".to_string(),
            version: request.version,
            request_id: request.request_id.clone(),
            chain_id: request.chain_id,
            escrow_id: request.escrow_id.clone(),
            escrow_hash: request.escrow_hash.clone(),
            dispute_id: request.dispute_id.clone(),
            round: request.round,
            dispute_open_height: request.dispute_open_height,
            committee_id: request.committee_id.clone(),
            selection_block: request.selection_block,
            selection_commitment_id: request.selection_commitment_id.clone(),
            arbitration_open_hash: request.arbitration_open_hash.clone(),
            vote_request_hash: vote_request_hash.clone(),
            evidence_hash: request.evidence_hash.clone(),
            evidence_manifest_hash: request.evidence_manifest_hash.clone(),
            selected_jurors_hash: request.selected_jurors_hash.clone(),
            committee_policy_hash: request.committee_policy_hash.clone(),
            juror_pubkey: juror_pubkey.clone(),
            juror_account: juror_account.clone(),
            vote: vote_choice,
            voted_at: now as u64,
            signature: juror_signature.clone(),
        };

        verify_signed(&vote, &juror_signature, &juror_pubkey)?;

        let case = JurorCase {
            request_id: request.request_id.clone(),
            vote_request_hash,
            vote: Some(vote.clone()),
            submitted: false,
            updated_at: now as u64,
        };
        save_juror_case(&case)?;

        Ok(vote)
    }
}

fn validate_timestamps(issued_at: u64, expires_at: u64) -> Result<(), ArbitrationError> {
    let now = get_current_time_in_seconds() as u64;
    if expires_at <= issued_at {
        return Err(ArbitrationError::InvalidMessage("expires_at".to_string()));
    }
    let drift = if now > issued_at {
        now - issued_at
    } else {
        issued_at - now
    };
    if drift > MAX_CLOCK_DRIFT_SECS {
        return Err(ArbitrationError::Expired);
    }
    if now > expires_at {
        return Err(ArbitrationError::Expired);
    }
    Ok(())
}

fn verify_signed<T: serde::Serialize>(
    message: &T,
    signature: &tos_common::crypto::Signature,
    pubkey: &tos_common::crypto::PublicKey,
) -> Result<(), ArbitrationError> {
    let hash = canonical_hash_without_signature(message, "signature")
        .map_err(|e| ArbitrationError::InvalidMessage(e.to_string()))?;
    let decompressed = pubkey
        .decompress()
        .map_err(|_| ArbitrationError::InvalidSignature)?;
    if !signature.verify(hash.as_bytes(), &decompressed) {
        return Err(ArbitrationError::InvalidSignature);
    }
    Ok(())
}
