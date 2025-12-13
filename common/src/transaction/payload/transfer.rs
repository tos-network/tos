use crate::{
    crypto::{elgamal::CompressedPublicKey, Hash},
    serializer::*,
    transaction::extra_data::UnknownExtraDataFormat,
};
use serde::{Deserialize, Serialize};

// TransferPayload is a public payload allowing to transfer an asset to another account
// It contains the asset hash, the destination account, and the plaintext transfer amount
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransferPayload {
    asset: Hash,
    destination: CompressedPublicKey,
    // Plaintext transfer amount (u64)
    amount: u64,
    // we can put whatever we want up to EXTRA_DATA_LIMIT_SIZE bytes (128 bytes for memo/exchange IDs)
    extra_data: Option<UnknownExtraDataFormat>,
}

impl TransferPayload {
    // Create a new transfer payload with plaintext amount
    pub fn new(
        asset: Hash,
        destination: CompressedPublicKey,
        amount: u64,
        extra_data: Option<UnknownExtraDataFormat>,
    ) -> Self {
        TransferPayload {
            asset,
            destination,
            amount,
            extra_data,
        }
    }

    // Get the destination key
    #[inline]
    pub fn get_destination(&self) -> &CompressedPublicKey {
        &self.destination
    }

    // Get the asset hash spent in this transfer
    #[inline]
    pub fn get_asset(&self) -> &Hash {
        &self.asset
    }

    // Get the plaintext transfer amount
    #[inline]
    pub fn get_amount(&self) -> u64 {
        self.amount
    }

    // Get the extra data if any
    #[inline]
    pub fn get_extra_data(&self) -> &Option<UnknownExtraDataFormat> {
        &self.extra_data
    }

    // Take all data
    #[inline]
    pub fn consume(
        self,
    ) -> (
        Hash,
        CompressedPublicKey,
        u64,
        Option<UnknownExtraDataFormat>,
    ) {
        (self.asset, self.destination, self.amount, self.extra_data)
    }
}

impl Serializer for TransferPayload {
    fn write(&self, writer: &mut Writer) {
        self.asset.write(writer);
        self.destination.write(writer);
        self.amount.write(writer);
        self.extra_data.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<TransferPayload, ReaderError> {
        let asset = Hash::read(reader)?;
        let destination = CompressedPublicKey::read(reader)?;
        let amount = u64::read(reader)?;
        let extra_data = Option::read(reader)?;

        Ok(TransferPayload {
            asset,
            destination,
            amount,
            extra_data,
        })
    }

    fn size(&self) -> usize {
        self.asset.size() + self.destination.size() + self.amount.size() + self.extra_data.size()
    }
}
