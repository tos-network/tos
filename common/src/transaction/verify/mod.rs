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
    account::EnergyResource,
    config::{
        BURN_PER_CONTRACT, MAX_GAS_USAGE_PER_TX, MIN_SHIELD_TOS_AMOUNT, TOS_ASSET, UNO_ASSET,
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

        // UNO_ASSET is required for fees (paid from encrypted balance)
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

    async fn verify_energy_payload<'a, E, B: BlockchainVerificationState<'a, E>>(
        &self,
        payload: &'a EnergyPayload,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        match payload {
            EnergyPayload::FreezeTos { amount, duration } => {
                if self.fee != 0 {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Energy transactions must have zero fee"
                    )));
                }

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

                if !duration.is_valid() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Freeze duration must be between 3 and 365 days"
                    )));
                }
            }
            EnergyPayload::FreezeTosDelegate {
                delegatees,
                duration,
            } => {
                if self.fee != 0 {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Energy transactions must have zero fee"
                    )));
                }

                if delegatees.is_empty() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Delegatees list cannot be empty"
                    )));
                }

                if delegatees.len() > crate::config::MAX_DELEGATEES {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Too many delegatees (max {})",
                        crate::config::MAX_DELEGATEES
                    )));
                }

                // Check for duplicates and self-delegation
                let mut seen = std::collections::HashSet::new();
                for entry in delegatees {
                    if entry.amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Delegation amount must be greater than zero"
                        )));
                    }

                    if entry.amount % crate::config::COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Delegation amount must be a whole number of TOS"
                        )));
                    }

                    if entry.amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Delegation amount must be at least 1 TOS"
                        )));
                    }

                    if !seen.insert(&entry.delegatee) {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Duplicate delegatee in list"
                        )));
                    }

                    // Reject self-delegation (sender cannot delegate to themselves)
                    if entry.delegatee == self.source {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Cannot delegate energy to yourself"
                        )));
                    }

                    // Delegatee account must already exist
                    let delegatee_exists = state
                        .account_exists(&entry.delegatee)
                        .await
                        .map_err(VerificationError::State)?;
                    if !delegatee_exists {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Delegatee account does not exist: {:?}",
                            entry.delegatee
                        )));
                    }
                }

                if !duration.is_valid() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Freeze duration must be between 3 and 365 days"
                    )));
                }
            }
            EnergyPayload::UnfreezeTos {
                amount,
                from_delegation,
                delegatee_address,
                ..
            } => {
                if self.fee != 0 {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Energy transactions must have zero fee"
                    )));
                }

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

                if !from_delegation && delegatee_address.is_some() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Invalid delegatee_address usage"
                    )));
                }
            }
            EnergyPayload::WithdrawUnfrozen => {
                if self.fee != 0 {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Energy transactions must have zero fee"
                    )));
                }
            }
        }

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
            TransactionType::Energy(payload) => {
                self.verify_energy_payload(payload, state).await?;
            }
            TransactionType::AIMining(_) => {
                // AI Mining transactions don't require special verification beyond basic checks for now
            }
            TransactionType::BindReferrer(payload) => {
                // Validate extra_data size to prevent mempool/storage bloat
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

                    // Validate minimum Shield amount (anti-money-laundering measure)
                    if transfer.get_amount() < MIN_SHIELD_TOS_AMOUNT {
                        return Err(VerificationError::ShieldAmountTooLow);
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
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

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
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                // If invoking constructor, add deposits and max_gas
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
                    EnergyPayload::FreezeTos { amount, .. } => {
                        // Expired Freeze Recycling: Only charge balance for non-recyclable portion
                        let recyclable_tos = state
                            .get_recyclable_tos(&self.source)
                            .await
                            .map_err(VerificationError::State)?;

                        // Only charge for balance portion (amount - recyclable)
                        let balance_required = amount.saturating_sub(recyclable_tos);

                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(balance_required)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::FreezeTosDelegate { delegatees, .. } => {
                        // Calculate total delegation amount
                        // Delegation does NOT support recycling - must use full balance
                        let total: u64 = delegatees
                            .iter()
                            .try_fold(0u64, |acc, entry| acc.checked_add(entry.amount))
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. } => {
                        // Unfreeze doesn't spend - TOS goes to pending (two-phase)
                    }
                    EnergyPayload::WithdrawUnfrozen => {
                        // Withdraw doesn't spend - it releases pending funds to balance
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // Enforce AIMining fee/stake/reward spending on-chain
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

        // Add fee to TOS spending (unless using energy fee)
        if !self.get_fee_type().is_energy() {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(self.fee)
                .ok_or(VerificationError::Overflow)?;
        }

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

        // Two-phase unfreeze: UnfreezeTos creates pending, WithdrawUnfrozen credits balance
        // UnfreezeTos does NOT credit balance immediately - TOS stays in pending state

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
        let mut transcript = Self::prepare_transcript(
            self.version,
            &self.source,
            self.fee,
            &self.fee_type,
            self.nonce,
        );

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
        let mut transcript = Self::prepare_transcript(
            self.version,
            &self.source,
            self.fee,
            &self.fee_type,
            self.nonce,
        );

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

        // Validate that Energy fee type can only be used with transfer-type transactions
        if self.get_fee_type().is_energy() {
            match &self.data {
                TransactionType::Transfers(_)
                | TransactionType::UnoTransfers(_)
                | TransactionType::ShieldTransfers(_)
                | TransactionType::UnshieldTransfers(_) => {
                    // These transaction types can use Energy fees
                }
                _ => {
                    return Err(VerificationError::InvalidFormat);
                }
            }
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

                    // NOTE: Energy fee type is now allowed for transfers to new addresses
                    // The previous restriction has been removed to improve Energy usability

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
                let fee = self.fee;
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
            TransactionType::Energy(payload) => {
                self.verify_energy_payload(payload, state).await?;
            }
            TransactionType::AIMining(_) => {
                // AI Mining transactions don't require special verification beyond basic checks for now
            }
            TransactionType::BindReferrer(payload) => {
                // Validate extra_data size to prevent mempool/storage bloat
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

                    // Validate minimum Shield amount (anti-money-laundering measure)
                    if transfer.get_amount() < MIN_SHIELD_TOS_AMOUNT {
                        return Err(VerificationError::ShieldAmountTooLow);
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
                        "Energy transaction verification - payload: {:?}, fee: {}, nonce: {}",
                        payload, self.fee, self.nonce
                    );
                }
            }
            TransactionType::AIMining(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "AI Mining transaction verification - payload: {:?}, fee: {}, nonce: {}",
                        payload, self.fee, self.nonce
                    );
                }
            }
            TransactionType::BindReferrer(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BindReferrer transaction verification - referrer: {:?}, fee: {}, nonce: {}",
                        payload.get_referrer(), self.fee, self.nonce
                    );
                }
            }
            TransactionType::BatchReferralReward(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BatchReferralReward verification - levels: {}, total_amount: {}, fee: {}",
                        payload.get_levels(),
                        payload.get_total_amount(),
                        self.fee
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
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
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
                    EnergyPayload::FreezeTos { amount, .. } => {
                        // Expired Freeze Recycling: Only charge balance for non-recyclable portion
                        let recyclable_tos = state
                            .get_recyclable_tos(&self.source)
                            .await
                            .map_err(VerificationError::State)?;

                        // Only charge for balance portion (amount - recyclable)
                        let balance_required = amount.saturating_sub(recyclable_tos);

                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(balance_required)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::FreezeTosDelegate { delegatees, .. } => {
                        // Calculate total delegation amount
                        // Delegation does NOT support recycling - must use full balance
                        let total: u64 = delegatees
                            .iter()
                            .try_fold(0u64, |acc, entry| acc.checked_add(entry.amount))
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. } => {
                        // Unfreeze doesn't spend - TOS goes to pending (two-phase)
                    }
                    EnergyPayload::WithdrawUnfrozen => {
                        // Withdraw doesn't spend - it releases pending funds to balance
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // Enforce AIMining fee/stake/reward spending on-chain
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

        // Add fee to TOS spending (unless using energy fee)
        if !self.get_fee_type().is_energy() {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(self.fee)
                .ok_or(VerificationError::Overflow)?;
        }

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

        // Two-phase unfreeze: UnfreezeTos creates pending, WithdrawUnfrozen credits balance
        // UnfreezeTos does NOT credit balance immediately - TOS stays in pending state

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
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                // If invoking constructor, add deposits and max_gas
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
                    EnergyPayload::FreezeTos { amount, .. } => {
                        // Expired Freeze Recycling: Only charge balance for non-recyclable portion
                        let recyclable_tos = state
                            .get_recyclable_tos(&self.source)
                            .await
                            .map_err(VerificationError::State)?;

                        // Only charge for balance portion (amount - recyclable)
                        let balance_required = amount.saturating_sub(recyclable_tos);

                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(balance_required)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::FreezeTosDelegate { delegatees, .. } => {
                        // Calculate total delegation amount
                        // Delegation does NOT support recycling - must use full balance
                        let total: u64 = delegatees
                            .iter()
                            .try_fold(0u64, |acc, entry| acc.checked_add(entry.amount))
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. } => {
                        // Unfreeze doesn't spend - TOS goes to pending (two-phase)
                    }
                    EnergyPayload::WithdrawUnfrozen => {
                        // Withdraw doesn't spend - it releases pending funds to balance
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // Enforce AIMining fee/stake/reward spending on-chain
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

        // Add fee to TOS spending (unless using energy fee)
        if !self.get_fee_type().is_energy() {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(self.fee)
                .ok_or(VerificationError::Overflow)?;

            // Add fee to gas fee counter
            state
                .add_gas_fee(self.fee)
                .await
                .map_err(VerificationError::State)?;
        }

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

        // Handle UNO (encrypted) balance spending for UnshieldTransfers
        // This is separate from plaintext spending because it uses homomorphic ciphertext operations
        if let TransactionType::UnshieldTransfers(transfers) = &self.data {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "apply: Processing UnshieldTransfers UNO spending for source {:?}",
                    self.source
                );
            }

            // Decompress transfer ciphertexts to compute total spending
            let mut output = Ciphertext::zero();
            for transfer in transfers.iter() {
                let decompressed = DecompressedUnshieldTransferCt::decompress(transfer)
                    .map_err(ProofVerificationError::from)?;
                output += decompressed.get_sender_ciphertext();
            }

            // Get sender's UNO balance and deduct spending
            let source_uno_balance = state
                .get_sender_uno_balance(&self.source, &UNO_ASSET, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            // Subtract output from UNO balance (homomorphic subtraction)
            *source_uno_balance -= &output;

            // Track the spending for final balance calculation
            state
                .add_sender_uno_output(&self.source, &UNO_ASSET, output)
                .await
                .map_err(VerificationError::State)?;
        }

        // Handle UNO (encrypted) balance spending for UnoTransfers
        // This is separate from plaintext spending because it uses homomorphic ciphertext operations
        if let TransactionType::UnoTransfers(transfers) = &self.data {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "apply: Processing UnoTransfers UNO spending for source {:?}",
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
            let source_uno_balance = state
                .get_sender_uno_balance(&self.source, &UNO_ASSET, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            // Subtract output from UNO balance (homomorphic subtraction)
            *source_uno_balance -= &output;

            // Track the spending for final balance calculation
            state
                .add_sender_uno_output(&self.source, &UNO_ASSET, output)
                .await
                .map_err(VerificationError::State)?;
        }

        // Handle energy consumption if this transaction uses energy for fees
        if self.get_fee_type().is_energy() {
            // Transfer-type transactions can use energy fees
            match &self.data {
                TransactionType::Transfers(_)
                | TransactionType::UnoTransfers(_)
                | TransactionType::ShieldTransfers(_)
                | TransactionType::UnshieldTransfers(_) => {
                    let energy_cost = self.calculate_energy_cost();

                    // Get user's energy resource
                    let energy_resource = state
                        .get_energy_resource(Cow::Borrowed(&self.source))
                        .await
                        .map_err(VerificationError::State)?;

                    if let Some(mut energy_resource) = energy_resource {
                        let topoheight = state.get_verification_topoheight();

                        // Check if user has enough energy
                        if !energy_resource.has_enough_energy(topoheight, energy_cost) {
                            return Err(VerificationError::InsufficientEnergy(energy_cost));
                        }

                        // Consume energy
                        energy_resource
                            .consume_energy(energy_cost, topoheight)
                            .map_err(|_| VerificationError::InsufficientEnergy(energy_cost))?;

                        // Update energy resource in state
                        state
                            .set_energy_resource(Cow::Borrowed(&self.source), energy_resource)
                            .await
                            .map_err(VerificationError::State)?;

                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Consumed {energy_cost} energy for transaction {tx_hash}");
                        }
                    } else {
                        return Err(VerificationError::InsufficientEnergy(energy_cost));
                    }
                }
                _ => {}
            }
        }

        // Apply receiver balances
        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    // Update receiver balance with plain u64 amount
                    let current_balance = state
                        .get_receiver_balance(
                            Cow::Borrowed(transfer.get_destination()),
                            Cow::Borrowed(transfer.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;

                    // Balance simplification: Add plain u64 amount to receiver's balance
                    let plain_amount = transfer.get_amount();
                    *current_balance = current_balance
                        .checked_add(plain_amount)
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
                    self.invoke_contract(
                        tx_hash,
                        state,
                        &payload.contract,
                        &payload.deposits,
                        payload.parameters.iter().cloned(),
                        payload.max_gas,
                        InvokeContract::Entry(payload.entry_id),
                    )
                    .await?;
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
                    let is_success = self
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
                // Handle energy operations (freeze/unfreeze TOS)
                match payload {
                    EnergyPayload::FreezeTos { amount, duration } => {
                        // Get current energy resource for the account
                        let energy_resource = state
                            .get_energy_resource(Cow::Borrowed(&self.source))
                            .await
                            .map_err(VerificationError::State)?;

                        let mut energy_resource =
                            energy_resource.unwrap_or_else(EnergyResource::new);

                        // Freeze TOS for energy with expired freeze recycling
                        // - Prioritizes recycling TOS from expired freeze records
                        // - Recycled TOS preserves existing energy (no new energy)
                        // - Only TOS from balance generates new energy
                        let topoheight = state.get_verification_topoheight();
                        let network = state.get_network();
                        let result = energy_resource
                            .freeze_tos_with_recycling(*amount, *duration, topoheight, &network)
                            .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{e}")))?;

                        // Update energy resource in state
                        state
                            .set_energy_resource(Cow::Borrowed(&self.source), energy_resource)
                            .await
                            .map_err(VerificationError::State)?;

                        if log::log_enabled!(log::Level::Debug) {
                            if result.recycled_tos > 0 {
                                debug!(
                                    "FreezeTos applied with recycling: {} TOS total ({} recycled, {} from balance), \
                                     {} duration, new energy: {}, recycled energy preserved: {}",
                                    amount,
                                    result.recycled_tos,
                                    result.balance_tos,
                                    duration.name(),
                                    result.new_energy,
                                    result.recycled_energy
                                );
                            } else {
                                debug!(
                                    "FreezeTos applied: {} TOS frozen for {} duration, energy gained: {} units",
                                    amount, duration.name(), result.new_energy
                                );
                            }
                        }
                    }
                    EnergyPayload::FreezeTosDelegate {
                        delegatees,
                        duration,
                    } => {
                        // Get current energy resource for the delegator
                        let energy_resource = state
                            .get_energy_resource(Cow::Borrowed(&self.source))
                            .await
                            .map_err(VerificationError::State)?;

                        let mut energy_resource =
                            energy_resource.unwrap_or_else(EnergyResource::new);

                        // Check record limit
                        if !energy_resource.can_add_freeze_record() {
                            return Err(VerificationError::AnyError(anyhow::anyhow!(
                                "Maximum freeze records reached"
                            )));
                        }

                        let topoheight = state.get_verification_topoheight();
                        let network = state.get_network();

                        // Build delegation entries
                        use crate::account::DelegateRecordEntry;
                        let entries: Vec<DelegateRecordEntry> = delegatees
                            .iter()
                            .map(|d| {
                                let amount_whole = d.amount / crate::config::COIN_VALUE;
                                let energy = amount_whole
                                    .checked_mul(duration.reward_multiplier())
                                    .ok_or(VerificationError::Overflow)?;
                                Ok(DelegateRecordEntry {
                                    delegatee: d.delegatee.clone(),
                                    amount: amount_whole,
                                    energy,
                                })
                            })
                            .collect::<Result<_, VerificationError<E>>>()?;

                        let total_amount: u64 = delegatees
                            .iter()
                            .try_fold(0u64, |acc, entry| {
                                acc.checked_add(entry.amount / crate::config::COIN_VALUE)
                            })
                            .ok_or(VerificationError::Overflow)?;

                        // Create delegated freeze record
                        energy_resource
                            .create_delegated_freeze(
                                entries,
                                *duration,
                                total_amount,
                                topoheight,
                                &network,
                            )
                            .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{e}")))?;

                        // Prepare delegatee updates first to avoid partial writes on overflow
                        let mut updated_delegatees = Vec::with_capacity(delegatees.len());
                        for entry in delegatees.iter() {
                            let amount_whole = entry.amount / crate::config::COIN_VALUE;
                            let energy = amount_whole
                                .checked_mul(duration.reward_multiplier())
                                .ok_or(VerificationError::Overflow)?;

                            let delegatee_resource = state
                                .get_energy_resource(Cow::Borrowed(&entry.delegatee))
                                .await
                                .map_err(VerificationError::State)?;

                            let mut delegatee_resource =
                                delegatee_resource.unwrap_or_else(EnergyResource::new);

                            delegatee_resource
                                .add_delegated_energy(energy, topoheight)
                                .map_err(|_| VerificationError::Overflow)?;

                            updated_delegatees.push((entry.delegatee.clone(), delegatee_resource));
                        }

                        // Update delegator's energy resource
                        state
                            .set_energy_resource(Cow::Borrowed(&self.source), energy_resource)
                            .await
                            .map_err(VerificationError::State)?;

                        // Apply delegatee updates
                        for (delegatee, delegatee_resource) in updated_delegatees {
                            state
                                .set_energy_resource(Cow::Owned(delegatee), delegatee_resource)
                                .await
                                .map_err(VerificationError::State)?;
                        }

                        if log::log_enabled!(log::Level::Debug) {
                            debug!(
                                "FreezeTosDelegate applied: {} TOS delegated to {} accounts",
                                total_amount,
                                delegatees.len()
                            );
                        }
                    }
                    EnergyPayload::UnfreezeTos {
                        amount,
                        from_delegation,
                        record_index,
                        delegatee_address,
                    } => {
                        if !*from_delegation && delegatee_address.is_some() {
                            return Err(VerificationError::AnyError(anyhow::anyhow!(
                                "Invalid delegatee_address usage"
                            )));
                        }

                        // Get current energy resource for the account
                        let energy_resource = state
                            .get_energy_resource(Cow::Borrowed(&self.source))
                            .await
                            .map_err(VerificationError::State)?;

                        if let Some(mut energy_resource) = energy_resource {
                            // Check pending unfreeze limit
                            if !energy_resource.can_add_pending_unfreeze() {
                                return Err(VerificationError::AnyError(anyhow::anyhow!(
                                    "Maximum pending unfreezes reached"
                                )));
                            }

                            let topoheight = state.get_verification_topoheight();
                            let network = state.get_network();

                            if *from_delegation {
                                // Unfreeze from delegated records
                                // This removes energy from delegatees and creates pending unfreeze for delegator
                                let (_delegatee_key, _energy_removed, _pending_amount) =
                                    if let Some(delegatee_address) = delegatee_address.as_ref() {
                                        energy_resource
                                            .unfreeze_delegated_entry(
                                                *amount,
                                                topoheight,
                                                *record_index,
                                                delegatee_address,
                                                &network,
                                            )
                                            .map_err(|e| {
                                                VerificationError::AnyError(anyhow::anyhow!("{e}"))
                                            })?
                                    } else {
                                        if energy_resource.delegated_records.is_empty() {
                                            return Err(VerificationError::AnyError(
                                                anyhow::anyhow!("No delegated records found"),
                                            ));
                                        }

                                        let record_idx = match *record_index {
                                            Some(idx) => {
                                                let idx = idx as usize;
                                                if idx >= energy_resource.delegated_records.len() {
                                                    return Err(VerificationError::AnyError(
                                                        anyhow::anyhow!(
                                                            "Record index out of bounds"
                                                        ),
                                                    ));
                                                }
                                                idx
                                            }
                                            None => {
                                                if energy_resource.delegated_records.len() > 1 {
                                                    return Err(VerificationError::AnyError(
                                                        anyhow::anyhow!(
                                                            "Multiple delegation records exist, record_index required"
                                                        ),
                                                    ));
                                                }
                                                0
                                            }
                                        };

                                        let record = &energy_resource.delegated_records[record_idx];
                                        if record.entries.len() > 1 {
                                            return Err(VerificationError::AnyError(
                                                anyhow::anyhow!(
                                            "Delegatee address required for batch delegations"
                                        ),
                                            ));
                                        }

                                        let delegatee = record
                                            .entries
                                            .first()
                                            .ok_or_else(|| {
                                                VerificationError::AnyError(anyhow::anyhow!(
                                                    "Delegatee not found in record"
                                                ))
                                            })?
                                            .delegatee
                                            .clone();

                                        energy_resource
                                            .unfreeze_delegated_entry(
                                                *amount,
                                                topoheight,
                                                Some(record_idx as u32),
                                                &delegatee,
                                                &network,
                                            )
                                            .map_err(|e| {
                                                VerificationError::AnyError(anyhow::anyhow!("{e}"))
                                            })?
                                    };

                                // Update sender's energy resource first
                                state
                                    .set_energy_resource(
                                        Cow::Borrowed(&self.source),
                                        energy_resource,
                                    )
                                    .await
                                    .map_err(VerificationError::State)?;

                                if log::log_enabled!(log::Level::Debug) {
                                    debug!("UnfreezeTos (delegation) applied: {amount} TOS moved to pending (14-day cooldown)");
                                }
                            } else {
                                // Unfreeze from self-freeze records (two-phase: creates pending)
                                energy_resource
                                    .unfreeze_tos(*amount, topoheight, *record_index, &network)
                                    .map_err(|e| {
                                        VerificationError::AnyError(anyhow::anyhow!("{e}"))
                                    })?;

                                // Update energy resource in state
                                state
                                    .set_energy_resource(
                                        Cow::Borrowed(&self.source),
                                        energy_resource,
                                    )
                                    .await
                                    .map_err(VerificationError::State)?;

                                // NOTE: TOS goes to pending state, NOT to balance
                                // Use WithdrawUnfrozen after 14-day cooldown to get TOS back

                                if log::log_enabled!(log::Level::Debug) {
                                    debug!("UnfreezeTos applied: {amount} TOS moved to pending (14-day cooldown)");
                                }
                            }
                        } else {
                            return Err(VerificationError::AnyError(anyhow::anyhow!(
                                "No energy resource found"
                            )));
                        }
                    }
                    EnergyPayload::WithdrawUnfrozen => {
                        // Get current energy resource for the account
                        let energy_resource = state
                            .get_energy_resource(Cow::Borrowed(&self.source))
                            .await
                            .map_err(VerificationError::State)?;

                        if let Some(mut energy_resource) = energy_resource {
                            let topoheight = state.get_verification_topoheight();

                            // Check if there are withdrawable funds
                            if energy_resource.pending_unfreezes.is_empty() {
                                return Err(VerificationError::AnyError(anyhow::anyhow!(
                                    "No pending unfreezes"
                                )));
                            }
                            let withdrawable = energy_resource
                                .withdrawable_unfreeze(topoheight)
                                .map_err(|_| VerificationError::Overflow)?;
                            if withdrawable == 0 {
                                return Err(VerificationError::AnyError(anyhow::anyhow!(
                                    "No expired unfreezes"
                                )));
                            }

                            // Withdraw unfrozen TOS
                            let withdrawn = energy_resource
                                .withdraw_unfrozen(topoheight)
                                .map_err(|_| VerificationError::Overflow)?;

                            // Update energy resource in state
                            state
                                .set_energy_resource(Cow::Borrowed(&self.source), energy_resource)
                                .await
                                .map_err(VerificationError::State)?;

                            // Credit TOS to user's balance
                            let balance = state
                                .get_receiver_balance(
                                    Cow::Borrowed(self.get_source()),
                                    Cow::Borrowed(&TOS_ASSET),
                                )
                                .await
                                .map_err(VerificationError::State)?;

                            *balance = balance
                                .checked_add(withdrawn)
                                .ok_or(VerificationError::Overflow)?;

                            if log::log_enabled!(log::Level::Debug) {
                                debug!(
                                    "WithdrawUnfrozen applied: {} TOS returned to balance",
                                    withdrawn
                                );
                            }
                        } else {
                            return Err(VerificationError::AnyError(anyhow::anyhow!(
                                "No energy resource found"
                            )));
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

                // Credit rewards to each upline's balance
                // Note: distribution.recipient is already a CompressedPublicKey (aka crypto::PublicKey)
                for distribution in &distribution_result.distributions {
                    let balance = state
                        .get_receiver_balance(
                            std::borrow::Cow::Owned(distribution.recipient.clone()),
                            std::borrow::Cow::Borrowed(payload.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;
                    *balance = balance
                        .checked_add(distribution.amount)
                        .ok_or(VerificationError::Overflow)?;
                }

                // Refund remainder to sender (prevents burning undistributed funds)
                let remainder = payload
                    .get_total_amount()
                    .saturating_sub(distribution_result.total_distributed);
                if remainder > 0 {
                    let sender_balance = state
                        .get_receiver_balance(
                            std::borrow::Cow::Borrowed(self.get_source()),
                            std::borrow::Cow::Borrowed(payload.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;
                    *sender_balance = sender_balance
                        .checked_add(remainder)
                        .ok_or(VerificationError::Overflow)?;
                }

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BatchReferralReward transaction applied - levels: {}, total_amount: {}, distributed: {}, refunded: {}",
                        payload.get_levels(),
                        payload.get_total_amount(),
                        distribution_result.total_distributed,
                        remainder
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

                // Pass transferred_at to bind approval signatures to timestamp
                // Pass network for cross-network replay protection
                // Pass current_level to bind approval to user's KYC level
                crate::kyc::verify_transfer_kyc_dest_approvals(
                    &network,
                    &dest_committee,
                    payload.get_dest_approvals(),
                    payload.get_source_committee_id(),
                    payload.get_account(),
                    current_level,
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
                // Defense-in-depth: Re-verify authorization in apply phase
                let bootstrap_pubkey = {
                    use crate::crypto::Address;
                    let addr =
                        Address::from_string(crate::config::BOOTSTRAP_ADDRESS).map_err(|e| {
                            VerificationError::AnyError(anyhow::anyhow!(
                                "Invalid bootstrap address configuration: {}",
                                e
                            ))
                        })?;
                    addr.to_public_key()
                };

                if self.get_source() != &bootstrap_pubkey {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "BootstrapCommittee can only be submitted by BOOTSTRAP_ADDRESS"
                    )));
                }

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
                        // Member MUST exist for these operations
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
                    // Add plaintext amount to receiver's TOS balance
                    let current_balance = state
                        .get_receiver_balance(
                            Cow::Borrowed(transfer.get_destination()),
                            Cow::Borrowed(&TOS_ASSET),
                        )
                        .await
                        .map_err(VerificationError::State)?;

                    *current_balance = current_balance
                        .checked_add(transfer.get_amount())
                        .ok_or(VerificationError::Overflow)?;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Unshield transfer applied - receiver: {:?}, amount: {}",
                            transfer.get_destination(),
                            transfer.get_amount()
                        );
                    }
                }
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
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
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
                    EnergyPayload::FreezeTos { amount, .. } => {
                        // Expired Freeze Recycling: Only charge balance for non-recyclable portion
                        let recyclable_tos = state
                            .get_recyclable_tos(&self.source)
                            .await
                            .map_err(VerificationError::State)?;

                        // Only charge for balance portion (amount - recyclable)
                        let balance_required = amount.saturating_sub(recyclable_tos);

                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(balance_required)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::FreezeTosDelegate { delegatees, .. } => {
                        // Calculate total delegation amount
                        // Delegation does NOT support recycling - must use full balance
                        let total: u64 = delegatees
                            .iter()
                            .try_fold(0u64, |acc, entry| acc.checked_add(entry.amount))
                            .ok_or(VerificationError::Overflow)?;
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(total)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. } => {
                        // Unfreeze doesn't spend - TOS goes to pending (two-phase)
                    }
                    EnergyPayload::WithdrawUnfrozen => {
                        // Withdraw doesn't spend - it releases pending funds to balance
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // Enforce AIMining fee/stake/reward spending on-chain
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

        // Add fee to TOS spending (unless using energy fee)
        if !self.get_fee_type().is_energy() {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(self.fee)
                .ok_or(VerificationError::Overflow)?;
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
