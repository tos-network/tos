// Compact Block P2P Packet Types
// Implements bandwidth-efficient block propagation

use tos_common::{
    block::{CompactBlock, MissingTransactionsRequest, MissingTransactionsResponse},
    serializer::{Serializer, Reader, ReaderError, Writer},
};
use std::borrow::Cow;

/// Compact block propagation packet
/// Sent instead of full BlockPropagation to save bandwidth
#[derive(Debug, Clone)]
pub struct CompactBlockPropagation<'a> {
    pub compact_block: Cow<'a, CompactBlock>,
}

impl<'a> CompactBlockPropagation<'a> {
    pub fn new(compact_block: Cow<'a, CompactBlock>) -> Self {
        Self { compact_block }
    }

    pub fn into_owned(self) -> CompactBlock {
        self.compact_block.into_owned()
    }
}

impl<'a> Serializer for CompactBlockPropagation<'a> {
    fn write(&self, writer: &mut Writer) {
        self.compact_block.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let compact_block = CompactBlock::read(reader)?;
        Ok(Self::new(Cow::Owned(compact_block)))
    }

    fn size(&self) -> usize {
        self.compact_block.size()
    }
}

/// Request for missing transactions when reconstructing a compact block
#[derive(Debug, Clone)]
pub struct GetMissingTransactions<'a> {
    pub request: Cow<'a, MissingTransactionsRequest>,
}

impl<'a> GetMissingTransactions<'a> {
    pub fn new(request: Cow<'a, MissingTransactionsRequest>) -> Self {
        Self { request }
    }
}

impl<'a> Serializer for GetMissingTransactions<'a> {
    fn write(&self, writer: &mut Writer) {
        self.request.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let request = MissingTransactionsRequest::read(reader)?;
        Ok(Self::new(Cow::Owned(request)))
    }

    fn size(&self) -> usize {
        self.request.size()
    }
}

/// Response with missing transactions
#[derive(Debug, Clone)]
pub struct MissingTransactions<'a> {
    pub response: Cow<'a, MissingTransactionsResponse>,
}

impl<'a> MissingTransactions<'a> {
    pub fn new(response: Cow<'a, MissingTransactionsResponse>) -> Self {
        Self { response }
    }
}

impl<'a> Serializer for MissingTransactions<'a> {
    fn write(&self, writer: &mut Writer) {
        self.response.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let response = MissingTransactionsResponse::read(reader)?;
        Ok(Self::new(Cow::Owned(response)))
    }

    fn size(&self) -> usize {
        self.response.size()
    }
}
