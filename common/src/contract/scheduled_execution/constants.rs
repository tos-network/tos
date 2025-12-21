// Constants for Scheduled Execution and OFFERCALL
// These values define limits and costs for the scheduling system

// ============================================================================
// Execution Limits
// ============================================================================

/// Maximum number of scheduled executions per block
/// Prevents block bloat and ensures predictable execution time
/// At ~5ms per execution overhead, 100 executions ≈ 500ms
pub const MAX_SCHEDULED_EXECUTIONS_PER_BLOCK: usize = 100;

/// Maximum total gas for scheduled executions per block
/// 100M CU at ~1ns/CU = ~100ms VM execution time
/// Combined with overhead: ~800ms total (fits in 3s block)
pub const MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK: u64 = 100_000_000;

/// Maximum scheduling horizon (topoheights into the future)
/// Approximately 7 days at 6-second blocks: 7 × 24 × 60 × 60 / 6 = 100,800
/// Prevents unbounded storage growth from far-future scheduling
pub const MAX_SCHEDULING_HORIZON: u64 = 100_800;

/// Maximum number of times an execution can be deferred due to block capacity
/// After MAX_DEFER_COUNT deferrals, execution is cancelled with gas refund
pub const MAX_DEFER_COUNT: u8 = 10;

/// Minimum blocks before execution that cancellation is allowed
/// Must cancel at least this many blocks before the target topoheight
/// Prevents last-minute cancellation manipulation and MEV attacks
pub const MIN_CANCELLATION_WINDOW: u64 = 1;

// ============================================================================
// Gas Requirements
// ============================================================================

/// Minimum gas required to schedule an execution
/// Must cover worst-case failure penalty (BASE + MODULE_LOAD + BYTECODE_LOAD)
pub const MIN_SCHEDULED_EXECUTION_GAS: u64 = 20_000;

/// Base cost for scheduling an execution (tos_offer_call syscall)
/// Covers: validation, hash computation, storage write
pub const OFFER_CALL_BASE_COST: u64 = 5_000;

/// Additional cost per byte of input data
pub const OFFER_CALL_BYTE_COST: u64 = 16;

/// Base cost charged on contract module not found
pub const BASE_SCHEDULED_EXECUTION_COST: u64 = 10_000;

/// Cost charged when module is loaded but bytecode missing
pub const MODULE_LOAD_COST: u64 = 5_000;

/// Cost charged when bytecode loading fails (VM error)
pub const BYTECODE_LOAD_COST: u64 = 5_000;

// ============================================================================
// Offer Handling (EIP-7833 Inspired)
// ============================================================================

/// Percentage of offer amount that is burned (30%)
/// Burned on registration to prevent manipulation
/// Consistent with TOS gas model (TX_GAS_BURN_PERCENT)
pub const OFFER_BURN_PERCENT: u64 = 30;

/// Percentage of offer amount paid to miner (70%)
/// Paid when execution is processed, not on registration
pub const OFFER_MINER_PERCENT: u64 = 70;

/// Minimum offer amount (0 = no minimum, pure FIFO fallback allowed)
/// Set to 0 to allow zero-offer scheduling with FIFO ordering
pub const MIN_OFFER_AMOUNT: u64 = 0;

// ============================================================================
// Rate Limiting
// ============================================================================

/// Maximum scheduled executions per contract per rate limit window
/// Prevents a single contract from flooding the scheduling queue
pub const MAX_SCHEDULES_PER_CONTRACT_PER_WINDOW: u64 = 100;

/// Rate limit window size in topoheights
/// Approximately 10 minutes at 6-second blocks
pub const SCHEDULE_RATE_LIMIT_WINDOW: u64 = 100;

/// Minimum offer amount to bypass rate limiting
/// High-value offers are not rate-limited (market-based DoS prevention)
/// 1 TOS = 100_000_000 atomic units (assuming 8 decimals)
pub const RATE_LIMIT_BYPASS_OFFER: u64 = 100_000_000;

// ============================================================================
// Syscall Error Codes
// ============================================================================

/// Error: Insufficient contract balance for offer + gas
pub const ERR_INSUFFICIENT_BALANCE: u64 = 1;

/// Error: Target topoheight is in the past
pub const ERR_TOPOHEIGHT_IN_PAST: u64 = 2;

/// Error: Target topoheight exceeds maximum scheduling horizon
pub const ERR_TOPOHEIGHT_TOO_FAR: u64 = 3;

/// Error: Execution already scheduled at this topoheight for this contract
pub const ERR_ALREADY_SCHEDULED: u64 = 4;

/// Error: Max gas below minimum threshold
pub const ERR_GAS_TOO_LOW: u64 = 5;

/// Error: Invalid params pointer or memory access
pub const ERR_INVALID_PARAMS: u64 = 6;

/// Error: Offer amount below minimum (if MIN_OFFER_AMOUNT > 0)
pub const ERR_OFFER_TOO_LOW: u64 = 7;

/// Error: Rate limit exceeded for this contract
pub const ERR_RATE_LIMIT_EXCEEDED: u64 = 8;

/// Error: Scheduled execution not found
pub const ERR_NOT_FOUND: u64 = 9;

/// Error: Not authorized to cancel (not the scheduler)
pub const ERR_NOT_AUTHORIZED: u64 = 10;

/// Error: Cannot cancel - already executed or too close to execution
pub const ERR_CANNOT_CANCEL: u64 = 11;

/// Error: Static call context - scheduling not allowed
pub const ERR_STATIC_CALL: u64 = 12;

// ============================================================================
// Flags for tos_offer_call
// ============================================================================

/// Flag: Schedule for block end instead of specific topoheight
pub const OFFER_CALL_FLAG_BLOCK_END: u64 = 0x01;

/// Flag: Use next topoheight (current + 1) instead of explicit target
pub const OFFER_CALL_FLAG_NEXT_TOPO: u64 = 0x02;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offer_percentages() {
        assert_eq!(OFFER_BURN_PERCENT + OFFER_MINER_PERCENT, 100);
    }

    #[test]
    fn test_min_gas_covers_penalties() {
        let max_penalty = BASE_SCHEDULED_EXECUTION_COST + MODULE_LOAD_COST + BYTECODE_LOAD_COST;
        assert!(MIN_SCHEDULED_EXECUTION_GAS >= max_penalty);
    }

    #[test]
    fn test_scheduling_horizon_reasonable() {
        // 7 days at 6-second blocks
        let expected_days = 7;
        let blocks_per_day = 24 * 60 * 60 / 6;
        let expected_blocks = expected_days * blocks_per_day;
        assert_eq!(MAX_SCHEDULING_HORIZON, expected_blocks);
    }
}
