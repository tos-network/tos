use serde::{Deserialize, Serialize};

use crate::{
    crypto::Hash,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// CommitSelectionCommitment payload stores the selection commitment bytes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitSelectionCommitmentPayload {
    pub request_id: Hash,
    pub selection_commitment_id: Hash,
    pub selection_commitment_payload: Vec<u8>,
}

impl CommitSelectionCommitmentPayload {
    pub fn get_payload_bytes(&self) -> &[u8] {
        &self.selection_commitment_payload
    }
}

impl Serializer for CommitSelectionCommitmentPayload {
    fn write(&self, writer: &mut Writer) {
        self.request_id.write(writer);
        self.selection_commitment_id.write(writer);
        let len = self.selection_commitment_payload.len() as u32;
        writer.write_u32(&len);
        writer.write_bytes(&self.selection_commitment_payload);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let request_id = Hash::read(reader)?;
        let selection_commitment_id = Hash::read(reader)?;
        let len = reader.read_u32()? as usize;
        let bytes = reader.read_bytes_ref(len)?.to_vec();
        Ok(Self {
            request_id,
            selection_commitment_id,
            selection_commitment_payload: bytes,
        })
    }

    fn size(&self) -> usize {
        self.request_id.size()
            + self.selection_commitment_id.size()
            + 4
            + self.selection_commitment_payload.len()
    }
}
