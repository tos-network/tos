// TOS GHOSTDAG Types
// Based on Kaspa's GHOSTDAG implementation
// Reference: rusty-kaspa/consensus/src/model/stores/ghostdag.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tos_common::crypto::Hash;

/// Blue work type - represents cumulative work in the blue chain
/// Using U256 to support very large work values
pub type BlueWorkType = primitive_types::U256;

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
        selected_parent: Hash,
        mergeset_blues: Vec<Hash>,
        mergeset_reds: Vec<Hash>,
        blues_anticone_sizes: HashMap<Hash, KType>,
    ) -> Self {
        Self {
            blue_score,
            blue_work,
            selected_parent,
            mergeset_blues: Arc::new(mergeset_blues),
            mergeset_reds: Arc::new(mergeset_reds),
            blues_anticone_sizes: Arc::new(blues_anticone_sizes),
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
            selected_parent,
            mergeset_blues: Arc::new(mergeset_blues),
            mergeset_reds: Arc::new(Vec::new()),
            blues_anticone_sizes: Arc::new(blues_anticone_sizes),
        }
    }

    /// Get the total size of the mergeset (blues + reds)
    pub fn mergeset_size(&self) -> usize {
        self.mergeset_blues.len() + self.mergeset_reds.len()
    }

    /// Add a blue block to the mergeset
    /// This is called during GHOSTDAG algorithm execution
    pub fn add_blue(&mut self, block: Hash, anticone_size: KType, blues_anticone_sizes: &HashMap<Hash, KType>) {
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
    pub fn finalize(&mut self, parent_blue_score: u64, parent_blue_work: BlueWorkType, block_work: BlueWorkType) {
        // Blue score = parent's blue score + number of blues in mergeset
        self.blue_score = parent_blue_score + self.mergeset_blues.len() as u64;

        // Blue work = parent's blue work + work of all blues in mergeset
        // Note: In simplified model, we assume each blue block adds block_work
        self.blue_work = parent_blue_work + (block_work * self.mergeset_blues.len());
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
            selected_parent: Hash::new([0u8; 32]),  // Zero hash
            mergeset_blues: Arc::new(Vec::new()),
            mergeset_reds: Arc::new(Vec::new()),
            blues_anticone_sizes: Arc::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ghostdag_data_creation() {
        let selected_parent = Hash::new([0u8; 32]);
        let k = 10;
        let data = TosGhostdagData::new_with_selected_parent(selected_parent, k);

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
        anticone_sizes.insert(blue_block, 5);

        data.add_blue(blue_block, 5, &anticone_sizes);

        assert_eq!(data.mergeset_blues.len(), 2);
        assert!(data.mergeset_blues.contains(&blue_block));
        assert_eq!(data.blues_anticone_sizes.get(&blue_block), Some(&5));
    }

    #[test]
    fn test_add_red() {
        let selected_parent = Hash::new([0u8; 32]);
        let mut data = TosGhostdagData::new_with_selected_parent(selected_parent, 10);

        let red_block = Hash::new([2u8; 32]);
        data.add_red(red_block);

        assert_eq!(data.mergeset_reds.len(), 1);
        assert!(data.mergeset_reds.contains(&red_block));
    }

    #[test]
    fn test_mergeset_size() {
        let selected_parent = Hash::new([0u8; 32]);
        let mut data = TosGhostdagData::new_with_selected_parent(selected_parent, 10);

        assert_eq!(data.mergeset_size(), 1); // Only selected parent

        let blue_block = Hash::new([1u8; 32]);
        data.add_blue(blue_block, 0, &HashMap::new());
        assert_eq!(data.mergeset_size(), 2);

        let red_block = Hash::new([2u8; 32]);
        data.add_red(red_block);
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
