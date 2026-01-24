// Phase 16: Scheduled Execution Processor Tests (Layer 1)
//
// Pure unit tests for scheduled execution logic using direct daemon/common imports.
// Tests priority ordering, offer calculations, defer mechanics, and boundary validation.

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use tos_common::contract::scheduled_execution::ScheduledExecutionPriority;
    use tos_common::contract::{
        ScheduledExecution, ScheduledExecutionKind, ScheduledExecutionStatus, MAX_DEFER_COUNT,
        MAX_INPUT_DATA_SIZE, MAX_OFFER_AMOUNT, MAX_SCHEDULED_EXECUTIONS_PER_BLOCK,
        MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK, MAX_SCHEDULING_HORIZON, MIN_CANCELLATION_WINDOW,
        MIN_OFFER_AMOUNT, MIN_SCHEDULED_EXECUTION_GAS, OFFER_BURN_PERCENT, OFFER_MINER_PERCENT,
        RATE_LIMIT_BYPASS_OFFER, SCHEDULE_RATE_LIMIT_WINDOW,
    };
    use tos_common::crypto::Hash;
    use tos_daemon::tako_integration::{calculate_offer_burn, calculate_offer_miner_reward};

    /// Helper: create a ScheduledExecution with specified parameters
    fn make_exec(
        contract_byte: u8,
        offer: u64,
        reg_topo: u64,
        target_topo: u64,
    ) -> ScheduledExecution {
        ScheduledExecution::new_offercall(
            Hash::new([contract_byte; 32]),
            0,
            vec![],
            MIN_SCHEDULED_EXECUTION_GAS,
            offer,
            Hash::new([0xFFu8; 32]),
            ScheduledExecutionKind::TopoHeight(target_topo),
            reg_topo,
        )
    }

    fn make_exec_block_end(contract_byte: u8, offer: u64, reg_topo: u64) -> ScheduledExecution {
        ScheduledExecution::new_offercall(
            Hash::new([contract_byte; 32]),
            0,
            vec![],
            MIN_SCHEDULED_EXECUTION_GAS,
            offer,
            Hash::new([0xFFu8; 32]),
            ScheduledExecutionKind::BlockEnd,
            reg_topo,
        )
    }

    // ========================================================================
    // Priority Score Formula Tests
    // ========================================================================

    #[test]
    fn priority_score_formula() {
        let exec = make_exec(0x01, 1000, 100, 200);
        let score = exec.priority_score();

        // score = (offer_amount * 1000) + (MAX_TOPO - registration_topoheight)
        let expected = (1000u128 * 1000) + (u64::MAX as u128 - 100);
        assert_eq!(score, expected);
    }

    #[test]
    fn priority_score_zero_offer() {
        let exec = make_exec(0x01, 0, 100, 200);
        let score = exec.priority_score();

        // With zero offer, score is purely FIFO component
        let expected = u64::MAX as u128 - 100;
        assert_eq!(score, expected);
    }

    #[test]
    fn priority_score_higher_offer_dominates() {
        let high = make_exec(0x01, 1000, 500, 600);
        let low = make_exec(0x02, 100, 100, 600); // earlier registration but lower offer

        assert!(
            high.priority_score() > low.priority_score(),
            "Higher offer must dominate even with later registration"
        );
    }

    #[test]
    fn priority_cmp_offer_amount_first() {
        let high = make_exec(0x01, 100_000, 500, 600);
        let low = make_exec(0x02, 1_000, 100, 600);

        assert_eq!(high.priority_cmp(&low), Ordering::Greater);
        assert_eq!(low.priority_cmp(&high), Ordering::Less);
    }

    #[test]
    fn priority_cmp_fifo_for_equal_offers() {
        let earlier = make_exec(0x01, 5000, 100, 300);
        let later = make_exec(0x02, 5000, 200, 300);

        assert_eq!(
            earlier.priority_cmp(&later),
            Ordering::Greater,
            "Earlier registration wins for equal offers"
        );
    }

    #[test]
    fn priority_cmp_hash_tiebreaker() {
        // Same offer, same registration_topoheight
        let exec_a = make_exec(0x01, 5000, 100, 300);
        let exec_b = make_exec(0x02, 5000, 100, 300);

        let cmp = exec_a.priority_cmp(&exec_b);
        assert_ne!(
            cmp,
            Ordering::Equal,
            "Hash should break ties deterministically"
        );
    }

    // ========================================================================
    // Defer Mechanics Tests
    // ========================================================================

    #[test]
    fn max_defer_count_expiry() {
        let mut exec = make_exec(0x01, 1000, 100, 200);
        assert_eq!(exec.defer_count, 0);

        // Defer MAX_DEFER_COUNT - 1 times without hitting max
        for i in 1..MAX_DEFER_COUNT {
            let max_reached = exec.defer();
            assert!(!max_reached, "Should not expire at defer {}", i);
            assert_eq!(exec.defer_count, i);
        }

        // Final defer should trigger expiry
        let max_reached = exec.defer();
        assert!(max_reached, "Should expire at defer {}", MAX_DEFER_COUNT);
        assert_eq!(exec.defer_count, MAX_DEFER_COUNT);
    }

    #[test]
    fn defer_count_saturates() {
        let mut exec = make_exec(0x01, 1000, 100, 200);

        // Defer well beyond MAX_DEFER_COUNT
        for _ in 0..20 {
            exec.defer();
        }

        // defer_count should saturate at u8::MAX (but MAX_DEFER_COUNT is 10, so it stops at 20)
        // Actually saturating_add prevents overflow
        assert!(exec.defer_count >= MAX_DEFER_COUNT);
    }

    #[test]
    fn initial_defer_count_zero() {
        let exec = make_exec(0x01, 1000, 100, 200);
        assert_eq!(
            exec.defer_count, 0,
            "New execution must start with defer_count=0"
        );
    }

    // ========================================================================
    // Offer Calculation Tests
    // ========================================================================

    #[test]
    fn offer_burn_at_registration() {
        let offer = 1000u64;
        let burn = calculate_offer_burn(offer);
        let expected = offer * OFFER_BURN_PERCENT / 100;
        assert_eq!(burn, expected);
        assert_eq!(burn, 300); // 30% of 1000
    }

    #[test]
    fn miner_reward_at_execution() {
        let offer = 1000u64;
        let reward = calculate_offer_miner_reward(offer);
        let expected = offer * OFFER_MINER_PERCENT / 100;
        assert_eq!(reward, expected);
        assert_eq!(reward, 700); // 70% of 1000
    }

    #[test]
    fn offer_burn_plus_miner_equals_offer() {
        for offer in [0, 1, 99, 100, 101, 999, 1000, 10_000, 1_000_000_000_000] {
            let burn = calculate_offer_burn(offer);
            let miner = calculate_offer_miner_reward(offer);
            assert_eq!(
                burn + miner,
                offer,
                "Burn + miner must equal offer for amount={}",
                offer
            );
        }
    }

    #[test]
    fn offer_zero_no_burn_no_reward() {
        let burn = calculate_offer_burn(0);
        let miner = calculate_offer_miner_reward(0);
        assert_eq!(burn, 0);
        assert_eq!(miner, 0);
    }

    #[test]
    fn offer_one_rounding() {
        // 1 * 30 / 100 = 0 (truncated)
        let burn = calculate_offer_burn(1);
        let miner = calculate_offer_miner_reward(1);
        assert_eq!(burn, 0);
        assert_eq!(miner, 1);
        assert_eq!(burn + miner, 1);
    }

    #[test]
    fn offer_max_no_overflow() {
        let burn = calculate_offer_burn(MAX_OFFER_AMOUNT);
        let miner = calculate_offer_miner_reward(MAX_OFFER_AMOUNT);
        assert_eq!(burn + miner, MAX_OFFER_AMOUNT);
        assert!(burn > 0);
        assert!(miner > 0);
    }

    // ========================================================================
    // Cancellation Window Tests
    // ========================================================================

    #[test]
    fn can_cancel_far_future() {
        let current_topo = 100u64;
        let target_topo = 200u64;

        let exec = make_exec(0x01, 1000, current_topo, target_topo);
        assert!(
            exec.can_cancel(current_topo),
            "Should be cancellable when target is far in the future"
        );
    }

    #[test]
    fn cannot_cancel_near_future() {
        let current_topo = 100u64;
        // target = current + MIN_CANCELLATION_WINDOW (exactly at boundary)
        let target_topo = current_topo + MIN_CANCELLATION_WINDOW;

        let exec = make_exec(0x01, 1000, 50, target_topo);
        assert!(
            !exec.can_cancel(current_topo),
            "Should NOT be cancellable at boundary"
        );
    }

    #[test]
    fn cannot_cancel_past_target() {
        let current_topo = 200u64;
        let target_topo = 150u64; // already past

        let exec = make_exec(0x01, 1000, 50, target_topo);
        assert!(
            !exec.can_cancel(current_topo),
            "Should NOT be cancellable when target is in the past"
        );
    }

    #[test]
    fn cannot_cancel_block_end() {
        let current_topo = 100u64;
        let exec = make_exec_block_end(0x01, 1000, current_topo);
        assert!(
            !exec.can_cancel(current_topo),
            "BlockEnd executions should never be cancellable"
        );
    }

    #[test]
    fn cannot_cancel_non_pending() {
        let mut exec = make_exec(0x01, 1000, 50, 200);
        exec.status = ScheduledExecutionStatus::Executed;
        assert!(
            !exec.can_cancel(100),
            "Non-pending execution should not be cancellable"
        );

        exec.status = ScheduledExecutionStatus::Failed;
        assert!(!exec.can_cancel(100));

        exec.status = ScheduledExecutionStatus::Cancelled;
        assert!(!exec.can_cancel(100));

        exec.status = ScheduledExecutionStatus::Expired;
        assert!(!exec.can_cancel(100));
    }

    // ========================================================================
    // Status Tests
    // ========================================================================

    #[test]
    fn new_execution_is_pending() {
        let exec = make_exec(0x01, 1000, 100, 200);
        assert!(matches!(exec.status, ScheduledExecutionStatus::Pending));
        assert!(exec.is_pending());
    }

    #[test]
    fn status_transitions() {
        let mut exec = make_exec(0x01, 1000, 100, 200);

        // Pending -> Executed
        exec.status = ScheduledExecutionStatus::Executed;
        assert!(!exec.is_pending());

        // Pending -> Failed
        let mut exec2 = make_exec(0x02, 1000, 100, 200);
        exec2.status = ScheduledExecutionStatus::Failed;
        assert!(!exec2.is_pending());

        // Pending -> Cancelled
        let mut exec3 = make_exec(0x03, 1000, 100, 200);
        exec3.status = ScheduledExecutionStatus::Cancelled;
        assert!(!exec3.is_pending());

        // Pending -> Expired
        let mut exec4 = make_exec(0x04, 1000, 100, 200);
        exec4.status = ScheduledExecutionStatus::Expired;
        assert!(!exec4.is_pending());
    }

    // ========================================================================
    // Hash Computation Tests
    // ========================================================================

    #[test]
    fn compute_hash_deterministic() {
        let contract = Hash::new([0x01u8; 32]);
        let kind = ScheduledExecutionKind::TopoHeight(200);
        let reg_topo = 100u64;
        let chunk_id = 0u16;

        let hash1 = ScheduledExecution::compute_hash(&contract, &kind, reg_topo, chunk_id);
        let hash2 = ScheduledExecution::compute_hash(&contract, &kind, reg_topo, chunk_id);
        assert_eq!(hash1, hash2, "compute_hash must be deterministic");
    }

    #[test]
    fn compute_hash_differs_by_contract() {
        let kind = ScheduledExecutionKind::TopoHeight(200);
        let hash_a = ScheduledExecution::compute_hash(&Hash::new([0x01u8; 32]), &kind, 100, 0);
        let hash_b = ScheduledExecution::compute_hash(&Hash::new([0x02u8; 32]), &kind, 100, 0);
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn compute_hash_differs_by_kind() {
        let contract = Hash::new([0x01u8; 32]);
        let hash_topo = ScheduledExecution::compute_hash(
            &contract,
            &ScheduledExecutionKind::TopoHeight(200),
            100,
            0,
        );
        let hash_block_end =
            ScheduledExecution::compute_hash(&contract, &ScheduledExecutionKind::BlockEnd, 100, 0);
        assert_ne!(hash_topo, hash_block_end);
    }

    #[test]
    fn compute_hash_differs_by_registration_topo() {
        let contract = Hash::new([0x01u8; 32]);
        let kind = ScheduledExecutionKind::TopoHeight(200);
        let hash_a = ScheduledExecution::compute_hash(&contract, &kind, 100, 0);
        let hash_b = ScheduledExecution::compute_hash(&contract, &kind, 101, 0);
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn compute_hash_differs_by_chunk_id() {
        let contract = Hash::new([0x01u8; 32]);
        let kind = ScheduledExecutionKind::TopoHeight(200);
        let hash_a = ScheduledExecution::compute_hash(&contract, &kind, 100, 0);
        let hash_b = ScheduledExecution::compute_hash(&contract, &kind, 100, 1);
        assert_ne!(hash_a, hash_b);
    }

    // ========================================================================
    // Input Data Tests
    // ========================================================================

    #[test]
    fn input_data_stored() {
        let input = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let exec = ScheduledExecution::new_offercall(
            Hash::new([0x01u8; 32]),
            42,
            input.clone(),
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            Hash::new([0xFFu8; 32]),
            ScheduledExecutionKind::TopoHeight(200),
            100,
        );

        assert_eq!(exec.input_data, input);
        assert_eq!(exec.chunk_id, 42);
    }

    #[test]
    fn empty_input_data_valid() {
        let exec = ScheduledExecution::new_offercall(
            Hash::new([0x01u8; 32]),
            0,
            vec![],
            MIN_SCHEDULED_EXECUTION_GAS,
            1000,
            Hash::new([0xFFu8; 32]),
            ScheduledExecutionKind::TopoHeight(200),
            100,
        );

        assert!(exec.input_data.is_empty());
    }

    // ========================================================================
    // Constants Validation Tests
    // ========================================================================

    #[test]
    fn constants_reasonable() {
        assert_eq!(MAX_DEFER_COUNT, 10);
        assert_eq!(MAX_SCHEDULED_EXECUTIONS_PER_BLOCK, 100);
        assert_eq!(MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK, 100_000_000);
        assert_eq!(MAX_SCHEDULING_HORIZON, 100_800);
        assert_eq!(MIN_CANCELLATION_WINDOW, 1);
        assert_eq!(MIN_SCHEDULED_EXECUTION_GAS, 20_000);
        assert_eq!(OFFER_BURN_PERCENT, 30);
        assert_eq!(OFFER_MINER_PERCENT, 70);
        assert_eq!(OFFER_BURN_PERCENT + OFFER_MINER_PERCENT, 100);
        assert_eq!(MIN_OFFER_AMOUNT, 0);
        assert_eq!(MAX_OFFER_AMOUNT, 1_000_000_000_000);
        assert_eq!(MAX_INPUT_DATA_SIZE, 64 * 1024);
        assert_eq!(SCHEDULE_RATE_LIMIT_WINDOW, 100);
        assert_eq!(RATE_LIMIT_BYPASS_OFFER, 100_000_000);
    }

    // ========================================================================
    // Execution Kind Tests
    // ========================================================================

    #[test]
    fn kind_topoheight_stores_target() {
        let exec = make_exec(0x01, 1000, 100, 200);
        match exec.kind {
            ScheduledExecutionKind::TopoHeight(t) => assert_eq!(t, 200),
            _ => panic!("Expected TopoHeight kind"),
        }
    }

    #[test]
    fn kind_block_end() {
        let exec = make_exec_block_end(0x01, 1000, 100);
        assert!(matches!(exec.kind, ScheduledExecutionKind::BlockEnd));
    }

    // ========================================================================
    // Sorting and Ordering Tests
    // ========================================================================

    #[test]
    fn sort_multiple_by_priority_descending() {
        let mut executions = [
            make_exec(0x01, 100, 300, 500),    // Low offer, late
            make_exec(0x02, 10_000, 100, 500), // High offer, early
            make_exec(0x03, 5_000, 200, 500),  // Medium offer
            make_exec(0x04, 10_000, 200, 500), // High offer, late
            make_exec(0x05, 0, 50, 500),       // Zero offer, earliest
        ];

        executions.sort_by(|a, b| b.cmp(a));

        // Order: high-early, high-late, medium, low, zero
        assert_eq!(executions[0].offer_amount, 10_000);
        assert_eq!(executions[0].registration_topoheight, 100);
        assert_eq!(executions[1].offer_amount, 10_000);
        assert_eq!(executions[1].registration_topoheight, 200);
        assert_eq!(executions[2].offer_amount, 5_000);
        assert_eq!(executions[3].offer_amount, 100);
        assert_eq!(executions[4].offer_amount, 0);
    }

    #[test]
    fn priority_ordering_is_total() {
        // Verify that Ord implementation provides total ordering
        let a = make_exec(0x01, 1000, 100, 200);
        let b = make_exec(0x02, 1000, 100, 200);

        // a.cmp(b) and b.cmp(a) should be consistent
        let ab = a.cmp(&b);
        let ba = b.cmp(&a);
        assert_eq!(ab, ba.reverse());
    }
}
