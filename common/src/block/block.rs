use std::{
    fmt::{Display, Formatter},
    fmt::Error,
    ops::Deref,
    sync::Arc
};
use crate::{
    crypto::{
        Hashable,
        Hash,
    },
    immutable::Immutable,
    transaction::Transaction,
    serializer::{Serializer, Writer, Reader, ReaderError},
};
use super::BlockHeader;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Block {
    #[serde(flatten)]
    header: Immutable<BlockHeader>,
    transactions: Vec<Arc<Transaction>>
}

impl Block {
    pub fn new(header: Immutable<BlockHeader>, transactions: Vec<Arc<Transaction>>) -> Self {
        Block {
            header,
            transactions
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
        &self.get_header()        
    }
}

impl Display for Block {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        let parents: Vec<String> = self.get_parents()
            .iter()
            .map(|h| format!("{}", h))
            .collect();

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