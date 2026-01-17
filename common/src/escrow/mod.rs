use serde::{Deserialize, Serialize};

use crate::{
    crypto::{Hash, PublicKey},
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Escrow state for on-chain settlement.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EscrowState {
    /// Escrow created, awaiting deposit.
    Created,
    /// Funds deposited, task in progress.
    Funded,
    /// Provider requested release, challenge window active.
    PendingRelease,
    /// Client challenged during window, awaiting arbitration.
    Challenged,
    /// Funds released to provider.
    Released,
    /// Funds refunded to client.
    Refunded,
    /// Resolved by arbiter verdict.
    Resolved,
    /// Escrow expired (timeout).
    Expired,
}

impl Serializer for EscrowState {
    fn write(&self, writer: &mut Writer) {
        let value = match self {
            EscrowState::Created => 0u8,
            EscrowState::Funded => 1u8,
            EscrowState::PendingRelease => 2u8,
            EscrowState::Challenged => 3u8,
            EscrowState::Released => 4u8,
            EscrowState::Refunded => 5u8,
            EscrowState::Resolved => 6u8,
            EscrowState::Expired => 7u8,
        };
        value.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        match value {
            0 => Ok(EscrowState::Created),
            1 => Ok(EscrowState::Funded),
            2 => Ok(EscrowState::PendingRelease),
            3 => Ok(EscrowState::Challenged),
            4 => Ok(EscrowState::Released),
            5 => Ok(EscrowState::Refunded),
            6 => Ok(EscrowState::Resolved),
            7 => Ok(EscrowState::Expired),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

/// Arbitration configuration for an escrow.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArbitrationConfig {
    /// Arbitration mode.
    pub mode: ArbitrationMode,
    /// Arbiter(s) - single arbiter or committee members.
    #[serde(default)]
    pub arbiters: Vec<PublicKey>,
    /// Committee voting threshold (e.g., 2 for 2-of-3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<u8>,
    /// Arbitration fee (deducted from escrow on dispute).
    pub fee_amount: u64,
    /// Allow appeal to higher tier.
    pub allow_appeal: bool,
}

/// Dispute information.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisputeInfo {
    /// Who initiated the dispute.
    pub initiator: PublicKey,
    /// Dispute reason.
    pub reason: String,
    /// Evidence hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_hash: Option<Hash>,
    /// Block height when disputed.
    pub disputed_at: u64,
    /// Dispute deadline (arbiter must resolve by this block).
    pub deadline: u64,
}

impl Serializer for DisputeInfo {
    fn write(&self, writer: &mut Writer) {
        self.initiator.write(writer);
        self.reason.write(writer);
        self.evidence_hash.write(writer);
        self.disputed_at.write(writer);
        self.deadline.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            initiator: PublicKey::read(reader)?,
            reason: String::read(reader)?,
            evidence_hash: Option::read(reader)?,
            disputed_at: u64::read(reader)?,
            deadline: u64::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.initiator.size()
            + self.reason.size()
            + self.evidence_hash.size()
            + self.disputed_at.size()
            + self.deadline.size()
    }
}

/// Committee vote record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitteeVote {
    /// Voter (committee member).
    pub voter: PublicKey,
    /// Voted client amount.
    pub client_amount: u64,
    /// Voted provider amount.
    pub provider_amount: u64,
    /// Vote timestamp (block height).
    pub voted_at: u64,
    /// Justification hash.
    pub justification_hash: Hash,
}

impl Serializer for CommitteeVote {
    fn write(&self, writer: &mut Writer) {
        self.voter.write(writer);
        self.client_amount.write(writer);
        self.provider_amount.write(writer);
        self.voted_at.write(writer);
        self.justification_hash.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            voter: PublicKey::read(reader)?,
            client_amount: u64::read(reader)?,
            provider_amount: u64::read(reader)?,
            voted_at: u64::read(reader)?,
            justification_hash: Hash::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.voter.size()
            + self.client_amount.size()
            + self.provider_amount.size()
            + self.voted_at.size()
            + self.justification_hash.size()
    }
}

/// Appeal information.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppealInfo {
    /// Who initiated the appeal.
    pub appellant: PublicKey,
    /// Appeal reason.
    pub reason: String,
    /// New evidence hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_evidence_hash: Option<Hash>,
    /// Appeal deposit amount.
    pub deposit: u64,
    /// Block height when appealed.
    pub appealed_at: u64,
    /// Appeal deadline.
    pub deadline: u64,
    /// Committee votes collected.
    #[serde(default)]
    pub votes: Vec<CommitteeVote>,
    /// Committee members assigned.
    #[serde(default)]
    pub committee: Vec<PublicKey>,
    /// Required vote threshold.
    pub threshold: u8,
}

impl Serializer for AppealInfo {
    fn write(&self, writer: &mut Writer) {
        self.appellant.write(writer);
        self.reason.write(writer);
        self.new_evidence_hash.write(writer);
        self.deposit.write(writer);
        self.appealed_at.write(writer);
        self.deadline.write(writer);
        self.votes.write(writer);
        self.committee.write(writer);
        self.threshold.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            appellant: PublicKey::read(reader)?,
            reason: String::read(reader)?,
            new_evidence_hash: Option::read(reader)?,
            deposit: u64::read(reader)?,
            appealed_at: u64::read(reader)?,
            deadline: u64::read(reader)?,
            votes: Vec::read(reader)?,
            committee: Vec::read(reader)?,
            threshold: u8::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.appellant.size()
            + self.reason.size()
            + self.new_evidence_hash.size()
            + self.deposit.size()
            + self.appealed_at.size()
            + self.deadline.size()
            + self.votes.size()
            + self.committee.size()
            + self.threshold.size()
    }
}

/// Resolution record for audit trail.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionRecord {
    /// Resolution tier (1 = arbiter, 2 = committee, 3 = DAO).
    pub tier: u8,
    /// Resolver(s).
    #[serde(default)]
    pub resolver: Vec<PublicKey>,
    /// Client amount decided.
    pub client_amount: u64,
    /// Provider amount decided.
    pub provider_amount: u64,
    /// Resolution hash.
    pub resolution_hash: Hash,
    /// Block height when resolved.
    pub resolved_at: u64,
    /// Was this resolution appealed?
    pub appealed: bool,
}

impl Serializer for ResolutionRecord {
    fn write(&self, writer: &mut Writer) {
        self.tier.write(writer);
        self.resolver.write(writer);
        self.client_amount.write(writer);
        self.provider_amount.write(writer);
        self.resolution_hash.write(writer);
        self.resolved_at.write(writer);
        self.appealed.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            tier: u8::read(reader)?,
            resolver: Vec::read(reader)?,
            client_amount: u64::read(reader)?,
            provider_amount: u64::read(reader)?,
            resolution_hash: Hash::read(reader)?,
            resolved_at: u64::read(reader)?,
            appealed: bool::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.tier.size()
            + self.resolver.size()
            + self.client_amount.size()
            + self.provider_amount.size()
            + self.resolution_hash.size()
            + self.resolved_at.size()
            + self.appealed.size()
    }
}

impl Serializer for ArbitrationConfig {
    fn write(&self, writer: &mut Writer) {
        self.mode.write(writer);
        self.arbiters.write(writer);
        self.threshold.write(writer);
        self.fee_amount.write(writer);
        self.allow_appeal.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let mode = ArbitrationMode::read(reader)?;
        let arbiters = Vec::read(reader)?;
        let threshold = Option::read(reader)?;
        let fee_amount = u64::read(reader)?;
        let allow_appeal = bool::read(reader)?;
        Ok(Self {
            mode,
            arbiters,
            threshold,
            fee_amount,
            allow_appeal,
        })
    }

    fn size(&self) -> usize {
        self.mode.size()
            + self.arbiters.size()
            + self.threshold.size()
            + self.fee_amount.size()
            + self.allow_appeal.size()
    }
}

/// Arbitration mode enumeration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ArbitrationMode {
    /// No arbitration - timeout refund only.
    None,
    /// Single designated arbiter.
    Single,
    /// Committee vote (M-of-N threshold).
    Committee,
    /// TOS DAO governance (for high-value disputes).
    DaoGovernance,
}

impl Serializer for ArbitrationMode {
    fn write(&self, writer: &mut Writer) {
        let value = match self {
            ArbitrationMode::None => 0u8,
            ArbitrationMode::Single => 1u8,
            ArbitrationMode::Committee => 2u8,
            ArbitrationMode::DaoGovernance => 3u8,
        };
        value.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        match value {
            0 => Ok(ArbitrationMode::None),
            1 => Ok(ArbitrationMode::Single),
            2 => Ok(ArbitrationMode::Committee),
            3 => Ok(ArbitrationMode::DaoGovernance),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

/// On-chain escrow account state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EscrowAccount {
    /// Escrow ID (hash of CreateEscrow transaction).
    pub id: Hash,
    /// A2A task ID (links to off-chain task).
    pub task_id: String,
    /// Client/payer public key.
    pub payer: PublicKey,
    /// Service provider/payee public key.
    pub payee: PublicKey,
    /// Escrow amount in atomic units.
    pub amount: u64,
    /// Total deposited amount (including challenge deposits).
    pub total_amount: u64,
    /// Amount released to payee.
    pub released_amount: u64,
    /// Amount refunded to payer.
    pub refunded_amount: u64,
    /// Amount currently pending release (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_release_amount: Option<u64>,
    /// Challenge deposit amount locked in escrow (if challenged).
    pub challenge_deposit: u64,
    /// Asset type (TOS native or token).
    pub asset: Hash,
    /// Current escrow state.
    pub state: EscrowState,
    /// Current dispute id (set on verdict submission).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispute_id: Option<Hash>,
    /// Current dispute round (set on verdict submission).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispute_round: Option<u32>,
    /// Challenge window in blocks.
    pub challenge_window: u64,
    /// Challenge deposit percentage (basis points).
    pub challenge_deposit_bps: u16,
    /// Enable optimistic auto-release.
    pub optimistic_release: bool,
    /// Block height when release was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_requested_at: Option<u64>,
    /// Block height when created.
    pub created_at: u64,
    /// Block height when last updated.
    pub updated_at: u64,
    /// Timeout block height.
    pub timeout_at: u64,
    /// Escrow timeout in blocks.
    pub timeout_blocks: u64,
    /// Arbitration configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arbitration_config: Option<ArbitrationConfig>,
    /// Dispute info (if disputed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispute: Option<DisputeInfo>,
    /// Appeal info (if appealed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appeal: Option<AppealInfo>,
    /// Resolution history (for audit trail).
    #[serde(default)]
    pub resolutions: Vec<ResolutionRecord>,
}

impl Serializer for EscrowAccount {
    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.task_id.write(writer);
        self.payer.write(writer);
        self.payee.write(writer);
        self.amount.write(writer);
        self.total_amount.write(writer);
        self.released_amount.write(writer);
        self.refunded_amount.write(writer);
        self.pending_release_amount.write(writer);
        self.challenge_deposit.write(writer);
        self.asset.write(writer);
        self.state.write(writer);
        self.dispute_id.write(writer);
        self.dispute_round.write(writer);
        self.challenge_window.write(writer);
        self.challenge_deposit_bps.write(writer);
        self.optimistic_release.write(writer);
        self.release_requested_at.write(writer);
        self.created_at.write(writer);
        self.updated_at.write(writer);
        self.timeout_at.write(writer);
        self.timeout_blocks.write(writer);
        self.arbitration_config.write(writer);
        self.dispute.write(writer);
        self.appeal.write(writer);
        self.resolutions.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            id: Hash::read(reader)?,
            task_id: String::read(reader)?,
            payer: PublicKey::read(reader)?,
            payee: PublicKey::read(reader)?,
            amount: u64::read(reader)?,
            total_amount: u64::read(reader)?,
            released_amount: u64::read(reader)?,
            refunded_amount: u64::read(reader)?,
            pending_release_amount: Option::read(reader)?,
            challenge_deposit: u64::read(reader)?,
            asset: Hash::read(reader)?,
            state: EscrowState::read(reader)?,
            dispute_id: Option::read(reader)?,
            dispute_round: Option::read(reader)?,
            challenge_window: u64::read(reader)?,
            challenge_deposit_bps: u16::read(reader)?,
            optimistic_release: bool::read(reader)?,
            release_requested_at: Option::read(reader)?,
            created_at: u64::read(reader)?,
            updated_at: u64::read(reader)?,
            timeout_at: u64::read(reader)?,
            timeout_blocks: u64::read(reader)?,
            arbitration_config: Option::read(reader)?,
            dispute: Option::read(reader)?,
            appeal: Option::read(reader)?,
            resolutions: Vec::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.id.size()
            + self.task_id.size()
            + self.payer.size()
            + self.payee.size()
            + self.amount.size()
            + self.total_amount.size()
            + self.released_amount.size()
            + self.refunded_amount.size()
            + self.pending_release_amount.size()
            + self.challenge_deposit.size()
            + self.asset.size()
            + self.state.size()
            + self.dispute_id.size()
            + self.dispute_round.size()
            + self.challenge_window.size()
            + self.challenge_deposit_bps.size()
            + self.optimistic_release.size()
            + self.release_requested_at.size()
            + self.created_at.size()
            + self.updated_at.size()
            + self.timeout_at.size()
            + self.timeout_blocks.size()
            + self.arbitration_config.size()
            + self.dispute.size()
            + self.appeal.size()
            + self.resolutions.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serializer::Serializer;

    #[test]
    fn escrow_state_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let state = EscrowState::PendingRelease;
        let data = serde_json::to_vec(&state)?;
        let decoded: EscrowState = serde_json::from_slice(&data)?;
        assert_eq!(state, decoded);
        Ok(())
    }

    #[test]
    fn escrow_account_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let account = EscrowAccount {
            id: Hash::zero(),
            task_id: "task-1".to_string(),
            payer: PublicKey::from_bytes(&[1u8; 32])?,
            payee: PublicKey::from_bytes(&[2u8; 32])?,
            amount: 1000,
            total_amount: 1000,
            released_amount: 0,
            refunded_amount: 0,
            pending_release_amount: None,
            challenge_deposit: 0,
            asset: Hash::max(),
            state: EscrowState::Created,
            dispute_id: None,
            dispute_round: None,
            challenge_window: 10,
            challenge_deposit_bps: 500,
            optimistic_release: true,
            release_requested_at: None,
            created_at: 1,
            updated_at: 1,
            timeout_at: 101,
            timeout_blocks: 100,
            arbitration_config: Some(ArbitrationConfig {
                mode: ArbitrationMode::Single,
                arbiters: vec![PublicKey::from_bytes(&[3u8; 32])?],
                threshold: None,
                fee_amount: 5,
                allow_appeal: false,
            }),
            dispute: None,
            appeal: None,
            resolutions: Vec::new(),
        };
        let data = serde_json::to_vec(&account)?;
        let decoded: EscrowAccount = serde_json::from_slice(&data)?;
        assert_eq!(account.amount, decoded.amount);
        assert_eq!(account.state, decoded.state);
        assert_eq!(account.challenge_window, decoded.challenge_window);
        Ok(())
    }
}
