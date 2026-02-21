use serde::{Deserialize, Serialize};
use tos_crypto::merlin::Transcript;

use crate::{
    account::Nonce,
    crypto::{
        elgamal::CompressedPublicKey, proofs::RangeProof, Hash, Hashable, ProtocolTranscript,
        Signature,
    },
    serializer::*,
};

use multisig::MultiSig;

pub mod builder;
pub mod encoding;
pub mod extra_data;
pub mod multisig;
pub mod verify;

mod payload;
mod reference;
mod source_commitment;
mod version;

pub use payload::*;
pub use reference::Reference;
pub use source_commitment::SourceCommitment;
pub use version::TxVersion;

#[cfg(test)]
mod tests;

// Maximum size of extra data per transfer (memo field)
// Optimized for real-world usage: covers 99%+ actual needs (exchange IDs, order info, etc.)
// while preventing storage bloat and attack vectors
pub const EXTRA_DATA_LIMIT_SIZE: usize = 128; // 128 bytes - balanced for security and usability
                                              // Maximum total size of payload across all transfers per transaction
pub const EXTRA_DATA_LIMIT_SUM_SIZE: usize = EXTRA_DATA_LIMIT_SIZE * 32; // 4KB total limit
                                                                         // Maximum number of transfers per transaction
pub const MAX_TRANSFER_COUNT: usize = 500;
// Maximum number of deposits per Invoke Call
pub const MAX_DEPOSIT_PER_INVOKE_CALL: usize = 255;
// Maximum number of participants in a multi signature account
pub const MAX_MULTISIG_PARTICIPANTS: usize = 255;

// TransactionType serialization opcodes.
// Keep these values stable across protocol participants.
pub const TX_TYPE_OPCODE_BURN: u8 = 0;
pub const TX_TYPE_OPCODE_TRANSFERS: u8 = 1;
pub const TX_TYPE_OPCODE_MULTISIG: u8 = 2;
pub const TX_TYPE_OPCODE_INVOKE_CONTRACT: u8 = 3;
pub const TX_TYPE_OPCODE_DEPLOY_CONTRACT: u8 = 4;
pub const TX_TYPE_OPCODE_UNO_TRANSFERS: u8 = 5;
pub const TX_TYPE_OPCODE_SHIELD_TRANSFERS: u8 = 6;
pub const TX_TYPE_OPCODE_UNSHIELD_TRANSFERS: u8 = 7;

// this enum represent all types of transaction available on Tos Network
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TransactionType {
    Transfers(Vec<TransferPayload>),
    Burn(BurnPayload),
    MultiSig(MultiSigPayload),
    InvokeContract(InvokeContractPayload),
    DeployContract(DeployContractPayload),
    /// UNO privacy transfers (encrypted amounts)
    UnoTransfers(Vec<UnoTransferPayload>),
    /// Shield transfers: TOS (plaintext) -> UNO (encrypted)
    ShieldTransfers(Vec<ShieldTransferPayload>),
    /// Unshield transfers: UNO (encrypted) -> TOS (plaintext)
    UnshieldTransfers(Vec<UnshieldTransferPayload>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum FeeType {
    /// Transaction uses TOS for fees (traditional fee model)
    TOS,
    /// Transaction uses UNO for fees (UnoTransfers only, fee burned)
    UNO,
}

impl FeeType {
    /// Check if this fee type is TOS-based
    pub fn is_tos(&self) -> bool {
        matches!(self, FeeType::TOS)
    }
    /// Check if this fee type is UNO-based
    pub fn is_uno(&self) -> bool {
        matches!(self, FeeType::UNO)
    }
}

impl Serializer for FeeType {
    fn write(&self, writer: &mut Writer) {
        let v = match self {
            FeeType::TOS => 0u8,
            FeeType::UNO => 2u8,
        };
        writer.write_u8(v);
    }
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        match reader.read_u8()? {
            0 => Ok(FeeType::TOS),
            2 => Ok(FeeType::UNO),
            _ => Err(ReaderError::InvalidValue),
        }
    }
    fn size(&self) -> usize {
        1
    }
}

// Transaction to be sent over the network
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transaction {
    /// Version of the transaction
    version: TxVersion,
    /// Chain ID for cross-network replay protection (T1+)
    /// 0 = Mainnet, 1 = Testnet, 2 = Stagenet, 3 = Devnet
    chain_id: u8,
    // Source of the transaction
    source: CompressedPublicKey,
    /// Type of the transaction
    data: TransactionType,
    /// Fees in Tos (TOS or UNO depending on fee_type)
    fee: u64,
    /// Fee type: TOS or UNO
    fee_type: FeeType,
    /// nonce must be equal to the one on chain account
    /// used to prevent replay attacks and have ordered transactions
    nonce: Nonce,
    /// Source commitments for UNO transfers (one per asset)
    /// Empty for plaintext-only transactions
    source_commitments: Vec<SourceCommitment>,
    /// Aggregated range proof for all UNO transfers
    /// None for plaintext-only transactions
    range_proof: Option<RangeProof>,
    /// At which block the TX is built
    reference: Reference,
    /// MultiSig contains the signatures of the transaction
    /// Only available since V1
    multisig: Option<MultiSig>,
    /// The signature of the source key
    signature: Signature,
}

impl Transaction {
    // Create a new transaction (plaintext, no UNO)
    #[inline(always)]
    pub fn new(
        version: TxVersion,
        chain_id: u8,
        source: CompressedPublicKey,
        data: TransactionType,
        fee: u64,
        fee_type: FeeType,
        nonce: Nonce,
        reference: Reference,
        multisig: Option<MultiSig>,
        signature: Signature,
    ) -> Self {
        Self {
            version,
            chain_id,
            source,
            data,
            fee,
            fee_type,
            nonce,
            source_commitments: Vec::new(),
            range_proof: None,
            reference,
            multisig,
            signature,
        }
    }

    // Create a new UNO transaction with privacy proofs
    #[inline(always)]
    pub fn new_with_uno(
        version: TxVersion,
        chain_id: u8,
        source: CompressedPublicKey,
        data: TransactionType,
        fee: u64,
        fee_type: FeeType,
        nonce: Nonce,
        source_commitments: Vec<SourceCommitment>,
        range_proof: RangeProof,
        reference: Reference,
        multisig: Option<MultiSig>,
        signature: Signature,
    ) -> Self {
        Self {
            version,
            chain_id,
            source,
            data,
            fee,
            fee_type,
            nonce,
            source_commitments,
            range_proof: Some(range_proof),
            reference,
            multisig,
            signature,
        }
    }

    /// Prepare a transcript for ZK proof generation/verification
    /// This establishes the common reference string for all proofs in the transaction
    pub fn prepare_transcript(
        version: TxVersion,
        source_pubkey: &CompressedPublicKey,
        fee: u64,
        fee_type: &FeeType,
        nonce: Nonce,
    ) -> Transcript {
        let mut transcript = Transcript::new(b"transaction-proof");
        transcript.append_u64(b"version", version.into());
        transcript.append_public_key(b"source_pubkey", source_pubkey);
        transcript.append_u64(b"fee", fee);
        transcript.append_u64(
            b"fee_type",
            match fee_type {
                FeeType::TOS => 0u64,
                FeeType::UNO => 2u64,
            },
        );
        transcript.append_u64(b"nonce", nonce);
        transcript
    }

    // Get the transaction version
    pub fn get_version(&self) -> TxVersion {
        self.version
    }

    // Get the chain ID (for cross-network replay protection)
    pub fn get_chain_id(&self) -> u8 {
        self.chain_id
    }

    // Get the source key
    pub fn get_source(&self) -> &CompressedPublicKey {
        &self.source
    }

    // Get the transaction type
    pub fn get_data(&self) -> &TransactionType {
        &self.data
    }

    // Get fees paid to miners
    pub fn get_fee(&self) -> u64 {
        self.fee
    }

    // Get the nonce used
    pub fn get_nonce(&self) -> Nonce {
        self.nonce
    }

    // Get the multisig
    pub fn get_multisig(&self) -> &Option<MultiSig> {
        &self.multisig
    }

    // Get the count of signatures in a multisig transaction
    pub fn get_multisig_count(&self) -> usize {
        self.multisig.as_ref().map(|m| m.len()).unwrap_or(0)
    }

    // Get the signature of source key
    pub fn get_signature(&self) -> &Signature {
        &self.signature
    }

    // Get the block reference to determine which block the transaction is built
    pub fn get_reference(&self) -> &Reference {
        &self.reference
    }

    // Get source commitments for UNO transfers
    pub fn get_source_commitments(&self) -> &[SourceCommitment] {
        &self.source_commitments
    }

    // Get the range proof for UNO transfers
    pub fn get_range_proof(&self) -> Option<&RangeProof> {
        self.range_proof.as_ref()
    }

    // Check if this transaction involves UNO transfers (including Shield/Unshield)
    pub fn has_uno_transfers(&self) -> bool {
        matches!(
            self.data,
            TransactionType::UnoTransfers(_)
                | TransactionType::ShieldTransfers(_)
                | TransactionType::UnshieldTransfers(_)
        )
    }

    // Get the burned amount
    // This will returns the burned amount by a Burn payload
    // Or the % of execution fees to burn due to a Smart Contracts call
    // only if the asset is Tos
    pub fn get_burned_amount(&self, asset: &Hash) -> Option<u64> {
        match &self.data {
            TransactionType::Burn(payload) if payload.asset == *asset => Some(payload.amount),
            _ => None,
        }
    }

    // Get all assets used in this transaction
    // Returns a HashSet containing all unique assets referenced in the transaction
    pub fn get_assets(&self) -> std::collections::HashSet<&Hash> {
        use std::collections::HashSet;
        let mut assets = HashSet::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    assets.insert(transfer.get_asset());
                }
            }
            TransactionType::UnoTransfers(transfers) => {
                for transfer in transfers {
                    assets.insert(transfer.get_asset());
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                for transfer in transfers {
                    assets.insert(transfer.get_asset());
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                for transfer in transfers {
                    assets.insert(transfer.get_asset());
                }
            }
            TransactionType::Burn(payload) => {
                assets.insert(&payload.asset);
            }
            TransactionType::InvokeContract(payload) => {
                for (asset, _) in &payload.deposits {
                    assets.insert(asset);
                }
            }
            TransactionType::DeployContract(payload) => {
                if let Some(invoke) = &payload.invoke {
                    for (asset, _) in &invoke.deposits {
                        assets.insert(asset);
                    }
                }
            }
            // MultiSig-like payloads don't have explicit assets
            _ => {}
        }

        assets
    }

    // Get the total outputs count per TX
    // default is 1
    // Transfers / Deposits are their own len
    pub fn get_outputs_count(&self) -> usize {
        match &self.data {
            TransactionType::Transfers(transfers) => transfers.len(),
            TransactionType::UnoTransfers(transfers) => transfers.len(),
            TransactionType::ShieldTransfers(transfers) => transfers.len(),
            TransactionType::UnshieldTransfers(transfers) => transfers.len(),
            TransactionType::InvokeContract(payload) => payload.deposits.len().max(1),
            _ => 1,
        }
    }

    // Get the fee type
    pub fn get_fee_type(&self) -> &FeeType {
        &self.fee_type
    }

    /// Get the bytes that were used for signing this transaction
    /// This matches the logic used in UnsignedTransaction::finalize
    pub fn get_signing_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);

        self.version.write(&mut writer);

        // T1+: include chain_id for cross-network replay protection
        if self.version >= TxVersion::T1 {
            self.chain_id.write(&mut writer);
        }

        self.source.write(&mut writer);
        self.data.write(&mut writer);
        self.fee.write(&mut writer);
        self.fee_type.write(&mut writer);
        self.nonce.write(&mut writer);
        self.reference.write(&mut writer);
        // Do NOT include multisig - multisig participants sign without it

        buffer
    }

    /// Get the bytes that multisig participants signed
    /// This matches the logic used in UnsignedTransaction::get_hash_for_multisig
    pub fn get_multisig_signing_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);

        // Multisig participants sign the transaction data without the multisig field
        // This matches the logic in UnsignedTransaction::write_no_signature
        self.version.write(&mut writer);

        // T1+: include chain_id for cross-network replay protection
        if self.version >= TxVersion::T1 {
            self.chain_id.write(&mut writer);
        }

        self.source.write(&mut writer);
        self.data.write(&mut writer);
        self.fee.write(&mut writer);
        self.fee_type.write(&mut writer);
        self.nonce.write(&mut writer);
        self.reference.write(&mut writer);
        // Do NOT include multisig field - it should not be part of the main signature

        buffer
    }

    pub fn consume(self) -> (CompressedPublicKey, TransactionType) {
        (self.source, self.data)
    }
}

impl Serializer for TransactionType {
    fn write(&self, writer: &mut Writer) {
        match self {
            TransactionType::Burn(payload) => {
                writer.write_u8(TX_TYPE_OPCODE_BURN);
                payload.write(writer);
            }
            TransactionType::Transfers(txs) => {
                writer.write_u8(TX_TYPE_OPCODE_TRANSFERS);
                let len: u16 = txs.len() as u16;
                writer.write_u16(len);
                for tx in txs {
                    tx.write(writer);
                }
            }
            TransactionType::MultiSig(payload) => {
                writer.write_u8(TX_TYPE_OPCODE_MULTISIG);
                payload.write(writer);
            }
            TransactionType::InvokeContract(payload) => {
                writer.write_u8(TX_TYPE_OPCODE_INVOKE_CONTRACT);
                payload.write(writer);
            }
            TransactionType::DeployContract(module) => {
                writer.write_u8(TX_TYPE_OPCODE_DEPLOY_CONTRACT);
                module.write(writer);
            }
            TransactionType::UnoTransfers(transfers) => {
                writer.write_u8(TX_TYPE_OPCODE_UNO_TRANSFERS);
                let len: u16 = transfers.len() as u16;
                writer.write_u16(len);
                for tx in transfers {
                    tx.write(writer);
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                writer.write_u8(TX_TYPE_OPCODE_SHIELD_TRANSFERS);
                let len: u16 = transfers.len() as u16;
                writer.write_u16(len);
                for tx in transfers {
                    tx.write(writer);
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                writer.write_u8(TX_TYPE_OPCODE_UNSHIELD_TRANSFERS);
                let len: u16 = transfers.len() as u16;
                writer.write_u16(len);
                for tx in transfers {
                    tx.write(writer);
                }
            }
        };
    }

    fn read(reader: &mut Reader) -> Result<TransactionType, ReaderError> {
        Ok(match reader.read_u8()? {
            TX_TYPE_OPCODE_BURN => {
                let payload = BurnPayload::read(reader)?;
                TransactionType::Burn(payload)
            }
            TX_TYPE_OPCODE_TRANSFERS => {
                let txs_count = reader.read_u16()?;
                if txs_count == 0 || txs_count as usize > MAX_TRANSFER_COUNT {
                    return Err(ReaderError::InvalidSize);
                }

                let mut txs = Vec::with_capacity(txs_count as usize);
                for _ in 0..txs_count {
                    txs.push(TransferPayload::read(reader)?);
                }
                TransactionType::Transfers(txs)
            }
            TX_TYPE_OPCODE_MULTISIG => TransactionType::MultiSig(MultiSigPayload::read(reader)?),
            TX_TYPE_OPCODE_INVOKE_CONTRACT => {
                TransactionType::InvokeContract(InvokeContractPayload::read(reader)?)
            }
            TX_TYPE_OPCODE_DEPLOY_CONTRACT => {
                TransactionType::DeployContract(DeployContractPayload::read(reader)?)
            }
            TX_TYPE_OPCODE_UNO_TRANSFERS => {
                let txs_count = reader.read_u16()?;
                if txs_count == 0 || txs_count as usize > MAX_TRANSFER_COUNT {
                    return Err(ReaderError::InvalidSize);
                }
                let mut txs = Vec::with_capacity(txs_count as usize);
                for _ in 0..txs_count {
                    txs.push(UnoTransferPayload::read(reader)?);
                }
                TransactionType::UnoTransfers(txs)
            }
            TX_TYPE_OPCODE_SHIELD_TRANSFERS => {
                let txs_count = reader.read_u16()?;
                if txs_count == 0 || txs_count as usize > MAX_TRANSFER_COUNT {
                    return Err(ReaderError::InvalidSize);
                }
                let mut txs = Vec::with_capacity(txs_count as usize);
                for _ in 0..txs_count {
                    txs.push(ShieldTransferPayload::read(reader)?);
                }
                TransactionType::ShieldTransfers(txs)
            }
            TX_TYPE_OPCODE_UNSHIELD_TRANSFERS => {
                let txs_count = reader.read_u16()?;
                if txs_count == 0 || txs_count as usize > MAX_TRANSFER_COUNT {
                    return Err(ReaderError::InvalidSize);
                }
                let mut txs = Vec::with_capacity(txs_count as usize);
                for _ in 0..txs_count {
                    txs.push(UnshieldTransferPayload::read(reader)?);
                }
                TransactionType::UnshieldTransfers(txs)
            }
            _ => return Err(ReaderError::InvalidValue),
        })
    }

    fn size(&self) -> usize {
        1 + match self {
            TransactionType::Burn(payload) => payload.size(),
            TransactionType::Transfers(txs) => {
                // 2 bytes for count of transfers (u16)
                let mut size = 2;
                for tx in txs {
                    size += tx.size();
                }
                size
            }
            TransactionType::MultiSig(payload) => {
                // 1 byte for variant, 1 byte for threshold, 1 byte for count of participants
                1 + 1 + payload.participants.iter().map(|p| p.size()).sum::<usize>()
            }
            TransactionType::InvokeContract(payload) => payload.size(),
            TransactionType::DeployContract(module) => module.size(),
            TransactionType::UnoTransfers(txs) => {
                let mut size = 2;
                for tx in txs {
                    size += tx.size();
                }
                size
            }
            TransactionType::ShieldTransfers(txs) => {
                let mut size = 2;
                for tx in txs {
                    size += tx.size();
                }
                size
            }
            TransactionType::UnshieldTransfers(txs) => {
                let mut size = 2;
                for tx in txs {
                    size += tx.size();
                }
                size
            }
        }
    }
}

impl Serializer for Transaction {
    fn write(&self, writer: &mut Writer) {
        self.version.write(writer);

        // T1+: include chain_id for cross-network replay protection
        if self.version >= TxVersion::T1 {
            self.chain_id.write(writer);
        }

        self.source.write(writer);
        self.data.write(writer);
        self.fee.write(writer);
        self.fee_type.write(writer);
        self.nonce.write(writer);

        // UNO fields: source_commitments and range_proof
        // Only written for UNO transactions
        if self.has_uno_transfers() {
            // source_commitments represents assets in UNO transfers, realistically limited
            // This debug_assert catches programming errors; verification phase enforces the limit
            debug_assert!(
                self.source_commitments.len() <= u8::MAX as usize,
                "source_commitments length {} exceeds u8 max, serialization would truncate",
                self.source_commitments.len()
            );
            // Clamp length to u8::MAX and only write that many items
            // This ensures the written length matches actual items written
            // preventing deserialization corruption if limit is exceeded
            let len: u8 = self.source_commitments.len().min(u8::MAX as usize) as u8;
            writer.write_u8(len);
            // CRITICAL: Only iterate up to `len` items to match the header
            // Previously this wrote ALL items regardless of clamped length
            for sc in self.source_commitments.iter().take(len as usize) {
                sc.write(writer);
            }
            if let Some(ref rp) = self.range_proof {
                rp.write(writer);
            }
        }

        self.reference.write(writer);

        self.multisig.write(writer);

        self.signature.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Transaction, ReaderError> {
        let version = TxVersion::read(reader)?;

        reader.context_mut().store(version);

        // T1+: read chain_id for cross-network replay protection
        let chain_id = reader.read_u8()?;

        let source = CompressedPublicKey::read(reader)?;
        let data = TransactionType::read(reader)?;
        let fee = reader.read_u64()?;
        let fee_type = FeeType::read(reader)?;
        let nonce = Nonce::read(reader)?;

        // UNO fields: source_commitments and range_proof
        // Read for UNO transactions (including Shield/Unshield)
        let (source_commitments, range_proof) = match &data {
            TransactionType::UnoTransfers(_) => {
                // UNO transfers always have source_commitments and range_proof
                let sc_count = reader.read_u8()?;
                let mut scs = Vec::with_capacity(sc_count as usize);
                for _ in 0..sc_count {
                    scs.push(SourceCommitment::read(reader)?);
                }
                let rp = RangeProof::read(reader)?;
                (scs, Some(rp))
            }
            TransactionType::ShieldTransfers(_) => {
                // Shield transfers have source_commitments count (should be 0) but no range_proof
                let sc_count = reader.read_u8()?;
                let mut scs = Vec::with_capacity(sc_count as usize);
                for _ in 0..sc_count {
                    scs.push(SourceCommitment::read(reader)?);
                }
                // Shield doesn't require range proof (amount is public)
                (scs, None)
            }
            TransactionType::UnshieldTransfers(_) => {
                // Unshield transfers have source_commitments and range_proof
                let sc_count = reader.read_u8()?;
                let mut scs = Vec::with_capacity(sc_count as usize);
                for _ in 0..sc_count {
                    scs.push(SourceCommitment::read(reader)?);
                }
                // Unshield requires range proof for remaining balance
                let rp = if sc_count > 0 {
                    Some(RangeProof::read(reader)?)
                } else {
                    None
                };
                (scs, rp)
            }
            _ => (Vec::new(), None),
        };

        let reference = Reference::read(reader)?;

        let multisig = Option::read(reader)?;

        let signature = Signature::read(reader)?;

        Ok(Transaction {
            version,
            chain_id,
            source,
            data,
            fee,
            fee_type,
            nonce,
            source_commitments,
            range_proof,
            reference,
            multisig,
            signature,
        })
    }

    fn size(&self) -> usize {
        // Version byte
        let mut size = 1
            + self.source.size()
            + self.data.size()
            + self.fee.size()
            + self.fee_type.size()
            + self.nonce.size()
            + self.reference.size()
            + self.signature.size();

        // T1+: add chain_id byte
        if self.version >= TxVersion::T1 {
            size += 1;
        }

        // UNO fields
        if self.has_uno_transfers() {
            size += 1; // source_commitments count
            for sc in &self.source_commitments {
                size += sc.size();
            }
            if let Some(ref rp) = self.range_proof {
                size += rp.size();
            }
        }

        size += self.multisig.size();

        size
    }
}

impl Hashable for Transaction {}

impl AsRef<Transaction> for Transaction {
    fn as_ref(&self) -> &Transaction {
        self
    }
}
