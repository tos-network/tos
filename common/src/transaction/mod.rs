use serde::{Deserialize, Serialize};

use crate::{
    account::Nonce,
    crypto::{
        elgamal::CompressedPublicKey,
        Hash,
        Hashable,
        Signature,
    },
    serializer::*,
    ai_mining::AIMiningPayload,
};

use multisig::MultiSig;

pub mod builder;
pub mod verify;
pub mod extra_data;
pub mod multisig;

mod payload;
mod reference;
mod version;

pub use payload::*;
pub use reference::Reference;
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
pub const MAX_TRANSFER_COUNT: usize = 255;
// Maximum number of deposits per Invoke Call
pub const MAX_DEPOSIT_PER_INVOKE_CALL: usize = 255;
// Maximum number of participants in a multi signature account
pub const MAX_MULTISIG_PARTICIPANTS: usize = 255;

// this enum represent all types of transaction available on Tos Network
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TransactionType {
    Transfers(Vec<TransferPayload>),
    Burn(BurnPayload),
    MultiSig(MultiSigPayload),
    InvokeContract(InvokeContractPayload),
    DeployContract(DeployContractPayload),
    Energy(EnergyPayload),
    AIMining(AIMiningPayload),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum FeeType {
    /// Transaction uses TOS for fees (traditional fee model)
    TOS,
    /// Transaction uses Energy for fees (only available for Transfer transactions)
    Energy,
}

impl FeeType {
    /// Check if this fee type is Energy-based
    pub fn is_energy(&self) -> bool {
        matches!(self, FeeType::Energy)
    }
    /// Check if this fee type is TOS-based
    pub fn is_tos(&self) -> bool {
        matches!(self, FeeType::TOS)
    }
}

impl Serializer for FeeType {
    fn write(&self, writer: &mut Writer) {
        let v = match self {
            FeeType::TOS => 0u8,
            FeeType::Energy => 1u8,
        };
        writer.write_u8(v);
    }
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        match reader.read_u8()? {
            0 => Ok(FeeType::TOS),
            1 => Ok(FeeType::Energy),
            _ => Err(ReaderError::InvalidValue),
        }
    }
    fn size(&self) -> usize { 1 }
}

// Transaction to be sent over the network
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transaction {
    /// Version of the transaction
    version: TxVersion,
    // Source of the transaction
    source: CompressedPublicKey,
    /// Type of the transaction
    data: TransactionType,
    /// Fees in Tos (TOS or Energy depending on fee_type)
    fee: u64,
    /// Fee type: TOS or Energy
    fee_type: FeeType,
    /// nonce must be equal to the one on chain account
    /// used to prevent replay attacks and have ordered transactions
    nonce: Nonce,
    /// At which block the TX is built
    reference: Reference,
    /// MultiSig contains the signatures of the transaction
    /// Only available since V1
    multisig: Option<MultiSig>,
    /// The signature of the source key
    signature: Signature,
}

impl Transaction {
    // Create a new transaction
    #[inline(always)]
    pub fn new(
        version: TxVersion,
        source: CompressedPublicKey,
        data: TransactionType,
        fee: u64,
        fee_type: FeeType,
        nonce: Nonce,
        reference: Reference,
        multisig: Option<MultiSig>,
        signature: Signature
    ) -> Self {
        Self {
            version,
            source,
            data,
            fee,
            fee_type,
            nonce,
            reference,
            multisig,
            signature,
        }
    }

    // Get the transaction version
    pub fn get_version(&self) -> TxVersion {
        self.version
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

    // Get the burned amount
    // This will returns the burned amount by a Burn payload
    // Or the % of execution fees to burn due to a Smart Contracts call
    // only if the asset is Tos
    pub fn get_burned_amount(&self, asset: &Hash) -> Option<u64> {
        match &self.data {
            TransactionType::Burn(payload) if payload.asset == *asset => Some(payload.amount),
            _ => None
        }
    }

    // Get all assets used in this transaction (plaintext balance version)
    // Returns a HashSet containing all unique assets referenced in the transaction
    pub fn get_assets(&self) -> std::collections::HashSet<&Hash> {
        use std::collections::HashSet;
        let mut assets = HashSet::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    assets.insert(transfer.get_asset());
                }
            },
            TransactionType::Burn(payload) => {
                assets.insert(&payload.asset);
            },
            TransactionType::InvokeContract(payload) => {
                for (asset, _) in &payload.deposits {
                    assets.insert(asset);
                }
            },
            TransactionType::DeployContract(payload) => {
                if let Some(invoke) = &payload.invoke {
                    for (asset, _) in &invoke.deposits {
                        assets.insert(asset);
                    }
                }
            },
            // Energy, MultiSig, and AIMining don't have explicit assets
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
            TransactionType::InvokeContract(payload) => payload.deposits.len().max(1),
            _ => 1
        }
    }

    // Get the fee type
    pub fn get_fee_type(&self) -> &FeeType {
        &self.fee_type
    }

    /// Calculate energy cost for this transaction
    /// Only applicable for transfer transactions with energy fees
    pub fn calculate_energy_cost(&self) -> u64 {
        if !self.fee_type.is_energy() {
            return 0;
        }

        match &self.data {
            TransactionType::Transfers(transfers) => {
                let tx_size = self.size();
                let output_count = transfers.len();
                let new_addresses = 0; // This would need to be calculated from state
                
                use crate::utils::calculate_energy_fee;
                calculate_energy_fee(tx_size, output_count, new_addresses)
            },
            _ => 0, // Only transfer transactions can use energy fees
        }
    }

    /// Get the bytes that were used for signing this transaction
    /// This matches the logic used in UnsignedTransaction::finalize
    pub fn get_signing_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);
        
        // T0 format: always include fee_type but NOT multisig (multisig participants sign without multisig field)
        self.version.write(&mut writer);
        self.source.write(&mut writer);
        self.data.write(&mut writer);
        self.fee.write(&mut writer);
        self.fee_type.write(&mut writer); // Always include fee_type for T0
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
        self.source.write(&mut writer);
        self.data.write(&mut writer);
        self.fee.write(&mut writer);
        self.fee_type.write(&mut writer); // Always include fee_type for T0
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
                writer.write_u8(0);
                payload.write(writer);
            }
            TransactionType::Transfers(txs) => {
                writer.write_u8(1);
                // max 255 txs per transaction
                let len: u8 = txs.len() as u8;
                writer.write_u8(len);
                for tx in txs {
                    tx.write(writer);
                }
            },
            TransactionType::MultiSig(payload) => {
                writer.write_u8(2);
                payload.write(writer);
            },
            TransactionType::InvokeContract(payload) => {
                writer.write_u8(3);
                payload.write(writer);
            },
            TransactionType::DeployContract(module) => {
                writer.write_u8(4);
                module.write(writer);
            },
            TransactionType::Energy(payload) => {
                writer.write_u8(5);
                payload.write(writer);
            },
            TransactionType::AIMining(payload) => {
                writer.write_u8(6);
                payload.write(writer);
            }
        };
    }

    fn read(reader: &mut Reader) -> Result<TransactionType, ReaderError> {
        Ok(match reader.read_u8()? {
            0 => {
                let payload = BurnPayload::read(reader)?;
                TransactionType::Burn(payload)
            },
            1 => {
                let txs_count = reader.read_u8()?;
                if txs_count == 0 || txs_count > MAX_TRANSFER_COUNT as u8 {
                    return Err(ReaderError::InvalidSize)
                }

                let mut txs = Vec::with_capacity(txs_count as usize);
                for _ in 0..txs_count {
                    txs.push(TransferPayload::read(reader)?);
                }
                TransactionType::Transfers(txs)
            },
            2 => TransactionType::MultiSig(MultiSigPayload::read(reader)?),
            3 => TransactionType::InvokeContract(InvokeContractPayload::read(reader)?),
            4 => TransactionType::DeployContract(DeployContractPayload::read(reader)?),
            5 => TransactionType::Energy(EnergyPayload::read(reader)?),
            6 => TransactionType::AIMining(AIMiningPayload::read(reader)?),
            _ => {
                return Err(ReaderError::InvalidValue)
            }
        })
    }

    fn size(&self) -> usize {
        1 + match self {
            TransactionType::Burn(payload) => payload.size(),
            TransactionType::Transfers(txs) => {
                // 1 byte for variant, 1 byte for count of transfers
                let mut size = 1;
                for tx in txs {
                    size += tx.size();
                }
                size
            },
            TransactionType::MultiSig(payload) => {
                // 1 byte for variant, 1 byte for threshold, 1 byte for count of participants
                1 + 1 + payload.participants.iter().map(|p| p.size()).sum::<usize>()
            },
            TransactionType::InvokeContract(payload) => payload.size(),
            TransactionType::DeployContract(module) => module.size(),
            TransactionType::Energy(payload) => payload.size(),
            TransactionType::AIMining(payload) => payload.size(),
        }
    }
}

impl Serializer for Transaction {
    fn write(&self, writer: &mut Writer) {
        self.version.write(writer);
        self.source.write(writer);
        self.data.write(writer);
        self.fee.write(writer);
        self.fee_type.write(writer);
        self.nonce.write(writer);
        self.reference.write(writer);

        // Always include multisig for T0
        self.multisig.write(writer);

        self.signature.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Transaction, ReaderError> {
        let version = TxVersion::read(reader)?;

        reader.context_mut()
            .store(version);

        let source = CompressedPublicKey::read(reader)?;
        let data = TransactionType::read(reader)?;
        let fee = reader.read_u64()?;
        let fee_type = FeeType::read(reader)?;
        let nonce = Nonce::read(reader)?;
        let reference = Reference::read(reader)?;

        // Always read multisig for T0
        let multisig = Option::read(reader)?;

        let signature = Signature::read(reader)?;

        Ok(Transaction::new(
            version,
            source,
            data,
            fee,
            fee_type,
            nonce,
            reference,
            multisig,
            signature,
        ))
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

        // Always include multisig size for T0
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