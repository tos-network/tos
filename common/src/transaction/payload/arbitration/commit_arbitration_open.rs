use serde::{Deserialize, Serialize};

use crate::{
    crypto::{Hash, Signature},
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// CommitArbitrationOpen payload stores the signed ArbitrationOpen message bytes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitArbitrationOpenPayload {
    pub escrow_id: Hash,
    pub dispute_id: Hash,
    pub round: u32,
    pub request_id: Hash,
    pub arbitration_open_hash: Hash,
    pub opener_signature: Signature,
    pub arbitration_open_payload: Vec<u8>,
}

impl CommitArbitrationOpenPayload {
    pub fn get_payload_bytes(&self) -> &[u8] {
        &self.arbitration_open_payload
    }
}

impl Serializer for CommitArbitrationOpenPayload {
    fn write(&self, writer: &mut Writer) {
        self.escrow_id.write(writer);
        self.dispute_id.write(writer);
        self.round.write(writer);
        self.request_id.write(writer);
        self.arbitration_open_hash.write(writer);
        self.opener_signature.write(writer);
        let len = self.arbitration_open_payload.len() as u32;
        writer.write_u32(&len);
        writer.write_bytes(&self.arbitration_open_payload);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let escrow_id = Hash::read(reader)?;
        let dispute_id = Hash::read(reader)?;
        let round = u32::read(reader)?;
        let request_id = Hash::read(reader)?;
        let arbitration_open_hash = Hash::read(reader)?;
        let opener_signature = Signature::read(reader)?;
        let len = reader.read_u32()? as usize;
        let bytes = reader.read_bytes_ref(len)?.to_vec();
        Ok(Self {
            escrow_id,
            dispute_id,
            round,
            request_id,
            arbitration_open_hash,
            opener_signature,
            arbitration_open_payload: bytes,
        })
    }

    fn size(&self) -> usize {
        self.escrow_id.size()
            + self.dispute_id.size()
            + self.round.size()
            + self.request_id.size()
            + self.arbitration_open_hash.size()
            + self.opener_signature.size()
            + 4
            + self.arbitration_open_payload.len()
    }
}
