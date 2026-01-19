use std::collections::HashMap;

use tokio::time::sleep;
use tos_common::arbitration::verdict::{derive_dispute_outcome, VerdictArtifact};
use tos_common::arbitration::{
    canonical_hash_without_signature, ArbiterStatus, ArbitrationOpen, JurorVote, VerdictBundle,
    VoteChoice, VoteRequest,
};
use tos_common::config::{COIN_VALUE, FEE_PER_KB};
use tos_common::crypto::{Address, AddressType, Hash, KeyPair, PublicKey};
use tos_common::escrow::EscrowAccount;
use tos_common::time::get_current_time_in_seconds;
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::{
    ArbiterSignature, FeeType, Reference, SubmitVerdictPayload, TransactionType, TxVersion,
};

use crate::core::blockchain::Blockchain;
use crate::core::error::BlockchainError;
use crate::core::storage::Storage;

use super::audit::append_event;
use super::keys::coordinator_keypair;
use super::persistence::{load_coordinator_case, save_coordinator_case, CoordinatorCase};
use super::replay::ReplayCache;
use super::{ArbitrationError, COORDINATOR_GRACE_PERIOD, MAX_CLOCK_DRIFT_SECS, MAX_JUROR_COUNT};

const MIN_JUROR_STAKE: u64 = COIN_VALUE * 10;

pub struct CoordinatorService;

impl CoordinatorService {
    pub fn new() -> Self {
        Self
    }

    pub async fn handle_arbitration_open<S: Storage>(
        &self,
        blockchain: &Blockchain<S>,
        open: ArbitrationOpen,
    ) -> Result<VoteRequest, ArbitrationError> {
        let keypair = coordinator_keypair()?;
        ensure_coordinator_key_matches(&open, &keypair)?;
        validate_timestamps(open.issued_at, open.expires_at)?;
        verify_signed(&open, &open.signature, &open.opener_pubkey)?;

        let mut replay = ReplayCache::load("coordinator")?;
        let now = get_current_time_in_seconds();
        if replay.check_and_insert(&open.request_id.to_hex(), open.expires_at, now)? {
            return Err(ArbitrationError::Replay);
        }

        let storage = blockchain.get_storage().read().await;
        let committee = storage
            .get_committee(&open.committee_id)
            .await
            .map_err(|e| ArbitrationError::Storage(e.to_string()))?
            .ok_or(ArbitrationError::CommitteeNotFound)?;
        if committee.status != tos_common::kyc::CommitteeStatus::Active {
            return Err(ArbitrationError::CommitteeInactive);
        }

        let selection_block = blockchain.get_topo_height();
        let (block_hash, _) = storage
            .get_block_header_at_topoheight(selection_block)
            .await
            .map_err(|e| ArbitrationError::Storage(e.to_string()))?;

        let min_jurors = committee.threshold.max(1) as usize;
        let mut members = Vec::new();
        for member in committee.members.iter() {
            if member.status != tos_common::kyc::MemberStatus::Active {
                continue;
            }
            let arbiter = storage
                .get_arbiter(&member.public_key)
                .await
                .map_err(|e| ArbitrationError::Storage(e.to_string()))?;
            if let Some(arbiter) = arbiter {
                if arbiter.status == ArbiterStatus::Active
                    && arbiter.stake_amount >= MIN_JUROR_STAKE
                {
                    members.push(member.public_key.clone());
                }
            }
        }
        members.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));

        if members.len() < min_jurors {
            return Err(ArbitrationError::InsufficientJurors {
                required: min_jurors,
                available: members.len(),
            });
        }

        let selected = select_jurors(&members, min_jurors, &open, selection_block, &block_hash);
        let selected_jurors = selected
            .iter()
            .map(|pk| pubkey_to_address(blockchain, pk))
            .collect::<Vec<_>>();

        let selected_jurors_hash = hash_string_list(&selected_jurors);
        let arbitration_open_hash = canonical_hash_without_signature(&open, "signature")
            .map_err(|e| ArbitrationError::Storage(e.to_string()))?;

        let mut vote_request = VoteRequest {
            message_type: "VoteRequest".to_string(),
            version: open.version,
            request_id: open.request_id.clone(),
            chain_id: open.chain_id,
            escrow_id: open.escrow_id.clone(),
            escrow_hash: open.escrow_hash.clone(),
            dispute_id: open.dispute_id.clone(),
            round: open.round,
            dispute_open_height: open.dispute_open_height,
            committee_id: open.committee_id.clone(),
            committee_policy_hash: open.committee_policy_hash.clone(),
            selection_block,
            selection_commitment_id: selected_jurors_hash.clone(),
            arbitration_open_hash: arbitration_open_hash.clone(),
            issued_at: now as u64,
            vote_deadline: open.expires_at,
            selected_jurors: selected_jurors.clone(),
            selected_jurors_hash: selected_jurors_hash.clone(),
            evidence_hash: open.evidence_hash.clone(),
            evidence_manifest_hash: open.evidence_manifest_hash.clone(),
            evidence_uri: open.evidence_uri.clone(),
            evidence_manifest_uri: open.evidence_manifest_uri.clone(),
            coordinator_pubkey: open.coordinator_pubkey.clone(),
            coordinator_account: open.coordinator_account.clone(),
            signature: open.signature.clone(),
        };
        vote_request.signature = sign_message(&vote_request, "signature", &keypair)?;

        let case = CoordinatorCase {
            request_id: open.request_id.clone(),
            open: open.clone(),
            vote_request: vote_request.clone(),
            votes: Vec::new(),
            verdict: None,
            verdict_submitted: false,
            updated_at: now as u64,
        };
        save_coordinator_case(&case)?;

        let vote_request_hash = canonical_hash_without_signature(&vote_request, "signature")
            .map_err(|e| ArbitrationError::Storage(e.to_string()))?;
        let _ = append_event(
            "arbitration_open",
            &open.request_id,
            &arbitration_open_hash,
            now as u64,
        );
        let _ = append_event(
            "vote_request",
            &open.request_id,
            &vote_request_hash,
            now as u64,
        );

        Ok(vote_request)
    }

    pub async fn handle_juror_vote<S: Storage>(
        &self,
        blockchain: &Blockchain<S>,
        vote: JurorVote,
    ) -> Result<Option<VerdictBundle>, ArbitrationError> {
        let keypair = coordinator_keypair()?;
        validate_timestamps(vote.voted_at, vote.voted_at + COORDINATOR_GRACE_PERIOD)?;
        verify_signed(&vote, &vote.signature, &vote.juror_pubkey)?;

        let mut replay = ReplayCache::load("coordinator_votes")?;
        let now = get_current_time_in_seconds();
        let replay_key = format!("{}:{}", vote.juror_account, vote.request_id.to_hex());
        if replay.check_and_insert(&replay_key, vote.voted_at + COORDINATOR_GRACE_PERIOD, now)? {
            return Ok(None);
        }

        let mut case = load_coordinator_case(&vote.request_id)?
            .ok_or_else(|| ArbitrationError::InvalidMessage("unknown request".to_string()))?;

        if !case
            .vote_request
            .selected_jurors
            .iter()
            .any(|account| account == &vote.juror_account)
        {
            return Err(ArbitrationError::NotSelectedJuror);
        }

        if case
            .votes
            .iter()
            .any(|existing| existing.juror_account == vote.juror_account)
        {
            return Ok(case.verdict.clone());
        }

        case.votes.push(vote.clone());
        case.updated_at = now as u64;
        let vote_hash = canonical_hash_without_signature(&vote, "signature")
            .map_err(|e| ArbitrationError::InvalidMessage(e.to_string()))?;
        let _ = append_event("juror_vote", &vote.request_id, &vote_hash, now as u64);

        let storage = blockchain.get_storage().read().await;
        let committee = storage
            .get_committee(&case.open.committee_id)
            .await
            .map_err(|e| ArbitrationError::Storage(e.to_string()))?
            .ok_or(ArbitrationError::CommitteeNotFound)?;
        let min_jurors = committee.threshold.max(1) as usize;

        if case.votes.len() >= min_jurors {
            let escrow = storage
                .get_escrow(&case.open.escrow_id)
                .await
                .map_err(|e| ArbitrationError::Storage(e.to_string()))?
                .ok_or_else(|| ArbitrationError::InvalidMessage("escrow not found".to_string()))?;
            if case.verdict.is_none() {
                let verdict = build_verdict(&case, &escrow, &keypair)?;
                case.verdict = Some(verdict.clone());
                let verdict_hash =
                    canonical_hash_without_signature(&verdict, "coordinatorSignature")
                        .map_err(|e| ArbitrationError::InvalidMessage(e.to_string()))?;
                let _ = append_event(
                    "verdict_bundle",
                    &vote.request_id,
                    &verdict_hash,
                    now as u64,
                );
            }

            if let Some(verdict) = case.verdict.clone() {
                if !case.verdict_submitted {
                    submit_verdict_on_chain(blockchain, &escrow, &verdict, &keypair).await?;
                    case.verdict_submitted = true;
                }
            }
        }

        save_coordinator_case(&case)?;
        Ok(case.verdict)
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
    pubkey: &PublicKey,
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

fn select_jurors(
    candidates: &[PublicKey],
    min_jurors: usize,
    open: &ArbitrationOpen,
    selection_block: u64,
    block_hash: &Hash,
) -> Vec<PublicKey> {
    let count = min_jurors.min(MAX_JUROR_COUNT).min(candidates.len());
    let mut selected = Vec::with_capacity(count);
    if count == 0 {
        return selected;
    }

    let mut seed_material = Vec::new();
    seed_material.extend_from_slice(&open.chain_id.to_le_bytes());
    seed_material.extend_from_slice(open.escrow_id.as_bytes());
    seed_material.extend_from_slice(open.dispute_id.as_bytes());
    seed_material.extend_from_slice(&open.round.to_le_bytes());
    seed_material.extend_from_slice(block_hash.as_bytes());
    seed_material.extend_from_slice(&selection_block.to_le_bytes());

    let mut i = 0u64;
    while selected.len() < count {
        let mut data = seed_material.clone();
        data.extend_from_slice(&i.to_le_bytes());
        let hash = sha3_hash(&data);
        // hash is [u8; 32], so hash[0..8] is always exactly 8 bytes
        let index = (u64::from_le_bytes(hash[0..8].try_into().unwrap_or_default()) as usize)
            % candidates.len();
        let candidate = candidates[index].clone();
        if !selected.contains(&candidate) {
            selected.push(candidate);
        }
        i = i.wrapping_add(1);
    }

    selected
}

fn sha3_hash(bytes: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hash_string_list(items: &[String]) -> Hash {
    let mut sorted = items.to_vec();
    sorted.sort();
    let payload = serde_json::to_vec(&sorted).unwrap_or_default();
    Hash::new(sha3_hash(&payload))
}

fn pubkey_to_address<S: Storage>(blockchain: &Blockchain<S>, pubkey: &PublicKey) -> String {
    let mainnet = blockchain.get_network().is_mainnet();
    Address::new(mainnet, AddressType::Normal, pubkey.clone()).to_string()
}

fn build_verdict(
    case: &CoordinatorCase,
    escrow: &EscrowAccount,
    coordinator_keypair: &KeyPair,
) -> Result<VerdictBundle, ArbitrationError> {
    let outcome = aggregate_votes(&case.votes);
    let _amounts = compute_amounts(&outcome, escrow.amount);

    let signatures = case
        .votes
        .iter()
        .map(|vote| vote.signature.clone())
        .collect::<Vec<_>>();

    let mut verdict = VerdictBundle {
        message_type: "VerdictBundle".to_string(),
        version: case.open.version,
        chain_id: case.open.chain_id,
        escrow_id: case.open.escrow_id.clone(),
        dispute_id: case.open.dispute_id.clone(),
        round: case.open.round,
        committee_id: case.open.committee_id.clone(),
        selection_commitment_id: case.vote_request.selection_commitment_id.clone(),
        selected_jurors_hash: case.vote_request.selected_jurors_hash.clone(),
        vote_request_hash: canonical_hash_without_signature(&case.vote_request, "signature")
            .map_err(|e| ArbitrationError::InvalidMessage(e.to_string()))?,
        outcome: outcome.clone(),
        vote_count: case.votes.len() as u32,
        juror_signatures: signatures,
        coordinator_pubkey: coordinator_keypair.get_public_key().compress(),
        coordinator_signature: case.open.signature.clone(),
        issued_at: get_current_time_in_seconds() as u64,
    };
    verdict.coordinator_signature =
        sign_message(&verdict, "coordinatorSignature", coordinator_keypair)?;
    Ok(verdict)
}

fn aggregate_votes(votes: &[JurorVote]) -> VoteChoice {
    let mut counts: HashMap<String, (VoteChoice, usize)> = HashMap::new();
    for vote in votes {
        let key = vote_choice_key(&vote.vote);
        let entry = counts.entry(key).or_insert((vote.vote.clone(), 0));
        entry.1 += 1;
    }
    counts
        .values()
        .max_by_key(|(_, count)| *count)
        .map(|(choice, _)| choice.clone())
        .unwrap_or(VoteChoice::Split { payer_bps: 5000 })
}

fn vote_choice_key(choice: &VoteChoice) -> String {
    match choice {
        VoteChoice::Pay => "pay".to_string(),
        VoteChoice::Refund => "refund".to_string(),
        VoteChoice::Split { payer_bps } => format!("split:{payer_bps}"),
    }
}

fn compute_amounts(choice: &VoteChoice, total: u64) -> (u64, u64) {
    match choice {
        VoteChoice::Pay => (0, total),
        VoteChoice::Refund => (total, 0),
        VoteChoice::Split { payer_bps } => {
            let payer_amount = total.saturating_mul(*payer_bps as u64) / 10_000;
            let payee_amount = total.saturating_sub(payer_amount);
            (payer_amount, payee_amount)
        }
    }
}

fn sign_message<T: serde::Serialize>(
    message: &T,
    signature_field: &str,
    keypair: &KeyPair,
) -> Result<tos_common::crypto::Signature, ArbitrationError> {
    let hash = canonical_hash_without_signature(message, signature_field)
        .map_err(|e| ArbitrationError::InvalidMessage(e.to_string()))?;
    Ok(keypair.sign(hash.as_bytes()))
}

fn ensure_coordinator_key_matches(
    open: &ArbitrationOpen,
    keypair: &KeyPair,
) -> Result<(), ArbitrationError> {
    let expected = keypair.get_public_key().compress();
    if open.coordinator_pubkey != expected {
        return Err(ArbitrationError::CoordinatorKeyMismatch);
    }
    Ok(())
}

async fn submit_verdict_on_chain<S: Storage>(
    blockchain: &Blockchain<S>,
    escrow: &EscrowAccount,
    verdict: &VerdictBundle,
    keypair: &KeyPair,
) -> Result<(), ArbitrationError> {
    const MAX_ATTEMPTS: usize = 3;
    const BASE_BACKOFF_MS: u64 = 200;

    for attempt in 0..MAX_ATTEMPTS {
        let (payer_amount, payee_amount) = compute_amounts(&verdict.outcome, escrow.amount);
        let artifact = VerdictArtifact {
            chain_id: verdict.chain_id,
            escrow_id: verdict.escrow_id.clone(),
            dispute_id: verdict.dispute_id.clone(),
            round: verdict.round,
            outcome: derive_dispute_outcome(payer_amount, payee_amount),
            payer_amount,
            payee_amount,
            signatures: Vec::new(),
        };
        let signature = keypair.sign(&artifact.message());
        let arbiter_signature = ArbiterSignature {
            arbiter_pubkey: keypair.get_public_key().compress(),
            signature,
            timestamp: get_current_time_in_seconds() as u64,
        };

        let payload = SubmitVerdictPayload {
            escrow_id: verdict.escrow_id.clone(),
            dispute_id: verdict.dispute_id.clone(),
            round: verdict.round,
            payer_amount,
            payee_amount,
            signatures: vec![arbiter_signature],
        };

        let storage = blockchain.get_storage().read().await;
        let source = keypair.get_public_key().compress();
        let topoheight = blockchain.get_topo_height();
        let (reference_hash, _) = storage
            .get_block_header_at_topoheight(topoheight)
            .await
            .map_err(|e| ArbitrationError::Transaction(e.to_string()))?;
        let reference = Reference {
            hash: reference_hash,
            topoheight,
        };

        let nonce = match storage.get_last_nonce(&source).await {
            Ok((_, versioned)) => versioned.get_nonce().saturating_add(1),
            Err(BlockchainError::NoNonce(_)) => 0,
            Err(err) => return Err(ArbitrationError::Transaction(err.to_string())),
        };

        let chain_id = u8::try_from(blockchain.get_network().chain_id())
            .map_err(|_| ArbitrationError::Transaction("invalid chain id".to_string()))?;
        let tx = UnsignedTransaction::new_with_fee_type(
            TxVersion::T0,
            chain_id,
            source,
            TransactionType::SubmitVerdict(payload),
            FEE_PER_KB,
            FeeType::TOS,
            nonce,
            reference,
        )
        .finalize(keypair);

        match blockchain.add_tx_to_mempool(tx, true).await {
            Ok(()) => return Ok(()),
            Err(BlockchainError::TxAlreadyInMempool(_))
            | Err(BlockchainError::TxAlreadyInBlockchain(_)) => {
                return Ok(());
            }
            Err(err) => {
                if attempt + 1 == MAX_ATTEMPTS || !should_retry_submit(&err) {
                    return Err(ArbitrationError::Transaction(err.to_string()));
                }
                let backoff = BASE_BACKOFF_MS.saturating_mul(1u64 << attempt);
                sleep(std::time::Duration::from_millis(backoff.min(5_000))).await;
            }
        }
    }

    Err(ArbitrationError::Transaction(
        "failed to submit verdict".to_string(),
    ))
}

fn should_retry_submit(err: &BlockchainError) -> bool {
    matches!(
        err,
        BlockchainError::IsSyncing
            | BlockchainError::InvalidReferenceTopoheight(_, _)
            | BlockchainError::InvalidReferenceHash
            | BlockchainError::NoStableReferenceFound
    )
}
