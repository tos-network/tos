// Transaction verification module

mod contract;
mod error;
mod kyc;
mod state;
mod zkp_cache;

use std::{borrow::Cow, iter, sync::Arc};

use anyhow::{anyhow, Context};
use indexmap::IndexMap;
use log::{debug, trace};
use tos_crypto::{
    bulletproofs::RangeProof,
    curve25519_dalek::{ristretto::CompressedRistretto, traits::Identity, RistrettoPoint},
    merlin::Transcript,
};
use tos_kernel::ModuleValidator;

use super::{payload::EnergyPayload, ContractDeposit, Role, Transaction, TransactionType};
use crate::{
    config::{
        BURN_PER_CONTRACT, COIN_VALUE, FEE_PER_ACCOUNT_CREATION, FEE_PER_MULTISIG_SIGNATURE,
        MAX_FEE_LIMIT, MAX_GAS_USAGE_PER_TX, MIN_DELEGATION_AMOUNT, TOS_ASSET, UNO_ASSET,
    },
    contract::ContractProvider,
    crypto::{
        elgamal::{Ciphertext, DecompressionError, DecryptHandle, PedersenCommitment, PublicKey},
        hash,
        proofs::{BatchCollector, ProofVerificationError, BP_GENS, BULLET_PROOF_SIZE, PC_GENS},
        Hash, ProtocolTranscript,
    },
    serializer::Serializer,
    tokio::spawn_blocking_safe,
    transaction::{
        payload::{UnoTransferPayload, UnshieldTransferPayload},
        TxVersion, EXTRA_DATA_LIMIT_SIZE, EXTRA_DATA_LIMIT_SUM_SIZE, MAX_DEPOSIT_PER_INVOKE_CALL,
        MAX_MULTISIG_PARTICIPANTS, MAX_TRANSFER_COUNT,
    },
    utils::energy_fee::EnergyResourceManager,
};
use contract::InvokeContract;

pub use error::*;
pub use state::*;
pub use zkp_cache::*;

/// Prepared UNO transaction data for batch range proof verification
type UnoPreparedData = (
    Arc<Transaction>,
    Transcript,
    Vec<(RistrettoPoint, CompressedRistretto)>,
);

// Decompressed UNO transfer ciphertext
// UNO transfers are stored in a compressed format for efficiency
// We decompress them once for verification and balance updates
struct DecompressedUnoTransferCt {
    commitment: PedersenCommitment,
    sender_handle: DecryptHandle,
    receiver_handle: DecryptHandle,
}

impl DecompressedUnoTransferCt {
    fn decompress(transfer: &UnoTransferPayload) -> Result<Self, DecompressionError> {
        Ok(Self {
            commitment: transfer.get_commitment().decompress()?,
            sender_handle: transfer.get_sender_handle().decompress()?,
            receiver_handle: transfer.get_receiver_handle().decompress()?,
        })
    }

    fn get_ciphertext(&self, role: Role) -> Ciphertext {
        let handle = match role {
            Role::Receiver => self.receiver_handle.clone(),
            Role::Sender => self.sender_handle.clone(),
        };
        Ciphertext::new(self.commitment.clone(), handle)
    }
}

// Decompressed Unshield transfer ciphertext
// Unshield transfers have commitment and sender_handle (no receiver_handle since receiver gets plaintext)
struct DecompressedUnshieldTransferCt {
    commitment: PedersenCommitment,
    sender_handle: DecryptHandle,
}

impl DecompressedUnshieldTransferCt {
    fn decompress(transfer: &UnshieldTransferPayload) -> Result<Self, DecompressionError> {
        Ok(Self {
            commitment: transfer.get_commitment().decompress()?,
            sender_handle: transfer.get_sender_handle().decompress()?,
        })
    }

    fn get_sender_ciphertext(&self) -> Ciphertext {
        Ciphertext::new(self.commitment.clone(), self.sender_handle.clone())
    }
}

impl Transaction {
    pub fn has_valid_version_format(&self) -> bool {
        match self.version {
            TxVersion::T0 | TxVersion::T1 => {
                // T0 and T1 support all transaction types
                // T1 adds chain_id for cross-network replay protection
                match &self.data {
                    TransactionType::Transfers(_)
                    | TransactionType::Burn(_)
                    | TransactionType::MultiSig(_)
                    | TransactionType::InvokeContract(_)
                    | TransactionType::DeployContract(_)
                    | TransactionType::Energy(_)
                    | TransactionType::AIMining(_)
                    | TransactionType::BindReferrer(_)
                    | TransactionType::BatchReferralReward(_)
                    // KYC transaction types
                    | TransactionType::SetKyc(_)
                    | TransactionType::RevokeKyc(_)
                    | TransactionType::RenewKyc(_)
                    | TransactionType::TransferKyc(_)
                    | TransactionType::AppealKyc(_)
                    | TransactionType::BootstrapCommittee(_)
                    | TransactionType::RegisterCommittee(_)
                    | TransactionType::UpdateCommittee(_)
                    | TransactionType::EmergencySuspend(_)
                    | TransactionType::UnoTransfers(_)
                    | TransactionType::ShieldTransfers(_)
                    | TransactionType::UnshieldTransfers(_) => true,
                }
            }
        }
    }

    /// Get the new output ciphertext
    /// This is used to substract the amount from the sender's balance
    /// Get the new output amounts for the sender
    /// Balance simplification: Returns plain u64 amounts instead of ciphertexts
    pub fn get_expected_sender_outputs(&self) -> Result<Vec<(&Hash, u64)>, DecompressionError> {
        // This method previously collected sender output ciphertexts for each asset
        // With plaintext balances, no commitments or ciphertexts needed
        // Return empty vector for now (amounts are handled directly in apply())
        let outputs = Vec::new();
        Ok(outputs)
    }

    /// Get the UNO output ciphertext for a specific asset
    /// This is used to calculate the total spending from encrypted balance
    fn get_uno_sender_output_ct(
        &self,
        asset: &Hash,
        decompressed_transfers: &[DecompressedUnoTransferCt],
    ) -> Ciphertext {
        let mut output = Ciphertext::zero();

        // Fees are paid via TOS (Energy consumption), not UNO
        // In Energy model, fees are paid via TOS (Energy consumption), not UNO
        // Fee handling is done in plaintext TOS balance, not encrypted UNO balance
        // OLD CODE (REMOVED):
        // if *asset == UNO_ASSET {
        //     output += tos_crypto::curve25519_dalek::Scalar::from(self.get_fee_limit());
        // }

        // Sum up all UNO transfers for this asset
        if let TransactionType::UnoTransfers(transfers) = &self.data {
            for (transfer, d) in transfers.iter().zip(decompressed_transfers.iter()) {
                if asset == transfer.get_asset() {
                    output += d.get_ciphertext(Role::Sender);
                }
            }
        }

        output
    }

    /// Verify that source commitment assets match the UNO transfer assets
    /// UNO is a single dedicated asset - all transfers must use UNO_ASSET only
    fn verify_uno_commitment_assets(&self) -> bool {
        let has_commitment_for_asset = |asset: &Hash| {
            self.source_commitments
                .iter()
                .any(|c| c.get_asset() == asset)
        };

        // UNO_ASSET is required for transfer amounts (fees paid via TOS Energy)
        // Since UNO is a single dedicated asset, we only need UNO_ASSET
        if !has_commitment_for_asset(&UNO_ASSET) {
            return false;
        }

        // Check for duplicates in source commitments
        if self.source_commitments.iter().enumerate().any(|(i, c)| {
            self.source_commitments
                .iter()
                .enumerate()
                .any(|(i2, c2)| i != i2 && c.get_asset() == c2.get_asset())
        }) {
            return false;
        }

        // All UNO transfers must use UNO_ASSET only
        // This enforces UNO as a single dedicated privacy asset
        if let TransactionType::UnoTransfers(transfers) = &self.data {
            return transfers
                .iter()
                .all(|transfer| *transfer.get_asset() == UNO_ASSET);
        }

        true
    }

    /// Get the total sender output ciphertext for Unshield transfers
    /// This is used to calculate the total spending from encrypted balance
    fn get_unshield_sender_output_ct(
        &self,
        decompressed_transfers: &[DecompressedUnshieldTransferCt],
    ) -> Ciphertext {
        let mut output = Ciphertext::zero();

        // Sum up all Unshield transfers (all use UNO_ASSET)
        if let TransactionType::UnshieldTransfers(transfers) = &self.data {
            for (_, d) in transfers.iter().zip(decompressed_transfers.iter()) {
                output += d.get_sender_ciphertext();
            }
        }

        output
    }

    /// Verify that source commitment assets match the Unshield transfer assets
    /// Unshield transfers always use UNO_ASSET (converting UNO -> TOS)
    fn verify_unshield_commitment_assets(&self) -> bool {
        let has_commitment_for_asset = |asset: &Hash| {
            self.source_commitments
                .iter()
                .any(|c| c.get_asset() == asset)
        };

        // UNO_ASSET is required for unshield (deducting from encrypted balance)
        if !has_commitment_for_asset(&UNO_ASSET) {
            return false;
        }

        // Check for duplicates in source commitments
        if self.source_commitments.iter().enumerate().any(|(i, c)| {
            self.source_commitments
                .iter()
                .enumerate()
                .any(|(i2, c2)| i != i2 && c.get_asset() == c2.get_asset())
        }) {
            return false;
        }

        // All Unshield transfers must use UNO_ASSET
        // Unshield converts encrypted UNO to plaintext TOS
        if let TransactionType::UnshieldTransfers(transfers) = &self.data {
            return transfers
                .iter()
                .all(|transfer| *transfer.get_asset() == UNO_ASSET);
        }

        true
    }

    // This method previously decompressed private contract deposits for proof verification
    // With plaintext balances (ContractDeposit::Public only), no decompression needed
    //
    // Previous functionality:
    // 1. Validated deposits.len() <= MAX_DEPOSIT_PER_INVOKE_CALL
    // 2. Validated max_gas <= MAX_GAS_USAGE_PER_TX
    // 3. For Public deposits: validated amount > 0
    // 4. For Private deposits: decompressed commitment, sender_handle, receiver_handle
    //
    // New plaintext approach:
    // 1. Validate deposit count and max_gas limits
    // 2. Validate all deposits are Public with amount > 0
    // 3. No decompression needed
    fn verify_invoke_contract<E>(
        &self,
        deposits: &IndexMap<Hash, ContractDeposit>,
        max_gas: u64,
    ) -> Result<(), VerificationError<E>> {
        if deposits.len() > MAX_DEPOSIT_PER_INVOKE_CALL {
            return Err(VerificationError::DepositCount);
        }

        if max_gas > MAX_GAS_USAGE_PER_TX {
            return Err(VerificationError::MaxGasReached);
        }

        // Validate all deposits are public with non-zero amounts
        // Balance simplification: Validate plaintext deposits
        for (_asset, deposit) in deposits.iter() {
            if deposit.amount() == 0 {
                return Err(VerificationError::InvalidFormat);
            }
        }

        Ok(())
    }

    // This method previously verified CiphertextValidityProof for private contract deposits
    // With plaintext balances (ContractDeposit::Public only), no proof verification needed
    //
    // Previous functionality:
    // 1. Decompressed deposit commitments and handles
    // 2. Verified CiphertextValidityProof.pre_verify() for each private deposit
    // 3. Added commitments to value_commitments for range proof verification
    //
    // New plaintext approach (to be implemented):
    // 1. All deposits are ContractDeposit::Public(amount)
    // 2. Simply validate amount > 0 (already done in verify_invoke_contract)
    // 3. Deduct amount from sender balance
    // 4. Add amount to contract balance
    fn verify_contract_deposits<E>(
        &self,
        _source_decompressed: &PublicKey,
        _dest_pubkey: &PublicKey,
        deposits: &IndexMap<Hash, ContractDeposit>,
    ) -> Result<(), VerificationError<E>> {
        // Stub implementation - proof verification removed
        // In production, implement plaintext deposit verification here
        trace!("Skipping contract deposit proof verification (plaintext balances)");

        // Balance simplification: All deposits are now plaintext
        // Basic validation is performed in verify_invoke_contract
        let _ = (deposits, _source_decompressed, _dest_pubkey); // Suppress unused warnings

        Ok(())
    }

    async fn verify_dynamic_parts<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        // Balance simplification: No decompression needed for plaintext balances

        trace!("Pre-verifying transaction on state");
        state
            .pre_verify_tx(self)
            .await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce to prevent TOCTOU race condition
        let success = state
            .compare_and_swap_nonce(
                &self.source,
                self.nonce,     // Expected value
                self.nonce + 1, // New value
            )
            .await
            .map_err(VerificationError::State)?;

        if !success {
            // CAS failed, get current nonce for error reporting
            let current = state
                .get_account_nonce(&self.source)
                .await
                .map_err(VerificationError::State)?;
            return Err(VerificationError::InvalidNonce(
                tx_hash.clone(),
                self.nonce,
                current,
            ));
        }

        match &self.data {
            TransactionType::Transfers(_transfers) => {
                // Balance simplification: No decompression needed
            }
            TransactionType::Burn(_) => {}
            TransactionType::MultiSig(payload) => {
                let is_reset = payload.threshold == 0 && payload.participants.is_empty();
                // If the multisig is reset, we need to check if it was already configured
                if is_reset
                    && state
                        .get_multisig_state(&self.source)
                        .await
                        .map_err(VerificationError::State)?
                        .is_none()
                {
                    return Err(VerificationError::MultiSigNotConfigured);
                }
            }
            TransactionType::InvokeContract(payload) => {
                self.verify_invoke_contract(&payload.deposits, payload.max_gas)?;

                // We need to load the contract module if not already in cache
                if !self.is_contract_available(state, &payload.contract).await? {
                    return Err(VerificationError::ContractNotFound);
                }

                let (module, environment) = state
                    .get_contract_module_with_environment(&payload.contract)
                    .await
                    .map_err(VerificationError::State)?;

                // TAKO contracts: entry_id validation is handled by tos-tbpf at runtime
                // Just verify the parameters are valid ValueCells
                let validator = ModuleValidator::new(module, environment);
                for constant in payload.parameters.iter() {
                    validator
                        .verify_constant(constant)
                        .map_err(|err| VerificationError::ModuleError(format!("{err:#}")))?;
                }
            }
            TransactionType::DeployContract(payload) => {
                if let Some(invoke) = payload.invoke.as_ref() {
                    self.verify_invoke_contract(&invoke.deposits, invoke.max_gas)?;
                }

                let environment = state
                    .get_environment()
                    .await
                    .map_err(VerificationError::State)?;

                let validator = ModuleValidator::new(&payload.module, environment);
                validator
                    .verify()
                    .map_err(|err| VerificationError::ModuleError(format!("{err:#}")))?;
            }
            TransactionType::Energy(payload) => match payload {
                EnergyPayload::FreezeTos { amount } => {
                    if *amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be greater than zero"
                        )));
                    }

                    if *amount % crate::config::COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be a whole number of TOS"
                        )));
                    }

                    if *amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be at least 1 TOS"
                        )));
                    }
                }
                EnergyPayload::UnfreezeTos { amount } => {
                    if *amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be greater than zero"
                        )));
                    }

                    if *amount % crate::config::COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be a whole number of TOS"
                        )));
                    }

                    if *amount < crate::config::MIN_UNFREEZE_TOS_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be at least 1 TOS"
                        )));
                    }

                    // Add queue capacity and delegation checks to match apply phase
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();

                    // Check queue capacity (max 32 entries)
                    if sender_energy.unfreezing_list.len()
                        >= crate::config::MAX_UNFREEZING_LIST_SIZE
                    {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreezing queue is full (max {} entries)",
                            crate::config::MAX_UNFREEZING_LIST_SIZE
                        )));
                    }

                    // Check frozen balance
                    if sender_energy.frozen_balance < *amount {
                        return Err(VerificationError::InsufficientFrozenBalance);
                    }

                    // Check available_for_delegation (can't unfreeze delegated TOS)
                    if *amount > sender_energy.available_for_delegation() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Cannot unfreeze delegated TOS"
                        )));
                    }
                }
                EnergyPayload::WithdrawExpireUnfreeze => {
                    // Check that there are unfreezing entries to withdraw
                    // Note: We can only check if the unfreezing list is non-empty during verification
                    // The actual expiry check happens at apply time when we have the block timestamp
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();

                    if sender_energy.unfreezing_list.is_empty() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "No pending unfreeze entries to withdraw"
                        )));
                    }
                }
                EnergyPayload::CancelAllUnfreeze => {
                    // Check that there are pending unfreeze entries to cancel
                    // Reject empty queue to prevent zero-cost spam transactions
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();
                    if sender_energy.unfreezing_list.is_empty() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "No pending unfreeze entries to cancel"
                        )));
                    }
                }
                EnergyPayload::DelegateResource {
                    receiver,
                    amount,
                    lock,
                    lock_period,
                } => {
                    // Check self-delegation
                    if receiver == self.get_source() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Cannot delegate to self"
                        )));
                    }
                    // Check minimum delegation amount (1 TOS)
                    if *amount < MIN_DELEGATION_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Delegation amount must be at least 1 TOS ({} atomic units)",
                            MIN_DELEGATION_AMOUNT
                        )));
                    }
                    // Check whole-TOS amount (must be multiple of COIN_VALUE)
                    if *amount % COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Delegation amount must be a whole number of TOS"
                        )));
                    }
                    if *lock_period > crate::config::MAX_DELEGATE_LOCK_DAYS {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Lock period cannot exceed 365 days"
                        )));
                    }
                    // Validate lock/lock_period consistency
                    if *lock && *lock_period == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "lock=true requires lock_period > 0"
                        )));
                    }
                    if !*lock && *lock_period > 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "lock=false should have lock_period=0 (non-zero value would be ignored)"
                        )));
                    }
                    // Check sender has sufficient available frozen balance
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();
                    if sender_energy.available_for_delegation() < *amount {
                        return Err(VerificationError::InsufficientFrozenBalance);
                    }
                }
                EnergyPayload::UndelegateResource { receiver, amount } => {
                    if *amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Undelegate amount must be greater than zero"
                        )));
                    }
                    // Check whole-TOS amount (must be multiple of COIN_VALUE)
                    // This prevents stranding fractional balances that can't be re-delegated
                    if *amount % COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Undelegate amount must be a whole number of TOS"
                        )));
                    }

                    // Check delegation exists and lock has expired
                    let sender = self.get_source();
                    if let Some(delegation) = state
                        .get_delegated_resource(sender, receiver)
                        .await
                        .map_err(VerificationError::State)?
                    {
                        // Check lock has expired (use verification timestamp)
                        let now_ms = state.get_verification_timestamp() * 1000; // Convert to ms
                        if delegation.expire_time > now_ms {
                            return Err(VerificationError::DelegationStillLocked);
                        }

                        // Check sufficient delegated balance
                        if delegation.frozen_balance < *amount {
                            return Err(VerificationError::InsufficientDelegatedBalance);
                        }

                        // Record pending undelegation for subsequent TX verification
                        state
                            .record_pending_undelegation(sender, receiver, *amount)
                            .await
                            .map_err(VerificationError::State)?;
                    } else {
                        return Err(VerificationError::DelegationNotFound);
                    }
                }
                // === Batch Operations (TOS Innovation) ===
                EnergyPayload::ActivateAccounts { accounts } => {
                    // Validate batch limits
                    if let Err(e) = payload.validate_batch_limits() {
                        return Err(VerificationError::AnyError(anyhow!("{}", e)));
                    }
                    // Check for duplicates
                    let mut seen = std::collections::HashSet::new();
                    for account in accounts {
                        if !seen.insert(account) {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Duplicate account in ActivateAccounts"
                            )));
                        }
                        // Cannot activate self
                        if account == self.get_source() {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Cannot activate self"
                            )));
                        }
                    }
                }
                EnergyPayload::BatchDelegateResource { delegations } => {
                    // Validate batch limits
                    if let Err(e) = payload.validate_batch_limits() {
                        return Err(VerificationError::AnyError(anyhow!("{}", e)));
                    }
                    // Reject duplicate receivers in batch delegation
                    let mut seen_receivers = std::collections::HashSet::new();
                    for item in delegations {
                        if !seen_receivers.insert(&item.receiver) {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Duplicate receiver in BatchDelegateResource"
                            )));
                        }
                        // Check self-delegation
                        if &item.receiver == self.get_source() {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Cannot delegate to self"
                            )));
                        }
                        // NOTE: Receiver registration check removed for implicit account model
                        // Consistent with single DelegateResource which doesn't require
                        // receiver to be registered. Receivers are implicitly created
                        // when they receive delegation.
                        // Check minimum delegation amount
                        if item.amount < MIN_DELEGATION_AMOUNT {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Delegation amount must be at least 1 TOS"
                            )));
                        }
                        // Check whole-TOS amount
                        if item.amount % COIN_VALUE != 0 {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Delegation amount must be a whole number of TOS"
                            )));
                        }
                        // Check lock period
                        if item.lock_period > crate::config::MAX_DELEGATE_LOCK_DAYS {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Lock period cannot exceed 365 days"
                            )));
                        }
                        // Validate lock/lock_period consistency
                        if item.lock && item.lock_period == 0 {
                            return Err(VerificationError::AnyError(anyhow!(
                                "lock=true requires lock_period > 0"
                            )));
                        }
                        if !item.lock && item.lock_period > 0 {
                            return Err(VerificationError::AnyError(anyhow!(
                                "lock=false should have lock_period=0 (non-zero value would be ignored)"
                            )));
                        }
                    }
                    // Check sender has sufficient available frozen balance for total delegation
                    let total_delegation: u64 = delegations
                        .iter()
                        .map(|d| d.amount)
                        .try_fold(0u64, |acc, amount| acc.checked_add(amount))
                        .ok_or(VerificationError::Overflow)?;
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();
                    if sender_energy.available_for_delegation() < total_delegation {
                        return Err(VerificationError::InsufficientFrozenBalance);
                    }
                }
                EnergyPayload::ActivateAndDelegate { items } => {
                    // Validate batch limits
                    if let Err(e) = payload.validate_batch_limits() {
                        return Err(VerificationError::AnyError(anyhow!("{}", e)));
                    }
                    // Check for duplicates
                    let mut seen = std::collections::HashSet::new();
                    for item in items {
                        if !seen.insert(&item.account) {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Duplicate account in ActivateAndDelegate"
                            )));
                        }
                        // Cannot activate/delegate to self
                        if &item.account == self.get_source() {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Cannot activate or delegate to self"
                            )));
                        }
                        // If delegating, check amount requirements
                        if item.delegate_amount > 0 {
                            if item.delegate_amount < MIN_DELEGATION_AMOUNT {
                                return Err(VerificationError::AnyError(anyhow!(
                                    "Delegation amount must be at least 1 TOS"
                                )));
                            }
                            if item.delegate_amount % COIN_VALUE != 0 {
                                return Err(VerificationError::AnyError(anyhow!(
                                    "Delegation amount must be a whole number of TOS"
                                )));
                            }
                            // Validate lock/lock_period consistency (only when delegating)
                            if item.lock && item.lock_period == 0 {
                                return Err(VerificationError::AnyError(anyhow!(
                                    "lock=true requires lock_period > 0"
                                )));
                            }
                            if !item.lock && item.lock_period > 0 {
                                return Err(VerificationError::AnyError(anyhow!(
                                    "lock=false should have lock_period=0 (non-zero value would be ignored)"
                                )));
                            }
                        }
                        // Check lock period
                        if item.lock_period > crate::config::MAX_DELEGATE_LOCK_DAYS {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Lock period cannot exceed 365 days"
                            )));
                        }
                    }
                    // Check sender has sufficient available frozen balance for total delegation
                    let total_delegation: u64 = items
                        .iter()
                        .map(|item| item.delegate_amount)
                        .try_fold(0u64, |acc, amount| acc.checked_add(amount))
                        .ok_or(VerificationError::Overflow)?;
                    if total_delegation > 0 {
                        let sender = self.get_source();
                        let sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();
                        if sender_energy.available_for_delegation() < total_delegation {
                            return Err(VerificationError::InsufficientFrozenBalance);
                        }
                    }
                }
            },
            TransactionType::AIMining(_) => {
                // AI Mining transactions don't require special verification beyond basic checks for now
            }
            TransactionType::BindReferrer(payload) => {
                // BUG-072 FIX: Validate extra_data size to prevent mempool/storage bloat
                // BindReferrer has extra_data field but was missing size validation
                if let Some(extra_data) = payload.get_extra_data() {
                    let size = extra_data.size();
                    if size > EXTRA_DATA_LIMIT_SIZE {
                        return Err(VerificationError::TransferExtraDataSize);
                    }
                }
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward validation
                if !payload.validate() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Invalid batch referral reward payload"
                    )));
                }
            }
            // KYC transaction types - structural validation (dynamic parts)
            // Uses state.get_verification_timestamp() for deterministic consensus validation
            // Block validation uses block timestamp; mempool uses system time
            TransactionType::SetKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_set_kyc(payload, current_time)?;
            }
            TransactionType::RevokeKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_revoke_kyc(payload, current_time)?;
            }
            TransactionType::RenewKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_renew_kyc(payload, current_time)?;
            }
            TransactionType::TransferKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_transfer_kyc(payload, current_time)?;
            }
            TransactionType::BootstrapCommittee(payload) => {
                // SECURITY FIX (Issue #33): Pass sender to verify only BOOTSTRAP_ADDRESS can bootstrap
                kyc::verify_bootstrap_committee(payload, &self.source)?;
            }
            TransactionType::RegisterCommittee(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_register_committee(payload, current_time)?;
            }
            TransactionType::UpdateCommittee(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_update_committee(payload, current_time)?;
            }
            TransactionType::EmergencySuspend(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_emergency_suspend(payload, current_time)?;
            }
            TransactionType::AppealKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_appeal_kyc(payload, current_time)?;
            }
            TransactionType::UnoTransfers(transfers) => {
                // UNO transfers: privacy-preserving transfers with ZKP proofs
                // Validation of extra data size and destination checks
                if transfers.is_empty() || transfers.len() > MAX_TRANSFER_COUNT {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers {
                    if *transfer.get_destination() == self.source {
                        return Err(VerificationError::SenderIsReceiver);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers: TOS -> UNO (enter privacy mode)
                // Amount is public, commitment proof required to prevent forged commitments
                if transfers.is_empty() || transfers.len() > MAX_TRANSFER_COUNT {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers {
                    // Validate amount is non-zero
                    if transfer.get_amount() == 0 {
                        return Err(VerificationError::InvalidTransferAmount);
                    }

                    // Shield transfers only support TOS asset
                    // UNO is a single-asset privacy layer for TOS only
                    if *transfer.get_asset() != TOS_ASSET {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Shield transfers only support TOS asset"
                        )));
                    }

                    // SECURITY: Verify Shield commitment proof
                    // This ensures the commitment is correctly formed for the claimed amount
                    // and prevents inflation attacks via forged commitments
                    let commitment = transfer
                        .get_commitment()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_pubkey = transfer
                        .get_destination()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_handle = transfer
                        .get_receiver_handle()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;

                    let mut transcript = Transcript::new(b"shield_commitment_proof");
                    transfer
                        .get_proof()
                        .verify(
                            &commitment,
                            &receiver_pubkey,
                            &receiver_handle,
                            transfer.get_amount(),
                            &mut transcript,
                        )
                        .map_err(|e| {
                            if log::log_enabled!(log::Level::Debug) {
                                debug!("Shield commitment proof verification failed: {:?}", e);
                            }
                            VerificationError::Proof(e)
                        })?;

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                // Unshield transfers: UNO -> TOS (exit privacy mode)
                // Amount is revealed, requires CiphertextValidityProof
                if transfers.is_empty() || transfers.len() > MAX_TRANSFER_COUNT {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers {
                    // Validate amount is non-zero
                    if transfer.get_amount() == 0 {
                        return Err(VerificationError::InvalidTransferAmount);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
        };

        // SECURITY FIX: Verify sender has sufficient balance for all spending
        // Calculate total spending per asset
        // Use references to original Hash values in transaction (they live for 'a)
        //
        // NOTE: Under Stake 2.0 Energy model, contract max_gas
        // is NOT added to spending_per_asset. Only fee_limit (added later) covers
        // the maximum TOS that can be burned when energy is insufficient.
        // This prevents double-charging and double-refunding.
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset(); // Returns &Hash
                    let amount = transfer.get_amount();

                    // TOS-Only Fee: Check account creation fee during verification
                    // This catches insufficient fee errors early (mempool validation)
                    // Also check pending registrations for same-block/same-TX visibility
                    if *asset == TOS_ASSET {
                        let destination = transfer.get_destination();
                        let is_registered = state
                            .is_account_registered(destination)
                            .await
                            .map_err(VerificationError::State)?;
                        let is_pending = state.is_pending_registration(destination);
                        let is_new_account = !is_registered && !is_pending;

                        if is_new_account {
                            if amount < FEE_PER_ACCOUNT_CREATION {
                                return Err(VerificationError::AmountTooSmallForAccountCreation {
                                    amount,
                                    fee: FEE_PER_ACCOUNT_CREATION,
                                });
                            }
                            // Record pending registration so subsequent outputs
                            // to the same new account won't fail the creation fee check
                            state.record_pending_registration(destination);
                        }
                    }

                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(&payload.asset).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                // Add deposits
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // max_gas is NOT added to TOS spending under Stake 2.0.
                // Contract execution costs are handled via the Energy model.
                // fee_limit (added below) covers the maximum TOS that can be burned.
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                // If invoking constructor, add deposits only (not max_gas - handled by Energy model)
                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Constructor max_gas NOT added under Stake 2.0
                }
            }
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount } => {
                        // FreezeTos spends TOS to add to frozen balance
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. }
                    | EnergyPayload::WithdrawExpireUnfreeze
                    | EnergyPayload::CancelAllUnfreeze
                    | EnergyPayload::UndelegateResource { .. } => {
                        // These operations don't spend TOS directly
                    }
                    EnergyPayload::DelegateResource { .. } => {
                        // DelegateResource uses frozen balance, not direct TOS spending
                    }
                    EnergyPayload::ActivateAccounts { accounts } => {
                        // ActivateAccounts spends 0.1 TOS per NEW account only (idempotent)
                        // Count only unregistered accounts to match apply phase logic
                        // Also check pending registrations for same-block visibility
                        let mut unregistered_count = 0u64;
                        for account in accounts {
                            let is_registered = state
                                .is_account_registered(account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(account);
                            if !is_registered && !is_pending {
                                unregistered_count += 1;
                            }
                        }
                        let total_fee = unregistered_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::BatchDelegateResource { .. } => {
                        // BatchDelegateResource uses frozen balance for delegation
                    }
                    EnergyPayload::ActivateAndDelegate { items } => {
                        // ActivateAndDelegate spends 0.1 TOS per NEW account only (idempotent)
                        // Count only unregistered accounts to match apply phase logic
                        // Also check pending registrations for same-block visibility
                        let mut unregistered_count = 0u64;
                        for item in items {
                            let is_registered = state
                                .is_account_registered(&item.account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(&item.account);
                            if !is_registered && !is_pending {
                                unregistered_count += 1;
                            }
                        }
                        let total_fee = unregistered_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // BUG-066 FIX: Enforce AIMining fee/stake/reward spending on-chain
                // Previously only modeled in builder, now enforced during verification
                use crate::ai_mining::AIMiningPayload;
                match payload {
                    AIMiningPayload::RegisterMiner { registration_fee, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*registration_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::SubmitAnswer { stake_amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*stake_amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::PublishTask { reward_amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*reward_amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::ValidateAnswer { .. } => {
                        // ValidateAnswer does not spend TOS directly
                    }
                }
            }
            TransactionType::MultiSig(_)
            | TransactionType::BindReferrer(_)
            // KYC transactions don't spend assets directly (only fee)
            | TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // No asset spending for these types
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward spends total_amount of the specified asset
                let current = spending_per_asset.entry(payload.get_asset()).or_insert(0);
                *current = current
                    .checked_add(payload.get_total_amount())
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::UnoTransfers(_)
            | TransactionType::ShieldTransfers(_)
            | TransactionType::UnshieldTransfers(_) => {
                // Privacy transfers spend from encrypted balances
                // Spending verification is done through ZKP proofs (CommitmentEqProof)
                // No plaintext spending to verify here
                // Shield/Unshield: actual balance checks happen in apply()
            }
        };

        // For Shield transfers, add TOS spending (plaintext balance deduction)
        if let TransactionType::ShieldTransfers(transfers) = &self.data {
            for transfer in transfers {
                let asset = transfer.get_asset();
                let amount = transfer.get_amount();
                let current = spending_per_asset.entry(asset).or_insert(0);
                *current = current
                    .checked_add(amount)
                    .ok_or(VerificationError::Overflow)?;
            }
        }

        // Energy model: fee_limit is the max TOS burned when energy is insufficient
        // Add fee_limit to TOS spending for balance verification
        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
        *current = current
            .checked_add(self.get_fee_limit())
            .ok_or(VerificationError::Overflow)?;

        // Verify sender has sufficient balance for each asset
        // CRITICAL: Mutate balance during verification (like old encrypted balance code)
        // This ensures mempool verification reduces cached balances, preventing
        // users from submitting sequential transactions that total more than their funds
        for (asset_hash, total_spending) in &spending_per_asset {
            // Use transaction's reference for balance check (pre-transaction state)
            let current_balance = state
                .get_sender_balance(&self.source, asset_hash, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            if *current_balance < *total_spending {
                return Err(VerificationError::InsufficientFunds {
                    available: *current_balance,
                    required: *total_spending,
                });
            }

            // Deduct spending from balance immediately (matches old behavior)
            // This mutation updates mempool cached balances so subsequent txs see reduced funds
            *current_balance = current_balance
                .checked_sub(*total_spending)
                .ok_or(VerificationError::Overflow)?;
        }

        // NOTE: UnfreezeTos does NOT credit balance here. Balance is credited in apply phase
        // only via WithdrawExpireUnfreeze after the 14-day waiting period.

        // Deduct sender UNO balance for UnoTransfers
        // This ensures cached UNO transactions also update balance during verification
        // Previously, only pre_verify_uno updated balance, but cached TXs use verify_dynamic_parts
        if let TransactionType::UnoTransfers(transfers) = &self.data {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "verify_dynamic_parts: Processing UnoTransfers for source {:?}",
                    self.source
                );
            }

            // Decompress transfer ciphertexts to compute total spending
            let mut output = Ciphertext::zero();
            for transfer in transfers.iter() {
                let decompressed = DecompressedUnoTransferCt::decompress(transfer)
                    .map_err(ProofVerificationError::from)?;
                output += decompressed.get_ciphertext(Role::Sender);
            }

            // Get sender's UNO balance and deduct spending
            let sender_uno_balance = state
                .get_sender_uno_balance(&self.source, &UNO_ASSET, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "verify_dynamic_parts: UnoTransfer deducting from UNO balance for source {:?}",
                    self.source
                );
            }

            *sender_uno_balance -= &output;

            // Track sender output for final balance calculation
            state
                .add_sender_uno_output(&self.source, &UNO_ASSET, output)
                .await
                .map_err(VerificationError::State)?;
        }

        // Deduct sender UNO balance for UnshieldTransfers
        // Similar to pre_verify_uno but without CommitmentEqProof (amount is plaintext)
        if let TransactionType::UnshieldTransfers(transfers) = &self.data {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "verify_dynamic_parts: Processing UnshieldTransfers for source {:?}",
                    self.source
                );
            }
            for transfer in transfers {
                // Create sender ciphertext from commitment and sender handle
                let commitment = transfer
                    .get_commitment()
                    .decompress()
                    .map_err(|_| VerificationError::InvalidFormat)?;
                let sender_handle = transfer
                    .get_sender_handle()
                    .decompress()
                    .map_err(|_| VerificationError::InvalidFormat)?;
                let sender_ct = Ciphertext::new(commitment, sender_handle);

                // Get sender's UNO balance and deduct
                let sender_uno_balance = state
                    .get_sender_uno_balance(&self.source, &UNO_ASSET, &self.reference)
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Trace) {
                    trace!("verify_dynamic_parts: UnshieldTransfer deducting from UNO balance for source {:?}", self.source);
                }

                *sender_uno_balance -= sender_ct.clone();

                // Track sender output for final balance calculation
                state
                    .add_sender_uno_output(&self.source, &UNO_ASSET, sender_ct)
                    .await
                    .map_err(VerificationError::State)?;
            }
        }

        Ok(())
    }

    /// Pre-verify UNO (privacy-preserving) transaction with ZK proof verification
    /// Returns the transcript and commitments needed for range proof verification
    async fn pre_verify_uno<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
        sigma_batch_collector: &mut BatchCollector,
    ) -> Result<(Transcript, Vec<(RistrettoPoint, CompressedRistretto)>), VerificationError<E>>
    {
        trace!("Pre-verifying UNO transaction");

        if !self.has_valid_version_format() {
            return Err(VerificationError::InvalidFormat);
        }

        // UNO transactions must have source commitments and range proof
        if self.source_commitments.is_empty() {
            return Err(VerificationError::Commitments);
        }

        let Some(ref _range_proof) = self.range_proof else {
            return Err(VerificationError::Proof(ProofVerificationError::Format));
        };

        // Verify source commitment assets match UNO transfer assets
        if !self.verify_uno_commitment_assets() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid UNO commitment assets");
            }
            return Err(VerificationError::Commitments);
        }

        // Pre-verify on state (nonce check, etc.)
        state
            .pre_verify_tx(self)
            .await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce
        //
        // NOTE (BUG-076): Nonce is incremented here BEFORE ZK proof verification completes.
        // If proofs later fail, the nonce is "burned" - subsequent transactions with the
        // expected nonce will be rejected. This is a design trade-off:
        // - Pro: Prevents nonce reuse attacks within a batch (security)
        // - Con: Failed transactions consume nonces even on proof failure
        //
        // A proper fix would require separating "check" from "apply" phases, but this
        // is a significant architectural change. For now, this behavior is documented.
        let success = state
            .compare_and_swap_nonce(&self.source, self.nonce, self.nonce + 1)
            .await
            .map_err(VerificationError::State)?;

        if !success {
            let current = state
                .get_account_nonce(&self.source)
                .await
                .map_err(VerificationError::State)?;
            return Err(VerificationError::InvalidNonce(
                tx_hash.clone(),
                self.nonce,
                current,
            ));
        }

        // BUG-070 FIX: Check sender has sufficient TOS balance for fee_limit
        // UNO transactions require plaintext TOS for energy burn when energy is insufficient
        // This must be checked BEFORE expensive ZK proof verification
        let fee_limit = self.get_fee_limit();
        if fee_limit > 0 {
            let sender_tos_balance = state
                .get_sender_balance(&self.source, &TOS_ASSET, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            if *sender_tos_balance < fee_limit {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "UNO pre-verify: Insufficient TOS balance for fee_limit. Balance: {}, fee_limit: {}",
                        sender_tos_balance, fee_limit
                    );
                }
                return Err(VerificationError::InsufficientFunds {
                    available: *sender_tos_balance,
                    required: fee_limit,
                });
            }

            // Reserve fee_limit by deducting from balance (will be refunded if TX fails or energy sufficient)
            *sender_tos_balance = sender_tos_balance
                .checked_sub(fee_limit)
                .ok_or(VerificationError::Underflow)?;
        }

        // Decompress UNO transfers
        let TransactionType::UnoTransfers(transfers) = &self.data else {
            return Err(VerificationError::InvalidFormat);
        };

        if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("incorrect UNO transfers size: {}", transfers.len());
            }
            return Err(VerificationError::TransferCount);
        }

        // Validate extra data and decompress transfers
        let mut extra_data_size = 0;
        let mut transfers_decompressed = Vec::with_capacity(transfers.len());

        for transfer in transfers.iter() {
            if *transfer.get_destination() == self.source {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("sender cannot be the receiver in the same TX");
                }
                return Err(VerificationError::SenderIsReceiver);
            }

            if let Some(extra_data) = transfer.get_extra_data() {
                let size = extra_data.size();
                if size > EXTRA_DATA_LIMIT_SIZE {
                    return Err(VerificationError::TransferExtraDataSize);
                }
                extra_data_size += size;
            }

            let decompressed = DecompressedUnoTransferCt::decompress(transfer)
                .map_err(ProofVerificationError::from)?;
            transfers_decompressed.push(decompressed);
        }

        if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
            return Err(VerificationError::TransactionExtraDataSize);
        }

        // Decompress source commitments
        let new_source_commitments_decompressed = self
            .source_commitments
            .iter()
            .map(|c| c.get_commitment().decompress())
            .collect::<Result<Vec<_>, DecompressionError>>()
            .map_err(ProofVerificationError::from)?;

        let source_decompressed = self
            .source
            .decompress()
            .map_err(|err| VerificationError::Proof(err.into()))?;

        // Prepare transcript for proof verification
        let mut transcript =
            Self::prepare_transcript(self.version, &self.source, self.get_fee_limit(), self.nonce);

        // Verify signature
        let bytes = self.get_signing_bytes();
        if !self.signature.verify(&bytes, &source_decompressed) {
            debug!("transaction signature is invalid");
            return Err(VerificationError::InvalidSignature);
        }

        // Verify multisig if configured
        if let Some(config) = state
            .get_multisig_state(&self.source)
            .await
            .map_err(VerificationError::State)?
        {
            let Some(multisig) = self.get_multisig() else {
                return Err(VerificationError::MultiSigNotFound);
            };

            if (config.threshold as usize) != multisig.len()
                || multisig.len() > MAX_MULTISIG_PARTICIPANTS
            {
                return Err(VerificationError::MultiSigParticipants);
            }

            let multisig_bytes = self.get_multisig_signing_bytes();
            let hash_val = hash(&multisig_bytes);
            for sig in multisig.get_signatures() {
                let index = sig.id as usize;
                let Some(key) = config.participants.get_index(index) else {
                    return Err(VerificationError::MultiSigParticipants);
                };

                let decompressed = key.decompress().map_err(ProofVerificationError::from)?;
                if !sig.signature.verify(hash_val.as_bytes(), &decompressed) {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Multisig signature verification failed for participant {index}");
                    }
                    return Err(VerificationError::InvalidSignature);
                }
            }
        } else if self.get_multisig().is_some() {
            return Err(VerificationError::MultiSigNotConfigured);
        }

        // 1. Verify CommitmentEqProofs for source balances
        trace!("verifying UNO commitments eq proofs");

        for (commitment, new_source_commitment) in self
            .source_commitments
            .iter()
            .zip(&new_source_commitments_decompressed)
        {
            // Calculate output ciphertext (total spending for this asset)
            let output =
                self.get_uno_sender_output_ct(commitment.get_asset(), &transfers_decompressed);

            // Get sender's UNO balance ciphertext
            let source_verification_ciphertext = state
                .get_sender_uno_balance(&self.source, commitment.get_asset(), &self.reference)
                .await
                .map_err(VerificationError::State)?;

            let source_ct_compressed = source_verification_ciphertext.compress();

            // Compute new balance: old_balance - output
            //
            // NOTE (BUG-077): Balance is mutated here BEFORE range proof verification completes.
            // Range proofs are verified later in batch (verify_batch -> spawn_blocking_safe).
            // If proofs later fail, this balance mutation persists in verification state,
            // causing subsequent UNO transactions to see incorrect balances.
            //
            // This is a design trade-off similar to BUG-076:
            // - Pro: Prevents double-spending within a batch (security)
            // - Con: Failed transactions leave reduced balance in verification state
            //
            // The balance reduction is intentional for batch verification correctness.
            // A proper fix would require a two-phase verification approach.
            *source_verification_ciphertext -= &output;

            // Prepare transcript for CommitmentEqProof
            transcript.new_commitment_eq_proof_domain_separator();
            transcript.append_hash(b"new_source_commitment_asset", commitment.get_asset());
            transcript.append_commitment(b"new_source_commitment", commitment.get_commitment());
            transcript.append_ciphertext(b"source_ct", &source_ct_compressed);

            // Pre-verify the equality proof (adds to batch collector)
            commitment.get_proof().pre_verify(
                &source_decompressed,
                source_verification_ciphertext,
                new_source_commitment,
                &mut transcript,
                sigma_batch_collector,
            )?;

            // Track sender output for final balance
            state
                .add_sender_uno_output(&self.source, commitment.get_asset(), output)
                .await
                .map_err(VerificationError::State)?;
        }

        // 2. Verify CiphertextValidityProofs for transfers
        trace!("verifying UNO transfer ciphertext validity proofs");

        let mut value_commitments: Vec<(RistrettoPoint, CompressedRistretto)> = Vec::new();

        for (transfer, decompressed) in transfers.iter().zip(&transfers_decompressed) {
            let receiver = transfer
                .get_destination()
                .decompress()
                .map_err(ProofVerificationError::from)?;

            // Update receiver's UNO balance
            let current_balance = state
                .get_receiver_uno_balance(
                    Cow::Borrowed(transfer.get_destination()),
                    Cow::Borrowed(transfer.get_asset()),
                )
                .await
                .map_err(VerificationError::State)?;

            let receiver_ct = decompressed.get_ciphertext(Role::Receiver);
            *current_balance += receiver_ct;

            // Prepare transcript for CiphertextValidityProof
            transcript.transfer_proof_domain_separator();
            transcript.append_public_key(b"dest_pubkey", transfer.get_destination());
            transcript.append_commitment(b"amount_commitment", transfer.get_commitment());
            transcript.append_handle(b"amount_sender_handle", transfer.get_sender_handle());
            transcript.append_handle(b"amount_receiver_handle", transfer.get_receiver_handle());

            // Pre-verify the validity proof (adds to batch collector)
            transfer.get_proof().pre_verify(
                &decompressed.commitment,
                &receiver,
                &source_decompressed,
                &decompressed.receiver_handle,
                &decompressed.sender_handle,
                self.version,
                &mut transcript,
                sigma_batch_collector,
            )?;

            // Collect commitment for range proof
            value_commitments.push((
                *decompressed.commitment.as_point(),
                *transfer.get_commitment().as_point(),
            ));
        }

        // 3. Prepare commitments for range proof verification
        // Count total commitments (source + transfer)
        let n_commitments = self.source_commitments.len() + value_commitments.len();
        let n_dud_commitments = n_commitments
            .checked_next_power_of_two()
            .ok_or(ProofVerificationError::Format)?
            - n_commitments;

        // Combine source and transfer commitments
        let final_commitments = self
            .source_commitments
            .iter()
            .zip(new_source_commitments_decompressed)
            .map(|(commitment, new_source_commitment)| {
                (
                    new_source_commitment.to_point(),
                    *commitment.get_commitment().as_point(),
                )
            })
            .chain(value_commitments)
            .chain(iter::repeat_n(
                (RistrettoPoint::identity(), CompressedRistretto::identity()),
                n_dud_commitments,
            ))
            .collect();

        Ok((transcript, final_commitments))
    }

    /// Pre-verify an Unshield transaction (UNO -> TOS)
    /// Returns the transcript and commitments needed for range proof batch verification
    async fn pre_verify_unshield<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
        sigma_batch_collector: &mut BatchCollector,
    ) -> Result<(Transcript, Vec<(RistrettoPoint, CompressedRistretto)>), VerificationError<E>>
    {
        trace!("Pre-verifying Unshield transaction");

        if !self.has_valid_version_format() {
            return Err(VerificationError::InvalidFormat);
        }

        // Unshield transactions must have source commitments and range proof
        if self.source_commitments.is_empty() {
            return Err(VerificationError::Commitments);
        }

        let Some(ref _range_proof) = self.range_proof else {
            return Err(VerificationError::Proof(ProofVerificationError::Format));
        };

        // Verify source commitment assets match Unshield transfer assets
        if !self.verify_unshield_commitment_assets() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid Unshield commitment assets");
            }
            return Err(VerificationError::Commitments);
        }

        // Pre-verify on state (nonce check, etc.)
        state
            .pre_verify_tx(self)
            .await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce
        //
        // NOTE (BUG-076): See pre_verify_uno for explanation of nonce burning trade-off.
        let success = state
            .compare_and_swap_nonce(&self.source, self.nonce, self.nonce + 1)
            .await
            .map_err(VerificationError::State)?;

        if !success {
            let current = state
                .get_account_nonce(&self.source)
                .await
                .map_err(VerificationError::State)?;
            return Err(VerificationError::InvalidNonce(
                tx_hash.clone(),
                self.nonce,
                current,
            ));
        }

        // BUG-070 FIX: Check sender has sufficient TOS balance for fee_limit
        // Unshield transactions require plaintext TOS for energy burn when energy is insufficient
        // This must be checked BEFORE expensive ZK proof verification
        let fee_limit = self.get_fee_limit();
        if fee_limit > 0 {
            let sender_tos_balance = state
                .get_sender_balance(&self.source, &TOS_ASSET, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            if *sender_tos_balance < fee_limit {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Unshield pre-verify: Insufficient TOS balance for fee_limit. Balance: {}, fee_limit: {}",
                        sender_tos_balance, fee_limit
                    );
                }
                return Err(VerificationError::InsufficientFunds {
                    available: *sender_tos_balance,
                    required: fee_limit,
                });
            }

            // Reserve fee_limit by deducting from balance (will be refunded if TX fails or energy sufficient)
            *sender_tos_balance = sender_tos_balance
                .checked_sub(fee_limit)
                .ok_or(VerificationError::Underflow)?;
        }

        // Decompress Unshield transfers
        let TransactionType::UnshieldTransfers(transfers) = &self.data else {
            return Err(VerificationError::InvalidFormat);
        };

        if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("incorrect Unshield transfers size: {}", transfers.len());
            }
            return Err(VerificationError::TransferCount);
        }

        // Validate extra data and decompress transfers
        // Note: For Unshield, sender == receiver is valid (unshielding to own address)
        let mut extra_data_size = 0;
        let mut transfers_decompressed = Vec::with_capacity(transfers.len());

        for transfer in transfers.iter() {
            if let Some(extra_data) = transfer.get_extra_data() {
                let size = extra_data.size();
                if size > EXTRA_DATA_LIMIT_SIZE {
                    return Err(VerificationError::TransferExtraDataSize);
                }
                extra_data_size += size;
            }

            let decompressed = DecompressedUnshieldTransferCt::decompress(transfer)
                .map_err(ProofVerificationError::from)?;
            transfers_decompressed.push(decompressed);
        }

        if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
            return Err(VerificationError::TransactionExtraDataSize);
        }

        // Decompress source commitments
        let new_source_commitments_decompressed = self
            .source_commitments
            .iter()
            .map(|c| c.get_commitment().decompress())
            .collect::<Result<Vec<_>, DecompressionError>>()
            .map_err(ProofVerificationError::from)?;

        let source_decompressed = self
            .source
            .decompress()
            .map_err(|err| VerificationError::Proof(err.into()))?;

        // Prepare transcript for proof verification
        let mut transcript =
            Self::prepare_transcript(self.version, &self.source, self.get_fee_limit(), self.nonce);

        // Verify signature
        let bytes = self.get_signing_bytes();
        if !self.signature.verify(&bytes, &source_decompressed) {
            debug!("transaction signature is invalid");
            return Err(VerificationError::InvalidSignature);
        }

        // Verify multisig if configured
        if let Some(config) = state
            .get_multisig_state(&self.source)
            .await
            .map_err(VerificationError::State)?
        {
            let Some(multisig) = self.get_multisig() else {
                return Err(VerificationError::MultiSigNotFound);
            };

            if (config.threshold as usize) != multisig.len()
                || multisig.len() > MAX_MULTISIG_PARTICIPANTS
            {
                return Err(VerificationError::MultiSigParticipants);
            }

            let multisig_bytes = self.get_multisig_signing_bytes();
            let hash_val = hash(&multisig_bytes);
            for sig in multisig.get_signatures() {
                let index = sig.id as usize;
                let Some(key) = config.participants.get_index(index) else {
                    return Err(VerificationError::MultiSigParticipants);
                };

                let decompressed = key.decompress().map_err(ProofVerificationError::from)?;
                if !sig.signature.verify(hash_val.as_bytes(), &decompressed) {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Multisig signature verification failed for participant {index}");
                    }
                    return Err(VerificationError::InvalidSignature);
                }
            }
        } else if self.get_multisig().is_some() {
            return Err(VerificationError::MultiSigNotConfigured);
        }

        // IMPORTANT: Proof verification order MUST match proof generation order in build_unshield_unsigned!
        // Generation order: 1) CiphertextValidityProofs for transfers, 2) CommitmentEqProof for source
        // The transcript state must be identical between generation and verification.

        // 1. Verify CiphertextValidityProofs for transfers (FIRST - matches generation order)
        trace!("verifying Unshield transfer ciphertext validity proofs");

        let mut value_commitments: Vec<(RistrettoPoint, CompressedRistretto)> = Vec::new();

        for (transfer, decompressed) in transfers.iter().zip(&transfers_decompressed) {
            // For Unshield, the destination receives plaintext TOS, not encrypted UNO
            // So we don't update receiver's UNO balance here

            // Prepare transcript for CiphertextValidityProof
            transcript.transfer_proof_domain_separator();
            transcript.append_public_key(b"dest_pubkey", transfer.get_destination());
            transcript.append_commitment(b"amount_commitment", transfer.get_commitment());
            transcript.append_handle(b"amount_sender_handle", transfer.get_sender_handle());

            // For Unshield, we verify the sender handle proves the commitment matches the amount
            // The receiver is a plaintext recipient (no receiver handle needed)
            let dest_pubkey = transfer
                .get_destination()
                .decompress()
                .map_err(ProofVerificationError::from)?;

            // Pre-verify the validity proof (adds to batch collector)
            // Use sender handle for both since receiver gets plaintext
            transfer.get_proof().pre_verify(
                &decompressed.commitment,
                &source_decompressed,
                &dest_pubkey,
                &decompressed.sender_handle,
                &decompressed.sender_handle,
                self.version,
                &mut transcript,
                sigma_batch_collector,
            )?;

            // Collect commitment for range proof
            value_commitments.push((
                *decompressed.commitment.as_point(),
                *transfer.get_commitment().as_point(),
            ));
        }

        // 2. Verify CommitmentEqProofs for source balances (SECOND - matches generation order)
        trace!("verifying Unshield commitments eq proofs");

        for (commitment, new_source_commitment) in self
            .source_commitments
            .iter()
            .zip(&new_source_commitments_decompressed)
        {
            // Calculate output ciphertext (total spending for this asset)
            let output = self.get_unshield_sender_output_ct(&transfers_decompressed);

            // Get sender's UNO balance ciphertext
            let source_verification_ciphertext = state
                .get_sender_uno_balance(&self.source, commitment.get_asset(), &self.reference)
                .await
                .map_err(VerificationError::State)?;

            let source_ct_compressed = source_verification_ciphertext.compress();

            // Compute new balance: old_balance - output
            *source_verification_ciphertext -= &output;

            // Prepare transcript for CommitmentEqProof
            transcript.new_commitment_eq_proof_domain_separator();
            transcript.append_hash(b"new_source_commitment_asset", commitment.get_asset());
            transcript.append_commitment(b"new_source_commitment", commitment.get_commitment());

            // Only append source_ct for version >= T0 (matches generation)
            if self.version >= TxVersion::T0 {
                transcript.append_ciphertext(b"source_ct", &source_ct_compressed);
            }

            // Pre-verify the equality proof (adds to batch collector)
            commitment.get_proof().pre_verify(
                &source_decompressed,
                source_verification_ciphertext,
                new_source_commitment,
                &mut transcript,
                sigma_batch_collector,
            )?;

            // Track sender output for final balance
            state
                .add_sender_uno_output(&self.source, commitment.get_asset(), output)
                .await
                .map_err(VerificationError::State)?;
        }

        // 3. Prepare commitments for range proof verification
        // Count total commitments (source + transfer)
        let n_commitments = self.source_commitments.len() + value_commitments.len();
        let n_dud_commitments = n_commitments
            .checked_next_power_of_two()
            .ok_or(ProofVerificationError::Format)?
            - n_commitments;

        // Combine source and transfer commitments
        let final_commitments = self
            .source_commitments
            .iter()
            .zip(new_source_commitments_decompressed)
            .map(|(commitment, new_source_commitment)| {
                (
                    new_source_commitment.to_point(),
                    *commitment.get_commitment().as_point(),
                )
            })
            .chain(value_commitments)
            .chain(iter::repeat_n(
                (RistrettoPoint::identity(), CompressedRistretto::identity()),
                n_dud_commitments,
            ))
            .collect();

        Ok((transcript, final_commitments))
    }

    // This method no longer needs to return transcript or commitments
    // Signature kept for compatibility during refactoring
    async fn pre_verify<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        trace!("Pre-verifying transaction");
        if !self.has_valid_version_format() {
            return Err(VerificationError::InvalidFormat);
        }

        // All transactions use the Energy model (Stake 2.0)
        // No special validation needed for fee types

        // Validate fee_limit does not exceed MAX_FEE_LIMIT
        let fee_limit = self.get_fee_limit();
        if fee_limit > MAX_FEE_LIMIT {
            return Err(VerificationError::FeeLimitExceedsMax {
                provided: fee_limit,
                max: MAX_FEE_LIMIT,
            });
        }

        trace!("Pre-verifying transaction on state");
        state
            .pre_verify_tx(self)
            .await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce to prevent TOCTOU race condition
        let success = state
            .compare_and_swap_nonce(
                &self.source,
                self.nonce,     // Expected value
                self.nonce + 1, // New value
            )
            .await
            .map_err(VerificationError::State)?;

        if !success {
            // CAS failed, get current nonce for error reporting
            let current = state
                .get_account_nonce(&self.source)
                .await
                .map_err(VerificationError::State)?;
            return Err(VerificationError::InvalidNonce(
                tx_hash.clone(),
                self.nonce,
                current,
            ));
        }

        // Balance simplification: No commitment verification needed
        // Balance simplification: No decompression needed for plaintext balances

        match &self.data {
            TransactionType::Transfers(transfers) => {
                if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("incorrect transfers size: {}", transfers.len());
                    }
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                // Prevent sending to ourself
                for transfer in transfers.iter() {
                    if *transfer.get_destination() == self.source {
                        debug!("sender cannot be the receiver in the same TX");
                        return Err(VerificationError::SenderIsReceiver);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }

                    // Balance simplification: No decompression needed
                }

                // Check the sum of extra data size
                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::Burn(payload) => {
                let fee = self.get_fee_limit();
                let amount = payload.amount;

                if amount == 0 {
                    return Err(VerificationError::InvalidFormat);
                }

                let total = fee
                    .checked_add(amount)
                    .ok_or(VerificationError::InvalidFormat)?;

                if total < fee || total < amount {
                    return Err(VerificationError::InvalidFormat);
                }
            }
            TransactionType::MultiSig(payload) => {
                if payload.participants.len() > MAX_MULTISIG_PARTICIPANTS {
                    return Err(VerificationError::MultiSigParticipants);
                }

                // Threshold should be less than or equal to the number of participants
                if payload.threshold as usize > payload.participants.len() {
                    return Err(VerificationError::MultiSigThreshold);
                }

                // If the threshold is set to 0, while we have participants, its invalid
                // Threshold should be always > 0
                if payload.threshold == 0 && !payload.participants.is_empty() {
                    return Err(VerificationError::MultiSigThreshold);
                }

                // You can't contains yourself in the participants
                if payload.participants.contains(self.get_source()) {
                    return Err(VerificationError::MultiSigParticipants);
                }

                let is_reset = payload.threshold == 0 && payload.participants.is_empty();
                // If the multisig is reset, we need to check if it was already configured
                if is_reset
                    && state
                        .get_multisig_state(&self.source)
                        .await
                        .map_err(VerificationError::State)?
                        .is_none()
                {
                    return Err(VerificationError::MultiSigNotConfigured);
                }
            }
            TransactionType::InvokeContract(payload) => {
                self.verify_invoke_contract(&payload.deposits, payload.max_gas)?;

                // We need to load the contract module if not already in cache
                if !self.is_contract_available(state, &payload.contract).await? {
                    return Err(VerificationError::ContractNotFound);
                }

                let (module, environment) = state
                    .get_contract_module_with_environment(&payload.contract)
                    .await
                    .map_err(VerificationError::State)?;

                // TAKO contracts: entry_id validation is handled by tos-tbpf at runtime
                // Just verify the parameters are valid ValueCells
                let validator = ModuleValidator::new(module, environment);
                for constant in payload.parameters.iter() {
                    validator
                        .verify_constant(constant)
                        .map_err(|err| VerificationError::ModuleError(format!("{err:#}")))?;
                }
            }
            TransactionType::DeployContract(payload) => {
                if let Some(invoke) = payload.invoke.as_ref() {
                    self.verify_invoke_contract(&invoke.deposits, invoke.max_gas)?;
                }

                // Compute deterministic contract address using eBPF bytecode
                let bytecode = payload
                    .module
                    .get_bytecode()
                    .map(|b| b.to_vec())
                    .unwrap_or_default();

                let contract_address =
                    crate::crypto::compute_deterministic_contract_address(&self.source, &bytecode);

                // Check if contract already exists at this deterministic address
                if state
                    .load_contract_module(&contract_address)
                    .await
                    .map_err(VerificationError::State)?
                {
                    return Err(VerificationError::ContractAlreadyExists(
                        contract_address.clone(),
                    ));
                }

                // CRITICAL: Reserve address immediately to prevent front-running
                // This ensures subsequent TXs in the same block will see this address as occupied
                // during their pre-verification phase, blocking duplicate deployments
                state
                    .set_contract_module(&contract_address, &payload.module)
                    .await
                    .map_err(VerificationError::State)?;

                let environment = state
                    .get_environment()
                    .await
                    .map_err(VerificationError::State)?;

                let validator = ModuleValidator::new(&payload.module, environment);
                validator
                    .verify()
                    .map_err(|err| VerificationError::ModuleError(format!("{err:#}")))?;
            }
            TransactionType::Energy(payload) => match payload {
                EnergyPayload::FreezeTos { amount } => {
                    if *amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be greater than zero"
                        )));
                    }

                    if *amount % crate::config::COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be a whole number of TOS"
                        )));
                    }

                    if *amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be at least 1 TOS"
                        )));
                    }
                }
                EnergyPayload::UnfreezeTos { amount } => {
                    if *amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be greater than zero"
                        )));
                    }

                    if *amount % crate::config::COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be a whole number of TOS"
                        )));
                    }

                    if *amount < crate::config::MIN_UNFREEZE_TOS_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be at least 1 TOS"
                        )));
                    }

                    // Add queue capacity and delegation checks to match apply phase
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();

                    // Check queue capacity (max 32 entries)
                    if sender_energy.unfreezing_list.len()
                        >= crate::config::MAX_UNFREEZING_LIST_SIZE
                    {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreezing queue is full (max {} entries)",
                            crate::config::MAX_UNFREEZING_LIST_SIZE
                        )));
                    }

                    // Check frozen balance
                    if sender_energy.frozen_balance < *amount {
                        return Err(VerificationError::InsufficientFrozenBalance);
                    }

                    // Check available_for_delegation (can't unfreeze delegated TOS)
                    if *amount > sender_energy.available_for_delegation() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Cannot unfreeze delegated TOS"
                        )));
                    }
                }
                EnergyPayload::WithdrawExpireUnfreeze => {
                    // Check that there are unfreezing entries to withdraw
                    // Note: We can only check if the unfreezing list is non-empty during verification
                    // The actual expiry check happens at apply time when we have the block timestamp
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();

                    if sender_energy.unfreezing_list.is_empty() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "No pending unfreeze entries to withdraw"
                        )));
                    }
                }
                EnergyPayload::CancelAllUnfreeze => {
                    // Check that there are pending unfreeze entries to cancel
                    // Reject empty queue to prevent zero-cost spam transactions
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();
                    if sender_energy.unfreezing_list.is_empty() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "No pending unfreeze entries to cancel"
                        )));
                    }
                }
                EnergyPayload::DelegateResource {
                    receiver,
                    amount,
                    lock,
                    lock_period,
                } => {
                    // Check self-delegation
                    if receiver == self.get_source() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Cannot delegate to self"
                        )));
                    }
                    // Check minimum delegation amount (1 TOS)
                    if *amount < MIN_DELEGATION_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Delegation amount must be at least 1 TOS ({} atomic units)",
                            MIN_DELEGATION_AMOUNT
                        )));
                    }
                    // Check whole-TOS amount (must be multiple of COIN_VALUE)
                    if *amount % COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Delegation amount must be a whole number of TOS"
                        )));
                    }
                    if *lock_period > crate::config::MAX_DELEGATE_LOCK_DAYS {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Lock period cannot exceed 365 days"
                        )));
                    }
                    // Validate lock/lock_period consistency
                    if *lock && *lock_period == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "lock=true requires lock_period > 0"
                        )));
                    }
                    if !*lock && *lock_period > 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "lock=false should have lock_period=0 (non-zero value would be ignored)"
                        )));
                    }
                    // Check sender has sufficient available frozen balance
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();
                    if sender_energy.available_for_delegation() < *amount {
                        return Err(VerificationError::InsufficientFrozenBalance);
                    }
                }
                EnergyPayload::UndelegateResource { receiver, amount } => {
                    if *amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Undelegate amount must be greater than zero"
                        )));
                    }
                    // Check whole-TOS amount (must be multiple of COIN_VALUE)
                    // This prevents stranding fractional balances that can't be re-delegated
                    if *amount % COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Undelegate amount must be a whole number of TOS"
                        )));
                    }

                    // Check delegation exists and lock has expired
                    let sender = self.get_source();
                    if let Some(delegation) = state
                        .get_delegated_resource(sender, receiver)
                        .await
                        .map_err(VerificationError::State)?
                    {
                        // Check lock has expired (use verification timestamp)
                        let now_ms = state.get_verification_timestamp() * 1000; // Convert to ms
                        if delegation.expire_time > now_ms {
                            return Err(VerificationError::DelegationStillLocked);
                        }

                        // Check sufficient delegated balance
                        if delegation.frozen_balance < *amount {
                            return Err(VerificationError::InsufficientDelegatedBalance);
                        }

                        // Record pending undelegation for subsequent TX verification
                        state
                            .record_pending_undelegation(sender, receiver, *amount)
                            .await
                            .map_err(VerificationError::State)?;
                    } else {
                        return Err(VerificationError::DelegationNotFound);
                    }
                }
                // === Batch Operations (TOS Innovation) ===
                EnergyPayload::ActivateAccounts { accounts } => {
                    // Validate batch limits
                    if let Err(e) = payload.validate_batch_limits() {
                        return Err(VerificationError::AnyError(anyhow!("{}", e)));
                    }
                    // Check for duplicates
                    let mut seen = std::collections::HashSet::new();
                    for account in accounts {
                        if !seen.insert(account) {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Duplicate account in ActivateAccounts"
                            )));
                        }
                        // Cannot activate self
                        if account == self.get_source() {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Cannot activate self"
                            )));
                        }
                    }
                }
                EnergyPayload::BatchDelegateResource { delegations } => {
                    // Validate batch limits
                    if let Err(e) = payload.validate_batch_limits() {
                        return Err(VerificationError::AnyError(anyhow!("{}", e)));
                    }
                    // Reject duplicate receivers in batch delegation
                    let mut seen_receivers = std::collections::HashSet::new();
                    for item in delegations {
                        if !seen_receivers.insert(&item.receiver) {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Duplicate receiver in BatchDelegateResource"
                            )));
                        }
                        // Check self-delegation
                        if &item.receiver == self.get_source() {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Cannot delegate to self"
                            )));
                        }
                        // NOTE: Receiver registration check removed for implicit account model
                        // Consistent with single DelegateResource which doesn't require
                        // receiver to be registered. Receivers are implicitly created
                        // when they receive delegation.
                        // Check minimum delegation amount
                        if item.amount < MIN_DELEGATION_AMOUNT {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Delegation amount must be at least 1 TOS"
                            )));
                        }
                        // Check whole-TOS amount
                        if item.amount % COIN_VALUE != 0 {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Delegation amount must be a whole number of TOS"
                            )));
                        }
                        // Check lock period
                        if item.lock_period > crate::config::MAX_DELEGATE_LOCK_DAYS {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Lock period cannot exceed 365 days"
                            )));
                        }
                        // Validate lock/lock_period consistency
                        if item.lock && item.lock_period == 0 {
                            return Err(VerificationError::AnyError(anyhow!(
                                "lock=true requires lock_period > 0"
                            )));
                        }
                        if !item.lock && item.lock_period > 0 {
                            return Err(VerificationError::AnyError(anyhow!(
                                "lock=false should have lock_period=0 (non-zero value would be ignored)"
                            )));
                        }
                    }
                    // Check sender has sufficient available frozen balance for total delegation
                    let total_delegation: u64 = delegations
                        .iter()
                        .map(|d| d.amount)
                        .try_fold(0u64, |acc, amount| acc.checked_add(amount))
                        .ok_or(VerificationError::Overflow)?;
                    let sender = self.get_source();
                    let sender_energy = state
                        .get_account_energy(sender)
                        .await
                        .map_err(VerificationError::State)?
                        .unwrap_or_default();
                    if sender_energy.available_for_delegation() < total_delegation {
                        return Err(VerificationError::InsufficientFrozenBalance);
                    }
                }
                EnergyPayload::ActivateAndDelegate { items } => {
                    // Validate batch limits
                    if let Err(e) = payload.validate_batch_limits() {
                        return Err(VerificationError::AnyError(anyhow!("{}", e)));
                    }
                    // Check for duplicates
                    let mut seen = std::collections::HashSet::new();
                    for item in items {
                        if !seen.insert(&item.account) {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Duplicate account in ActivateAndDelegate"
                            )));
                        }
                        // Cannot activate/delegate to self
                        if &item.account == self.get_source() {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Cannot activate or delegate to self"
                            )));
                        }
                        // If delegating, check amount requirements
                        if item.delegate_amount > 0 {
                            if item.delegate_amount < MIN_DELEGATION_AMOUNT {
                                return Err(VerificationError::AnyError(anyhow!(
                                    "Delegation amount must be at least 1 TOS"
                                )));
                            }
                            if item.delegate_amount % COIN_VALUE != 0 {
                                return Err(VerificationError::AnyError(anyhow!(
                                    "Delegation amount must be a whole number of TOS"
                                )));
                            }
                            // Validate lock/lock_period consistency (only when delegating)
                            if item.lock && item.lock_period == 0 {
                                return Err(VerificationError::AnyError(anyhow!(
                                    "lock=true requires lock_period > 0"
                                )));
                            }
                            if !item.lock && item.lock_period > 0 {
                                return Err(VerificationError::AnyError(anyhow!(
                                    "lock=false should have lock_period=0 (non-zero value would be ignored)"
                                )));
                            }
                        }
                        // Check lock period
                        if item.lock_period > crate::config::MAX_DELEGATE_LOCK_DAYS {
                            return Err(VerificationError::AnyError(anyhow!(
                                "Lock period cannot exceed 365 days"
                            )));
                        }
                    }
                    // Check sender has sufficient available frozen balance for total delegation
                    let total_delegation: u64 = items
                        .iter()
                        .map(|item| item.delegate_amount)
                        .try_fold(0u64, |acc, amount| acc.checked_add(amount))
                        .ok_or(VerificationError::Overflow)?;
                    if total_delegation > 0 {
                        let sender = self.get_source();
                        let sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();
                        if sender_energy.available_for_delegation() < total_delegation {
                            return Err(VerificationError::InsufficientFrozenBalance);
                        }
                    }
                }
            },
            TransactionType::AIMining(_) => {
                // AI Mining transactions don't require special verification beyond basic checks for now
            }
            TransactionType::BindReferrer(payload) => {
                // BUG-072 FIX: Validate extra_data size to prevent mempool/storage bloat
                if let Some(extra_data) = payload.get_extra_data() {
                    let size = extra_data.size();
                    if size > EXTRA_DATA_LIMIT_SIZE {
                        return Err(VerificationError::TransferExtraDataSize);
                    }
                }
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward validation - check payload is valid
                if !payload.validate() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Invalid batch referral reward payload"
                    )));
                }
                // Authorization: sender must be the from_user (prevents abuse)
                if self.get_source() != payload.get_from_user() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "BatchReferralReward sender must be the from_user"
                    )));
                }
            }
            // KYC transaction types - structural validation (pre-verify)
            // Uses state.get_verification_timestamp() for deterministic consensus validation
            TransactionType::SetKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_set_kyc(payload, current_time)?;
            }
            TransactionType::RevokeKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_revoke_kyc(payload, current_time)?;
            }
            TransactionType::RenewKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_renew_kyc(payload, current_time)?;
            }
            TransactionType::TransferKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_transfer_kyc(payload, current_time)?;
            }
            TransactionType::AppealKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_appeal_kyc(payload, current_time)?;
            }
            TransactionType::BootstrapCommittee(payload) => {
                // SECURITY FIX (Issue #33): Pass sender to verify only BOOTSTRAP_ADDRESS can bootstrap
                kyc::verify_bootstrap_committee(payload, &self.source)?;
            }
            TransactionType::RegisterCommittee(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_register_committee(payload, current_time)?;
            }
            TransactionType::UpdateCommittee(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_update_committee(payload, current_time)?;
            }
            TransactionType::EmergencySuspend(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_emergency_suspend(payload, current_time)?;
            }
            TransactionType::UnoTransfers(transfers) => {
                // UNO transfers: privacy-preserving transfers with ZKP proofs
                if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("incorrect UNO transfers size: {}", transfers.len());
                    }
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers.iter() {
                    if *transfer.get_destination() == self.source {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("sender cannot be the receiver in the same TX");
                        }
                        return Err(VerificationError::SenderIsReceiver);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers: TOS -> UNO
                if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers.iter() {
                    if transfer.get_amount() == 0 {
                        return Err(VerificationError::InvalidTransferAmount);
                    }

                    // Shield transfers only support TOS asset
                    // UNO is a single-asset privacy layer for TOS only
                    if *transfer.get_asset() != TOS_ASSET {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Shield transfers only support TOS asset"
                        )));
                    }

                    // SECURITY: Verify Shield commitment proof
                    // This ensures the commitment is correctly formed for the claimed amount
                    // and prevents inflation attacks via forged commitments
                    let commitment = transfer
                        .get_commitment()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_pubkey = transfer
                        .get_destination()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_handle = transfer
                        .get_receiver_handle()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;

                    let mut transcript = Transcript::new(b"shield_commitment_proof");
                    transfer
                        .get_proof()
                        .verify(
                            &commitment,
                            &receiver_pubkey,
                            &receiver_handle,
                            transfer.get_amount(),
                            &mut transcript,
                        )
                        .map_err(|e| {
                            if log::log_enabled!(log::Level::Debug) {
                                debug!("Shield commitment proof verification failed: {:?}", e);
                            }
                            VerificationError::Proof(e)
                        })?;

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                // Unshield transfers: UNO -> TOS
                if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers.iter() {
                    if transfer.get_amount() == 0 {
                        return Err(VerificationError::InvalidTransferAmount);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
        };

        let source_decompressed = self
            .source
            .decompress()
            .map_err(|err| VerificationError::Proof(err.into()))?;

        // 0.a Verify chain_id for T1+ transactions (cross-network replay protection)
        if self.version >= TxVersion::T1 {
            let expected_chain_id = state.get_network().chain_id() as u8;
            if self.chain_id != expected_chain_id {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "transaction chain_id mismatch: expected {}, got {}",
                        expected_chain_id, self.chain_id
                    );
                }
                return Err(VerificationError::InvalidChainId {
                    expected: expected_chain_id,
                    got: self.chain_id,
                });
            }
        }

        // 0.b Verify Signature
        let bytes = self.get_signing_bytes();
        if !self.signature.verify(&bytes, &source_decompressed) {
            debug!("transaction signature is invalid");
            return Err(VerificationError::InvalidSignature);
        }

        // 0.c Verify multisig
        if let Some(config) = state
            .get_multisig_state(&self.source)
            .await
            .map_err(VerificationError::State)?
        {
            let Some(multisig) = self.get_multisig() else {
                return Err(VerificationError::MultiSigNotFound);
            };

            if (config.threshold as usize) != multisig.len()
                || multisig.len() > MAX_MULTISIG_PARTICIPANTS
            {
                return Err(VerificationError::MultiSigParticipants);
            }

            // Multisig participants sign the transaction data without the multisig field
            let multisig_bytes = self.get_multisig_signing_bytes();
            let hash = hash(&multisig_bytes);
            for sig in multisig.get_signatures() {
                // A participant can't sign more than once because of the IndexSet (SignatureId impl Hash on id)
                let index = sig.id as usize;
                let Some(key) = config.participants.get_index(index) else {
                    return Err(VerificationError::MultiSigParticipants);
                };

                let decompressed = key.decompress().map_err(ProofVerificationError::from)?;
                if !sig.signature.verify(hash.as_bytes(), &decompressed) {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Multisig signature verification failed for participant {index}");
                    }
                    return Err(VerificationError::InvalidSignature);
                }
            }
        } else if self.get_multisig().is_some() {
            return Err(VerificationError::MultiSigNotConfigured);
        }

        // Balance verification handled by plaintext balance system
        trace!("Balance verification handled by plaintext balance system");

        // With plaintext balances, we no longer need:
        // - CiphertextValidityProof verification
        // - Pedersen commitment verification
        // - Twisted ElGamal decrypt handle verification

        // New plaintext approach (to be implemented):
        // For each transfer:
        // 1. Get receiver's current balance from state
        // 2. Add transfer.amount to receiver balance
        // 3. Update state with new receiver balance

        trace!("Processing transfers with plaintext amounts");

        // NOTE: Transfer verification is implemented below in the spending_per_asset logic (lines 700-709)
        // where all transfer amounts are accumulated and verified against sender balances.
        match &self.data {
            TransactionType::Transfers(_transfers) => {
                // Transfer verification happens in spending_per_asset accumulation below
            }
            TransactionType::Burn(_payload) => {}
            TransactionType::MultiSig(payload) => {
                // Setup the multisig
                state
                    .set_multisig_state(&self.source, payload)
                    .await
                    .map_err(VerificationError::State)?;
            }
            TransactionType::InvokeContract(payload) => {
                let dest_pubkey = PublicKey::from_hash(&payload.contract);
                self.verify_contract_deposits(
                    &source_decompressed,
                    &dest_pubkey,
                    &payload.deposits,
                )?;
            }
            TransactionType::DeployContract(payload) => {
                // TAKO contracts: constructor invocation is optional
                // Entry point validation is handled by tos-tbpf at runtime

                // Compute deterministic contract address using eBPF bytecode
                let bytecode = payload
                    .module
                    .get_bytecode()
                    .map(|b| b.to_vec())
                    .unwrap_or_default();

                let contract_address =
                    crate::crypto::compute_deterministic_contract_address(&self.source, &bytecode);

                if let Some(invoke) = payload.invoke.as_ref() {
                    // Use deterministic contract address for deposit verification
                    let dest_pubkey = PublicKey::from_hash(&contract_address);
                    self.verify_contract_deposits(
                        &source_decompressed,
                        &dest_pubkey,
                        &invoke.deposits,
                    )?;
                }

                // Cache module under deterministic address (not tx_hash!)
                // Note: If pre-verification already reserved this address, this will succeed
                // because the module is already cached in mempool state
                // We check first to avoid unnecessary error handling
                if !state
                    .load_contract_module(&contract_address)
                    .await
                    .map_err(VerificationError::State)?
                {
                    // Address not yet cached (shouldn't happen after pre-verify, but handle it)
                    state
                        .set_contract_module(&contract_address, &payload.module)
                        .await
                        .map_err(VerificationError::State)?;
                }
            }
            TransactionType::Energy(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Energy transaction verification - payload: {:?}, fee_limit: {}, nonce: {}",
                        payload,
                        self.get_fee_limit(),
                        self.nonce
                    );
                }
            }
            TransactionType::AIMining(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "AI Mining transaction verification - payload: {:?}, fee_limit: {}, nonce: {}",
                        payload, self.get_fee_limit(), self.nonce
                    );
                }
            }
            TransactionType::BindReferrer(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BindReferrer transaction verification - referrer: {:?}, fee_limit: {}, nonce: {}",
                        payload.get_referrer(), self.get_fee_limit(), self.nonce
                    );
                }
            }
            TransactionType::BatchReferralReward(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BatchReferralReward verification - levels: {}, total_amount: {}, fee_limit: {}",
                        payload.get_levels(),
                        payload.get_total_amount(),
                        self.get_fee_limit()
                    );
                }
            }
            // KYC transaction types
            TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // KYC transactions are logged at execution time
            }
            TransactionType::UnoTransfers(_)
            | TransactionType::ShieldTransfers(_)
            | TransactionType::UnshieldTransfers(_) => {
                // UNO/Shield/Unshield transfers are verified through ZKP proofs
                // Logging handled during apply phase
            }
        }

        // With plaintext balances, we don't need Bulletproofs range proofs
        // Balances are plain u64, always in valid range [0, 2^64)
        trace!("Skipping range proof verification (plaintext balances)");

        // SECURITY FIX: Check balances inline (can't call verify_dynamic_parts as it also does CAS nonce update)
        // Calculate total spending per asset and verify sender has sufficient balance
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();

                    // TOS-Only Fee: Check account creation fee during verification
                    // This catches insufficient fee errors early (mempool validation)
                    if *asset == TOS_ASSET {
                        let destination = transfer.get_destination();
                        let is_new_account = !state
                            .is_account_registered(destination)
                            .await
                            .map_err(VerificationError::State)?;

                        if is_new_account && amount < FEE_PER_ACCOUNT_CREATION {
                            return Err(VerificationError::AmountTooSmallForAccountCreation {
                                amount,
                                fee: FEE_PER_ACCOUNT_CREATION,
                            });
                        }
                    }

                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(&payload.asset).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // max_gas is NOT added to TOS spending under Stake 2.0.
                // Contract execution costs are handled via the Energy model.
                // fee_limit (added below) covers the maximum TOS that can be burned.
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount } => {
                        // FreezeTos spends TOS to add to frozen balance
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. }
                    | EnergyPayload::WithdrawExpireUnfreeze
                    | EnergyPayload::CancelAllUnfreeze
                    | EnergyPayload::UndelegateResource { .. } => {
                        // These operations don't spend TOS directly
                    }
                    EnergyPayload::DelegateResource { .. } => {
                        // DelegateResource uses frozen balance, not direct TOS spending
                    }
                    EnergyPayload::ActivateAccounts { accounts } => {
                        // ActivateAccounts spends 0.1 TOS per NEW account only (idempotent)
                        // Count only unregistered accounts to match apply phase logic
                        // Also check pending registrations for same-block visibility
                        let mut unregistered_count = 0u64;
                        for account in accounts {
                            let is_registered = state
                                .is_account_registered(account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(account);
                            if !is_registered && !is_pending {
                                unregistered_count += 1;
                            }
                        }
                        let total_fee = unregistered_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::BatchDelegateResource { .. } => {
                        // BatchDelegateResource uses frozen balance for delegation
                    }
                    EnergyPayload::ActivateAndDelegate { items } => {
                        // ActivateAndDelegate spends 0.1 TOS per NEW account only (idempotent)
                        // Count only unregistered accounts to match apply phase logic
                        // Also check pending registrations for same-block visibility
                        let mut unregistered_count = 0u64;
                        for item in items {
                            let is_registered = state
                                .is_account_registered(&item.account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(&item.account);
                            if !is_registered && !is_pending {
                                unregistered_count += 1;
                            }
                        }
                        let total_fee = unregistered_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // BUG-066 FIX: Enforce AIMining fee/stake/reward spending on-chain
                use crate::ai_mining::AIMiningPayload;
                match payload {
                    AIMiningPayload::RegisterMiner { registration_fee, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*registration_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::SubmitAnswer { stake_amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*stake_amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::PublishTask { reward_amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*reward_amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::ValidateAnswer { .. } => {
                        // ValidateAnswer does not spend TOS directly
                    }
                }
            }
            TransactionType::MultiSig(_)
            | TransactionType::BindReferrer(_)
            // KYC transactions don't spend assets directly (only fee)
            | TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // No asset spending for these types
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward spends total_amount of the specified asset
                let current = spending_per_asset.entry(payload.get_asset()).or_insert(0);
                *current = current
                    .checked_add(payload.get_total_amount())
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::UnoTransfers(_) => {
                // UNO transfers spend from encrypted balances
                // Spending verification is done through ZKP proofs (CommitmentEqProof)
                // No plaintext spending to verify here
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers spend from plaintext TOS balance
                // Amount is public and verifiable
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::UnshieldTransfers(_) => {
                // Unshield transfers spend from encrypted UNO balances
                // Spending verification is done through ZKP proofs
                // No plaintext spending to verify here (adds to plaintext balance)
            }
        };

        // Energy model: fee_limit is the max TOS burned when energy is insufficient
        // Add fee_limit to TOS spending for balance verification
        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
        *current = current
            .checked_add(self.get_fee_limit())
            .ok_or(VerificationError::Overflow)?;

        // Verify sender has sufficient balance for each asset
        // CRITICAL: Mutate balance during verification (like old encrypted balance code)
        // This ensures mempool verification reduces cached balances, preventing
        // users from submitting sequential transactions that total more than their funds
        for (asset_hash, total_spending) in &spending_per_asset {
            let current_balance = state
                .get_sender_balance(&self.source, asset_hash, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            if *current_balance < *total_spending {
                return Err(VerificationError::InsufficientFunds {
                    available: *current_balance,
                    required: *total_spending,
                });
            }

            // Deduct spending from balance immediately (matches old behavior)
            // This mutation updates mempool cached balances so subsequent txs see reduced funds
            *current_balance = current_balance
                .checked_sub(*total_spending)
                .ok_or(VerificationError::Overflow)?;
        }

        // NOTE: UnfreezeTos does NOT credit balance here. Balance is credited in apply phase
        // only via WithdrawExpireUnfreeze after the 14-day waiting period.

        Ok(())
    }

    pub async fn verify_batch<'a, H, E, B, C>(
        txs: impl Iterator<Item = &'a (Arc<Transaction>, H)>,
        state: &mut B,
        cache: &C,
    ) -> Result<(), VerificationError<E>>
    where
        H: AsRef<Hash> + 'a,
        B: BlockchainVerificationState<'a, E>,
        C: ZKPCache<E>,
    {
        trace!("Verifying batch of transactions");

        // Batch collector for sigma proofs (CommitmentEqProof, CiphertextValidityProof)
        let mut sigma_batch_collector = BatchCollector::default();

        // Prepared UNO transactions for range proof verification
        let mut uno_prepared: Vec<UnoPreparedData> = Vec::new();

        for (tx, hash) in txs {
            let hash = hash.as_ref();

            // In case the cache already knows this TX
            // we don't need to spend time reverifying it again
            // because a TX is immutable, we can just verify the mutable parts
            // (balance & nonce related)
            let dynamic_parts_only = cache
                .is_already_verified(hash)
                .await
                .map_err(VerificationError::State)?;

            // Check if this is a UNO or Unshield transaction (both require ZKP verification)
            let is_uno = matches!(tx.data, TransactionType::UnoTransfers(_));
            let is_unshield = matches!(tx.data, TransactionType::UnshieldTransfers(_));

            if dynamic_parts_only {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("TX {hash} is known from ZKPCache, verifying dynamic parts only");
                }
                tx.verify_dynamic_parts(hash, state).await?;
            } else if is_uno {
                // UNO transactions require ZKP verification
                let (transcript, commitments) = tx
                    .pre_verify_uno(hash, state, &mut sigma_batch_collector)
                    .await?;
                uno_prepared.push((tx.clone(), transcript, commitments));
            } else if is_unshield {
                // Unshield transactions require ZKP verification (UNO -> TOS)
                let (transcript, commitments) = tx
                    .pre_verify_unshield(hash, state, &mut sigma_batch_collector)
                    .await?;
                uno_prepared.push((tx.clone(), transcript, commitments));
            } else {
                // Regular plaintext transaction
                tx.pre_verify(hash, state).await?;
            }
        }

        // Verify ZK proofs if there are any UNO transactions
        if !uno_prepared.is_empty() {
            // Spawn a dedicated thread for the ZK Proofs verification
            // to prevent blocking the async runtime
            spawn_blocking_safe(move || {
                // Verify sigma proofs (CommitmentEqProof, CiphertextValidityProof)
                sigma_batch_collector
                    .verify()
                    .map_err(|_| ProofVerificationError::GenericProof)?;

                // Verify range proofs in batch
                // First collect all verification views, checking for missing proofs
                let verification_views: Vec<_> = uno_prepared
                    .iter_mut()
                    .map(|(tx, transcript, commitments)| {
                        tx.range_proof
                            .as_ref()
                            .ok_or(ProofVerificationError::MissingRangeProof)
                            .map(|proof| {
                                proof.verification_view(transcript, commitments, BULLET_PROOF_SIZE)
                            })
                    })
                    .collect::<Result<_, _>>()?;

                RangeProof::verify_batch(verification_views.into_iter(), &BP_GENS, &PC_GENS)
                    .map_err(ProofVerificationError::from)
            })
            .await
            .context("spawning blocking thread for ZK verification")??;
        }

        Ok(())
    }

    /// Verify one transaction. Use `verify_batch` to verify a batch of transactions.
    pub async fn verify<'a, E, B, C>(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
        cache: &C,
    ) -> Result<(), VerificationError<E>>
    where
        B: BlockchainVerificationState<'a, E>,
        C: ZKPCache<E>,
    {
        let dynamic_parts_only = cache
            .is_already_verified(tx_hash)
            .await
            .map_err(VerificationError::State)?;

        // Check if this is a UNO or Unshield transaction (both require ZKP verification)
        let is_uno = matches!(self.data, TransactionType::UnoTransfers(_));
        let is_unshield = matches!(self.data, TransactionType::UnshieldTransfers(_));

        if dynamic_parts_only {
            if log::log_enabled!(log::Level::Debug) {
                debug!("TX {tx_hash} is known from ZKPCache, verifying dynamic parts only");
            }
            self.verify_dynamic_parts(tx_hash, state).await?;
        } else if is_uno {
            // UNO transactions require full ZKP verification
            let mut sigma_batch_collector = BatchCollector::default();
            let (mut transcript, commitments) = self
                .pre_verify_uno(tx_hash, state, &mut sigma_batch_collector)
                .await?;

            // Verify ZK proofs synchronously for single transaction
            let tx_clone = Arc::clone(self);
            spawn_blocking_safe(move || {
                trace!("Verifying UNO sigma proofs");
                sigma_batch_collector
                    .verify()
                    .map_err(|_| ProofVerificationError::GenericProof)?;

                trace!("Verifying UNO range proof");
                let range_proof = tx_clone
                    .range_proof
                    .as_ref()
                    .ok_or(ProofVerificationError::MissingRangeProof)?;
                RangeProof::verify_multiple(
                    range_proof,
                    &BP_GENS,
                    &PC_GENS,
                    &mut transcript,
                    &commitments,
                    BULLET_PROOF_SIZE,
                )
                .map_err(ProofVerificationError::from)
            })
            .await
            .context("spawning blocking thread for ZK verification")??;
        } else if is_unshield {
            // Unshield transactions require full ZKP verification (UNO -> TOS)
            let mut sigma_batch_collector = BatchCollector::default();
            let (mut transcript, commitments) = self
                .pre_verify_unshield(tx_hash, state, &mut sigma_batch_collector)
                .await?;

            // Verify ZK proofs synchronously for single transaction
            let tx_clone = Arc::clone(self);
            spawn_blocking_safe(move || {
                trace!("Verifying Unshield sigma proofs");
                sigma_batch_collector
                    .verify()
                    .map_err(|_| ProofVerificationError::GenericProof)?;

                trace!("Verifying Unshield range proof");
                let range_proof = tx_clone
                    .range_proof
                    .as_ref()
                    .ok_or(ProofVerificationError::MissingRangeProof)?;
                RangeProof::verify_multiple(
                    range_proof,
                    &BP_GENS,
                    &PC_GENS,
                    &mut transcript,
                    &commitments,
                    BULLET_PROOF_SIZE,
                )
                .map_err(ProofVerificationError::from)
            })
            .await
            .context("spawning blocking thread for ZK verification")??;
        } else {
            // Regular plaintext transaction
            self.pre_verify(tx_hash, state).await?;
        };

        Ok(())
    }

    // Apply the transaction to the state
    // Arc is required around Self to be shared easily into the VM if needed
    async fn apply<'a, P: ContractProvider + Send, E, B: BlockchainApplyState<'a, P, E>>(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        trace!("Applying transaction data");
        // Update nonce
        state
            .update_account_nonce(self.get_source(), self.nonce + 1)
            .await
            .map_err(VerificationError::State)?;

        // SECURITY FIX: Deduct sender balances BEFORE adding to receivers
        // Calculate total spending per asset for sender deduction
        // Use references to original Hash values in transaction (they live for 'a)
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

        // Track contract gas usage for energy refund after execution
        // Format: (max_gas, used_gas) - only set for InvokeContract/DeployContract with constructor
        let mut contract_gas_info: Option<(u64, u64)> = None;

        // BUG-057 FIX: Track new accounts created for energy cost calculation
        // ENERGY_COST_NEW_ACCOUNT (25,000) is charged per new account created
        let mut new_accounts_created: u64 = 0;

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset(); // Returns &Hash
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(&payload.asset).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                // Add deposits
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // max_gas is NOT added to TOS spending under Stake 2.0.
                // Contract execution costs are handled via the Energy model.
                // fee_limit (added below) covers the maximum TOS that can be burned.
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                // If invoking constructor, add deposits only (not max_gas - handled by Energy model)
                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Constructor max_gas NOT added under Stake 2.0
                }
            }
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount } => {
                        // FreezeTos spends TOS to add to frozen balance
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. }
                    | EnergyPayload::WithdrawExpireUnfreeze
                    | EnergyPayload::CancelAllUnfreeze
                    | EnergyPayload::UndelegateResource { .. } => {
                        // These operations don't spend TOS directly
                    }
                    EnergyPayload::DelegateResource { .. } => {
                        // DelegateResource uses frozen balance, not direct TOS spending
                    }
                    EnergyPayload::ActivateAccounts { accounts } => {
                        // ActivateAccounts spends 0.1 TOS per NEW account only (idempotent)
                        // Count only unregistered accounts to match apply phase logic
                        // Also check pending registrations for same-block visibility
                        let mut unregistered_count = 0u64;
                        for account in accounts {
                            let is_registered = state
                                .is_account_registered(account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(account);
                            if !is_registered && !is_pending {
                                unregistered_count += 1;
                            }
                        }
                        let total_fee = unregistered_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::BatchDelegateResource { .. } => {
                        // BatchDelegateResource uses frozen balance for delegation
                    }
                    EnergyPayload::ActivateAndDelegate { items } => {
                        // ActivateAndDelegate spends 0.1 TOS per NEW account only (idempotent)
                        // Count only unregistered accounts to match apply phase logic
                        // Also check pending registrations for same-block visibility
                        let mut unregistered_count = 0u64;
                        for item in items {
                            let is_registered = state
                                .is_account_registered(&item.account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(&item.account);
                            if !is_registered && !is_pending {
                                unregistered_count += 1;
                            }
                        }
                        let total_fee = unregistered_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // BUG-066 FIX: Enforce AIMining fee/stake/reward spending on-chain
                use crate::ai_mining::AIMiningPayload;
                match payload {
                    AIMiningPayload::RegisterMiner { registration_fee, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*registration_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::SubmitAnswer { stake_amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*stake_amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::PublishTask { reward_amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*reward_amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::ValidateAnswer { .. } => {
                        // ValidateAnswer does not spend TOS directly
                    }
                }
            }
            TransactionType::MultiSig(_)
            | TransactionType::BindReferrer(_)
            // KYC transactions don't spend assets directly (only fee)
            | TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // No asset spending for these types
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward spends total_amount of the specified asset
                let current = spending_per_asset.entry(payload.get_asset()).or_insert(0);
                *current = current
                    .checked_add(payload.get_total_amount())
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::UnoTransfers(_) => {
                // UNO transfers spend from encrypted balances
                // Spending verification is done through ZKP proofs (CommitmentEqProof)
                // No plaintext spending to verify here
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers spend from plaintext TOS balance
                // Amount is public and verifiable
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::UnshieldTransfers(_) => {
                // Unshield transfers spend from encrypted UNO balances
                // Spending verification is done through ZKP proofs
                // No plaintext spending to verify here (adds to plaintext balance)
            }
        };

        // Energy model: fee_limit is the max TOS burned when energy is insufficient
        // Add fee_limit to TOS spending for balance verification (reservation)
        // Note: fee_limit is reserved upfront; actual energy consumption and refund happen at end of apply
        let fee_limit = self.get_fee_limit();
        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
        *current = current
            .checked_add(fee_limit)
            .ok_or(VerificationError::Overflow)?;

        // Track sender outputs for final balance calculation
        // Note: Sender balance was already mutated during verification (pre_verify/verify_dynamic_parts)
        // so we only need to track outputs here, not mutate balance again (would cause double-subtract)
        for (asset_hash, total_spending) in &spending_per_asset {
            // IMPORTANT: Must call get_sender_balance first to ensure the asset exchange is loaded
            // This ensures the account assets are properly initialized before updating
            // Without this, internal_update_sender_exchange will fail with "account not found" error
            // because the account was created with empty assets HashMap
            let _ = state
                .get_sender_balance(&self.source, asset_hash, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            // Track the spending in output_sum for final balance calculation
            state
                .add_sender_output(&self.source, asset_hash, *total_spending)
                .await
                .map_err(VerificationError::State)?;
        }

        // UNO balance spending for UnshieldTransfers and UnoTransfers
        // is now handled in verify_dynamic_parts (for cached TXs) and pre_verify_* (for non-cached TXs)
        // Removed duplicate balance update from apply() to prevent double-subtract
        // OLD CODE REMOVED:
        // - UnshieldTransfers: balance was updated here AND in verify_dynamic_parts
        // - UnoTransfers: balance was updated here AND in pre_verify_uno
        // This caused double-subtraction of UNO balance for non-cached transactions

        // Stake 2.0: Energy consumption is handled in apply() using EnergyResourceManager
        // The fee_limit is reserved during balance verification; actual energy consumption
        // happens at block apply time with proper global energy weight calculation
        // See: EnergyResourceManager::consume_transaction_energy()

        // Apply receiver balances
        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let destination = transfer.get_destination();
                    let asset = transfer.get_asset();
                    let plain_amount = transfer.get_amount();

                    // TOS-Only Fee: Account creation fee (0.1 TOS)
                    // When sending TOS to a new (unregistered) account, deduct fee from transfer
                    // Also check pending registrations for same-block visibility
                    let amount_to_credit = if *asset == TOS_ASSET {
                        let is_registered = state
                            .is_account_registered(destination)
                            .await
                            .map_err(VerificationError::State)?;
                        let is_pending = state.is_pending_registration(destination);
                        let is_new_account = !is_registered && !is_pending;

                        if is_new_account {
                            // Verify transfer amount covers account creation fee
                            if plain_amount < FEE_PER_ACCOUNT_CREATION {
                                return Err(VerificationError::AmountTooSmallForAccountCreation {
                                    amount: plain_amount,
                                    fee: FEE_PER_ACCOUNT_CREATION,
                                });
                            }

                            // Deduct account creation fee from transfer amount
                            let net_amount = plain_amount - FEE_PER_ACCOUNT_CREATION;

                            // Burn the account creation fee
                            state
                                .add_burned_coins(FEE_PER_ACCOUNT_CREATION)
                                .await
                                .map_err(VerificationError::State)?;

                            // Record as pending registration for subsequent TXs in this block
                            state.record_pending_registration(destination);

                            // BUG-057 FIX: Track new account for energy cost
                            new_accounts_created += 1;

                            if log::log_enabled!(log::Level::Debug) {
                                debug!(
                                    "Account creation fee: {} TOS burned for new account {:?}",
                                    FEE_PER_ACCOUNT_CREATION as f64 / COIN_VALUE as f64,
                                    destination
                                );
                            }

                            net_amount
                        } else {
                            plain_amount
                        }
                    } else {
                        // Non-TOS assets: no account creation fee
                        plain_amount
                    };

                    // Update receiver balance with (possibly adjusted) amount
                    let current_balance = state
                        .get_receiver_balance(Cow::Borrowed(destination), Cow::Borrowed(asset))
                        .await
                        .map_err(VerificationError::State)?;

                    *current_balance = current_balance
                        .checked_add(amount_to_credit)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                if payload.asset == TOS_ASSET {
                    state
                        .add_burned_coins(payload.amount)
                        .await
                        .map_err(VerificationError::State)?;
                }
            }
            TransactionType::MultiSig(payload) => {
                state
                    .set_multisig_state(&self.source, payload)
                    .await
                    .map_err(VerificationError::State)?;
            }
            TransactionType::InvokeContract(payload) => {
                if self.is_contract_available(state, &payload.contract).await? {
                    let (_is_success, used_gas) = self
                        .invoke_contract(
                            tx_hash,
                            state,
                            &payload.contract,
                            &payload.deposits,
                            payload.parameters.iter().cloned(),
                            payload.max_gas,
                            InvokeContract::Entry(payload.entry_id),
                        )
                        .await?;
                    // Track gas usage for energy refund
                    contract_gas_info = Some((payload.max_gas, used_gas));
                } else {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Contract {} invoked from {} not available",
                            payload.contract, tx_hash
                        );
                    }

                    // Nothing was spent, we must refund the gas and deposits
                    self.handle_gas(state, 0, payload.max_gas).await?;
                    self.refund_deposits(state, &payload.deposits).await?;
                    // Contract not available = no gas used
                    contract_gas_info = Some((payload.max_gas, 0));
                }
            }
            TransactionType::DeployContract(payload) => {
                // Compute deterministic contract address using eBPF bytecode
                let bytecode = payload
                    .module
                    .get_bytecode()
                    .map(|b| b.to_vec())
                    .unwrap_or_default();

                let contract_address =
                    crate::crypto::compute_deterministic_contract_address(&self.source, &bytecode);

                // Deploy contract to deterministic address
                state
                    .set_contract_module(&contract_address, &payload.module)
                    .await
                    .map_err(VerificationError::State)?;

                if let Some(invoke) = payload.invoke.as_ref() {
                    let (is_success, used_gas) = self
                        .invoke_contract(
                            tx_hash,
                            state,
                            &contract_address,
                            &invoke.deposits,
                            iter::empty(),
                            invoke.max_gas,
                            InvokeContract::Hook(0),
                        )
                        .await?;

                    // Track gas usage for energy refund (constructor execution)
                    contract_gas_info = Some((invoke.max_gas, used_gas));

                    // if it has failed, we don't want to deploy the contract
                    // TODO: we must handle this carefully
                    if !is_success {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Contract deploy for {contract_address} failed");
                        }
                        state
                            .remove_contract_module(&contract_address)
                            .await
                            .map_err(VerificationError::State)?;
                    }
                }
            }
            TransactionType::Energy(payload) => {
                // Handle Stake 2.0 energy operations
                match payload {
                    EnergyPayload::FreezeTos { amount } => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("FreezeTos: {} TOS to be frozen (Stake 2.0)", amount);
                        }

                        let sender = self.get_source();

                        // Get sender's account energy
                        let mut sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Add to frozen balance
                        sender_energy.frozen_balance = sender_energy
                            .frozen_balance
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;

                        // Save updated energy
                        state
                            .set_account_energy(sender, sender_energy)
                            .await
                            .map_err(VerificationError::State)?;

                        // Update global energy state
                        let mut global_energy = state
                            .get_global_energy_state()
                            .await
                            .map_err(VerificationError::State)?;
                        global_energy.total_energy_weight =
                            global_energy.total_energy_weight.saturating_add(*amount);
                        state
                            .set_global_energy_state(global_energy)
                            .await
                            .map_err(VerificationError::State)?;
                    }
                    EnergyPayload::UnfreezeTos { amount } => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!(
                                "UnfreezeTos: {} TOS added to unfreezing queue (14-day delay)",
                                amount
                            );
                        }

                        let sender = self.get_source();
                        let now_ms = state.get_block().get_timestamp();

                        // Get sender's account energy
                        let mut sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Check frozen balance
                        if sender_energy.frozen_balance < *amount {
                            return Err(VerificationError::InsufficientFrozenBalance);
                        }

                        // Start unfreeze (adds to queue with 14-day delay)
                        sender_energy
                            .start_unfreeze(*amount, now_ms)
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;

                        // Save updated energy
                        state
                            .set_account_energy(sender, sender_energy)
                            .await
                            .map_err(VerificationError::State)?;

                        // Update global energy state (reduce weight)
                        let mut global_energy = state
                            .get_global_energy_state()
                            .await
                            .map_err(VerificationError::State)?;
                        global_energy.total_energy_weight =
                            global_energy.total_energy_weight.saturating_sub(*amount);
                        state
                            .set_global_energy_state(global_energy)
                            .await
                            .map_err(VerificationError::State)?;
                    }
                    EnergyPayload::WithdrawExpireUnfreeze => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("WithdrawExpireUnfreeze: Withdrawing expired unfreeze entries");
                        }

                        let sender = self.get_source();
                        let now_ms = state.get_block().get_timestamp();

                        // Get sender's account energy
                        let mut sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Withdraw expired entries (returns TOS to balance)
                        let withdrawn = sender_energy.withdraw_expired_unfreeze(now_ms);

                        if withdrawn == 0 {
                            return Err(VerificationError::AnyError(anyhow!(
                                "No expired unfreeze entries to withdraw"
                            )));
                        }

                        // Save updated energy
                        state
                            .set_account_energy(sender, sender_energy)
                            .await
                            .map_err(VerificationError::State)?;

                        // Credit withdrawn TOS to sender's balance
                        let balance = state
                            .get_receiver_balance(Cow::Borrowed(sender), Cow::Borrowed(&TOS_ASSET))
                            .await
                            .map_err(VerificationError::State)?;
                        *balance = balance
                            .checked_add(withdrawn)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::CancelAllUnfreeze => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("CancelAllUnfreeze: Cancelling pending unfreeze, returning to frozen");
                        }

                        let sender = self.get_source();
                        let now_ms = state.get_block().get_timestamp();

                        // Get sender's account energy
                        let mut sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Cancel all unfreeze (expired -> balance, not expired -> frozen)
                        let (withdrawn, cancelled) = sender_energy.cancel_all_unfreeze(now_ms);

                        // Save updated energy
                        state
                            .set_account_energy(sender, sender_energy)
                            .await
                            .map_err(VerificationError::State)?;

                        // Update global energy state (add back cancelled amount)
                        if cancelled > 0 {
                            let mut global_energy = state
                                .get_global_energy_state()
                                .await
                                .map_err(VerificationError::State)?;
                            global_energy.total_energy_weight =
                                global_energy.total_energy_weight.saturating_add(cancelled);
                            state
                                .set_global_energy_state(global_energy)
                                .await
                                .map_err(VerificationError::State)?;
                        }

                        // Credit withdrawn TOS to sender's balance
                        if withdrawn > 0 {
                            let balance = state
                                .get_receiver_balance(
                                    Cow::Borrowed(sender),
                                    Cow::Borrowed(&TOS_ASSET),
                                )
                                .await
                                .map_err(VerificationError::State)?;
                            *balance = balance
                                .checked_add(withdrawn)
                                .ok_or(VerificationError::Overflow)?;
                        }
                    }
                    EnergyPayload::DelegateResource {
                        receiver,
                        amount,
                        lock,
                        lock_period,
                    } => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!(
                                "DelegateResource: {} TOS to {:?}, lock={}, period={} days",
                                amount, receiver, lock, lock_period
                            );
                        }

                        // Get sender's account energy
                        let sender = self.get_source();
                        let mut sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Check available_for_delegation instead of frozen_balance
                        // frozen_balance includes already-delegated TOS, so we must check
                        // available_for_delegation() = frozen_balance - delegated_frozen_balance
                        if sender_energy.available_for_delegation() < *amount {
                            return Err(VerificationError::InsufficientFrozenBalance);
                        }

                        // Get receiver's account energy
                        let mut receiver_energy = state
                            .get_account_energy(receiver)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Calculate lock expire time (use saturating_add for safety)
                        let now_ms = state.get_block().get_timestamp();
                        let expire_time = if *lock {
                            now_ms.saturating_add((*lock_period as u64).saturating_mul(86_400_000))
                        // days to ms
                        } else {
                            0
                        };

                        // Check if delegation already exists
                        let existing = state
                            .get_delegated_resource(sender, receiver)
                            .await
                            .map_err(VerificationError::State)?;

                        if let Some(mut existing_delegation) = existing {
                            // Update existing delegation
                            existing_delegation.frozen_balance = existing_delegation
                                .frozen_balance
                                .checked_add(*amount)
                                .ok_or(VerificationError::Overflow)?;
                            // Update lock time if new lock is longer
                            if expire_time > existing_delegation.expire_time {
                                existing_delegation.expire_time = expire_time;
                            }
                            state
                                .set_delegated_resource(&existing_delegation)
                                .await
                                .map_err(VerificationError::State)?;
                        } else {
                            // Create new delegation
                            let delegation = crate::account::DelegatedResource {
                                from: sender.clone(),
                                to: receiver.clone(),
                                frozen_balance: *amount,
                                expire_time,
                            };
                            state
                                .set_delegated_resource(&delegation)
                                .await
                                .map_err(VerificationError::State)?;
                        }

                        // Only update delegated_frozen_balance, NOT frozen_balance
                        // frozen_balance represents total frozen TOS (unchanged by delegation)
                        // delegated_frozen_balance tracks what's delegated out
                        // effective_frozen_balance = frozen + acquired - delegated
                        //
                        // REMOVED: sender_energy.frozen_balance -= amount (incorrect double-subtract)
                        sender_energy.delegated_frozen_balance = sender_energy
                            .delegated_frozen_balance
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;

                        // Update receiver's energy: add acquired
                        receiver_energy.acquired_delegated_balance = receiver_energy
                            .acquired_delegated_balance
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;

                        // Save updated energies
                        state
                            .set_account_energy(sender, sender_energy)
                            .await
                            .map_err(VerificationError::State)?;
                        state
                            .set_account_energy(receiver, receiver_energy)
                            .await
                            .map_err(VerificationError::State)?;

                        // Record receiver as pending registration
                        // This ensures that subsequent transactions in the same block
                        // won't double-charge the account creation fee
                        let is_registered = state
                            .is_account_registered(receiver)
                            .await
                            .map_err(VerificationError::State)?;
                        if !is_registered {
                            state.record_pending_registration(receiver);
                        }
                    }
                    EnergyPayload::UndelegateResource { receiver, amount } => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("UndelegateResource: {} TOS from {:?}", amount, receiver);
                        }

                        let sender = self.get_source();

                        // Get existing delegation
                        let delegation = state
                            .get_delegated_resource(sender, receiver)
                            .await
                            .map_err(VerificationError::State)?
                            .ok_or(VerificationError::DelegationNotFound)?;

                        // BUG-065 FIX: Use same timestamp calculation as verification phase
                        // Verification uses: get_verification_timestamp() * 1000
                        // This ensures consistent lock expiry checks between verify and apply
                        let now_ms = state.get_verification_timestamp() * 1000;
                        if delegation.expire_time > now_ms {
                            return Err(VerificationError::DelegationStillLocked);
                        }

                        // Check amount
                        if delegation.frozen_balance < *amount {
                            return Err(VerificationError::InsufficientDelegatedBalance);
                        }

                        // Get sender's and receiver's account energy
                        let mut sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        let mut receiver_energy = state
                            .get_account_energy(receiver)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Update delegation record
                        let remaining = delegation
                            .frozen_balance
                            .checked_sub(*amount)
                            .ok_or(VerificationError::Underflow)?;

                        if remaining == 0 {
                            // Delete delegation if fully undelegated
                            state
                                .delete_delegated_resource(sender, receiver)
                                .await
                                .map_err(VerificationError::State)?;
                        } else {
                            // Update delegation with remaining amount
                            let updated_delegation = crate::account::DelegatedResource {
                                from: sender.clone(),
                                to: receiver.clone(),
                                frozen_balance: remaining,
                                expire_time: delegation.expire_time,
                            };
                            state
                                .set_delegated_resource(&updated_delegation)
                                .await
                                .map_err(VerificationError::State)?;
                        }

                        // Only update delegated_frozen_balance, NOT frozen_balance
                        // frozen_balance represents total frozen TOS (unchanged by delegation/undelegation)
                        // delegated_frozen_balance tracks what's delegated out
                        //
                        // REMOVED: sender_energy.frozen_balance += amount (incorrect inverse operation)
                        sender_energy.delegated_frozen_balance = sender_energy
                            .delegated_frozen_balance
                            .checked_sub(*amount)
                            .ok_or(VerificationError::Underflow)?;

                        // Update receiver's energy: remove acquired
                        receiver_energy.acquired_delegated_balance = receiver_energy
                            .acquired_delegated_balance
                            .checked_sub(*amount)
                            .ok_or(VerificationError::Underflow)?;

                        // Save updated energies
                        state
                            .set_account_energy(sender, sender_energy)
                            .await
                            .map_err(VerificationError::State)?;
                        state
                            .set_account_energy(receiver, receiver_energy)
                            .await
                            .map_err(VerificationError::State)?;
                    }
                    EnergyPayload::ActivateAccounts { accounts } => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("ActivateAccounts: Processing {} accounts", accounts.len());
                        }

                        // Skip already-activated accounts (idempotent operation)
                        // Only charge fees for accounts that are actually activated
                        let mut activated_count = 0u64;

                        for account in accounts {
                            // Check if account is already registered OR pending registration
                            // (same-block visibility fix)
                            let is_registered = state
                                .is_account_registered(account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(account);

                            if is_registered || is_pending {
                                // Skip already-activated or pending-activation accounts
                                if log::log_enabled!(log::Level::Debug) {
                                    debug!(
                                        "Skipping account: {:?} (registered={}, pending={})",
                                        account, is_registered, is_pending
                                    );
                                }
                                continue;
                            }

                            // Create the account with zero balance
                            // get_receiver_balance creates account implicitly
                            let balance = state
                                .get_receiver_balance(
                                    Cow::Borrowed(account),
                                    Cow::Borrowed(&TOS_ASSET),
                                )
                                .await
                                .map_err(VerificationError::State)?;
                            // Balance stays at 0 - we're just activating the account
                            let _ = balance;

                            // Record as pending registration for subsequent TXs in this block
                            state.record_pending_registration(account);

                            activated_count += 1;

                            if log::log_enabled!(log::Level::Debug) {
                                debug!("Activated account: {:?}", account);
                            }
                        }

                        // Burn fee only for actually activated accounts
                        let total_fee = activated_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;

                        if total_fee > 0 {
                            state
                                .add_burned_coins(total_fee)
                                .await
                                .map_err(VerificationError::State)?;
                        }

                        if log::log_enabled!(log::Level::Debug) {
                            debug!(
                                "ActivateAccounts complete: {} accounts activated (of {} requested), {} TOS burned",
                                activated_count,
                                accounts.len(),
                                total_fee as f64 / COIN_VALUE as f64
                            );
                        }
                    }
                    EnergyPayload::BatchDelegateResource { delegations } => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("BatchDelegateResource: {} delegations", delegations.len());
                        }

                        let sender = self.get_source();
                        let now_ms = state.get_block().get_timestamp();

                        // Get sender's account energy
                        let mut sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Calculate total delegation amount
                        let total_amount: u64 = delegations
                            .iter()
                            .map(|d| d.amount)
                            .try_fold(0u64, |acc, amount| acc.checked_add(amount))
                            .ok_or(VerificationError::Overflow)?;

                        // Check sender has enough frozen balance (minus already delegated)
                        if sender_energy.available_for_delegation() < total_amount {
                            return Err(VerificationError::InsufficientFrozenBalance);
                        }

                        // Process each delegation
                        for delegation_item in delegations {
                            let receiver = &delegation_item.receiver;

                            // NOTE: Registration check removed for implicit account model
                            // Receivers are implicitly created when they receive delegation.
                            // Energy state will be created via get_account_energy if needed.

                            // Get receiver's energy (creates default if not exists)
                            let mut receiver_energy = state
                                .get_account_energy(receiver)
                                .await
                                .map_err(VerificationError::State)?
                                .unwrap_or_default();

                            // Calculate new expiry time from this delegation item
                            let new_expire_time = if delegation_item.lock {
                                now_ms.saturating_add(
                                    (delegation_item.lock_period as u64).saturating_mul(86_400_000),
                                )
                            } else {
                                0
                            };

                            // Update or create delegation record
                            let existing_delegation = state
                                .get_delegated_resource(sender, receiver)
                                .await
                                .map_err(VerificationError::State)?;

                            let (new_frozen_balance, final_expire_time) = match existing_delegation
                            {
                                Some(existing) => {
                                    let balance = existing
                                        .frozen_balance
                                        .checked_add(delegation_item.amount)
                                        .ok_or(VerificationError::Overflow)?;
                                    // Preserve the longer lock period (protect receiver)
                                    // New delegations cannot shorten existing lock periods
                                    let expire = existing.expire_time.max(new_expire_time);
                                    (balance, expire)
                                }
                                None => (delegation_item.amount, new_expire_time),
                            };

                            let delegation_record = crate::account::DelegatedResource {
                                from: sender.clone(),
                                to: receiver.clone(),
                                frozen_balance: new_frozen_balance,
                                expire_time: final_expire_time,
                            };

                            state
                                .set_delegated_resource(&delegation_record)
                                .await
                                .map_err(VerificationError::State)?;

                            // Update receiver's acquired balance
                            receiver_energy.acquired_delegated_balance = receiver_energy
                                .acquired_delegated_balance
                                .checked_add(delegation_item.amount)
                                .ok_or(VerificationError::Overflow)?;

                            state
                                .set_account_energy(receiver, receiver_energy)
                                .await
                                .map_err(VerificationError::State)?;

                            // Record receiver as pending registration
                            // This ensures that subsequent transactions in the same block
                            // won't double-charge the account creation fee
                            let is_registered = state
                                .is_account_registered(receiver)
                                .await
                                .map_err(VerificationError::State)?;
                            if !is_registered {
                                state.record_pending_registration(receiver);
                            }
                        }

                        // Update sender's delegated balance
                        sender_energy.delegated_frozen_balance = sender_energy
                            .delegated_frozen_balance
                            .checked_add(total_amount)
                            .ok_or(VerificationError::Overflow)?;

                        state
                            .set_account_energy(sender, sender_energy)
                            .await
                            .map_err(VerificationError::State)?;
                    }
                    EnergyPayload::ActivateAndDelegate { items } => {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("ActivateAndDelegate: Processing {} items", items.len());
                        }

                        let sender = self.get_source();
                        let now_ms = state.get_block().get_timestamp();

                        // Get sender's account energy
                        let mut sender_energy = state
                            .get_account_energy(sender)
                            .await
                            .map_err(VerificationError::State)?
                            .unwrap_or_default();

                        // Calculate total delegation (needed for frozen balance check)
                        let total_delegation: u64 = items
                            .iter()
                            .map(|d| d.delegate_amount)
                            .try_fold(0u64, |acc, amount| acc.checked_add(amount))
                            .ok_or(VerificationError::Overflow)?;

                        // TOS balance check is handled by spending_per_asset verification

                        // Check sender has enough frozen balance for delegation
                        if sender_energy.available_for_delegation() < total_delegation {
                            return Err(VerificationError::InsufficientFrozenBalance);
                        }

                        // Track actual activations for fee calculation (skip already-active accounts)
                        let mut activated_count = 0u64;
                        let mut actual_delegation = 0u64;

                        // Process each item: activate account (if not registered) and delegate
                        for item in items {
                            let account = &item.account;

                            // Check if account is already registered OR pending registration
                            // (same-block visibility fix)
                            let is_registered = state
                                .is_account_registered(account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(account);

                            if !is_registered && !is_pending {
                                // Activate the account by creating balance entry
                                let balance = state
                                    .get_receiver_balance(
                                        Cow::Borrowed(account),
                                        Cow::Borrowed(&TOS_ASSET),
                                    )
                                    .await
                                    .map_err(VerificationError::State)?;
                                // Balance stays at 0 - we're just activating the account
                                let _ = balance;

                                // Record as pending registration for subsequent TXs in this block
                                state.record_pending_registration(account);

                                activated_count += 1;

                                if log::log_enabled!(log::Level::Debug) {
                                    debug!("Activated account: {:?}", account);
                                }
                            } else if log::log_enabled!(log::Level::Debug) {
                                debug!(
                                    "Skipping activation for account: {:?} (registered={}, pending={})",
                                    account, is_registered, is_pending
                                );
                            }

                            // Create delegation if amount > 0 (for both new and existing accounts)
                            if item.delegate_amount > 0 {
                                // Get/create account energy for receiver
                                let mut receiver_energy = state
                                    .get_account_energy(account)
                                    .await
                                    .map_err(VerificationError::State)?
                                    .unwrap_or_default();

                                // Calculate expiry time (use saturating_add for safety)
                                let new_expire_time = if item.lock {
                                    now_ms.saturating_add(
                                        (item.lock_period as u64).saturating_mul(86_400_000),
                                    )
                                } else {
                                    0
                                };

                                // Check for existing delegation and preserve longer lock
                                let existing_delegation = state
                                    .get_delegated_resource(sender, account)
                                    .await
                                    .map_err(VerificationError::State)?;

                                let (frozen_balance, expire_time) = match existing_delegation {
                                    Some(existing) => {
                                        let balance = existing
                                            .frozen_balance
                                            .checked_add(item.delegate_amount)
                                            .ok_or(VerificationError::Overflow)?;
                                        // Preserve longer lock period
                                        let expire = existing.expire_time.max(new_expire_time);
                                        (balance, expire)
                                    }
                                    None => (item.delegate_amount, new_expire_time),
                                };

                                // Create/update delegation record
                                let delegation_record = crate::account::DelegatedResource {
                                    from: sender.clone(),
                                    to: account.clone(),
                                    frozen_balance,
                                    expire_time,
                                };

                                state
                                    .set_delegated_resource(&delegation_record)
                                    .await
                                    .map_err(VerificationError::State)?;

                                // Update receiver's acquired balance
                                receiver_energy.acquired_delegated_balance = receiver_energy
                                    .acquired_delegated_balance
                                    .checked_add(item.delegate_amount)
                                    .ok_or(VerificationError::Overflow)?;

                                state
                                    .set_account_energy(account, receiver_energy)
                                    .await
                                    .map_err(VerificationError::State)?;

                                actual_delegation = actual_delegation
                                    .checked_add(item.delegate_amount)
                                    .ok_or(VerificationError::Overflow)?;
                            }
                        }

                        // Burn fee only for actually activated accounts
                        let total_activation_fee = activated_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;

                        if total_activation_fee > 0 {
                            state
                                .add_burned_coins(total_activation_fee)
                                .await
                                .map_err(VerificationError::State)?;
                        }

                        // Update sender's delegated balance
                        if actual_delegation > 0 {
                            sender_energy.delegated_frozen_balance = sender_energy
                                .delegated_frozen_balance
                                .checked_add(actual_delegation)
                                .ok_or(VerificationError::Overflow)?;

                            state
                                .set_account_energy(sender, sender_energy)
                                .await
                                .map_err(VerificationError::State)?;
                        }

                        if log::log_enabled!(log::Level::Debug) {
                            debug!(
                                "ActivateAndDelegate complete: {} activated (of {}), {} TOS delegated",
                                activated_count,
                                items.len(),
                                actual_delegation as f64 / COIN_VALUE as f64
                            );
                        }
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // Handle AI Mining operations with full validation
                use crate::ai_mining::AIMiningValidator;

                // Get or create AI mining state
                let mut ai_mining_state = state
                    .get_ai_mining_state()
                    .await
                    .map_err(VerificationError::State)?
                    .unwrap_or_default();

                // Create validator with current context
                let block_height = state.get_block().get_height();
                let timestamp = state.get_block().get_timestamp();
                let source = self.source.clone();

                let result = {
                    let mut validator = AIMiningValidator::new(
                        &mut ai_mining_state,
                        block_height,
                        timestamp,
                        source,
                    );

                    // Validate and apply the AI mining operation
                    validator.validate_and_apply(payload).map_err(|e| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "AI Mining validation failed: {e}"
                        ))
                    })?;

                    // Update tasks and process completions
                    validator.update_tasks().map_err(|e| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "AI Mining task update failed: {e}"
                        ))
                    })?;

                    validator.get_validation_summary()
                };

                // Save updated state back to blockchain
                state
                    .set_ai_mining_state(&ai_mining_state)
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!("AI Mining operation processed - payload: {:?}, miners: {}, active_tasks: {}, completed_tasks: {}",
                           payload, result.total_miners, result.active_tasks, result.completed_tasks);
                }
            }
            TransactionType::BindReferrer(payload) => {
                // Bind referrer to user via the ReferralProvider
                state
                    .bind_referrer(self.get_source(), payload.get_referrer(), tx_hash)
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BindReferrer transaction applied - user: {:?}, referrer: {:?}",
                        self.get_source(),
                        payload.get_referrer()
                    );
                }
            }
            TransactionType::BatchReferralReward(payload) => {
                // Distribute rewards to uplines via the ReferralProvider
                let distribution_result = state
                    .distribute_referral_rewards(
                        payload.get_from_user(),
                        payload.get_asset(),
                        payload.get_total_amount(),
                        payload.get_ratios(),
                    )
                    .await
                    .map_err(VerificationError::State)?;

                // BUG-067 FIX: Apply account creation fee for new accounts receiving rewards
                // Only applies to TOS rewards (non-TOS assets don't have account creation fee)
                let is_tos_reward = *payload.get_asset() == TOS_ASSET;
                let mut total_fees_burned = 0u64;

                // Credit rewards to each upline's balance
                // Note: distribution.recipient is already a CompressedPublicKey (aka crypto::PublicKey)
                for distribution in &distribution_result.distributions {
                    let recipient = &distribution.recipient;
                    let mut reward_amount = distribution.amount;

                    // BUG-067 FIX: Check if recipient is a new account (TOS rewards only)
                    if is_tos_reward {
                        let is_registered = state
                            .is_account_registered(recipient)
                            .await
                            .map_err(VerificationError::State)?;
                        let is_pending = state.is_pending_registration(recipient);
                        let is_new_account = !is_registered && !is_pending;

                        if is_new_account {
                            // Check if reward covers account creation fee
                            if reward_amount < FEE_PER_ACCOUNT_CREATION {
                                // Skip this recipient - reward too small to cover fee
                                // The amount will be refunded to sender below
                                if log::log_enabled!(log::Level::Debug) {
                                    debug!(
                                        "BatchReferralReward: Skipping new account {:?}, reward {} < fee {}",
                                        recipient, reward_amount, FEE_PER_ACCOUNT_CREATION
                                    );
                                }
                                continue;
                            }

                            // Deduct account creation fee from reward
                            reward_amount -= FEE_PER_ACCOUNT_CREATION;
                            total_fees_burned += FEE_PER_ACCOUNT_CREATION;

                            // Burn the account creation fee
                            state
                                .add_burned_coins(FEE_PER_ACCOUNT_CREATION)
                                .await
                                .map_err(VerificationError::State)?;

                            // Record as pending registration
                            state.record_pending_registration(recipient);

                            // BUG-057 FIX: Track new account for energy cost
                            new_accounts_created += 1;

                            if log::log_enabled!(log::Level::Debug) {
                                debug!(
                                    "BatchReferralReward: Account creation fee {} TOS burned for new account {:?}",
                                    FEE_PER_ACCOUNT_CREATION as f64 / COIN_VALUE as f64,
                                    recipient
                                );
                            }
                        }
                    }

                    let balance = state
                        .get_receiver_balance(
                            std::borrow::Cow::Owned(distribution.recipient.clone()),
                            std::borrow::Cow::Borrowed(payload.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;
                    *balance = balance
                        .checked_add(reward_amount)
                        .ok_or(VerificationError::Overflow)?;
                }

                // Refund remainder to sender (prevents burning undistributed funds)
                // Note: This now includes rewards that were skipped due to insufficient amount for new accounts
                let remainder = payload
                    .get_total_amount()
                    .saturating_sub(distribution_result.total_distributed)
                    .saturating_add(total_fees_burned); // Fees were already burned, don't count as distributed
                let actual_remainder = remainder.saturating_sub(total_fees_burned);
                if actual_remainder > 0 {
                    let sender_balance = state
                        .get_receiver_balance(
                            std::borrow::Cow::Borrowed(self.get_source()),
                            std::borrow::Cow::Borrowed(payload.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;
                    *sender_balance = sender_balance
                        .checked_add(actual_remainder)
                        .ok_or(VerificationError::Overflow)?;
                }

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BatchReferralReward transaction applied - levels: {}, total_amount: {}, distributed: {}, refunded: {}, fees_burned: {}",
                        payload.get_levels(),
                        payload.get_total_amount(),
                        distribution_result.total_distributed,
                        actual_remainder,
                        total_fees_burned
                    );
                }
            }
            // KYC transaction types - execution handled by BlockchainApplyState
            // SECURITY: All KYC operations require approval verification
            TransactionType::SetKyc(payload) => {
                // SECURITY: Check if user already has KYC from a different committee
                // If so, they must use TransferKyc to change committees, not SetKyc
                // This prevents cross-committee hijacking of users
                if let Some(existing_committee) = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                {
                    if &existing_committee != payload.get_committee_id() {
                        return Err(VerificationError::AnyError(anyhow::anyhow!(
                            "SetKyc: user already has KYC from committee {}. Use TransferKyc to change committees.",
                            existing_committee
                        )));
                    }
                }

                // Get the committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Verify approvals (signatures, membership, threshold)
                // SECURITY FIX (Issue #34): Pass verified_at to bind approval signatures to timestamp
                // SECURITY FIX (Issue #44): Pass network for cross-network replay protection
                let current_time = state.get_verification_timestamp();
                let network = state.get_network();
                crate::kyc::verify_set_kyc_approvals(
                    &network,
                    &committee,
                    payload.get_approvals(),
                    payload.get_account(),
                    payload.get_level(),
                    payload.get_data_hash(),
                    payload.get_verified_at(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .set_kyc(
                        payload.get_account(),
                        payload.get_level(),
                        payload.get_verified_at(),
                        payload.get_data_hash(),
                        payload.get_committee_id(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "SetKyc applied - account: {:?}, level: {}",
                        payload.get_account(),
                        payload.get_level()
                    );
                }
            }
            TransactionType::RevokeKyc(payload) => {
                // SECURITY: Verify that the committee is the user's verifying committee
                // This prevents unauthorized committees from revoking KYC they didn't issue
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "User has no KYC record to revoke"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "RevokeKyc: committee {} is not the user's verifying committee {}",
                        payload.get_committee_id(),
                        user_verifying_committee
                    )));
                }

                // Get the committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Verify approvals (signatures, membership, threshold)
                // SECURITY FIX (Issue #44): Pass network for cross-network replay protection
                let current_time = state.get_verification_timestamp();
                let network = state.get_network();
                crate::kyc::verify_revoke_kyc_approvals(
                    &network,
                    &committee,
                    payload.get_approvals(),
                    payload.get_account(),
                    payload.get_reason_hash(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .revoke_kyc(payload.get_account(), payload.get_reason_hash(), tx_hash)
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!("RevokeKyc applied - account: {:?}", payload.get_account());
                }
            }
            TransactionType::RenewKyc(payload) => {
                // SECURITY: Verify that the committee is the user's verifying committee
                // This prevents unauthorized committees from renewing KYC they didn't issue
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "User has no KYC record to renew"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "RenewKyc: committee {} is not the user's verifying committee {}",
                        payload.get_committee_id(),
                        user_verifying_committee
                    )));
                }

                // Get the committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Verify approvals (signatures, membership, threshold)
                // SECURITY FIX (Issue #34): Pass verified_at to bind approval signatures to timestamp
                // SECURITY FIX (Issue #44): Pass network for cross-network replay protection
                let current_time = state.get_verification_timestamp();
                let network = state.get_network();
                crate::kyc::verify_renew_kyc_approvals(
                    &network,
                    &committee,
                    payload.get_approvals(),
                    payload.get_account(),
                    payload.get_data_hash(),
                    payload.get_verified_at(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .renew_kyc(
                        payload.get_account(),
                        payload.get_verified_at(),
                        payload.get_data_hash(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!("RenewKyc applied - account: {:?}", payload.get_account());
                }
            }
            TransactionType::TransferKyc(payload) => {
                let current_time = state.get_verification_timestamp();

                // SECURITY: Verify that the user is currently bound to the source committee
                // This prevents unauthorized committees from transferring users they don't manage
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "User has no KYC record to transfer"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_source_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "TransferKyc: source committee {} does not match user's verifying committee {}",
                        payload.get_source_committee_id(),
                        user_verifying_committee
                    )));
                }

                // SECURITY FIX (Issue #45): Get current KYC level to bind source approval
                let current_level = state
                    .get_kyc_level(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "User has no KYC level to transfer"
                        ))
                    })?;

                // Get and verify source committee approvals
                let source_committee = state
                    .get_committee(payload.get_source_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Source committee not found: {}",
                            payload.get_source_committee_id()
                        ))
                    })?;

                // SECURITY FIX (Issue #34): Pass transferred_at to bind approval signatures to timestamp
                // SECURITY FIX (Issue #39): Pass new_data_hash so source committee approves the exact data
                // SECURITY FIX (Issue #44): Pass network for cross-network replay protection
                // SECURITY FIX (Issue #45): Pass current_level to bind approval to user's KYC level
                let network = state.get_network();
                crate::kyc::verify_transfer_kyc_source_approvals(
                    &network,
                    &source_committee,
                    payload.get_source_approvals(),
                    payload.get_dest_committee_id(),
                    payload.get_account(),
                    current_level,
                    payload.get_new_data_hash(),
                    payload.get_transferred_at(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("Source: {}", e)))?;

                // Get and verify destination committee approvals
                let dest_committee = state
                    .get_committee(payload.get_dest_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Destination committee not found: {}",
                            payload.get_dest_committee_id()
                        ))
                    })?;

                // SECURITY FIX (Issue #34): Pass transferred_at to bind approval signatures to timestamp
                // SECURITY FIX (Issue #44): Pass network for cross-network replay protection
                crate::kyc::verify_transfer_kyc_dest_approvals(
                    &network,
                    &dest_committee,
                    payload.get_dest_approvals(),
                    payload.get_source_committee_id(),
                    payload.get_account(),
                    payload.get_new_data_hash(),
                    payload.get_transferred_at(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("Destination: {}", e)))?;

                // Transfer KYC to destination committee
                // The max_kyc_level check is done inside transfer_kyc to ensure
                // user's KYC level doesn't exceed destination committee's max level
                // SECURITY FIX (Issue #26): Pass current_time (block/verification time) instead
                // of payload time for suspension expiry check
                state
                    .transfer_kyc(
                        payload.get_account(),
                        payload.get_source_committee_id(),
                        payload.get_dest_committee_id(),
                        payload.get_new_data_hash(),
                        payload.get_transferred_at(),
                        tx_hash,
                        dest_committee.max_kyc_level,
                        current_time,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "TransferKyc applied - account: {:?}, source: {}, dest: {}",
                        payload.get_account(),
                        payload.get_source_committee_id(),
                        payload.get_dest_committee_id()
                    );
                }
            }
            TransactionType::AppealKyc(payload) => {
                // Authorization check: only the account owner can submit their own appeal
                if self.get_source() != payload.get_account() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: sender {:?} does not match appeal account {:?}",
                        self.get_source(),
                        payload.get_account()
                    )));
                }

                // SECURITY: Verify that the user's verifying committee matches original_committee_id
                // This prevents appeals against arbitrary committees
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "AppealKyc: user has no KYC record to appeal"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_original_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: original_committee_id {} does not match user's verifying committee {}",
                        payload.get_original_committee_id(),
                        user_verifying_committee
                    )));
                }

                // SECURITY: Verify that user has Revoked status (appeals are for revoked KYC)
                let user_status = state
                    .get_kyc_status(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "AppealKyc: user has no KYC record to appeal"
                        ))
                    })?;

                if user_status != crate::kyc::KycStatus::Revoked {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: user KYC status is {:?}, only Revoked status can be appealed",
                        user_status
                    )));
                }

                // Verify original committee exists and is active
                let original_committee = state
                    .get_committee(payload.get_original_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Original committee not found: {}",
                            payload.get_original_committee_id()
                        ))
                    })?;

                // SECURITY FIX (Issue #29): Verify original committee is Active
                // Appeals should only be accepted against active committees
                if original_committee.status != crate::kyc::CommitteeStatus::Active {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: original committee {} is not active (status: {:?})",
                        payload.get_original_committee_id(),
                        original_committee.status
                    )));
                }

                // Verify parent committee exists
                let parent_committee = state
                    .get_committee(payload.get_parent_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Parent committee not found: {}",
                            payload.get_parent_committee_id()
                        ))
                    })?;

                // SECURITY FIX (Issue #29): Verify parent committee is Active
                // Appeals must be submitted to active parent committees that can review them
                if parent_committee.status != crate::kyc::CommitteeStatus::Active {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: parent committee {} is not active (status: {:?})",
                        payload.get_parent_committee_id(),
                        parent_committee.status
                    )));
                }

                // Verify the original committee's parent matches the claimed parent
                if let Some(ref actual_parent_id) = original_committee.parent_id {
                    if actual_parent_id != &parent_committee.id {
                        return Err(VerificationError::AnyError(anyhow::anyhow!(
                            "AppealKyc: claimed parent {} does not match original committee's actual parent {}",
                            payload.get_parent_committee_id(),
                            actual_parent_id
                        )));
                    }
                } else {
                    // Original committee is the global committee (no parent)
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: cannot appeal to parent - original committee {} is the global committee",
                        payload.get_original_committee_id()
                    )));
                }

                state
                    .submit_kyc_appeal(
                        payload.get_account(),
                        payload.get_original_committee_id(),
                        payload.get_parent_committee_id(),
                        payload.get_reason_hash(),
                        payload.get_documents_hash(),
                        payload.get_submitted_at(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "AppealKyc submitted - account: {:?}, original: {}, parent: {}",
                        payload.get_account(),
                        payload.get_original_committee_id(),
                        payload.get_parent_committee_id()
                    );
                }
            }
            TransactionType::BootstrapCommittee(payload) => {
                // Convert CommitteeMemberInit to CommitteeMemberInfo
                let members: Vec<crate::kyc::CommitteeMemberInfo> = payload
                    .get_members()
                    .iter()
                    .map(|m| {
                        crate::kyc::CommitteeMemberInfo::new(
                            m.public_key.clone(),
                            m.name.clone(),
                            m.role,
                        )
                    })
                    .collect();

                let committee_id = state
                    .bootstrap_global_committee(
                        payload.get_name().to_string(),
                        members,
                        payload.get_threshold(),
                        payload.get_kyc_threshold(),
                        payload.get_max_kyc_level(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BootstrapCommittee applied - name: {}, id: {}",
                        payload.get_name(),
                        committee_id
                    );
                }
            }
            TransactionType::RegisterCommittee(payload) => {
                // Get parent committee and verify approvals
                let parent_committee = state
                    .get_committee(payload.get_parent_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Parent committee not found: {}",
                            payload.get_parent_id()
                        ))
                    })?;

                // SECURITY: Verify parent committee can manage the requested region
                // Global committees can manage any region; regional committees can only
                // manage their own region. This prevents unauthorized cross-region registration.
                if !parent_committee.can_manage_region(&payload.get_region()) {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Parent committee {} (region: {}) cannot register committees in region {}",
                        payload.get_parent_id(),
                        parent_committee.region,
                        payload.get_region()
                    )));
                }

                // Compute config hash from payload to bind signatures to full configuration
                let members: Vec<_> = payload
                    .get_members()
                    .iter()
                    .map(|m| (m.public_key.clone(), m.name.clone(), m.role))
                    .collect();
                let config_hash = crate::kyc::CommitteeApproval::compute_register_config_hash(
                    &members,
                    payload.get_threshold(),
                    payload.get_kyc_threshold(),
                    payload.get_max_kyc_level(),
                );

                // Verify parent committee approvals with config binding
                // SECURITY FIX (Issue #44): Pass network for cross-network replay protection
                let current_time = state.get_verification_timestamp();
                let network = state.get_network();
                crate::kyc::verify_register_committee_approvals(
                    &network,
                    &parent_committee,
                    payload.get_approvals(),
                    payload.get_name(),
                    payload.get_region(),
                    &config_hash,
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                // Convert NewCommitteeMember to CommitteeMemberInfo
                let members: Vec<crate::kyc::CommitteeMemberInfo> = payload
                    .get_members()
                    .iter()
                    .map(|m| {
                        crate::kyc::CommitteeMemberInfo::new(
                            m.public_key.clone(),
                            m.name.clone(),
                            m.role,
                        )
                    })
                    .collect();

                let committee_id = state
                    .register_committee(
                        payload.get_name().to_string(),
                        payload.get_region(),
                        members,
                        payload.get_threshold(),
                        payload.get_kyc_threshold(),
                        payload.get_max_kyc_level(),
                        payload.get_parent_id(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "RegisterCommittee applied - name: {}, region: {:?}, id: {}",
                        payload.get_name(),
                        payload.get_region(),
                        committee_id
                    );
                }
            }
            TransactionType::UpdateCommittee(payload) => {
                // Get committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Validate governance constraints using committee state
                // SECURITY FIX (Issue #36, #37): Include approver_count and kyc_threshold
                // to properly validate threshold changes and role updates
                // BUG-075 FIX: Validate that member exists for update/remove operations
                let (target_is_active, target_can_approve) = match payload.get_update() {
                    crate::transaction::CommitteeUpdateData::RemoveMember { public_key }
                    | crate::transaction::CommitteeUpdateData::UpdateMemberRole {
                        public_key,
                        ..
                    }
                    | crate::transaction::CommitteeUpdateData::UpdateMemberStatus {
                        public_key,
                        ..
                    } => {
                        // BUG-075 FIX: Member MUST exist for these operations
                        let member = committee.get_member(public_key).ok_or_else(|| {
                            VerificationError::AnyError(anyhow::anyhow!(
                                "Member {:?} not found in committee {}",
                                public_key,
                                payload.get_committee_id()
                            ))
                        })?;
                        Some((
                            member.status == crate::kyc::MemberStatus::Active,
                            member.role.can_approve(),
                        ))
                    }
                    _ => None,
                }
                .unwrap_or((true, true));

                let committee_info = kyc::CommitteeGovernanceInfo {
                    member_count: committee.active_member_count(),
                    approver_count: committee.active_approver_count(),
                    total_member_count: committee.total_member_count(),
                    threshold: committee.threshold,
                    kyc_threshold: committee.kyc_threshold,
                    target_is_active: Some(target_is_active),
                    target_can_approve: Some(target_can_approve),
                };
                let current_time = state.get_verification_timestamp();
                kyc::verify_update_committee_with_state(payload, &committee_info, current_time)?;

                // Compute hash of update data for signature verification
                let update_data_hash = {
                    use crate::serializer::Serializer;
                    let mut buffer = Vec::new();
                    let mut writer = crate::serializer::Writer::new(&mut buffer);
                    payload.get_update().write(&mut writer);
                    crate::crypto::Hash::new(blake3::hash(&buffer).into())
                };

                // Get update type for message building
                let update_type = match payload.get_update() {
                    crate::transaction::CommitteeUpdateData::AddMember { .. } => 0u8,
                    crate::transaction::CommitteeUpdateData::RemoveMember { .. } => 1u8,
                    crate::transaction::CommitteeUpdateData::UpdateMemberRole { .. } => 2u8,
                    crate::transaction::CommitteeUpdateData::UpdateMemberStatus { .. } => 3u8,
                    crate::transaction::CommitteeUpdateData::UpdateThreshold { .. } => 4u8,
                    crate::transaction::CommitteeUpdateData::UpdateKycThreshold { .. } => 5u8,
                    crate::transaction::CommitteeUpdateData::UpdateName { .. } => 6u8,
                    crate::transaction::CommitteeUpdateData::SuspendCommittee => 7u8,
                    crate::transaction::CommitteeUpdateData::ActivateCommittee => 8u8,
                };

                // Verify committee approvals
                // SECURITY FIX (Issue #44): Pass network for cross-network replay protection
                let network = state.get_network();
                crate::kyc::verify_update_committee_approvals(
                    &network,
                    &committee,
                    payload.get_approvals(),
                    update_type,
                    &update_data_hash,
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .update_committee(payload.get_committee_id(), payload.get_update())
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "UpdateCommittee applied - committee: {}, update: {:?}",
                        payload.get_committee_id(),
                        payload.get_update()
                    );
                }
            }
            TransactionType::EmergencySuspend(payload) => {
                // SECURITY: Verify that the committee is the user's verifying committee
                // This prevents cross-committee denial-of-service attacks where any committee
                // could suspend arbitrary users for 24 hours
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "EmergencySuspend: user has no KYC record to suspend"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "EmergencySuspend: committee {} is not the user's verifying committee {}",
                        payload.get_committee_id(),
                        user_verifying_committee
                    )));
                }

                // Get committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Verify emergency suspend approvals (requires 2 members)
                // SECURITY FIX (Issue #44): Pass network for cross-network replay protection
                let current_time = state.get_verification_timestamp();
                let network = state.get_network();
                crate::kyc::verify_emergency_suspend_approvals(
                    &network,
                    &committee,
                    payload.get_approvals(),
                    payload.get_account(),
                    payload.get_reason_hash(),
                    payload.get_expires_at(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .emergency_suspend_kyc(
                        payload.get_account(),
                        payload.get_reason_hash(),
                        payload.get_expires_at(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "EmergencySuspend applied - account: {:?}, expires_at: {}",
                        payload.get_account(),
                        payload.get_expires_at()
                    );
                }
            }
            TransactionType::UnoTransfers(transfers) => {
                // UNO transfers: privacy-preserving transfers
                // Update receiver balances with encrypted ciphertexts
                for transfer in transfers {
                    let receiver_ct = transfer
                        .get_ciphertext(Role::Receiver)
                        .decompress()
                        .map_err(ProofVerificationError::from)?;

                    // Get receiver's UNO balance and add the ciphertext
                    let current_balance = state
                        .get_receiver_uno_balance(
                            Cow::Borrowed(transfer.get_destination()),
                            Cow::Borrowed(transfer.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;

                    *current_balance += receiver_ct;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "UNO transfer applied - receiver: {:?}, asset: {:?}",
                            transfer.get_destination(),
                            transfer.get_asset()
                        );
                    }
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers: TOS -> UNO
                // TOS is deducted via spending_per_asset (plaintext deduction)
                // Add encrypted UNO balance to receiver
                for transfer in transfers {
                    // Create ciphertext from commitment and receiver handle
                    // Ciphertext = (Commitment, ReceiverHandle)
                    let commitment = transfer
                        .get_commitment()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_handle = transfer
                        .get_receiver_handle()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_ct = Ciphertext::new(commitment, receiver_handle);

                    // Get receiver's UNO balance and add the ciphertext
                    let current_balance = state
                        .get_receiver_uno_balance(
                            Cow::Borrowed(transfer.get_destination()),
                            Cow::Borrowed(&UNO_ASSET),
                        )
                        .await
                        .map_err(VerificationError::State)?;

                    *current_balance += receiver_ct;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Shield transfer applied - receiver: {:?}, amount: {}",
                            transfer.get_destination(),
                            transfer.get_amount()
                        );
                    }
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                // Unshield transfers: UNO -> TOS
                // Sender UNO balance is deducted in verify_dynamic_parts
                // Here we only add plaintext TOS balance to receiver
                for transfer in transfers {
                    let destination = transfer.get_destination();
                    let plain_amount = transfer.get_amount();

                    // BUG-090 FIX: Apply account creation fee for new accounts
                    // Unshield transfers credit TOS to receivers, same fee rules as regular transfers
                    let is_registered = state
                        .is_account_registered(destination)
                        .await
                        .map_err(VerificationError::State)?;
                    let is_pending = state.is_pending_registration(destination);
                    let is_new_account = !is_registered && !is_pending;

                    let amount_to_credit = if is_new_account {
                        // Verify transfer amount covers account creation fee
                        if plain_amount < FEE_PER_ACCOUNT_CREATION {
                            return Err(VerificationError::AmountTooSmallForAccountCreation {
                                amount: plain_amount,
                                fee: FEE_PER_ACCOUNT_CREATION,
                            });
                        }

                        // Deduct account creation fee from transfer amount
                        let net_amount = plain_amount - FEE_PER_ACCOUNT_CREATION;

                        // Burn the account creation fee
                        state
                            .add_burned_coins(FEE_PER_ACCOUNT_CREATION)
                            .await
                            .map_err(VerificationError::State)?;

                        // Record as pending registration for subsequent TXs in this block
                        state.record_pending_registration(destination);

                        // BUG-057 FIX: Track new account for energy cost
                        new_accounts_created += 1;

                        if log::log_enabled!(log::Level::Debug) {
                            debug!(
                                "Unshield: Account creation fee {} TOS burned for new account {:?}",
                                FEE_PER_ACCOUNT_CREATION as f64 / COIN_VALUE as f64,
                                destination
                            );
                        }

                        net_amount
                    } else {
                        plain_amount
                    };

                    // Add plaintext amount to receiver's TOS balance
                    let current_balance = state
                        .get_receiver_balance(Cow::Borrowed(destination), Cow::Borrowed(&TOS_ASSET))
                        .await
                        .map_err(VerificationError::State)?;

                    *current_balance = current_balance
                        .checked_add(amount_to_credit)
                        .ok_or(VerificationError::Overflow)?;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Unshield transfer applied - receiver: {:?}, amount: {}",
                            destination, amount_to_credit
                        );
                    }
                }
            }
        }

        // ===== Energy Consumption (Stake 2.0) =====
        // Calculate actual energy cost and consume from account energy resources
        // Priority: 1. Free quota → 2. Frozen energy → 3. TOS burn (from fee_limit)
        let fee_limit = self.get_fee_limit();
        let mut required_energy = self.calculate_energy_cost();
        let now_ms = state.get_block().get_timestamp();

        // BUG-057 FIX: Add energy cost for new accounts created
        // ENERGY_COST_NEW_ACCOUNT (25,000) is charged per new account
        if new_accounts_created > 0 {
            let new_account_energy =
                Transaction::account_creation_energy_cost(new_accounts_created as usize);
            required_energy = required_energy.saturating_add(new_account_energy);
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "New account energy cost: {} accounts × 25,000 = {} energy added",
                    new_accounts_created, new_account_energy
                );
            }
        }

        // Adjust energy cost for contracts based on actual gas used
        // calculate_energy_cost() uses max_gas, but actual execution may use less
        // Subtract unused gas to prevent over-consumption of stake energy
        if let Some((max_gas, used_gas)) = contract_gas_info {
            if used_gas < max_gas {
                let unused_gas = max_gas.saturating_sub(used_gas);
                required_energy = required_energy.saturating_sub(unused_gas);
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Contract energy adjustment: max_gas={}, used_gas={}, unused={}, adjusted_energy={}",
                        max_gas, used_gas, unused_gas, required_energy
                    );
                }
            }
        }

        // Get sender's account energy
        let mut sender_energy = state
            .get_account_energy(self.get_source())
            .await
            .map_err(VerificationError::State)?
            .unwrap_or_default();

        // Get global energy state for proportional calculation
        let global_energy = state
            .get_global_energy_state()
            .await
            .map_err(VerificationError::State)?;

        // Compute energy consumption on a COPY first, check fee_limit,
        // then apply to the actual state. This prevents state mutation before rejection.
        let mut sender_energy_copy = sender_energy.clone();
        let tx_result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut sender_energy_copy,
            required_energy,
            global_energy.total_energy_weight,
            now_ms,
        );

        // Enforce fee_limit as hard cap BEFORE mutating state
        // If the required TOS burn exceeds fee_limit, reject the transaction
        // This prevents underpayment attacks where users set low fee_limit
        if tx_result.fee > fee_limit {
            return Err(VerificationError::InsufficientFeeLimit {
                required: tx_result.fee,
                provided: fee_limit,
            });
        }

        // fee_limit check passed - now apply the energy consumption to actual state
        sender_energy = sender_energy_copy;

        // Save updated energy state
        state
            .set_account_energy(self.get_source(), sender_energy)
            .await
            .map_err(VerificationError::State)?;

        // Calculate actual TOS burned (now always equals tx_result.fee since we checked above)
        let actual_tos_burned = tx_result.fee;

        // Add the burned TOS to gas fee (for block rewards)
        state
            .add_gas_fee(actual_tos_burned)
            .await
            .map_err(VerificationError::State)?;

        // Refund unused fee_limit back to sender's balance
        let refund = fee_limit.saturating_sub(actual_tos_burned);
        if refund > 0 {
            let sender_balance = state
                .get_receiver_balance(Cow::Borrowed(self.get_source()), Cow::Borrowed(&TOS_ASSET))
                .await
                .map_err(VerificationError::State)?;
            *sender_balance = sender_balance
                .checked_add(refund)
                .ok_or(VerificationError::Overflow)?;

            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Energy fee refund: {} TOS (fee_limit: {}, burned: {}, energy: {} free/{} frozen)",
                    refund, fee_limit, actual_tos_burned,
                    tx_result.free_energy_used, tx_result.frozen_energy_used
                );
            }
        }

        Ok(())
    }

    /// Assume the tx is valid, apply it to `state`. May panic if a ciphertext is ill-formed.
    ///
    /// BLOCKDAG alignment: This function now performs balance deduction inline,
    /// matching the original BLOCKDAG architecture where balance is deducted
    /// before calling add_sender_output.
    pub async fn apply_without_verify<
        'a,
        P: ContractProvider + Send,
        E,
        B: BlockchainApplyState<'a, P, E>,
    >(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        // Balance simplification: No decompression needed for plaintext balances
        // Private deposits are not supported, only Public deposits with plain u64 amounts

        // BLOCKDAG alignment: Calculate spending per asset and deduct from sender balance
        // This matches BLOCKDAG's approach where balance deduction happens here,
        // before calling add_sender_output.
        //
        // Original BLOCKDAG code (tos_common/src/transaction/verify/mod.rs:1321-1342):
        // for commitment in &self.source_commitments {
        //     let asset = commitment.get_asset();
        //     let current_source_balance = state.get_sender_balance(...).await?;
        //     let output = self.get_sender_output_ct(...)?;
        //     *current_source_balance -= &output;
        //     state.add_sender_output(&self.source, asset, output).await?;
        // }

        // Calculate spending per asset (same logic as pre_verify)
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(&payload.asset).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // max_gas is NOT added to TOS spending under Stake 2.0.
                // Contract execution costs are handled via the Energy model.
                // fee_limit (added below) covers the maximum TOS that can be burned.
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount } => {
                        // FreezeTos spends TOS to add to frozen balance
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. }
                    | EnergyPayload::WithdrawExpireUnfreeze
                    | EnergyPayload::CancelAllUnfreeze
                    | EnergyPayload::UndelegateResource { .. } => {
                        // These operations don't spend TOS directly
                    }
                    EnergyPayload::DelegateResource { .. } => {
                        // DelegateResource uses frozen balance, not direct TOS spending
                    }
                    EnergyPayload::ActivateAccounts { accounts } => {
                        // ActivateAccounts spends 0.1 TOS per NEW account only (idempotent)
                        // Count only unregistered accounts to match apply phase logic
                        // Also check pending registrations for same-block visibility
                        let mut unregistered_count = 0u64;
                        for account in accounts {
                            let is_registered = state
                                .is_account_registered(account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(account);
                            if !is_registered && !is_pending {
                                unregistered_count += 1;
                            }
                        }
                        let total_fee = unregistered_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::BatchDelegateResource { .. } => {
                        // BatchDelegateResource uses frozen balance for delegation
                    }
                    EnergyPayload::ActivateAndDelegate { items } => {
                        // ActivateAndDelegate spends 0.1 TOS per NEW account only (idempotent)
                        // Count only unregistered accounts to match apply phase logic
                        // Also check pending registrations for same-block visibility
                        let mut unregistered_count = 0u64;
                        for item in items {
                            let is_registered = state
                                .is_account_registered(&item.account)
                                .await
                                .map_err(VerificationError::State)?;
                            let is_pending = state.is_pending_registration(&item.account);
                            if !is_registered && !is_pending {
                                unregistered_count += 1;
                            }
                        }
                        let total_fee = unregistered_count
                            .checked_mul(FEE_PER_ACCOUNT_CREATION)
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // BUG-066 FIX: Enforce AIMining fee/stake/reward spending on-chain
                use crate::ai_mining::AIMiningPayload;
                match payload {
                    AIMiningPayload::RegisterMiner { registration_fee, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*registration_fee)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::SubmitAnswer { stake_amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*stake_amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::PublishTask { reward_amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*reward_amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    AIMiningPayload::ValidateAnswer { .. } => {
                        // ValidateAnswer does not spend TOS directly
                    }
                }
            }
            TransactionType::MultiSig(_)
            | TransactionType::BindReferrer(_)
            // KYC transactions don't spend assets directly (only fee)
            | TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // No asset spending for these types
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward spends total_amount of the specified asset
                let current = spending_per_asset.entry(payload.get_asset()).or_insert(0);
                *current = current
                    .checked_add(payload.get_total_amount())
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::UnoTransfers(_) => {
                // UNO transfers spend from encrypted balances
                // Spending verification is done through ZKP proofs (CommitmentEqProof)
                // No plaintext spending to verify here
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers spend from plaintext TOS balance
                // Amount is public and verifiable
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::UnshieldTransfers(_) => {
                // Unshield transfers spend from encrypted UNO balances
                // Spending verification is done through ZKP proofs
                // No plaintext spending to verify here (adds to plaintext balance)
            }
        };

        // Energy model: fee_limit is the max TOS burned when energy is insufficient
        // Add fee_limit to TOS spending for balance verification
        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
        *current = current
            .checked_add(self.get_fee_limit())
            .ok_or(VerificationError::Overflow)?;

        // TOS-Only Fee: MultiSig fee (1 TOS per signature)
        // Charged when transaction has 2+ signatures (main + multisig participants)
        // MultiSig fee: 1 TOS per signature
        let multisig_count = self.get_multisig_count();
        let total_signatures = if multisig_count > 0 {
            // MultiSig transaction: count = multisig signatures (excluding main signature)
            // Total signatures = 1 (main) + multisig_count
            1 + multisig_count
        } else {
            // Regular transaction: just main signature
            1
        };

        let multisig_fee = if total_signatures >= 2 {
            // Charge 1 TOS per signature for multisig transactions
            (total_signatures as u64)
                .checked_mul(FEE_PER_MULTISIG_SIGNATURE)
                .ok_or(VerificationError::Overflow)?
        } else {
            0
        };

        if multisig_fee > 0 {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(multisig_fee)
                .ok_or(VerificationError::Overflow)?;

            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "MultiSig fee: {} TOS for {} signatures",
                    multisig_fee as f64 / COIN_VALUE as f64,
                    total_signatures
                );
            }
        }

        // Deduct spending from sender balance
        // This matches BLOCKDAG's behavior where balance deduction happens before apply()
        // NOTE: We do NOT call add_sender_output() here because apply() already does that.
        // The apply() function tracks outputs for final balance calculation.
        for (asset, output_sum) in &spending_per_asset {
            let current_balance = state
                .get_sender_balance(&self.source, asset, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            // Deduct spending from balance (BLOCKDAG: *current_source_balance -= &output)
            *current_balance = current_balance
                .checked_sub(*output_sum)
                .ok_or(VerificationError::Overflow)?;

            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "apply_without_verify: deducted {} from sender balance for asset {}",
                    output_sum,
                    asset
                );
            }
        }

        // Burn the multisig fee (TOS-Only)
        if multisig_fee > 0 {
            state
                .add_burned_coins(multisig_fee)
                .await
                .map_err(VerificationError::State)?;
        }

        self.apply(tx_hash, state).await
    }

    /// Verify only that the final sender balance is the expected one for each commitment
    /// Then apply ciphertexts to the state
    /// Checks done are: commitment eq proofs only
    ///
    /// BLOCKDAG alignment: With plaintext balances, no proof verification is needed.
    /// This function delegates to apply_without_verify which handles balance deduction.
    pub async fn apply_with_partial_verify<
        'a,
        P: ContractProvider + Send,
        E,
        B: BlockchainApplyState<'a, P, E>,
    >(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        trace!("apply with partial verify");

        // Balance simplification: No decompression needed for plaintext balances
        // Private deposits are not supported, only Public deposits with plain u64 amounts
        trace!("Partial verify with plaintext balances - no proof verification needed");

        // Delegate to apply_without_verify which handles balance deduction
        // (BLOCKDAG alignment: both functions now perform inline balance deduction)
        self.apply_without_verify(tx_hash, state).await
    }
}
