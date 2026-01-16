use serde::{Deserialize, Serialize};

use crate::crypto::{Hash, PublicKey};

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
    /// Asset type (TOS native or token).
    pub asset: Hash,
    /// Current escrow state.
    pub state: EscrowState,
    /// Challenge window in blocks.
    pub challenge_window: u64,
    /// Challenge deposit percentage (basis points).
    pub challenge_deposit_bps: u16,
    /// Block height when release was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_requested_at: Option<u64>,
    /// Block height when created.
    pub created_at: u64,
    /// Escrow timeout in blocks.
    pub timeout_blocks: u64,
    /// Arbitration configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arbitration_config: Option<ArbitrationConfig>,
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
            asset: Hash::max(),
            state: EscrowState::Created,
            challenge_window: 10,
            challenge_deposit_bps: 500,
            release_requested_at: None,
            created_at: 1,
            timeout_blocks: 100,
            arbitration_config: Some(ArbitrationConfig {
                mode: ArbitrationMode::Single,
                arbiters: vec![PublicKey::from_bytes(&[3u8; 32])?],
                threshold: None,
                fee_amount: 5,
                allow_appeal: false,
            }),
        };
        let data = serde_json::to_vec(&account)?;
        let decoded: EscrowAccount = serde_json::from_slice(&data)?;
        assert_eq!(account.amount, decoded.amount);
        assert_eq!(account.state, decoded.state);
        assert_eq!(account.challenge_window, decoded.challenge_window);
        Ok(())
    }
}
