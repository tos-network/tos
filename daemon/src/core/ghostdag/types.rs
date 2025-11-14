// TOS GHOSTDAG Types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tos_common::crypto::Hash;
use tos_common::serializer::{Reader, ReaderError, Serializer, Writer};

/// Re-export BlueWorkType from common (includes Serializer implementation)
pub use tos_common::crypto::BlueWorkType;

/// K-cluster parameter type
/// Defines the maximum anticone size for blue blocks
pub type KType = u16;

/// Core GHOSTDAG data structure for each block
/// This contains all the information needed for GHOSTDAG consensus
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TosGhostdagData {
    /// Blue score: the number of blue blocks in the past of this block
    /// (similar to "height" but in a DAG context)
    pub blue_score: u64,

    /// Blue work: the cumulative difficulty/work of all blue blocks in the past
    /// Used for selecting the "heaviest" chain (blue chain)
    pub blue_work: BlueWorkType,

    /// DAA score: monotonic score for Difficulty Adjustment Algorithm
    /// Unlike blue_score (which can have jumps in DAG), daa_score is sequential:
    /// daa_score = max(parent_daa_scores) + 1
    /// This ensures accurate difficulty adjustment calculations.
    pub daa_score: u64,

    /// Selected parent: the parent with the highest blue_work
    /// This is the "main chain" parent in GHOSTDAG terminology
    pub selected_parent: Hash,

    /// Mergeset blues: all blue blocks in the mergeset (excluding selected parent)
    /// These are blocks that are added to the blue set
    pub mergeset_blues: Arc<Vec<Hash>>,

    /// Mergeset reds: all red blocks in the mergeset
    /// These are blocks that violate the k-cluster constraint
    pub mergeset_reds: Arc<Vec<Hash>>,

    /// Blues anticone sizes: for each blue block, stores the size of its anticone
    /// intersection with the current blue set
    /// Key: block hash, Value: anticone size (must be ≤ K)
    pub blues_anticone_sizes: Arc<HashMap<Hash, KType>>,

    /// Mergeset non-DAA: blocks in the mergeset that are too far in the past
    /// to participate in the Difficulty Adjustment Algorithm (DAA) window.
    /// These blocks are excluded from DAA score calculation to prevent
    /// timestamp manipulation attacks.
    /// (Phase 3 addition for complete DAA implementation)
    pub mergeset_non_daa: Arc<Vec<Hash>>,
}

/// Compact GHOSTDAG data - only essential fields
/// Used for storage optimization and network transmission
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CompactGhostdagData {
    pub blue_score: u64,
    pub blue_work: BlueWorkType,
    pub selected_parent: Hash,
}

impl TosGhostdagData {
    /// Create new GHOSTDAG data with all fields
    pub fn new(
        blue_score: u64,
        blue_work: BlueWorkType,
        daa_score: u64,
        selected_parent: Hash,
        mergeset_blues: Vec<Hash>,
        mergeset_reds: Vec<Hash>,
        blues_anticone_sizes: HashMap<Hash, KType>,
        mergeset_non_daa: Vec<Hash>,
    ) -> Self {
        Self {
            blue_score,
            blue_work,
            daa_score,
            selected_parent,
            mergeset_blues: Arc::new(mergeset_blues),
            mergeset_reds: Arc::new(mergeset_reds),
            blues_anticone_sizes: Arc::new(blues_anticone_sizes),
            mergeset_non_daa: Arc::new(mergeset_non_daa),
        }
    }

    /// Create new GHOSTDAG data with only selected parent
    /// Used as initial state before running the GHOSTDAG algorithm
    pub fn new_with_selected_parent(selected_parent: Hash, k: KType) -> Self {
        let mut mergeset_blues = Vec::with_capacity((k + 1) as usize);
        let mut blues_anticone_sizes = HashMap::with_capacity(k as usize);

        // Selected parent is always blue with anticone size 0
        mergeset_blues.push(selected_parent.clone());
        blues_anticone_sizes.insert(selected_parent.clone(), 0);

        Self {
            blue_score: 0,
            blue_work: BlueWorkType::zero(),
            daa_score: 0, // Will be calculated during GHOSTDAG processing
            selected_parent,
            mergeset_blues: Arc::new(mergeset_blues),
            mergeset_reds: Arc::new(Vec::new()),
            blues_anticone_sizes: Arc::new(blues_anticone_sizes),
            mergeset_non_daa: Arc::new(Vec::new()), // Empty initially
        }
    }

    /// Get the total size of the mergeset (blues + reds)
    pub fn mergeset_size(&self) -> usize {
        self.mergeset_blues.len() + self.mergeset_reds.len()
    }

    /// Add a blue block to the mergeset
    /// This is called during GHOSTDAG algorithm execution
    pub fn add_blue(
        &mut self,
        block: Hash,
        anticone_size: KType,
        blues_anticone_sizes: &HashMap<Hash, KType>,
    ) {
        // Make mutable copies if needed (Arc::make_mut for copy-on-write)
        let mergeset_blues = Arc::make_mut(&mut self.mergeset_blues);
        let self_blues_anticone_sizes = Arc::make_mut(&mut self.blues_anticone_sizes);

        mergeset_blues.push(block.clone());
        self_blues_anticone_sizes.insert(block, anticone_size);

        // Update anticone sizes for other blues
        for (blue, size) in blues_anticone_sizes {
            self_blues_anticone_sizes.insert(blue.clone(), *size);
        }
    }

    /// Add a red block to the mergeset
    /// Red blocks violate the k-cluster constraint
    pub fn add_red(&mut self, block: Hash) {
        let mergeset_reds = Arc::make_mut(&mut self.mergeset_reds);
        mergeset_reds.push(block);
    }

    /// Finalize GHOSTDAG data by calculating blue_score and blue_work
    /// This is called after all blues/reds have been determined
    pub fn finalize(
        &mut self,
        parent_blue_score: u64,
        parent_blue_work: BlueWorkType,
        block_work: BlueWorkType,
    ) {
        // Blue score = parent's blue score + number of blues in mergeset
        self.blue_score = parent_blue_score + self.mergeset_blues.len() as u64;

        // Blue work = parent's blue work + work of all blues in mergeset
        // Note: In simplified model, we assume each blue block adds block_work
        self.blue_work = parent_blue_work + (block_work * self.mergeset_blues.len());
    }

    /// Finalize GHOSTDAG data with explicit blue_score and blue_work values
    /// Used by GHOSTDAG algorithm after calculating final values
    pub fn finalize_score_and_work(&mut self, blue_score: u64, blue_work: BlueWorkType) {
        self.blue_score = blue_score;
        self.blue_work = blue_work;
    }

    /// Set DAA score explicitly
    /// DAA score = max(parent_daa_scores) + 1 (monotonic, unlike blue_score)
    /// This must be called after GHOSTDAG processing to ensure accurate difficulty adjustment
    pub fn set_daa_score(&mut self, daa_score: u64) {
        self.daa_score = daa_score;
    }

    /// Set mergeset_non_daa blocks
    /// These are blocks in the mergeset that are outside the DAA window
    /// and should not participate in difficulty adjustment calculations.
    /// (Phase 3 addition for complete DAA implementation)
    pub fn set_mergeset_non_daa(&mut self, non_daa_blocks: Vec<Hash>) {
        self.mergeset_non_daa = Arc::new(non_daa_blocks);
    }

    /// Get the number of blocks that participate in DAA
    /// This is the total mergeset blues minus those outside the DAA window
    pub fn daa_contributing_blues_count(&self) -> usize {
        self.mergeset_blues
            .len()
            .saturating_sub(self.mergeset_non_daa.len())
    }
}

impl From<&TosGhostdagData> for CompactGhostdagData {
    fn from(value: &TosGhostdagData) -> Self {
        Self {
            blue_score: value.blue_score,
            blue_work: value.blue_work,
            selected_parent: value.selected_parent.clone(),
        }
    }
}

impl Default for TosGhostdagData {
    fn default() -> Self {
        Self {
            blue_score: 0,
            blue_work: BlueWorkType::zero(),
            daa_score: 0,
            selected_parent: Hash::new([0u8; 32]), // Zero hash
            mergeset_blues: Arc::new(Vec::new()),
            mergeset_reds: Arc::new(Vec::new()),
            blues_anticone_sizes: Arc::new(HashMap::new()),
            mergeset_non_daa: Arc::new(Vec::new()),
        }
    }
}

// Serializer implementations for storage
// Using bincode for efficient serialization of complex structures
impl Serializer for TosGhostdagData {
    fn write(&self, writer: &mut Writer) {
        // Use bincode to serialize the entire structure
        // SAFETY: Serialization should never fail for well-formed TosGhostdagData
        // All fields are serializable types (u64, BlueWorkType, Hash, Arc<Vec>, Arc<HashMap>)
        // If this panics, it indicates a critical bug in the data structure
        let bytes = bincode::serialize(self).unwrap_or_else(|e| {
            panic!(
                "Critical: Failed to serialize TosGhostdagData - data structure is corrupted: {}",
                e
            )
        });
        writer.write_bytes(&bytes);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let bytes = reader.read_bytes_ref(reader.total_size())?;
        bincode::deserialize(bytes).map_err(|_e| ReaderError::InvalidSize)
    }

    fn size(&self) -> usize {
        // SAFETY: Size calculation should never fail for well-formed TosGhostdagData
        // All fields have deterministic sizes
        // If this panics, it indicates a critical bug in the data structure
        bincode::serialized_size(self).unwrap_or_else(|e| {
            panic!(
                "Critical: Failed to calculate size of TosGhostdagData - data structure is corrupted: {}",
                e
            )
        }) as usize
    }
}

impl Serializer for CompactGhostdagData {
    fn write(&self, writer: &mut Writer) {
        // Use bincode for compact data as well
        // SAFETY: Serialization should never fail for well-formed CompactGhostdagData
        // All fields are serializable types (u64, BlueWorkType, Hash)
        // If this panics, it indicates a critical bug in the data structure
        let bytes = bincode::serialize(self).unwrap_or_else(|e| {
            panic!(
                "Critical: Failed to serialize CompactGhostdagData - data structure is corrupted: {}",
                e
            )
        });
        writer.write_bytes(&bytes);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let bytes = reader.read_bytes_ref(reader.total_size())?;
        bincode::deserialize(bytes).map_err(|_e| ReaderError::InvalidSize)
    }

    fn size(&self) -> usize {
        // SAFETY: Size calculation should never fail for well-formed CompactGhostdagData
        // All fields have deterministic sizes
        // If this panics, it indicates a critical bug in the data structure
        bincode::serialized_size(self).unwrap_or_else(|e| {
            panic!(
                "Critical: Failed to calculate size of CompactGhostdagData - data structure is corrupted: {}",
                e
            )
        }) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ghostdag_data_creation() {
        let selected_parent = Hash::new([0u8; 32]);
        let k = 10;
        let data = TosGhostdagData::new_with_selected_parent(selected_parent.clone(), k);

        assert_eq!(data.blue_score, 0);
        assert_eq!(data.blue_work, BlueWorkType::zero());
        assert_eq!(data.selected_parent, selected_parent);
        assert_eq!(data.mergeset_blues.len(), 1); // Selected parent
        assert_eq!(data.mergeset_reds.len(), 0);
        assert_eq!(data.blues_anticone_sizes.len(), 1);
    }

    #[test]
    fn test_add_blue() {
        let selected_parent = Hash::new([0u8; 32]);
        let mut data = TosGhostdagData::new_with_selected_parent(selected_parent, 10);

        let blue_block = Hash::new([1u8; 32]);
        let mut anticone_sizes = HashMap::new();
        anticone_sizes.insert(blue_block.clone(), 5);

        data.add_blue(blue_block.clone(), 5, &anticone_sizes);

        assert_eq!(data.mergeset_blues.len(), 2);
        assert!(data.mergeset_blues.contains(&blue_block));
        assert_eq!(data.blues_anticone_sizes.get(&blue_block), Some(&5));
    }

    #[test]
    fn test_add_red() {
        let selected_parent = Hash::new([0u8; 32]);
        let mut data = TosGhostdagData::new_with_selected_parent(selected_parent, 10);

        let red_block = Hash::new([2u8; 32]);
        data.add_red(red_block.clone());

        assert_eq!(data.mergeset_reds.len(), 1);
        assert!(data.mergeset_reds.contains(&red_block));
    }

    #[test]
    fn test_mergeset_size() {
        let selected_parent = Hash::new([0u8; 32]);
        let mut data = TosGhostdagData::new_with_selected_parent(selected_parent, 10);

        assert_eq!(data.mergeset_size(), 1); // Only selected parent

        let blue_block = Hash::new([1u8; 32]);
        data.add_blue(blue_block.clone(), 0, &HashMap::new());
        assert_eq!(data.mergeset_size(), 2);

        let red_block = Hash::new([2u8; 32]);
        data.add_red(red_block.clone());
        assert_eq!(data.mergeset_size(), 3);
    }

    #[test]
    fn test_compact_conversion() {
        let selected_parent = Hash::new([0u8; 32]);
        let data = TosGhostdagData::new_with_selected_parent(selected_parent, 10);

        let compact: CompactGhostdagData = (&data).into();

        assert_eq!(compact.blue_score, data.blue_score);
        assert_eq!(compact.blue_work, data.blue_work);
        assert_eq!(compact.selected_parent, data.selected_parent);
    }
}
