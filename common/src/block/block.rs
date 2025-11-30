use super::BlockHeader;
use crate::{
    crypto::{Hash, Hashable},
    immutable::Immutable,
    serializer::{Reader, ReaderError, Serializer, Writer},
    transaction::Transaction,
};
use std::{
    fmt::Error,
    fmt::{Display, Formatter},
    ops::Deref,
    sync::Arc,
};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Block {
    #[serde(flatten)]
    header: Immutable<BlockHeader>,
    transactions: Vec<Arc<Transaction>>,
}

impl Block {
    pub fn new(header: Immutable<BlockHeader>, transactions: Vec<Arc<Transaction>>) -> Self {
        Block {
            header,
            transactions,
        }
    }

    pub fn to_header(self) -> Arc<BlockHeader> {
        self.header.into_arc()
    }

    pub fn get_header(&self) -> &BlockHeader {
        &self.header
    }

    pub fn get_txs_count(&self) -> usize {
        self.transactions.len()
    }

    pub fn get_transactions(&self) -> &Vec<Arc<Transaction>> {
        &self.transactions
    }

    pub fn split(self) -> (Immutable<BlockHeader>, Vec<Arc<Transaction>>) {
        (self.header, self.transactions)
    }

    /// Fallible serialization to bytes
    ///
    /// SECURITY FIX (Codex Audit): This method validates the block header before
    /// serialization, returning an error if the header is malformed. Use this in
    /// release builds where silent corruption must be prevented.
    pub fn try_to_bytes(&self) -> Result<Vec<u8>, ReaderError> {
        // Validate header before serialization (works in both debug and release)
        self.header
            .validate_parent_levels()
            .map_err(|e| ReaderError::SerializationError(e.to_string()))?;

        // If validation passes, proceed with normal serialization
        Ok(self.to_bytes())
    }
}

impl Serializer for Block {
    fn write(&self, writer: &mut Writer) {
        self.header.write(writer);
        for tx in &self.transactions {
            tx.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Block, ReaderError> {
        let header = BlockHeader::read(reader)?;
        // Note: Transaction count is no longer in header
        // We read transactions until EOF or use a different protocol
        // For now, we'll read all remaining transactions
        let mut txs = Vec::new();
        while reader.total_read() < reader.total_size() {
            match Transaction::read(reader) {
                Ok(tx) => txs.push(Arc::new(tx)),
                Err(_) => break,
            }
        }

        Ok(Block::new(Immutable::Owned(header), txs))
    }

    fn size(&self) -> usize {
        self.header.size() + self.transactions.iter().map(|tx| tx.size()).sum::<usize>()
    }
}

impl Hashable for Block {
    fn hash(&self) -> Hash {
        self.header.hash()
    }
}

impl Deref for Block {
    type Target = BlockHeader;

    fn deref(&self) -> &Self::Target {
        self.get_header()
    }
}

impl Display for Block {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        let parents: Vec<String> = self.get_parents().iter().map(|h| format!("{h}")).collect();

        write!(
            f,
            "Block[blue_score: {}, parents: [{}], timestamp: {}, nonce: {}, extra_nonce: {}, txs: {}]",
            self.blue_score,
            parents.join(", "),
            self.timestamp,
            self.nonce,
            hex::encode(self.extra_nonce),
            self.transactions.len()
        )
    }
}
