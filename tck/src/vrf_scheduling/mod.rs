// Phase 16: VRF & Scheduling Tests
//
// Tests VRF proof correctness/determinism and scheduled execution behavior:
// - Layer 1: VRF proof generation/verification, scheduling processor logic
// - Layer 1.5: ChainClient VRF block behavior, scheduled execution lifecycle
// - Layer 3: Multi-node VRF consensus, scheduling reorg scenarios

/// Layer 1.5: ChainClient scheduled execution lifecycle tests.
pub mod scheduled_chain;
/// Layer 1: Scheduled execution processor logic tests.
pub mod scheduled_processor;
/// Layer 3: Scheduled execution reorg scenario tests.
pub mod scheduled_reorg;
/// Layer 1.5: ChainClient VRF block-level behavior tests.
pub mod vrf_chain;
/// Layer 3: Multi-node VRF consensus consistency tests.
pub mod vrf_consensus;
/// Layer 1: VRF proof generation and verification tests (direct daemon imports).
pub mod vrf_proof;
/// Combined VRF + scheduling tests (VRF-driven execution paths).
pub mod vrf_scheduled_combined;
