use serde::{Deserialize, Serialize};

use crate::{
    crypto::PublicKey,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Arbiter account status.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ArbiterStatus {
    /// Arbiter is active and can accept disputes.
    Active,
    /// Arbiter is temporarily suspended.
    Suspended,
    /// Arbiter is exiting and in cooldown before withdrawal.
    Exiting,
    /// Arbiter has been removed and is no longer eligible.
    Removed,
}

impl Serializer for ArbiterStatus {
    fn write(&self, writer: &mut Writer) {
        let value = match self {
            ArbiterStatus::Active => 0u8,
            ArbiterStatus::Suspended => 1u8,
            ArbiterStatus::Exiting => 2u8,
            ArbiterStatus::Removed => 3u8,
        };
        value.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        match value {
            0 => Ok(ArbiterStatus::Active),
            1 => Ok(ArbiterStatus::Suspended),
            2 => Ok(ArbiterStatus::Exiting),
            3 => Ok(ArbiterStatus::Removed),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

/// Arbitration expertise domains.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExpertiseDomain {
    /// General arbitration (default).
    General,
    /// AI agent disputes.
    AIAgent,
    /// Smart contract disputes.
    SmartContract,
    /// Payments and billing.
    Payment,
    /// DeFi protocols and markets.
    DeFi,
    /// Governance-related disputes.
    Governance,
    /// Identity and reputation.
    Identity,
    /// Data delivery or verification.
    Data,
    /// Security and abuse handling.
    Security,
    /// Gaming or digital goods.
    Gaming,
    /// Data service disputes.
    DataService,
    /// Digital asset disputes (NFTs, tokens).
    DigitalAsset,
    /// Cross-chain disputes.
    CrossChain,
    /// NFT-specific disputes.
    Nft,
}

impl ExpertiseDomain {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExpertiseDomain::General => "general",
            ExpertiseDomain::AIAgent => "ai-agent",
            ExpertiseDomain::SmartContract => "smart-contract",
            ExpertiseDomain::Payment => "payment",
            ExpertiseDomain::DeFi => "defi",
            ExpertiseDomain::Governance => "governance",
            ExpertiseDomain::Identity => "identity",
            ExpertiseDomain::Data => "data",
            ExpertiseDomain::Security => "security",
            ExpertiseDomain::Gaming => "gaming",
            ExpertiseDomain::DataService => "data-service",
            ExpertiseDomain::DigitalAsset => "digital-asset",
            ExpertiseDomain::CrossChain => "cross-chain",
            ExpertiseDomain::Nft => "nft",
        }
    }

    pub fn skill_tag(&self) -> &'static str {
        match self {
            ExpertiseDomain::General => "arbitration:general",
            ExpertiseDomain::AIAgent => "arbitration:ai-agent",
            ExpertiseDomain::SmartContract => "arbitration:smart-contract",
            ExpertiseDomain::Payment => "arbitration:payment",
            ExpertiseDomain::DeFi => "arbitration:defi",
            ExpertiseDomain::Governance => "arbitration:governance",
            ExpertiseDomain::Identity => "arbitration:identity",
            ExpertiseDomain::Data => "arbitration:data",
            ExpertiseDomain::Security => "arbitration:security",
            ExpertiseDomain::Gaming => "arbitration:gaming",
            ExpertiseDomain::DataService => "arbitration:data-service",
            ExpertiseDomain::DigitalAsset => "arbitration:digital-asset",
            ExpertiseDomain::CrossChain => "arbitration:cross-chain",
            ExpertiseDomain::Nft => "arbitration:nft",
        }
    }
}

pub fn expertise_domains_to_skill_tags(domains: &[ExpertiseDomain]) -> Vec<&'static str> {
    domains.iter().map(ExpertiseDomain::skill_tag).collect()
}

impl Serializer for ExpertiseDomain {
    fn write(&self, writer: &mut Writer) {
        let value = match self {
            ExpertiseDomain::General => 0u8,
            ExpertiseDomain::AIAgent => 1u8,
            ExpertiseDomain::SmartContract => 2u8,
            ExpertiseDomain::Payment => 3u8,
            ExpertiseDomain::DeFi => 4u8,
            ExpertiseDomain::Governance => 5u8,
            ExpertiseDomain::Identity => 6u8,
            ExpertiseDomain::Data => 7u8,
            ExpertiseDomain::Security => 8u8,
            ExpertiseDomain::Gaming => 9u8,
            ExpertiseDomain::DataService => 10u8,
            ExpertiseDomain::DigitalAsset => 11u8,
            ExpertiseDomain::CrossChain => 12u8,
            ExpertiseDomain::Nft => 13u8,
        };
        value.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        match value {
            0 => Ok(ExpertiseDomain::General),
            1 => Ok(ExpertiseDomain::AIAgent),
            2 => Ok(ExpertiseDomain::SmartContract),
            3 => Ok(ExpertiseDomain::Payment),
            4 => Ok(ExpertiseDomain::DeFi),
            5 => Ok(ExpertiseDomain::Governance),
            6 => Ok(ExpertiseDomain::Identity),
            7 => Ok(ExpertiseDomain::Data),
            8 => Ok(ExpertiseDomain::Security),
            9 => Ok(ExpertiseDomain::Gaming),
            10 => Ok(ExpertiseDomain::DataService),
            11 => Ok(ExpertiseDomain::DigitalAsset),
            12 => Ok(ExpertiseDomain::CrossChain),
            13 => Ok(ExpertiseDomain::Nft),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

/// On-chain arbiter account state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArbiterAccount {
    /// Arbiter public key.
    pub public_key: PublicKey,
    /// Human-readable arbiter name.
    pub name: String,
    /// Current arbiter status.
    pub status: ArbiterStatus,
    /// Expertise domains.
    pub expertise: Vec<ExpertiseDomain>,
    /// Amount of stake locked in the arbiter account.
    pub stake_amount: u64,
    /// Arbitration fee (basis points).
    pub fee_basis_points: u16,
    /// Minimum escrow value this arbiter will accept.
    pub min_escrow_value: u64,
    /// Maximum escrow value this arbiter will accept.
    pub max_escrow_value: u64,
    /// Reputation score (0-10000).
    pub reputation_score: u16,
    /// Total number of cases handled.
    pub total_cases: u64,
    /// Number of cases overturned on appeal.
    pub cases_overturned: u64,
    /// Block height when registered.
    pub registered_at: u64,
    /// Block height when last active.
    pub last_active_at: u64,
    /// Amount pending withdrawal (after cooldown).
    pub pending_withdrawal: u64,
    /// Topoheight when deactivation was requested.
    pub deactivated_at: Option<u64>,
    /// Number of active cases currently assigned.
    pub active_cases: u64,
    /// Total slashed amount (cumulative).
    pub total_slashed: u64,
    /// Number of slashes applied.
    pub slash_count: u32,
}

impl Serializer for ArbiterAccount {
    fn write(&self, writer: &mut Writer) {
        self.public_key.write(writer);
        self.name.write(writer);
        self.status.write(writer);
        self.expertise.write(writer);
        self.stake_amount.write(writer);
        self.fee_basis_points.write(writer);
        self.min_escrow_value.write(writer);
        self.max_escrow_value.write(writer);
        self.reputation_score.write(writer);
        self.total_cases.write(writer);
        self.cases_overturned.write(writer);
        self.registered_at.write(writer);
        self.last_active_at.write(writer);
        self.pending_withdrawal.write(writer);
        self.deactivated_at.write(writer);
        self.active_cases.write(writer);
        self.total_slashed.write(writer);
        self.slash_count.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            public_key: PublicKey::read(reader)?,
            name: String::read(reader)?,
            status: ArbiterStatus::read(reader)?,
            expertise: Vec::read(reader)?,
            stake_amount: u64::read(reader)?,
            fee_basis_points: u16::read(reader)?,
            min_escrow_value: u64::read(reader)?,
            max_escrow_value: u64::read(reader)?,
            reputation_score: u16::read(reader)?,
            total_cases: u64::read(reader)?,
            cases_overturned: u64::read(reader)?,
            registered_at: u64::read(reader)?,
            last_active_at: u64::read(reader)?,
            pending_withdrawal: u64::read(reader)?,
            deactivated_at: Option::read(reader)?,
            active_cases: u64::read(reader)?,
            total_slashed: u64::read(reader)?,
            slash_count: u32::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.public_key.size()
            + self.name.size()
            + self.status.size()
            + self.expertise.size()
            + self.stake_amount.size()
            + self.fee_basis_points.size()
            + self.min_escrow_value.size()
            + self.max_escrow_value.size()
            + self.reputation_score.size()
            + self.total_cases.size()
            + self.cases_overturned.size()
            + self.registered_at.size()
            + self.last_active_at.size()
            + self.pending_withdrawal.size()
            + self.deactivated_at.size()
            + self.active_cases.size()
            + self.total_slashed.size()
            + self.slash_count.size()
    }
}

/// Cooldown period after deactivation (in topoheight, ~14 days at 15s blocks).
pub const ARBITER_COOLDOWN_TOPOHEIGHT: u64 = 14 * 24 * 60 * 4;
/// Minimum time between deactivation request and withdrawal.
pub const MIN_COOLDOWN_TOPOHEIGHT: u64 = 24 * 60 * 4;
/// Grace period after case assignment to complete (in topoheight).
pub const CASE_COMPLETION_GRACE_TOPOHEIGHT: u64 = 30 * 24 * 60 * 4;
/// Maximum withdrawal per transaction (0 = unlimited).
pub const MAX_WITHDRAWAL_PER_TX: u64 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArbiterWithdrawError {
    NotActive,
    NoStakeToWithdraw,
    NotInExitProcess,
    CooldownNotComplete { current: u64, required: u64 },
    HasActiveCases { count: u64 },
    ArbiterAlreadyRemoved,
}

impl ArbiterAccount {
    pub fn can_request_exit(&self) -> Result<(), ArbiterWithdrawError> {
        if self.status != ArbiterStatus::Active {
            return Err(ArbiterWithdrawError::NotActive);
        }
        if self.stake_amount == 0 {
            return Err(ArbiterWithdrawError::NoStakeToWithdraw);
        }
        Ok(())
    }

    pub fn can_withdraw(&self, current_topoheight: u64) -> Result<u64, ArbiterWithdrawError> {
        if self.status != ArbiterStatus::Exiting {
            return Err(ArbiterWithdrawError::NotInExitProcess);
        }
        let deactivated_at = self
            .deactivated_at
            .ok_or(ArbiterWithdrawError::NotInExitProcess)?;
        let cooldown_end = deactivated_at
            .checked_add(ARBITER_COOLDOWN_TOPOHEIGHT)
            .ok_or(ArbiterWithdrawError::CooldownNotComplete {
                current: current_topoheight,
                required: u64::MAX,
            })?;
        if current_topoheight < cooldown_end {
            return Err(ArbiterWithdrawError::CooldownNotComplete {
                current: current_topoheight,
                required: cooldown_end,
            });
        }
        if self.active_cases > 0 {
            return Err(ArbiterWithdrawError::HasActiveCases {
                count: self.active_cases,
            });
        }
        Ok(self.stake_amount)
    }

    pub fn apply_slash(&mut self, slash_amount: u64) -> Result<u64, ArbiterWithdrawError> {
        if self.status == ArbiterStatus::Removed {
            return Err(ArbiterWithdrawError::ArbiterAlreadyRemoved);
        }
        let actual_slash = slash_amount.min(self.stake_amount);
        self.stake_amount = self.stake_amount.saturating_sub(actual_slash);
        self.total_slashed = self.total_slashed.saturating_add(actual_slash);
        self.slash_count = self.slash_count.saturating_add(1);
        if self.pending_withdrawal > self.stake_amount {
            self.pending_withdrawal = self.stake_amount;
        }
        if self.stake_amount == 0 {
            self.status = ArbiterStatus::Removed;
            self.deactivated_at = None;
            self.pending_withdrawal = 0;
        }
        Ok(actual_slash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serializer::Serializer;

    #[test]
    fn arbiter_status_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let status = ArbiterStatus::Suspended;
        let data = serde_json::to_vec(&status)?;
        let decoded: ArbiterStatus = serde_json::from_slice(&data)?;
        assert_eq!(status, decoded);
        Ok(())
    }

    #[test]
    fn expertise_domain_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let domain = ExpertiseDomain::SmartContract;
        let data = serde_json::to_vec(&domain)?;
        let decoded: ExpertiseDomain = serde_json::from_slice(&data)?;
        assert_eq!(domain, decoded);
        Ok(())
    }

    #[test]
    fn arbiter_account_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let account = ArbiterAccount {
            public_key: PublicKey::from_bytes(&[1u8; 32])?,
            name: "arbiter-1".to_string(),
            status: ArbiterStatus::Active,
            expertise: vec![ExpertiseDomain::General, ExpertiseDomain::AIAgent],
            stake_amount: 1_000_000,
            fee_basis_points: 150,
            min_escrow_value: 10,
            max_escrow_value: 1_000_000,
            reputation_score: 9000,
            total_cases: 42,
            cases_overturned: 2,
            registered_at: 100,
            last_active_at: 120,
            pending_withdrawal: 0,
            deactivated_at: None,
            active_cases: 0,
            total_slashed: 0,
            slash_count: 0,
        };

        let data = serde_json::to_vec(&account)?;
        let decoded: ArbiterAccount = serde_json::from_slice(&data)?;
        assert_eq!(account.name, decoded.name);
        assert_eq!(account.status, decoded.status);
        assert_eq!(account.expertise, decoded.expertise);
        assert_eq!(account.reputation_score, decoded.reputation_score);
        Ok(())
    }
}
