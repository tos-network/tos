use crate::{
    config::{MAX_BATCH_ACTIVATE, MAX_BATCH_ACTIVATE_DELEGATE, MAX_BATCH_DELEGATE},
    crypto::elgamal::CompressedPublicKey,
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

/// Item for batch delegation operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchDelegationItem {
    /// Receiver of delegated energy
    pub receiver: CompressedPublicKey,
    /// Amount of TOS to delegate
    pub amount: u64,
    /// Lock the delegation for a period
    pub lock: bool,
    /// Lock period in days (0-365, only used if lock=true)
    pub lock_period: u32,
}

impl Serializer for BatchDelegationItem {
    fn write(&self, writer: &mut Writer) {
        self.receiver.write(writer);
        writer.write_u64(&self.amount);
        writer.write_bool(self.lock);
        writer.write_u32(&self.lock_period);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let receiver = CompressedPublicKey::read(reader)?;
        let amount = reader.read_u64()?;
        let lock = reader.read_bool()?;
        let lock_period = reader.read_u32()?;
        Ok(Self {
            receiver,
            amount,
            lock,
            lock_period,
        })
    }

    fn size(&self) -> usize {
        self.receiver.size() + 8 + 1 + 4
    }
}

/// Item for activate-and-delegate operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivateDelegateItem {
    /// Target account to activate and delegate to
    pub account: CompressedPublicKey,
    /// Amount of TOS to delegate (0 = activate only, no delegation)
    pub delegate_amount: u64,
    /// Lock the delegation for a period
    pub lock: bool,
    /// Lock period in days (0-365, only used if lock=true)
    pub lock_period: u32,
}

impl Serializer for ActivateDelegateItem {
    fn write(&self, writer: &mut Writer) {
        self.account.write(writer);
        writer.write_u64(&self.delegate_amount);
        writer.write_bool(self.lock);
        writer.write_u32(&self.lock_period);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let account = CompressedPublicKey::read(reader)?;
        let delegate_amount = reader.read_u64()?;
        let lock = reader.read_bool()?;
        let lock_period = reader.read_u32()?;
        Ok(Self {
            account,
            delegate_amount,
            lock,
            lock_period,
        })
    }

    fn size(&self) -> usize {
        self.account.size() + 8 + 1 + 4
    }
}

/// Energy-related transaction payloads for Stake 2.0 model
///
/// # Stake 2.0 Energy Model
/// - Proportional energy allocation: energy_limit = (frozen / total) × 18.4B
/// - 24-hour linear decay recovery
/// - 14-day unfreeze delay queue (max 32 entries)
/// - Delegation support
///
/// # Supported Operations
/// - `FreezeTos`: Lock TOS to gain proportional energy
/// - `UnfreezeTos`: Start 14-day unfreeze (adds to queue)
/// - `WithdrawExpireUnfreeze`: Withdraw expired entries from queue
/// - `CancelAllUnfreeze`: Cancel all pending, return to frozen
/// - `DelegateResource`: Delegate energy to another account
/// - `UndelegateResource`: Take back delegated energy
///
/// # Batch Operations (TOS Innovation)
/// - `ActivateAccounts`: Batch activate up to 500 accounts (0.1 TOS each)
/// - `BatchDelegateResource`: Batch delegate to up to 500 recipients
/// - `ActivateAndDelegate`: Activate and delegate in one tx (up to 500 items)
///
/// # Fee Model
/// - Energy operations are FREE (0 energy cost)
/// - Batch activation requires 0.1 TOS per account (TOS-only fee)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnergyPayload {
    /// Freeze TOS to gain proportional energy
    FreezeTos {
        /// Amount of TOS to freeze (atomic units)
        amount: u64,
    },
    /// Start unfreeze process (adds to 14-day queue)
    UnfreezeTos {
        /// Amount of TOS to unfreeze
        amount: u64,
    },
    /// Withdraw all expired unfreeze entries to balance
    WithdrawExpireUnfreeze,
    /// Cancel all pending unfreeze (expired → balance, pending → frozen)
    CancelAllUnfreeze,
    /// Delegate energy to another account
    DelegateResource {
        /// Receiver of delegated energy
        receiver: CompressedPublicKey,
        /// Amount of TOS to delegate
        amount: u64,
        /// Lock the delegation for a period
        lock: bool,
        /// Lock period in days (0-365, only used if lock=true)
        lock_period: u32,
    },
    /// Undelegate energy from another account
    UndelegateResource {
        /// Account to undelegate from
        receiver: CompressedPublicKey,
        /// Amount to undelegate
        amount: u64,
    },

    // === Batch Operations (TOS Innovation) ===
    /// Batch activate accounts (third-party payment)
    /// Activates up to 500 accounts in a single transaction.
    /// Fee: 0.1 TOS per account (skips already-activated accounts)
    ActivateAccounts {
        /// Accounts to activate (max 500)
        accounts: Vec<CompressedPublicKey>,
    },

    /// Batch delegate energy to multiple recipients
    /// Delegates energy to up to 500 recipients in one transaction.
    BatchDelegateResource {
        /// Delegation items (max 500)
        delegations: Vec<BatchDelegationItem>,
    },

    /// Activate accounts and delegate energy in one operation
    /// Combines activation and delegation for up to 500 accounts.
    /// Fee: 0.1 TOS per newly-activated account
    ActivateAndDelegate {
        /// Items to process (max 500)
        items: Vec<ActivateDelegateItem>,
    },
}

impl EnergyPayload {
    /// Create a new FreezeTos payload
    pub fn freeze_tos(amount: u64) -> Self {
        Self::FreezeTos { amount }
    }

    /// Create a new UnfreezeTos payload
    pub fn unfreeze_tos(amount: u64) -> Self {
        Self::UnfreezeTos { amount }
    }

    /// Create a new DelegateResource payload
    pub fn delegate_resource(
        receiver: CompressedPublicKey,
        amount: u64,
        lock: bool,
        lock_period: u32,
    ) -> Self {
        Self::DelegateResource {
            receiver,
            amount,
            lock,
            lock_period,
        }
    }

    /// Create a new UndelegateResource payload
    pub fn undelegate_resource(receiver: CompressedPublicKey, amount: u64) -> Self {
        Self::UndelegateResource { receiver, amount }
    }

    /// Create a new ActivateAccounts payload
    pub fn activate_accounts(accounts: Vec<CompressedPublicKey>) -> Self {
        Self::ActivateAccounts { accounts }
    }

    /// Create a new BatchDelegateResource payload
    pub fn batch_delegate_resource(delegations: Vec<BatchDelegationItem>) -> Self {
        Self::BatchDelegateResource { delegations }
    }

    /// Create a new ActivateAndDelegate payload
    pub fn activate_and_delegate(items: Vec<ActivateDelegateItem>) -> Self {
        Self::ActivateAndDelegate { items }
    }

    /// Get the energy cost for this operation
    /// All energy operations are FREE in Stake 2.0
    pub fn energy_cost(&self) -> u64 {
        0
    }

    /// Get the TOS fee required for this operation
    /// Returns 0 for most operations, but batch activation has per-account fees
    /// Note: Actual fee calculation happens during verification (checks existing accounts)
    pub fn tos_fee(&self) -> u64 {
        0 // Actual fee depends on how many accounts are newly activated
    }

    /// Check if this operation requires account activation
    pub fn requires_activation(&self) -> bool {
        false
    }

    /// Check if this is a batch operation
    pub fn is_batch_operation(&self) -> bool {
        matches!(
            self,
            Self::ActivateAccounts { .. }
                | Self::BatchDelegateResource { .. }
                | Self::ActivateAndDelegate { .. }
        )
    }

    /// Get the batch size for batch operations
    pub fn batch_size(&self) -> Option<usize> {
        match self {
            Self::ActivateAccounts { accounts } => Some(accounts.len()),
            Self::BatchDelegateResource { delegations } => Some(delegations.len()),
            Self::ActivateAndDelegate { items } => Some(items.len()),
            _ => None,
        }
    }

    /// Validate batch operation limits
    /// Returns Ok(()) if within limits, Err with message otherwise
    pub fn validate_batch_limits(&self) -> Result<(), &'static str> {
        match self {
            Self::ActivateAccounts { accounts } => {
                if accounts.is_empty() {
                    return Err("Empty account list");
                }
                if accounts.len() > MAX_BATCH_ACTIVATE {
                    return Err("Too many accounts (max 500)");
                }
                Ok(())
            }
            Self::BatchDelegateResource { delegations } => {
                if delegations.is_empty() {
                    return Err("Empty delegation list");
                }
                if delegations.len() > MAX_BATCH_DELEGATE {
                    return Err("Too many delegations (max 500)");
                }
                Ok(())
            }
            Self::ActivateAndDelegate { items } => {
                if items.is_empty() {
                    return Err("Empty item list");
                }
                if items.len() > MAX_BATCH_ACTIVATE_DELEGATE {
                    return Err("Too many items (max 500)");
                }
                Ok(())
            }
            _ => Ok(()), // Non-batch operations have no limits
        }
    }

    /// Get the amount of TOS involved in this operation (if applicable)
    pub fn get_amount(&self) -> Option<u64> {
        match self {
            Self::FreezeTos { amount } => Some(*amount),
            Self::UnfreezeTos { amount } => Some(*amount),
            Self::DelegateResource { amount, .. } => Some(*amount),
            Self::UndelegateResource { amount, .. } => Some(*amount),
            Self::BatchDelegateResource { delegations } => {
                Some(delegations.iter().map(|d| d.amount).sum())
            }
            Self::ActivateAndDelegate { items } => {
                Some(items.iter().map(|i| i.delegate_amount).sum())
            }
            Self::WithdrawExpireUnfreeze
            | Self::CancelAllUnfreeze
            | Self::ActivateAccounts { .. } => None,
        }
    }

    /// Get the receiver for delegation operations
    pub fn get_receiver(&self) -> Option<&CompressedPublicKey> {
        match self {
            Self::DelegateResource { receiver, .. } => Some(receiver),
            Self::UndelegateResource { receiver, .. } => Some(receiver),
            _ => None,
        }
    }

    /// Get all receivers for batch operations
    pub fn get_receivers(&self) -> Vec<&CompressedPublicKey> {
        match self {
            Self::DelegateResource { receiver, .. } => vec![receiver],
            Self::UndelegateResource { receiver, .. } => vec![receiver],
            Self::ActivateAccounts { accounts } => accounts.iter().collect(),
            Self::BatchDelegateResource { delegations } => {
                delegations.iter().map(|d| &d.receiver).collect()
            }
            Self::ActivateAndDelegate { items } => items.iter().map(|i| &i.account).collect(),
            _ => vec![],
        }
    }
}

impl Serializer for EnergyPayload {
    fn write(&self, writer: &mut Writer) {
        match self {
            Self::FreezeTos { amount } => {
                writer.write_u8(0);
                writer.write_u64(amount);
            }
            Self::UnfreezeTos { amount } => {
                writer.write_u8(1);
                writer.write_u64(amount);
            }
            Self::WithdrawExpireUnfreeze => {
                writer.write_u8(2);
            }
            Self::CancelAllUnfreeze => {
                writer.write_u8(3);
            }
            Self::DelegateResource {
                receiver,
                amount,
                lock,
                lock_period,
            } => {
                writer.write_u8(4);
                receiver.write(writer);
                writer.write_u64(amount);
                writer.write_bool(*lock);
                writer.write_u32(lock_period);
            }
            Self::UndelegateResource { receiver, amount } => {
                writer.write_u8(5);
                receiver.write(writer);
                writer.write_u64(amount);
            }
            Self::ActivateAccounts { accounts } => {
                writer.write_u8(6);
                writer.write_u16(accounts.len() as u16);
                for account in accounts {
                    account.write(writer);
                }
            }
            Self::BatchDelegateResource { delegations } => {
                writer.write_u8(7);
                writer.write_u16(delegations.len() as u16);
                for item in delegations {
                    item.write(writer);
                }
            }
            Self::ActivateAndDelegate { items } => {
                writer.write_u8(8);
                writer.write_u16(items.len() as u16);
                for item in items {
                    item.write(writer);
                }
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let variant = reader.read_u8()?;
        match variant {
            0 => {
                let amount = reader.read_u64()?;
                Ok(Self::FreezeTos { amount })
            }
            1 => {
                let amount = reader.read_u64()?;
                Ok(Self::UnfreezeTos { amount })
            }
            2 => Ok(Self::WithdrawExpireUnfreeze),
            3 => Ok(Self::CancelAllUnfreeze),
            4 => {
                let receiver = CompressedPublicKey::read(reader)?;
                let amount = reader.read_u64()?;
                let lock = reader.read_bool()?;
                let lock_period = reader.read_u32()?;
                Ok(Self::DelegateResource {
                    receiver,
                    amount,
                    lock,
                    lock_period,
                })
            }
            5 => {
                let receiver = CompressedPublicKey::read(reader)?;
                let amount = reader.read_u64()?;
                Ok(Self::UndelegateResource { receiver, amount })
            }
            6 => {
                let count = reader.read_u16()? as usize;
                if count > MAX_BATCH_ACTIVATE {
                    return Err(ReaderError::InvalidSize);
                }
                let mut accounts = Vec::with_capacity(count);
                for _ in 0..count {
                    accounts.push(CompressedPublicKey::read(reader)?);
                }
                Ok(Self::ActivateAccounts { accounts })
            }
            7 => {
                let count = reader.read_u16()? as usize;
                if count > MAX_BATCH_DELEGATE {
                    return Err(ReaderError::InvalidSize);
                }
                let mut delegations = Vec::with_capacity(count);
                for _ in 0..count {
                    delegations.push(BatchDelegationItem::read(reader)?);
                }
                Ok(Self::BatchDelegateResource { delegations })
            }
            8 => {
                let count = reader.read_u16()? as usize;
                if count > MAX_BATCH_ACTIVATE_DELEGATE {
                    return Err(ReaderError::InvalidSize);
                }
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(ActivateDelegateItem::read(reader)?);
                }
                Ok(Self::ActivateAndDelegate { items })
            }
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::FreezeTos { .. } => 1 + 8,
            Self::UnfreezeTos { .. } => 1 + 8,
            Self::WithdrawExpireUnfreeze => 1,
            Self::CancelAllUnfreeze => 1,
            Self::DelegateResource { receiver, .. } => 1 + receiver.size() + 8 + 1 + 4,
            Self::UndelegateResource { receiver, .. } => 1 + receiver.size() + 8,
            Self::ActivateAccounts { accounts } => {
                1 + 2 + accounts.iter().map(|a| a.size()).sum::<usize>()
            }
            Self::BatchDelegateResource { delegations } => {
                1 + 2 + delegations.iter().map(|d| d.size()).sum::<usize>()
            }
            Self::ActivateAndDelegate { items } => {
                1 + 2 + items.iter().map(|i| i.size()).sum::<usize>()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freeze_tos_payload() {
        let payload = EnergyPayload::freeze_tos(100_000_000); // 1 TOS

        assert_eq!(payload.get_amount(), Some(100_000_000));
        assert_eq!(payload.energy_cost(), 0);
        assert_eq!(payload.tos_fee(), 0);
    }

    #[test]
    fn test_unfreeze_tos_payload() {
        let payload = EnergyPayload::unfreeze_tos(50_000_000);

        assert_eq!(payload.get_amount(), Some(50_000_000));
        assert_eq!(payload.energy_cost(), 0);
    }

    #[test]
    fn test_withdraw_expire_unfreeze() {
        let payload = EnergyPayload::WithdrawExpireUnfreeze;

        assert_eq!(payload.get_amount(), None);
        assert_eq!(payload.energy_cost(), 0);
    }

    #[test]
    fn test_cancel_all_unfreeze() {
        let payload = EnergyPayload::CancelAllUnfreeze;

        assert_eq!(payload.get_amount(), None);
        assert_eq!(payload.energy_cost(), 0);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let payloads = vec![
            EnergyPayload::freeze_tos(1000),
            EnergyPayload::unfreeze_tos(500),
            EnergyPayload::WithdrawExpireUnfreeze,
            EnergyPayload::CancelAllUnfreeze,
        ];

        for payload in payloads {
            let bytes = payload.to_bytes();
            let mut reader = crate::serializer::Reader::new(&bytes);
            let restored = EnergyPayload::read(&mut reader).expect("test");
            assert_eq!(payload, restored);
        }
    }

    #[test]
    fn test_delegate_resource_payload() {
        use crate::crypto::KeyPair;

        let receiver = KeyPair::new().get_public_key().compress();
        let payload = EnergyPayload::delegate_resource(receiver.clone(), 1_000_000, true, 30);

        assert_eq!(payload.get_amount(), Some(1_000_000));
        assert_eq!(payload.get_receiver(), Some(&receiver));
        assert_eq!(payload.energy_cost(), 0);
        assert_eq!(payload.tos_fee(), 0);
        assert!(!payload.requires_activation());

        // Test unlocked delegation
        let unlocked = EnergyPayload::delegate_resource(receiver.clone(), 500_000, false, 0);
        assert_eq!(unlocked.get_amount(), Some(500_000));
    }

    #[test]
    fn test_undelegate_resource_payload() {
        use crate::crypto::KeyPair;

        let receiver = KeyPair::new().get_public_key().compress();
        let payload = EnergyPayload::undelegate_resource(receiver.clone(), 500_000);

        assert_eq!(payload.get_amount(), Some(500_000));
        assert_eq!(payload.get_receiver(), Some(&receiver));
        assert_eq!(payload.energy_cost(), 0);
        assert_eq!(payload.tos_fee(), 0);
    }

    #[test]
    fn test_delegation_serialization_roundtrip() {
        use crate::crypto::KeyPair;

        let receiver = KeyPair::new().get_public_key().compress();

        // Test DelegateResource with lock
        let delegate_locked =
            EnergyPayload::delegate_resource(receiver.clone(), 1_000_000, true, 90);
        let bytes = delegate_locked.to_bytes();
        let mut reader = crate::serializer::Reader::new(&bytes);
        let restored = EnergyPayload::read(&mut reader).expect("test");
        assert_eq!(delegate_locked, restored);

        // Test DelegateResource without lock
        let delegate_unlocked =
            EnergyPayload::delegate_resource(receiver.clone(), 500_000, false, 0);
        let bytes = delegate_unlocked.to_bytes();
        let mut reader = crate::serializer::Reader::new(&bytes);
        let restored = EnergyPayload::read(&mut reader).expect("test");
        assert_eq!(delegate_unlocked, restored);

        // Test UndelegateResource
        let undelegate = EnergyPayload::undelegate_resource(receiver, 250_000);
        let bytes = undelegate.to_bytes();
        let mut reader = crate::serializer::Reader::new(&bytes);
        let restored = EnergyPayload::read(&mut reader).expect("test");
        assert_eq!(undelegate, restored);
    }

    #[test]
    fn test_delegation_size_calculation() {
        use crate::crypto::KeyPair;

        let receiver = KeyPair::new().get_public_key().compress();

        // DelegateResource: 1 (opcode) + 32 (pubkey) + 8 (amount) + 1 (lock bool) + 4 (lock_period)
        let delegate = EnergyPayload::delegate_resource(receiver.clone(), 1000, true, 30);
        let bytes = delegate.to_bytes();
        assert_eq!(delegate.size(), bytes.len());

        // UndelegateResource: 1 (opcode) + 32 (pubkey) + 8 (amount)
        let undelegate = EnergyPayload::undelegate_resource(receiver, 500);
        let bytes = undelegate.to_bytes();
        assert_eq!(undelegate.size(), bytes.len());
    }
}
