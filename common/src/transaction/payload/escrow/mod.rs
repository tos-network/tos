use serde::{Deserialize, Serialize};

use crate::{
    crypto::{Hash, PublicKey, Signature},
    escrow::ArbitrationConfig,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Create escrow for A2A task payment (with optimistic settlement).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreateEscrowPayload {
    /// A2A task ID (links to off-chain task).
    pub task_id: String,
    /// Service provider's public key.
    pub provider: PublicKey,
    /// Escrow amount in atomic units.
    pub amount: u64,
    /// Asset type (TOS native or token).
    pub asset: Hash,
    /// Timeout in blocks (escrow expires if not completed).
    pub timeout_blocks: u64,
    /// Challenge window in blocks.
    pub challenge_window: u64,
    /// Challenge deposit percentage (basis points).
    pub challenge_deposit_bps: u16,
    /// Enable optimistic auto-release.
    pub optimistic_release: bool,
    /// Arbitration configuration (optional).
    pub arbitration_config: Option<ArbitrationConfig>,
    /// Optional metadata (e.g., task description hash).
    pub metadata: Option<Vec<u8>>,
}

impl Serializer for CreateEscrowPayload {
    fn write(&self, writer: &mut Writer) {
        self.task_id.write(writer);
        self.provider.write(writer);
        self.amount.write(writer);
        self.asset.write(writer);
        self.timeout_blocks.write(writer);
        self.challenge_window.write(writer);
        self.challenge_deposit_bps.write(writer);
        self.optimistic_release.write(writer);
        self.arbitration_config.write(writer);
        self.metadata.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let task_id = String::read(reader)?;
        let provider = PublicKey::read(reader)?;
        let amount = u64::read(reader)?;
        let asset = Hash::read(reader)?;
        let timeout_blocks = u64::read(reader)?;
        let challenge_window = u64::read(reader)?;
        let challenge_deposit_bps = u16::read(reader)?;
        let optimistic_release = bool::read(reader)?;
        let arbitration_config = Option::read(reader)?;
        let metadata = Option::read(reader)?;
        Ok(Self {
            task_id,
            provider,
            amount,
            asset,
            timeout_blocks,
            challenge_window,
            challenge_deposit_bps,
            optimistic_release,
            arbitration_config,
            metadata,
        })
    }

    fn size(&self) -> usize {
        self.task_id.size()
            + self.provider.size()
            + self.amount.size()
            + self.asset.size()
            + self.timeout_blocks.size()
            + self.challenge_window.size()
            + self.challenge_deposit_bps.size()
            + self.optimistic_release.size()
            + self.arbitration_config.size()
            + self.metadata.size()
    }
}

/// Deposit additional funds to existing escrow.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DepositEscrowPayload {
    /// Escrow ID.
    pub escrow_id: Hash,
    /// Additional amount to deposit.
    pub amount: u64,
}

impl Serializer for DepositEscrowPayload {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.amount.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let amount = u64::read(reader)?;
        Ok(Self { escrow_id, amount })
    }

    fn size(&self) -> usize {
        self.escrow_id.size() + self.amount.size()
    }
}

/// Release escrow funds to service provider.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReleaseEscrowPayload {
    /// Escrow ID.
    pub escrow_id: Hash,
    /// Amount to release (supports partial release).
    pub amount: u64,
    /// Optional completion proof hash.
    pub completion_proof: Option<Hash>,
}

impl Serializer for ReleaseEscrowPayload {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.amount.write(writer);
        self.completion_proof.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let amount = u64::read(reader)?;
        let completion_proof = Option::read(reader)?;
        Ok(Self {
            escrow_id,
            amount,
            completion_proof,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size() + self.amount.size() + self.completion_proof.size()
    }
}

/// Refund escrow funds to client.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RefundEscrowPayload {
    /// Escrow ID.
    pub escrow_id: Hash,
    /// Amount to refund (supports partial refund).
    pub amount: u64,
    /// Reason for refund.
    pub reason: Option<String>,
}

impl Serializer for RefundEscrowPayload {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.amount.write(writer);
        self.reason.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let amount = u64::read(reader)?;
        let reason = Option::read(reader)?;
        Ok(Self {
            escrow_id,
            amount,
            reason,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size() + self.amount.size() + self.reason.size()
    }
}

/// Challenge escrow during optimistic window.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChallengeEscrowPayload {
    /// Escrow ID.
    pub escrow_id: Hash,
    /// Challenge reason.
    pub reason: String,
    /// Evidence hash (off-chain evidence).
    pub evidence_hash: Option<Hash>,
    /// Challenge deposit amount.
    pub deposit: u64,
}

impl Serializer for ChallengeEscrowPayload {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.reason.write(writer);
        self.evidence_hash.write(writer);
        self.deposit.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let reason = String::read(reader)?;
        let evidence_hash = Option::read(reader)?;
        let deposit = u64::read(reader)?;
        Ok(Self {
            escrow_id,
            reason,
            evidence_hash,
            deposit,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size() + self.reason.size() + self.evidence_hash.size() + self.deposit.size()
    }
}

/// Initiate dispute on escrow.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DisputeEscrowPayload {
    /// Escrow ID.
    pub escrow_id: Hash,
    /// Dispute reason.
    pub reason: String,
    /// Evidence hash (off-chain evidence).
    pub evidence_hash: Option<Hash>,
}

impl Serializer for DisputeEscrowPayload {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.reason.write(writer);
        self.evidence_hash.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let reason = String::read(reader)?;
        let evidence_hash = Option::read(reader)?;
        Ok(Self {
            escrow_id,
            reason,
            evidence_hash,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size() + self.reason.size() + self.evidence_hash.size()
    }
}

/// Appeal mode for disputes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppealMode {
    /// Escalate to committee (M-of-N).
    Committee,
    /// Escalate to DAO governance (high-value cases).
    DaoGovernance,
}

impl Serializer for AppealMode {
    fn write(&self, writer: &mut Writer) {
        let value = match self {
            AppealMode::Committee => 0u8,
            AppealMode::DaoGovernance => 1u8,
        };
        value.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        match value {
            0 => Ok(AppealMode::Committee),
            1 => Ok(AppealMode::DaoGovernance),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

/// Appeal a resolved dispute.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppealEscrowPayload {
    /// Escrow ID.
    pub escrow_id: Hash,
    /// Appeal reason.
    pub reason: String,
    /// New evidence hash (optional).
    pub new_evidence_hash: Option<Hash>,
    /// Appeal deposit amount.
    pub appeal_deposit: u64,
    /// Preferred appeal mode.
    pub appeal_mode: AppealMode,
}

impl Serializer for AppealEscrowPayload {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.reason.write(writer);
        self.new_evidence_hash.write(writer);
        self.appeal_deposit.write(writer);
        self.appeal_mode.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let reason = String::read(reader)?;
        let new_evidence_hash = Option::read(reader)?;
        let appeal_deposit = u64::read(reader)?;
        let appeal_mode = AppealMode::read(reader)?;
        Ok(Self {
            escrow_id,
            reason,
            new_evidence_hash,
            appeal_deposit,
            appeal_mode,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size()
            + self.reason.size()
            + self.new_evidence_hash.size()
            + self.appeal_deposit.size()
            + self.appeal_mode.size()
    }
}

/// Submit arbitration verdict with threshold signatures.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubmitVerdictPayload {
    /// Escrow ID.
    pub escrow_id: Hash,
    /// Dispute ID (unique identifier for this arbitration round).
    pub dispute_id: Hash,
    /// Appeal round (0 = initial, 1+ = appeal).
    pub round: u32,
    /// Amount to return to client.
    pub payer_amount: u64,
    /// Amount to pay to provider.
    pub payee_amount: u64,
    /// Arbiter signatures (threshold required).
    pub signatures: Vec<ArbiterSignature>,
}

impl Serializer for SubmitVerdictPayload {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.dispute_id.write(writer);
        self.round.write(writer);
        self.payer_amount.write(writer);
        self.payee_amount.write(writer);
        self.signatures.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let dispute_id = Hash::read(reader)?;
        let round = u32::read(reader)?;
        let payer_amount = u64::read(reader)?;
        let payee_amount = u64::read(reader)?;
        let signatures = Vec::read(reader)?;
        Ok(Self {
            escrow_id,
            dispute_id,
            round,
            payer_amount,
            payee_amount,
            signatures,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size()
            + self.dispute_id.size()
            + self.round.size()
            + self.payer_amount.size()
            + self.payee_amount.size()
            + self.signatures.size()
    }
}

/// Arbiter signature for verdict.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArbiterSignature {
    /// Arbiter's public key (must be registered).
    pub arbiter_pubkey: PublicKey,
    /// Signature over verdict message.
    pub signature: Signature,
    /// Signature timestamp.
    pub timestamp: u64,
}

impl Serializer for ArbiterSignature {
    fn write(&self, writer: &mut Writer) {
        self.arbiter_pubkey.write(writer);
        self.signature.write(writer);
        self.timestamp.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let arbiter_pubkey = PublicKey::read(reader)?;
        let signature = Signature::read(reader)?;
        let timestamp = u64::read(reader)?;
        Ok(Self {
            arbiter_pubkey,
            signature,
            timestamp,
        })
    }

    fn size(&self) -> usize {
        self.arbiter_pubkey.size() + self.signature.size() + self.timestamp.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_escrow_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let payload = CreateEscrowPayload {
            task_id: "task-1".to_string(),
            provider: PublicKey::from_bytes(&[1u8; 32])?,
            amount: 1000,
            asset: Hash::max(),
            timeout_blocks: 100,
            challenge_window: 10,
            challenge_deposit_bps: 500,
            optimistic_release: true,
            arbitration_config: Some(ArbitrationConfig {
                mode: crate::escrow::ArbitrationMode::Single,
                arbiters: vec![PublicKey::from_bytes(&[2u8; 32])?],
                threshold: None,
                fee_amount: 10,
                allow_appeal: false,
            }),
            metadata: Some(vec![1, 2, 3]),
        };

        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        payload.write(&mut writer);
        let mut reader = Reader::new(&bytes);
        let decoded = CreateEscrowPayload::read(&mut reader)?;
        assert_eq!(payload.task_id, decoded.task_id);
        assert_eq!(payload.amount, decoded.amount);
        assert_eq!(payload.challenge_window, decoded.challenge_window);
        Ok(())
    }

    #[test]
    fn submit_verdict_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let payload = SubmitVerdictPayload {
            escrow_id: Hash::zero(),
            dispute_id: Hash::max(),
            round: 0,
            payer_amount: 50,
            payee_amount: 50,
            signatures: vec![ArbiterSignature {
                arbiter_pubkey: PublicKey::from_bytes(&[3u8; 32])?,
                signature: Signature::from_bytes(&[4u8; 64])?,
                timestamp: 42,
            }],
        };

        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        payload.write(&mut writer);
        let mut reader = Reader::new(&bytes);
        let decoded = SubmitVerdictPayload::read(&mut reader)?;
        assert_eq!(payload.round, decoded.round);
        assert_eq!(payload.signatures.len(), decoded.signatures.len());
        Ok(())
    }
}
