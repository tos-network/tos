//! Native Asset Types
//!
//! Core data structures for native assets.

use serde::{Deserialize, Serialize};

use crate::crypto::{Hash, Signature};
use crate::serializer::{Reader, ReaderError, Serializer, Writer};

// ===== Extended Asset Data =====

/// Extended native asset data with additional features
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeAssetData {
    /// Asset name
    pub name: String,
    /// Asset symbol/ticker
    pub symbol: String,
    /// Decimal places (0-18)
    pub decimals: u8,
    /// Total supply
    pub total_supply: u64,
    /// Maximum supply (None = unlimited)
    pub max_supply: Option<u64>,
    /// Whether the asset is mintable
    pub mintable: bool,
    /// Whether the asset is burnable
    pub burnable: bool,
    /// Whether the asset is pausable
    pub pausable: bool,
    /// Whether accounts can be frozen
    pub freezable: bool,
    /// Whether the asset supports governance (voting)
    pub governance: bool,
    /// Creator/owner address
    pub creator: [u8; 32],
    /// Creation block height
    pub created_at: u64,
    /// Optional metadata URI
    pub metadata_uri: Option<String>,
}

impl Default for NativeAssetData {
    fn default() -> Self {
        Self {
            name: String::new(),
            symbol: String::new(),
            decimals: 18,
            total_supply: 0,
            max_supply: None,
            mintable: true,
            burnable: true,
            pausable: false,
            freezable: false,
            governance: false,
            creator: [0u8; 32],
            created_at: 0,
            metadata_uri: None,
        }
    }
}

impl Serializer for NativeAssetData {
    fn write(&self, writer: &mut Writer) {
        self.name.write(writer);
        self.symbol.write(writer);
        self.decimals.write(writer);
        self.total_supply.write(writer);
        self.max_supply.write(writer);
        self.mintable.write(writer);
        self.burnable.write(writer);
        self.pausable.write(writer);
        self.freezable.write(writer);
        self.governance.write(writer);
        writer.write_bytes(&self.creator);
        self.created_at.write(writer);
        self.metadata_uri.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            name: reader.read()?,
            symbol: reader.read()?,
            decimals: reader.read()?,
            total_supply: reader.read()?,
            max_supply: reader.read()?,
            mintable: reader.read()?,
            burnable: reader.read()?,
            pausable: reader.read()?,
            freezable: reader.read()?,
            governance: reader.read()?,
            creator: reader.read_bytes_32()?,
            created_at: reader.read()?,
            metadata_uri: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        self.name.size()
            + self.symbol.size()
            + 1 // decimals
            + 8 // total_supply
            + self.max_supply.size()
            + 5 // bool fields
            + 32 // creator
            + 8 // created_at
            + self.metadata_uri.size()
    }
}

// ===== Allowance =====

/// Allowance record (owner -> spender -> amount)
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Allowance {
    /// Remaining allowance
    pub amount: u64,
    /// Last update block height
    pub updated_at: u64,
}

impl Serializer for Allowance {
    fn write(&self, writer: &mut Writer) {
        self.amount.write(writer);
        self.updated_at.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            amount: reader.read()?,
            updated_at: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        16
    }
}

// ===== Token Lock =====

/// Token lock record for timelock functionality
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenLock {
    /// Unique lock ID within asset+account
    pub id: u64,
    /// Locked amount
    pub amount: u64,
    /// Unix timestamp when lock expires
    pub unlock_at: u64,
    /// Whether the lock can be transferred
    pub transferable: bool,
    /// Original locker address
    pub locker: [u8; 32],
    /// Creation block height
    pub created_at: u64,
}

impl Serializer for TokenLock {
    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.amount.write(writer);
        self.unlock_at.write(writer);
        self.transferable.write(writer);
        writer.write_bytes(&self.locker);
        self.created_at.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            id: reader.read()?,
            amount: reader.read()?,
            unlock_at: reader.read()?,
            transferable: reader.read()?,
            locker: reader.read_bytes_32()?,
            created_at: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        8 + 8 + 8 + 1 + 32 + 8
    }
}

// ===== Governance/Voting =====

/// Checkpoint for vote tracking
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Block height when checkpoint was created
    pub from_block: u64,
    /// Voting power at this checkpoint
    pub votes: u64,
}

impl Serializer for Checkpoint {
    fn write(&self, writer: &mut Writer) {
        self.from_block.write(writer);
        self.votes.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            from_block: reader.read()?,
            votes: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        16
    }
}

/// Delegation record
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Delegation {
    /// Delegatee address (None = self-delegation)
    pub delegatee: Option<[u8; 32]>,
    /// Block height when delegation was set
    pub from_block: u64,
}

impl Serializer for Delegation {
    fn write(&self, writer: &mut Writer) {
        self.delegatee.write(writer);
        self.from_block.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            delegatee: reader.read()?,
            from_block: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        self.delegatee.size() + 8
    }
}

// ===== Escrow =====

/// Escrow status
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowStatus {
    /// Escrow is active and pending release
    #[default]
    Active,
    /// Escrow has been released to recipient
    Released,
    /// Escrow has been cancelled and refunded
    Cancelled,
    /// Escrow is in dispute
    Disputed,
}

impl Serializer for EscrowStatus {
    fn write(&self, writer: &mut Writer) {
        let v: u8 = match self {
            Self::Active => 0,
            Self::Released => 1,
            Self::Cancelled => 2,
            Self::Disputed => 3,
        };
        v.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let v: u8 = reader.read()?;
        match v {
            0 => Ok(Self::Active),
            1 => Ok(Self::Released),
            2 => Ok(Self::Cancelled),
            3 => Ok(Self::Disputed),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

/// Release condition for escrow
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ReleaseCondition {
    /// Release after specified block height
    TimeRelease { release_after: u64 },
    /// Multi-party approval required
    MultiApproval {
        approvers: Vec<[u8; 32]>,
        required: u8,
    },
    /// Hash lock (recipient must provide preimage)
    HashLock { hash: [u8; 32] },
}

impl Default for ReleaseCondition {
    fn default() -> Self {
        Self::TimeRelease { release_after: 0 }
    }
}

impl Serializer for ReleaseCondition {
    fn write(&self, writer: &mut Writer) {
        match self {
            Self::TimeRelease { release_after } => {
                0u8.write(writer);
                release_after.write(writer);
            }
            Self::MultiApproval {
                approvers,
                required,
            } => {
                1u8.write(writer);
                (approvers.len() as u8).write(writer);
                for approver in approvers {
                    writer.write_bytes(approver);
                }
                required.write(writer);
            }
            Self::HashLock { hash } => {
                2u8.write(writer);
                writer.write_bytes(hash);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let tag: u8 = reader.read()?;
        match tag {
            0 => Ok(Self::TimeRelease {
                release_after: reader.read()?,
            }),
            1 => {
                let count: u8 = reader.read()?;
                let mut approvers = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    approvers.push(reader.read_bytes_32()?);
                }
                let required = reader.read()?;
                Ok(Self::MultiApproval {
                    approvers,
                    required,
                })
            }
            2 => Ok(Self::HashLock {
                hash: reader.read_bytes_32()?,
            }),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::TimeRelease { .. } => 1 + 8,
            Self::MultiApproval { approvers, .. } => 1 + 1 + (approvers.len() * 32) + 1,
            Self::HashLock { .. } => 1 + 32,
        }
    }
}

/// Escrow record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Escrow {
    /// Unique escrow ID
    pub id: u64,
    /// Asset hash
    pub asset: Hash,
    /// Sender address
    pub sender: [u8; 32],
    /// Recipient address
    pub recipient: [u8; 32],
    /// Escrowed amount
    pub amount: u64,
    /// Release condition
    pub condition: ReleaseCondition,
    /// Current status
    pub status: EscrowStatus,
    /// Collected approvals (for MultiApproval)
    pub approvals: Vec<[u8; 32]>,
    /// Expiry block (for auto-cancel)
    pub expires_at: Option<u64>,
    /// Creation block
    pub created_at: u64,
    /// Optional metadata
    pub metadata: Option<Vec<u8>>,
}

impl Serializer for Escrow {
    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.asset.write(writer);
        writer.write_bytes(&self.sender);
        writer.write_bytes(&self.recipient);
        self.amount.write(writer);
        self.condition.write(writer);
        self.status.write(writer);
        (self.approvals.len() as u8).write(writer);
        for approval in &self.approvals {
            writer.write_bytes(approval);
        }
        self.expires_at.write(writer);
        self.created_at.write(writer);
        self.metadata.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let id = reader.read()?;
        let asset = reader.read()?;
        let sender = reader.read_bytes_32()?;
        let recipient = reader.read_bytes_32()?;
        let amount = reader.read()?;
        let condition = reader.read()?;
        let status = reader.read()?;
        let approval_count: u8 = reader.read()?;
        let mut approvals = Vec::with_capacity(approval_count as usize);
        for _ in 0..approval_count {
            approvals.push(reader.read_bytes_32()?);
        }
        let expires_at = reader.read()?;
        let created_at = reader.read()?;
        let metadata = reader.read()?;

        Ok(Self {
            id,
            asset,
            sender,
            recipient,
            amount,
            condition,
            status,
            approvals,
            expires_at,
            created_at,
            metadata,
        })
    }

    fn size(&self) -> usize {
        8 // id
            + self.asset.size()
            + 32 // sender
            + 32 // recipient
            + 8 // amount
            + self.condition.size()
            + self.status.size()
            + 1 + (self.approvals.len() * 32)
            + self.expires_at.size()
            + 8 // created_at
            + self.metadata.size()
    }
}

// ===== AGI/Agent =====

/// Spending limit period
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpendingPeriod {
    /// Limit applies to single transaction
    PerTransaction,
    /// Limit resets each block
    PerBlock,
    /// Limit resets daily (based on block time)
    Daily,
    /// Lifetime limit (never resets)
    #[default]
    Lifetime,
}

impl Serializer for SpendingPeriod {
    fn write(&self, writer: &mut Writer) {
        let v: u8 = match self {
            Self::PerTransaction => 0,
            Self::PerBlock => 1,
            Self::Daily => 2,
            Self::Lifetime => 3,
        };
        v.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let v: u8 = reader.read()?;
        match v {
            0 => Ok(Self::PerTransaction),
            1 => Ok(Self::PerBlock),
            2 => Ok(Self::Daily),
            3 => Ok(Self::Lifetime),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

/// Spending limit configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpendingLimit {
    /// Maximum amount per period
    pub max_amount: u64,
    /// Limit period type
    pub period: SpendingPeriod,
    /// Amount spent in current period
    pub current_spent: u64,
    /// Block height when current period started
    pub period_start: u64,
}

impl Default for SpendingLimit {
    fn default() -> Self {
        Self {
            max_amount: u64::MAX,
            period: SpendingPeriod::Lifetime,
            current_spent: 0,
            period_start: 0,
        }
    }
}

impl Serializer for SpendingLimit {
    fn write(&self, writer: &mut Writer) {
        self.max_amount.write(writer);
        self.period.write(writer);
        self.current_spent.write(writer);
        self.period_start.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            max_amount: reader.read()?,
            period: reader.read()?,
            current_spent: reader.read()?,
            period_start: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        8 + 1 + 8 + 8
    }
}

/// Agent authorization record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentAuthorization {
    /// Agent identifier (public key)
    pub agent: [u8; 32],
    /// Owner who authorized the agent
    pub owner: [u8; 32],
    /// Asset this authorization applies to
    pub asset: Hash,
    /// Spending limit
    pub spending_limit: SpendingLimit,
    /// Whether the agent can delegate to sub-agents
    pub can_delegate: bool,
    /// Allowed recipient addresses (empty = any)
    pub allowed_recipients: Vec<[u8; 32]>,
    /// Expiry block (0 = never)
    pub expires_at: u64,
    /// Creation block
    pub created_at: u64,
}

impl Serializer for AgentAuthorization {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(&self.agent);
        writer.write_bytes(&self.owner);
        self.asset.write(writer);
        self.spending_limit.write(writer);
        self.can_delegate.write(writer);
        (self.allowed_recipients.len() as u8).write(writer);
        for recipient in &self.allowed_recipients {
            writer.write_bytes(recipient);
        }
        self.expires_at.write(writer);
        self.created_at.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let agent = reader.read_bytes_32()?;
        let owner = reader.read_bytes_32()?;
        let asset = reader.read()?;
        let spending_limit = reader.read()?;
        let can_delegate = reader.read()?;
        let recipient_count: u8 = reader.read()?;
        let mut allowed_recipients = Vec::with_capacity(recipient_count as usize);
        for _ in 0..recipient_count {
            allowed_recipients.push(reader.read_bytes_32()?);
        }
        let expires_at = reader.read()?;
        let created_at = reader.read()?;

        Ok(Self {
            agent,
            owner,
            asset,
            spending_limit,
            can_delegate,
            allowed_recipients,
            expires_at,
            created_at,
        })
    }

    fn size(&self) -> usize {
        32 // agent
            + 32 // owner
            + self.asset.size()
            + self.spending_limit.size()
            + 1 // can_delegate
            + 1 + (self.allowed_recipients.len() * 32)
            + 8 // expires_at
            + 8 // created_at
    }
}

// ===== Permit =====

/// Domain separator for permit signatures (EIP-712 style)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermitDomain {
    /// Asset name
    pub name: String,
    /// Version string
    pub version: String,
    /// Chain ID
    pub chain_id: u64,
    /// Asset hash (verifying contract equivalent)
    pub verifying_asset: Hash,
}

/// Permit message for approval
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermitMessage {
    /// Owner address
    pub owner: [u8; 32],
    /// Spender address
    pub spender: [u8; 32],
    /// Approved amount
    pub value: u64,
    /// Nonce (prevents replay)
    pub nonce: u64,
    /// Deadline block height
    pub deadline: u64,
}

impl Serializer for PermitMessage {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(&self.owner);
        writer.write_bytes(&self.spender);
        self.value.write(writer);
        self.nonce.write(writer);
        self.deadline.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            owner: reader.read_bytes_32()?,
            spender: reader.read_bytes_32()?,
            value: reader.read()?,
            nonce: reader.read()?,
            deadline: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        32 + 32 + 8 + 8 + 8
    }
}

/// Signed permit
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Permit {
    /// Permit message
    pub message: PermitMessage,
    /// Cryptographic signature
    pub signature: Signature,
}

impl Serializer for Permit {
    fn write(&self, writer: &mut Writer) {
        self.message.write(writer);
        self.signature.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let message = reader.read()?;
        let signature = reader.read()?;
        Ok(Self { message, signature })
    }

    fn size(&self) -> usize {
        self.message.size() + self.signature.size()
    }
}

// ===== Admin State =====

/// Pause state for an asset
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PauseState {
    /// Whether the asset is paused
    pub is_paused: bool,
    /// Who paused it
    pub paused_by: Option<[u8; 32]>,
    /// When it was paused
    pub paused_at: Option<u64>,
}

impl Serializer for PauseState {
    fn write(&self, writer: &mut Writer) {
        self.is_paused.write(writer);
        self.paused_by.write(writer);
        self.paused_at.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            is_paused: reader.read()?,
            paused_by: reader.read()?,
            paused_at: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        1 + self.paused_by.size() + self.paused_at.size()
    }
}

/// Freeze state for an account
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FreezeState {
    /// Whether the account is frozen
    pub is_frozen: bool,
    /// Who froze it
    pub frozen_by: Option<[u8; 32]>,
    /// When it was frozen
    pub frozen_at: Option<u64>,
}

impl Serializer for FreezeState {
    fn write(&self, writer: &mut Writer) {
        self.is_frozen.write(writer);
        self.frozen_by.write(writer);
        self.frozen_at.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            is_frozen: reader.read()?,
            frozen_by: reader.read()?,
            frozen_at: reader.read()?,
        })
    }

    fn size(&self) -> usize {
        1 + self.frozen_by.size() + self.frozen_at.size()
    }
}
