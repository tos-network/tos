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
    /// Arbiter has been removed and is no longer eligible.
    Removed,
}

impl Serializer for ArbiterStatus {
    fn write(&self, writer: &mut Writer) {
        let value = match self {
            ArbiterStatus::Active => 0u8,
            ArbiterStatus::Suspended => 1u8,
            ArbiterStatus::Removed => 2u8,
        };
        value.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        match value {
            0 => Ok(ArbiterStatus::Active),
            1 => Ok(ArbiterStatus::Suspended),
            2 => Ok(ArbiterStatus::Removed),
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
