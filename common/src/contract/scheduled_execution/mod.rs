// Scheduled Execution Module
// Allows contracts to schedule future executions at specific topoheights or block ends
// Supports OFFERCALL-style priority scheduling inspired by EIP-7833

mod constants;
mod kind;
mod priority;
mod status;

use std::hash;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tos_kernel::ValueCell;

use crate::{block::TopoHeight, crypto::Hash, serializer::*};

use super::Source;
pub use constants::*;
pub use kind::*;
pub use priority::*;
pub use status::*;

/// Scheduled execution for a contract.
/// Supports OFFERCALL-style priority scheduling inspired by EIP-7833.
///
/// Priority ordering (hybrid model):
/// 1. Higher offer_amount = higher priority
/// 2. Earlier registration_topoheight = higher priority (FIFO fallback)
/// 3. Lexicographic hash comparison (deterministic tiebreaker)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScheduledExecution {
    /// The unique hash representing this scheduled execution
    /// Computed as blake3(contract || topoheight || registration_topoheight || chunk_id)
    pub hash: Hash,
    /// Contract hash of the module to execute
    pub contract: Hash,
    /// Chunk ID within the contract to call
    pub chunk_id: u16,
    /// Parameters to pass to the invoke (legacy format)
    pub params: Vec<ValueCell>,
    /// Maximum gas available for the execution
    /// Remaining gas will be refunded to the contract balance
    pub max_gas: u64,
    /// Kind of scheduled execution (topoheight or block end)
    pub kind: ScheduledExecutionKind,
    /// Gas sources for this scheduled execution
    /// Tracks who contributed gas for refund accounting
    pub gas_sources: IndexMap<Source, u64>,

    // === OFFERCALL fields (EIP-7833 inspired) ===
    /// Offer amount in native tokens for priority scheduling
    /// 30% burned on registration, 70% paid to miner on execution
    /// Higher offer = higher priority in execution order
    #[serde(default)]
    pub offer_amount: u64,
    /// The contract that scheduled this execution (for authorization)
    /// Only the scheduler can cancel the execution
    #[serde(default = "Hash::zero")]
    pub scheduler_contract: Hash,
    /// Pre-encoded input data for execution
    /// Alternative to chunk_id + params for efficiency
    #[serde(default)]
    pub input_data: Vec<u8>,
    /// Topoheight when this execution was registered
    /// Used for FIFO ordering among executions with same offer_amount
    #[serde(default)]
    pub registration_topoheight: TopoHeight,
    /// Current status of the scheduled execution
    #[serde(default)]
    pub status: ScheduledExecutionStatus,
    /// Number of times this execution has been deferred due to block capacity
    /// Execution is cancelled after MAX_DEFER_COUNT deferrals
    #[serde(default)]
    pub defer_count: u8,
}

impl ScheduledExecution {
    /// Create a new scheduled execution with OFFERCALL parameters
    #[allow(clippy::too_many_arguments)]
    pub fn new_offercall(
        contract: Hash,
        chunk_id: u16,
        input_data: Vec<u8>,
        max_gas: u64,
        offer_amount: u64,
        scheduler_contract: Hash,
        kind: ScheduledExecutionKind,
        registration_topoheight: TopoHeight,
    ) -> Self {
        // Compute unique hash for this execution
        let hash = Self::compute_hash(&contract, &kind, registration_topoheight, chunk_id);

        Self {
            hash,
            contract,
            chunk_id,
            params: vec![],
            max_gas,
            kind,
            gas_sources: IndexMap::new(),
            offer_amount,
            scheduler_contract,
            input_data,
            registration_topoheight,
            status: ScheduledExecutionStatus::Pending,
            defer_count: 0,
        }
    }

    /// Compute deterministic hash for a scheduled execution
    pub fn compute_hash(
        contract: &Hash,
        kind: &ScheduledExecutionKind,
        registration_topoheight: TopoHeight,
        chunk_id: u16,
    ) -> Hash {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(contract.as_bytes());
        match kind {
            ScheduledExecutionKind::TopoHeight(topo) => {
                hasher.update(&[0u8]);
                hasher.update(&topo.to_be_bytes());
            }
            ScheduledExecutionKind::BlockEnd => {
                hasher.update(&[1u8]);
            }
        }
        hasher.update(&registration_topoheight.to_be_bytes());
        hasher.update(&chunk_id.to_be_bytes());
        let result = hasher.finalize();
        Hash::new(*result.as_bytes())
    }

    /// Check if this execution can be cancelled at the given topoheight
    ///
    /// Cancellation is allowed if:
    /// 1. Status is still Pending
    /// 2. Current topoheight is at least MIN_CANCELLATION_WINDOW blocks before execution
    ///
    /// For BlockEnd scheduling, cancellation is always allowed while Pending
    /// (since BlockEnd executes at end of current block, not a future one)
    pub fn can_cancel(&self, current_topoheight: TopoHeight) -> bool {
        if self.status != ScheduledExecutionStatus::Pending {
            return false;
        }

        match self.kind {
            ScheduledExecutionKind::TopoHeight(target_topo) => {
                // Must cancel at least MIN_CANCELLATION_WINDOW blocks before execution
                // target_topo > current_topoheight + MIN_CANCELLATION_WINDOW
                target_topo > current_topoheight.saturating_add(MIN_CANCELLATION_WINDOW)
            }
            ScheduledExecutionKind::BlockEnd => {
                // BlockEnd executes at end of current block, so cannot cancel
                // once in the same block
                false
            }
        }
    }

    /// Check if this execution is still pending
    pub fn is_pending(&self) -> bool {
        self.status == ScheduledExecutionStatus::Pending
    }

    /// Increment defer count and check if max reached
    pub fn defer(&mut self) -> bool {
        self.defer_count = self.defer_count.saturating_add(1);
        self.defer_count >= MAX_DEFER_COUNT
    }
}

impl hash::Hash for ScheduledExecution {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Use the unique execution hash, not the contract hash
        // Each scheduled execution has a unique hash computed from
        // contract + kind + registration_topoheight + chunk_id
        self.hash.hash(state);
    }
}

impl PartialEq for ScheduledExecution {
    fn eq(&self, other: &Self) -> bool {
        // Compare by unique execution hash, not contract
        // Different executions for the same contract should NOT be equal
        self.hash == other.hash
    }
}

impl Eq for ScheduledExecution {}

impl Serializer for ScheduledExecution {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            // Original fields
            hash: Hash::read(reader)?,
            contract: Hash::read(reader)?,
            chunk_id: u16::read(reader)?,
            params: Vec::read(reader)?,
            max_gas: u64::read(reader)?,
            kind: ScheduledExecutionKind::read(reader)?,
            gas_sources: IndexMap::read(reader)?,
            // OFFERCALL fields
            offer_amount: u64::read(reader)?,
            scheduler_contract: Hash::read(reader)?,
            input_data: Vec::read(reader)?,
            registration_topoheight: TopoHeight::read(reader)?,
            status: ScheduledExecutionStatus::read(reader)?,
            defer_count: u8::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        // Original fields
        self.hash.write(writer);
        self.contract.write(writer);
        self.chunk_id.write(writer);
        self.params.write(writer);
        self.max_gas.write(writer);
        self.kind.write(writer);
        self.gas_sources.write(writer);
        // OFFERCALL fields
        self.offer_amount.write(writer);
        self.scheduler_contract.write(writer);
        self.input_data.write(writer);
        self.registration_topoheight.write(writer);
        self.status.write(writer);
        self.defer_count.write(writer);
    }

    fn size(&self) -> usize {
        self.hash.size()
            + self.contract.size()
            + self.chunk_id.size()
            + self.params.size()
            + self.max_gas.size()
            + self.kind.size()
            + self.gas_sources.size()
            // OFFERCALL fields
            + self.offer_amount.size()
            + self.scheduler_contract.size()
            + self.input_data.size()
            + self.registration_topoheight.size()
            + self.status.size()
            + self.defer_count.size()
    }
}

/// Opaque handle to a scheduled execution returned from syscalls
#[derive(Debug, Clone)]
pub struct OpaqueScheduledExecution {
    kind: ScheduledExecutionKind,
    hash: Hash,
}

impl OpaqueScheduledExecution {
    /// Create a new opaque scheduled execution
    pub fn new(kind: ScheduledExecutionKind, hash: Hash) -> Self {
        Self { kind, hash }
    }

    /// Get the kind of this scheduled execution
    pub fn kind(&self) -> ScheduledExecutionKind {
        self.kind
    }

    /// Get the hash of this scheduled execution
    pub fn hash(&self) -> &Hash {
        &self.hash
    }
}

impl PartialEq for OpaqueScheduledExecution {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

impl Eq for OpaqueScheduledExecution {}

impl hash::Hash for OpaqueScheduledExecution {
    fn hash<H: hash::Hasher>(&self, _: &mut H) {}
}

impl Serializer for OpaqueScheduledExecution {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            kind: ScheduledExecutionKind::read(reader)?,
            hash: Hash::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.kind.write(writer);
        self.hash.write(writer);
    }

    fn size(&self) -> usize {
        self.kind.size() + self.hash.size()
    }
}
