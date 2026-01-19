use crate::{
    crypto::{Hash, PublicKey},
    serializer::{Reader, ReaderError, Serializer, Writer},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArbitrationRoundKey {
    pub escrow_id: Hash,
    pub dispute_id: Hash,
    pub round: u32,
}

impl Serializer for ArbitrationRoundKey {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.dispute_id.write(writer);
        self.round.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let dispute_id = Hash::read(reader)?;
        let round = u32::read(reader)?;
        Ok(Self {
            escrow_id,
            dispute_id,
            round,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size() + self.dispute_id.size() + self.round.size()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArbitrationRequestKey {
    pub request_id: Hash,
}

impl Serializer for ArbitrationRequestKey {
    fn write(&self, writer: &mut Writer) {
        self.request_id.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            request_id: Hash::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.request_id.size()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArbitrationJurorVoteKey {
    pub request_id: Hash,
    pub juror_pubkey: PublicKey,
}

impl Serializer for ArbitrationJurorVoteKey {
    fn write(&self, writer: &mut Writer) {
        self.request_id.write(writer);
        self.juror_pubkey.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let request_id = Hash::read(reader)?;
        let juror_pubkey = PublicKey::read(reader)?;
        Ok(Self {
            request_id,
            juror_pubkey,
        })
    }

    fn size(&self) -> usize {
        self.request_id.size() + self.juror_pubkey.size()
    }
}
