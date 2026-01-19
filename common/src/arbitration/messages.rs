use serde::{Deserialize, Serialize};

use crate::crypto::{Hash, PublicKey, Signature};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArbitrationOpen {
    #[serde(rename = "type")]
    pub message_type: String,
    pub version: u32,
    pub chain_id: u64,
    pub escrow_id: Hash,
    pub escrow_hash: Hash,
    pub dispute_id: Hash,
    pub round: u32,
    pub dispute_open_height: u64,
    pub committee_id: Hash,
    pub committee_policy_hash: Hash,
    pub payer: String,
    pub payee: String,
    pub evidence_uri: String,
    pub evidence_hash: Hash,
    pub evidence_manifest_uri: String,
    pub evidence_manifest_hash: Hash,
    pub client_nonce: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub coordinator_pubkey: PublicKey,
    pub coordinator_account: String,
    pub request_id: Hash,
    pub opener_pubkey: PublicKey,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteRequest {
    #[serde(rename = "type")]
    pub message_type: String,
    pub version: u32,
    pub request_id: Hash,
    pub chain_id: u64,
    pub escrow_id: Hash,
    pub escrow_hash: Hash,
    pub dispute_id: Hash,
    pub round: u32,
    pub dispute_open_height: u64,
    pub committee_id: Hash,
    pub committee_policy_hash: Hash,
    pub selection_block: u64,
    pub selection_commitment_id: Hash,
    pub arbitration_open_hash: Hash,
    pub issued_at: u64,
    pub vote_deadline: u64,
    pub selected_jurors: Vec<String>,
    pub selected_jurors_hash: Hash,
    pub evidence_hash: Hash,
    pub evidence_manifest_hash: Hash,
    pub evidence_uri: String,
    pub evidence_manifest_uri: String,
    pub coordinator_pubkey: PublicKey,
    pub coordinator_account: String,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JurorVote {
    #[serde(rename = "type")]
    pub message_type: String,
    pub version: u32,
    pub request_id: Hash,
    pub chain_id: u64,
    pub escrow_id: Hash,
    pub escrow_hash: Hash,
    pub dispute_id: Hash,
    pub round: u32,
    pub dispute_open_height: u64,
    pub committee_id: Hash,
    pub selection_block: u64,
    pub selection_commitment_id: Hash,
    pub arbitration_open_hash: Hash,
    pub vote_request_hash: Hash,
    pub evidence_hash: Hash,
    pub evidence_manifest_hash: Hash,
    pub selected_jurors_hash: Hash,
    pub committee_policy_hash: Hash,
    pub juror_pubkey: PublicKey,
    pub juror_account: String,
    pub vote: VoteChoice,
    pub voted_at: u64,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerdictBundle {
    #[serde(rename = "type")]
    pub message_type: String,
    pub version: u32,
    pub chain_id: u64,
    pub escrow_id: Hash,
    pub dispute_id: Hash,
    pub round: u32,
    pub committee_id: Hash,
    pub selection_commitment_id: Hash,
    pub selected_jurors_hash: Hash,
    pub vote_request_hash: Hash,
    pub outcome: VoteChoice,
    pub vote_count: u32,
    pub juror_signatures: Vec<Signature>,
    pub coordinator_pubkey: PublicKey,
    pub coordinator_signature: Signature,
    pub issued_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbortVerdict {
    #[serde(rename = "type")]
    pub message_type: String,
    pub version: u32,
    pub chain_id: u64,
    pub escrow_id: Hash,
    pub dispute_id: Hash,
    pub round: u32,
    pub committee_id: Hash,
    pub selection_commitment_id: Hash,
    pub selected_jurors_hash: Hash,
    pub vote_request_hash: Hash,
    pub reason: String,
    pub issued_at: u64,
    pub coordinator_pubkey: PublicKey,
    pub coordinator_signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VoteChoice {
    Pay,
    Refund,
    Split { payer_bps: u32 },
}
