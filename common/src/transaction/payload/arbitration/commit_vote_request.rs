use serde::{Deserialize, Serialize};

use crate::{
    crypto::{Hash, Signature},
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// CommitVoteRequest payload stores the signed VoteRequest message bytes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitVoteRequestPayload {
    pub request_id: Hash,
    pub vote_request_hash: Hash,
    pub coordinator_signature: Signature,
    pub vote_request_payload: Vec<u8>,
}

impl CommitVoteRequestPayload {
    pub fn get_payload_bytes(&self) -> &[u8] {
        &self.vote_request_payload
    }
}

impl Serializer for CommitVoteRequestPayload {
    fn write(&self, writer: &mut Writer) {
        self.request_id.write(writer);
        self.vote_request_hash.write(writer);
        self.coordinator_signature.write(writer);
        let len = self.vote_request_payload.len() as u32;
        writer.write_u32(&len);
        writer.write_bytes(&self.vote_request_payload);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let request_id = Hash::read(reader)?;
        let vote_request_hash = Hash::read(reader)?;
        let coordinator_signature = Signature::read(reader)?;
        let len = reader.read_u32()? as usize;
        let bytes = reader.read_bytes_ref(len)?.to_vec();
        Ok(Self {
            request_id,
            vote_request_hash,
            coordinator_signature,
            vote_request_payload: bytes,
        })
    }

    fn size(&self) -> usize {
        self.request_id.size()
            + self.vote_request_hash.size()
            + self.coordinator_signature.size()
            + 4
            + self.vote_request_payload.len()
    }
}
