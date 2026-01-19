use serde::{Deserialize, Serialize};

use crate::{
    crypto::{Hash, PublicKey, Signature},
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// CommitJurorVote payload stores the signed JurorVote message bytes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitJurorVotePayload {
    pub request_id: Hash,
    pub juror_pubkey: PublicKey,
    pub vote_hash: Hash,
    pub juror_signature: Signature,
    pub vote_payload: Vec<u8>,
}

impl CommitJurorVotePayload {
    pub fn get_payload_bytes(&self) -> &[u8] {
        &self.vote_payload
    }
}

impl Serializer for CommitJurorVotePayload {
    fn write(&self, writer: &mut Writer) {
        self.request_id.write(writer);
        self.juror_pubkey.write(writer);
        self.vote_hash.write(writer);
        self.juror_signature.write(writer);
        let len = self.vote_payload.len() as u32;
        writer.write_u32(&len);
        writer.write_bytes(&self.vote_payload);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let request_id = Hash::read(reader)?;
        let juror_pubkey = PublicKey::read(reader)?;
        let vote_hash = Hash::read(reader)?;
        let juror_signature = Signature::read(reader)?;
        let len = reader.read_u32()? as usize;
        let bytes = reader.read_bytes_ref(len)?.to_vec();
        Ok(Self {
            request_id,
            juror_pubkey,
            vote_hash,
            juror_signature,
            vote_payload: bytes,
        })
    }

    fn size(&self) -> usize {
        self.request_id.size()
            + self.juror_pubkey.size()
            + self.vote_hash.size()
            + self.juror_signature.size()
            + 4
            + self.vote_payload.len()
    }
}
