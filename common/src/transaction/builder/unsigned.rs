use bulletproofs::RangeProof;
use serde::{Deserialize, Serialize};
use crate::{
    account::Nonce,
    crypto::{
        elgamal::CompressedPublicKey,
        hash,
        Hash,
        KeyPair,
    },
    serializer::{
        Reader,
        ReaderError,
        Serializer,
        Writer
    },
    transaction::{
        multisig::{MultiSig, SignatureId},
        FeeType,
        Reference,
        SourceCommitment,
        Transaction,
        TransactionType,
        TxVersion
    }
};

// Used to build the final transaction
// It can include the multi-signature logic
// by signing it
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UnsignedTransaction {
    version: TxVersion,
    source: CompressedPublicKey,
    data: TransactionType,
    fee: u64,
    fee_type: FeeType,
    nonce: Nonce,
    source_commitments: Vec<SourceCommitment>,
    reference: Reference,
    range_proof: RangeProof,
    multisig: Option<MultiSig>,
}

impl UnsignedTransaction {
    pub fn new(
        version: TxVersion,
        source: CompressedPublicKey,
        data: TransactionType,
        fee: u64,
        nonce: Nonce,
        source_commitments: Vec<SourceCommitment>,
        reference: Reference,
        range_proof: RangeProof,
    ) -> Self {
        Self {
            version,
            source,
            data,
            fee,
            fee_type: FeeType::TOS,
            nonce,
            source_commitments,
            reference,
            range_proof,
            multisig: None,
        }
    }
    pub fn new_with_fee_type(
        version: TxVersion,
        source: CompressedPublicKey,
        data: TransactionType,
        fee: u64,
        fee_type: FeeType,
        nonce: Nonce,
        source_commitments: Vec<SourceCommitment>,
        reference: Reference,
        range_proof: RangeProof,
    ) -> Self {
        Self {
            version,
            source,
            data,
            fee,
            fee_type,
            nonce,
            source_commitments,
            reference,
            range_proof,
            multisig: None,
        }
    }

    // Get the source of the transaction
    pub fn source(&self) -> &CompressedPublicKey {
        &self.source
    }

    /// Set a multi-signature to the transaction
    pub fn set_multisig(&mut self, multisig: MultiSig) {
        self.multisig = Some(multisig);
    }
    /// Get multisig from the transaction
    pub fn multisig(&self) -> Option<&MultiSig> {
        self.multisig.as_ref()
    }
    /// Sign the transaction for the multisig
    pub fn sign_multisig(&mut self, keypair: &KeyPair, id: u8) {
        let hash = self.get_hash_for_multisig();
        let multisig = self.multisig.get_or_insert_with(MultiSig::new);
        let signature = keypair.sign(hash.as_bytes());
        multisig.add_signature(SignatureId { id, signature });
    }

    // Get the bytes that need to be signed for the multi-signature
    fn write_no_signature(&self, writer: &mut Writer) {
        self.version.write(writer);
        self.source.write(writer);
        self.data.write(writer);
        self.fee.write(writer);
        // Always include fee_type for T0
        self.fee_type.write(writer);
        self.nonce.write(writer);

        writer.write_u8(self.source_commitments.len() as u8);
        for commitment in &self.source_commitments {
            commitment.write(writer);
        }

        self.range_proof.write(writer);
        self.reference.write(writer);
    }

    // Get the hash of the transaction for the multi-signature
    // This hash must be signed by each participant of the multisig
    pub fn get_hash_for_multisig(&self) -> Hash {
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);
        self.write_no_signature(&mut writer);
        hash(&buffer)
    }

    // Finalize the transaction by signing it
    pub fn finalize(self, keypair: &KeyPair) -> Transaction {
        // Use the same format as Transaction::get_signing_bytes (without multisig)
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);
        self.version.write(&mut writer);
        self.source.write(&mut writer);
        self.data.write(&mut writer);
        self.fee.write(&mut writer);
        self.fee_type.write(&mut writer);
        self.nonce.write(&mut writer);
        writer.write_u8(self.source_commitments.len() as u8);
        for commitment in &self.source_commitments {
            commitment.write(&mut writer);
        }
        self.range_proof.write(&mut writer);
        self.reference.write(&mut writer);
        // Do NOT include multisig - this matches Transaction::get_signing_bytes
        
        let signature = keypair.sign(&buffer);

        Transaction::new(
            self.version,
            self.source,
            self.data,
            self.fee,
            self.fee_type,
            self.nonce,
            self.source_commitments,
            self.range_proof,
            self.reference,
            self.multisig,
            signature,
        )
    }
}

impl Serializer for UnsignedTransaction {
    fn write(&self, writer: &mut Writer) {
        self.version.write(writer);
        self.source.write(writer);
        self.data.write(writer);
        self.fee.write(writer);
        self.fee_type.write(writer);
        self.nonce.write(writer);
        writer.write_u8(self.source_commitments.len() as u8);
        for commitment in &self.source_commitments {
            commitment.write(writer);
        }
        self.range_proof.write(writer);
        self.reference.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let version = TxVersion::read(reader)?;
        let source = CompressedPublicKey::read(reader)?;
        let data = TransactionType::read(reader)?;
        let fee = reader.read_u64()?;
        let fee_type = FeeType::read(reader)?;
        let nonce = Nonce::read(reader)?;
        let source_commitments = Vec::read(reader)?;
        let range_proof = RangeProof::read(reader)?;
        let reference = Reference::read(reader)?;

        Ok(Self {
            version,
            source,
            data,
            fee,
            fee_type,
            nonce,
            source_commitments,
            range_proof,
            reference,
            multisig: None,
        })
    }

    fn size(&self) -> usize {
        self.version.size()
        + self.source.size()
        + self.data.size()
        + self.fee.size()
        + self.fee_type.size()
        + self.nonce.size()
        + 1 // commitments length byte
        + self.source_commitments.iter().map(|c| c.size()).sum::<usize>()
        + self.range_proof.size()
        + self.reference.size()
    }
}