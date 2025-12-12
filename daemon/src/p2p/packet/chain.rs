use crate::config::{
    CHAIN_SYNC_REQUEST_MAX_BLOCKS, CHAIN_SYNC_RESPONSE_MAX_BLOCKS, CHAIN_SYNC_RESPONSE_MIN_BLOCKS,
    CHAIN_SYNC_TOP_BLOCKS,
};
use crate::core::ghostdag::{BlueWorkType, KType};
use indexmap::IndexSet;
use log::debug;
use std::collections::HashMap;
use std::hash::{Hash as StdHash, Hasher};
use tos_common::{
    config::TIPS_LIMIT,
    crypto::Hash,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// External GHOSTDAG data provided by a peer during sync.
///
/// This is similar to Kaspa's `ExternalGhostdagData` - during IBD (Initial Block Download),
/// GHOSTDAG data comes FROM the peer rather than being computed locally. This is necessary
/// because local GHOSTDAG calculation in a fork state produces different results than the
/// peer's GHOSTDAG, causing validation failures.
///
/// The security model is:
/// - We "trust" this data only because it's indirectly validated through PoW
/// - The PoW on later blocks commits to this GHOSTDAG data
/// - If a peer provides incorrect GHOSTDAG data, later blocks would fail PoW validation
///
/// See BUG-002 for the problem this solves:
/// - When node is in fork state (tips > 1), local GHOSTDAG calculation differs from peer
/// - This causes `Block height mismatch` errors during sync
/// - Solution: Use peer-provided GHOSTDAG data during IBD, validate via PoW
#[derive(Clone, Debug)]
pub struct ExternalGhostdagData {
    /// Blue score: number of blue blocks in the past of this block
    pub blue_score: u64,
    /// Blue work: cumulative work of all blue blocks
    pub blue_work: BlueWorkType,
    /// DAA score: monotonic score for difficulty adjustment
    pub daa_score: u64,
    /// Selected parent: parent with highest blue_work
    pub selected_parent: Hash,
    /// Blue blocks in the mergeset (excluding selected parent)
    pub mergeset_blues: Vec<Hash>,
    /// Red blocks in the mergeset
    pub mergeset_reds: Vec<Hash>,
    /// Anticone sizes for each blue block (must be <= K)
    pub blues_anticone_sizes: HashMap<Hash, KType>,
}

impl ExternalGhostdagData {
    /// Create new external GHOSTDAG data
    pub fn new(
        blue_score: u64,
        blue_work: BlueWorkType,
        daa_score: u64,
        selected_parent: Hash,
        mergeset_blues: Vec<Hash>,
        mergeset_reds: Vec<Hash>,
        blues_anticone_sizes: HashMap<Hash, KType>,
    ) -> Self {
        Self {
            blue_score,
            blue_work,
            daa_score,
            selected_parent,
            mergeset_blues,
            mergeset_reds,
            blues_anticone_sizes,
        }
    }
}

impl Serializer for ExternalGhostdagData {
    fn write(&self, writer: &mut Writer) {
        writer.write_u64(&self.blue_score);
        self.blue_work.write(writer);
        writer.write_u64(&self.daa_score);
        writer.write_hash(&self.selected_parent);

        // Write mergeset_blues
        writer.write_u16(self.mergeset_blues.len() as u16);
        for hash in &self.mergeset_blues {
            writer.write_hash(hash);
        }

        // Write mergeset_reds
        writer.write_u16(self.mergeset_reds.len() as u16);
        for hash in &self.mergeset_reds {
            writer.write_hash(hash);
        }

        // Write blues_anticone_sizes
        writer.write_u16(self.blues_anticone_sizes.len() as u16);
        for (hash, size) in &self.blues_anticone_sizes {
            writer.write_hash(hash);
            writer.write_u16(*size);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let blue_score = reader.read_u64()?;
        let blue_work = BlueWorkType::read(reader)?;
        let daa_score = reader.read_u64()?;
        let selected_parent = reader.read_hash()?;

        // Read mergeset_blues
        let blues_len = reader.read_u16()? as usize;
        let mut mergeset_blues = Vec::with_capacity(blues_len);
        for _ in 0..blues_len {
            mergeset_blues.push(reader.read_hash()?);
        }

        // Read mergeset_reds
        let reds_len = reader.read_u16()? as usize;
        let mut mergeset_reds = Vec::with_capacity(reds_len);
        for _ in 0..reds_len {
            mergeset_reds.push(reader.read_hash()?);
        }

        // Read blues_anticone_sizes
        let sizes_len = reader.read_u16()? as usize;
        let mut blues_anticone_sizes = HashMap::with_capacity(sizes_len);
        for _ in 0..sizes_len {
            let hash = reader.read_hash()?;
            let size = reader.read_u16()?;
            blues_anticone_sizes.insert(hash, size);
        }

        Ok(Self {
            blue_score,
            blue_work,
            daa_score,
            selected_parent,
            mergeset_blues,
            mergeset_reds,
            blues_anticone_sizes,
        })
    }

    fn size(&self) -> usize {
        8 // blue_score
        + self.blue_work.size()
        + 8 // daa_score
        + 32 // selected_parent
        + 2 + self.mergeset_blues.len() * 32 // mergeset_blues
        + 2 + self.mergeset_reds.len() * 32 // mergeset_reds
        + 2 + self.blues_anticone_sizes.len() * (32 + 2) // blues_anticone_sizes
    }
}

#[derive(Clone, Debug)]
pub struct BlockId {
    hash: Hash,
    topoheight: u64,
}

impl BlockId {
    pub fn new(hash: Hash, topoheight: u64) -> Self {
        Self { hash, topoheight }
    }

    pub fn get_hash(&self) -> &Hash {
        &self.hash
    }

    pub fn get_topoheight(&self) -> u64 {
        self.topoheight
    }

    pub fn consume(self) -> (Hash, u64) {
        (self.hash, self.topoheight)
    }
}

impl StdHash for BlockId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl PartialEq for BlockId {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for BlockId {}

impl Serializer for BlockId {
    fn write(&self, writer: &mut Writer) {
        writer.write_hash(self.get_hash());
        writer.write_u64(&self.get_topoheight());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self::new(reader.read_hash()?, reader.read_u64()?))
    }

    fn size(&self) -> usize {
        self.hash.size() + self.topoheight.size()
    }
}

#[derive(Clone, Debug)]
pub struct ChainRequest {
    blocks: IndexSet<BlockId>,
    // Number of maximum block responses allowed
    // This allow, directly in the protocol, to change the response param based on hardware resources
    accepted_response_size: u16,
}

impl ChainRequest {
    pub fn new(blocks: IndexSet<BlockId>, accepted_response_size: u16) -> Self {
        Self {
            blocks,
            accepted_response_size,
        }
    }

    pub fn size(&self) -> usize {
        self.blocks.len()
    }

    pub fn get_blocks(self) -> IndexSet<BlockId> {
        self.blocks
    }

    pub fn get_accepted_response_size(&self) -> u16 {
        self.accepted_response_size
    }
}

impl Serializer for ChainRequest {
    fn write(&self, writer: &mut Writer) {
        writer.write_u8(self.blocks.len() as u8);
        for block_id in &self.blocks {
            block_id.write(writer);
        }

        writer.write_u16(self.accepted_response_size);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let len = reader.read_u8()?;
        if len == 0 || len > CHAIN_SYNC_REQUEST_MAX_BLOCKS as u8 {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid chain request length: {}", len);
            }
            return Err(ReaderError::InvalidValue);
        }

        let mut blocks = IndexSet::with_capacity(len as usize);
        for _ in 0..len {
            if !blocks.insert(BlockId::read(reader)?) {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Duplicated block id in chain request");
                }
                return Err(ReaderError::InvalidValue);
            }
        }

        let accepted_response_size = reader.read_u16()?;
        // Verify that the requested response size is in the protocol bounds
        if accepted_response_size < CHAIN_SYNC_RESPONSE_MIN_BLOCKS as u16
            || accepted_response_size > CHAIN_SYNC_RESPONSE_MAX_BLOCKS as u16
        {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid accepted response size: {}", accepted_response_size);
            }
            return Err(ReaderError::InvalidValue);
        }

        Ok(Self {
            blocks,
            accepted_response_size,
        })
    }

    fn size(&self) -> usize {
        1 + self.blocks.len() + self.accepted_response_size.size()
    }
}

#[derive(Debug)]
pub struct CommonPoint {
    hash: Hash,
    topoheight: u64,
}

impl CommonPoint {
    pub fn new(hash: Hash, topoheight: u64) -> Self {
        Self { hash, topoheight }
    }

    pub fn get_hash(&self) -> &Hash {
        &self.hash
    }

    pub fn get_topoheight(&self) -> u64 {
        self.topoheight
    }
}

impl Serializer for CommonPoint {
    fn write(&self, writer: &mut Writer) {
        writer.write_hash(&self.hash);
        writer.write_u64(&self.topoheight);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let hash = reader.read_hash()?;
        let topoheight = reader.read_u64()?;
        Ok(Self { hash, topoheight })
    }

    fn size(&self) -> usize {
        self.hash.size() + self.topoheight.size()
    }
}

/// Chain response structure for P2P sync.
///
/// V2 adds GHOSTDAG data for each block (TrustedBlock pattern from Kaspa).
/// This solves BUG-002: sync stall in fork state due to local GHOSTDAG mismatch.
#[derive(Debug)]
pub struct ChainResponse {
    // Common point between us and the peer
    // This is based on the same DAG ordering for a block
    common_point: Option<CommonPoint>,
    // Lowest height of the blocks in the response
    lowest_height: Option<u64>,
    blocks: IndexSet<Hash>,
    top_blocks: IndexSet<Hash>,
    // V2: GHOSTDAG data for each block hash (for TrustedBlock validation)
    // If empty, use legacy validation (compute GHOSTDAG locally)
    // If present, use trusted validation (skip local GHOSTDAG computation)
    ghostdag_data: HashMap<Hash, ExternalGhostdagData>,
}

impl ChainResponse {
    /// Create new chain response (legacy, without GHOSTDAG data)
    pub fn new(
        common_point: Option<CommonPoint>,
        lowest_height: Option<u64>,
        blocks: IndexSet<Hash>,
        top_blocks: IndexSet<Hash>,
    ) -> Self {
        debug_assert!(common_point.is_some() == lowest_height.is_some());
        Self {
            common_point,
            lowest_height,
            blocks,
            top_blocks,
            ghostdag_data: HashMap::new(),
        }
    }

    /// Create new chain response with GHOSTDAG data (V2, TrustedBlock pattern)
    pub fn new_with_ghostdag(
        common_point: Option<CommonPoint>,
        lowest_height: Option<u64>,
        blocks: IndexSet<Hash>,
        top_blocks: IndexSet<Hash>,
        ghostdag_data: HashMap<Hash, ExternalGhostdagData>,
    ) -> Self {
        debug_assert!(common_point.is_some() == lowest_height.is_some());
        Self {
            common_point,
            lowest_height,
            blocks,
            top_blocks,
            ghostdag_data,
        }
    }

    // Get the common point for this response
    pub fn get_common_point(&mut self) -> Option<CommonPoint> {
        self.common_point.take()
    }

    // Get the lowest height of the blocks in the response
    pub fn get_lowest_height(&self) -> Option<u64> {
        self.lowest_height
    }

    // Get the count of blocks received
    pub fn blocks_size(&self) -> usize {
        self.blocks.len()
    }

    /// Check if this response has GHOSTDAG data (V2 format)
    pub fn has_ghostdag_data(&self) -> bool {
        !self.ghostdag_data.is_empty()
    }

    /// Get GHOSTDAG data for a specific block hash
    pub fn get_ghostdag_data(&self, hash: &Hash) -> Option<&ExternalGhostdagData> {
        self.ghostdag_data.get(hash)
    }

    /// Take GHOSTDAG data for a specific block hash
    pub fn take_ghostdag_data(&mut self, hash: &Hash) -> Option<ExternalGhostdagData> {
        self.ghostdag_data.remove(hash)
    }

    // Take ownership of the blocks
    pub fn consume(self) -> (IndexSet<Hash>, IndexSet<Hash>) {
        (self.blocks, self.top_blocks)
    }

    /// Take ownership of blocks and GHOSTDAG data
    pub fn consume_with_ghostdag(
        self,
    ) -> (
        IndexSet<Hash>,
        IndexSet<Hash>,
        HashMap<Hash, ExternalGhostdagData>,
    ) {
        (self.blocks, self.top_blocks, self.ghostdag_data)
    }
}

impl Serializer for ChainResponse {
    fn write(&self, writer: &mut Writer) {
        self.common_point.write(writer);
        // No need to write the blocks if we don't have a common point
        if self.common_point.is_none() {
            return;
        }

        // Write the lowest height
        if let Some(lowest_height) = self.lowest_height {
            writer.write_u64(&lowest_height);
        }

        writer.write_u16(self.blocks.len() as u16);
        for hash in &self.blocks {
            writer.write_hash(hash);
        }

        writer.write_u8(self.top_blocks.len() as u8);
        for hash in &self.top_blocks {
            writer.write_hash(hash);
        }

        // V2: Write GHOSTDAG data count and entries
        // Format: u16 count, then [hash, ExternalGhostdagData] pairs
        writer.write_u16(self.ghostdag_data.len() as u16);
        for (hash, data) in &self.ghostdag_data {
            writer.write_hash(hash);
            data.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let common_point = Option::read(reader)?;
        // No need to read the blocks if we don't have a common point
        if common_point.is_none() {
            return Ok(Self::new(None, None, IndexSet::new(), IndexSet::new()));
        }

        let lowest_height = reader.read_u64()?;
        let len = reader.read_u16()?;
        if len > CHAIN_SYNC_RESPONSE_MAX_BLOCKS as u16 {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid chain response length: {}", len);
            }
            return Err(ReaderError::InvalidValue);
        }

        let mut blocks: IndexSet<Hash> = IndexSet::with_capacity(len as usize);
        for _ in 0..len {
            let hash = reader.read_hash()?;
            if !blocks.insert(hash) {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Invalid chain response duplicate block");
                }
                return Err(ReaderError::InvalidValue);
            }
        }

        let len = reader.read_u8()?;
        if len > (CHAIN_SYNC_TOP_BLOCKS * TIPS_LIMIT) as u8 {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid chain response top blocks length: {}", len);
            }
            return Err(ReaderError::InvalidValue);
        }

        let mut top_blocks: IndexSet<Hash> = IndexSet::with_capacity(len as usize);
        for _ in 0..len {
            let hash = reader.read_hash()?;
            if blocks.contains(&hash) || !top_blocks.insert(hash) {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Invalid chain response duplicate top block");
                }
                return Err(ReaderError::InvalidValue);
            }
        }

        // V2: Read GHOSTDAG data if present
        // Note: Legacy peers don't send GHOSTDAG data, so we check if there's more data
        let mut ghostdag_data = HashMap::new();
        if reader.total_size() > 0 {
            let ghostdag_len = reader.read_u16()? as usize;
            // Validate GHOSTDAG data count
            let max_ghostdag =
                (CHAIN_SYNC_RESPONSE_MAX_BLOCKS + CHAIN_SYNC_TOP_BLOCKS * TIPS_LIMIT) as usize;
            if ghostdag_len > max_ghostdag {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Invalid GHOSTDAG data count: {}", ghostdag_len);
                }
                return Err(ReaderError::InvalidValue);
            }

            for _ in 0..ghostdag_len {
                let hash = reader.read_hash()?;
                let data = ExternalGhostdagData::read(reader)?;
                ghostdag_data.insert(hash, data);
            }
        }

        Ok(Self::new_with_ghostdag(
            common_point,
            Some(lowest_height),
            blocks,
            top_blocks,
            ghostdag_data,
        ))
    }

    fn size(&self) -> usize {
        if self.common_point.is_none() {
            return self.common_point.size();
        }

        let mut size = 0;
        if let Some(lowest_height) = self.lowest_height {
            size += lowest_height.size();
        }

        // Base size for blocks and top_blocks
        size += 2 + self.blocks.len() * 32 + 1 + self.top_blocks.len() * 32;

        // V2: Add GHOSTDAG data size
        size += 2; // ghostdag_data count
        for (_, data) in &self.ghostdag_data {
            size += 32 + data.size(); // hash + ExternalGhostdagData
        }

        size
    }
}
