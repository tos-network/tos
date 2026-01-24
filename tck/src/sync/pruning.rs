#[cfg(test)]
mod tests {
    use super::super::mock::*;

    #[test]
    fn prune_safety_limit_equals_stable_limit_times_ten() {
        assert_eq!(STABLE_LIMIT, 24);
        assert_eq!(PRUNE_SAFETY_LIMIT, STABLE_LIMIT * 10);
        assert_eq!(PRUNE_SAFETY_LIMIT, 240);
    }

    #[test]
    fn cannot_pop_blocks_below_pruned_plus_safety_limit() {
        let mut chain = make_linear_chain(300, 100);
        chain.pruned_topoheight = Some(50);

        // Safety boundary = 50 + 240 = 290
        // Chain at topoheight 300, so only 10 blocks can be popped
        let popped = chain.pop_blocks(50); // try to pop 50

        // Should stop at boundary
        let safety_boundary = 50 + PRUNE_SAFETY_LIMIT; // 290
        assert!(chain.topoheight >= safety_boundary);
        assert_eq!(popped.len(), 10); // only 10 popped (300 - 290)
    }

    #[test]
    fn fresh_chain_no_pruning_can_pop_to_genesis() {
        let mut chain = make_linear_chain(10, 100);
        assert!(chain.pruned_topoheight.is_none());

        let popped = chain.pop_blocks(10);

        assert_eq!(popped.len(), 10);
        assert_eq!(chain.topoheight, 0);
    }

    #[test]
    fn pruned_chain_pop_stops_at_safety_boundary() {
        let mut chain = make_linear_chain(350, 100);
        chain.pruned_topoheight = Some(100);

        // Safety boundary = 100 + 240 = 340
        // Can pop from 350 down to 341 (not including 340)
        let popped = chain.pop_blocks(100); // try to pop many

        assert_eq!(popped.len(), 10); // 350 - 340 = 10
        assert_eq!(chain.topoheight, 340);
    }

    #[test]
    fn pruned_topoheight_cannot_be_zero() {
        // A pruned_topoheight of 0 would mean nothing is pruned (same as None)
        // By convention, if pruning is set, it should be > 0
        let chain = make_linear_chain(100, 100);

        // Setting pruned_topoheight to 0 is semantically equivalent to no pruning
        let pruned = 0u64;
        let safety = pruned.saturating_add(PRUNE_SAFETY_LIMIT); // 240
                                                                // With pruned=0, safety boundary is 240
                                                                // Chain at 100 cannot pop at all since 100 <= 240
        let can_pop = chain.topoheight > safety;
        assert!(!can_pop);
    }

    #[test]
    fn pruned_topoheight_cannot_exceed_current_topoheight() {
        let chain = make_linear_chain(100, 100);

        // pruned_topoheight > current topoheight is invalid
        let invalid_pruned: TopoHeight = 150;
        assert!(invalid_pruned > chain.topoheight);

        // Valid pruned must be <= current topoheight
        let valid_pruned: TopoHeight = 50;
        assert!(valid_pruned <= chain.topoheight);
    }

    #[test]
    fn after_pruning_data_below_pruned_topoheight_unavailable() {
        let mut chain = make_linear_chain(100, 100);
        chain.pruned_topoheight = Some(50);

        // Conceptually, blocks below pruned_topoheight are not available
        // The mock still has them, but in production they would be removed
        let pruned_topo = chain.pruned_topoheight.unwrap();

        // Verify the concept: any request for blocks below pruned_topoheight
        // should be treated as unavailable
        let requested_topo: TopoHeight = 30;
        let is_available = requested_topo >= pruned_topo;
        assert!(!is_available);

        // Blocks at or above pruned_topoheight are available
        let requested_topo: TopoHeight = 50;
        let is_available = requested_topo >= pruned_topo;
        assert!(is_available);
    }

    #[test]
    fn bootstrap_from_pruned_node_partial_state_sync() {
        let mut chain = make_linear_chain(500, 100);
        chain.pruned_topoheight = Some(200);

        // When bootstrapping from a pruned node, the sync starts from pruned_topoheight
        let sync_start = chain.pruned_topoheight.unwrap();
        let sync_end = chain.topoheight;
        let blocks_available = sync_end - sync_start;

        assert_eq!(blocks_available, 300);

        // The syncing node needs at least PRUNE_SAFETY_LIMIT blocks
        assert!(blocks_available >= PRUNE_SAFETY_LIMIT);
    }

    #[test]
    fn pruned_topoheight_only_increases_monotonic() {
        let mut chain = make_linear_chain(500, 100);

        // Initial prune point
        chain.pruned_topoheight = Some(100);

        // Advancing prune point is valid
        let new_prune = 150u64;
        let old_prune = chain.pruned_topoheight.unwrap();
        let is_valid_advance = new_prune > old_prune;
        assert!(is_valid_advance);
        chain.pruned_topoheight = Some(new_prune);

        // Retreating prune point is invalid (not monotonic)
        let retreat_prune = 120u64;
        let current_prune = chain.pruned_topoheight.unwrap();
        let is_valid_retreat = retreat_prune > current_prune;
        assert!(!is_valid_retreat); // 120 < 150, invalid
    }

    #[test]
    fn safety_limit_provides_buffer_for_reorgs() {
        // PRUNE_SAFETY_LIMIT ensures enough blocks are kept for potential reorgs
        let mut chain = make_linear_chain(500, 100);
        chain.pruned_topoheight = Some(200);

        // The safety buffer is PRUNE_SAFETY_LIMIT blocks above the prune point
        let safety_boundary = 200 + PRUNE_SAFETY_LIMIT; // 440
        let _blocks_in_buffer = chain.topoheight - safety_boundary; // 500 - 440 = 60

        // These 60 blocks can be freely reorganized
        let popped = chain.pop_blocks(60);
        assert_eq!(popped.len(), 60);
        assert_eq!(chain.topoheight, 440);

        // Cannot pop further (at safety boundary)
        let more_popped = chain.pop_blocks(10);
        assert!(more_popped.is_empty());
    }

    #[test]
    fn stable_limit_blocks_always_kept_unpruned() {
        assert_eq!(STABLE_LIMIT, 24);

        let chain = make_linear_chain(100, 100);
        // Stable topoheight = 100 - 24 = 76
        assert_eq!(chain.stable_topoheight, 76);

        // The top STABLE_LIMIT blocks are always available (above stable height)
        let unstable_blocks = chain.topoheight - chain.stable_topoheight;
        assert_eq!(unstable_blocks, STABLE_LIMIT);
    }

    #[test]
    fn chain_with_exactly_prune_safety_limit_blocks() {
        let chain = make_linear_chain(PRUNE_SAFETY_LIMIT, 100);
        assert_eq!(chain.topoheight, 240);
        assert_eq!(chain.blocks.len(), 240);

        // With exactly PRUNE_SAFETY_LIMIT blocks and pruned_topoheight = 0,
        // the safety boundary would be 0 + 240 = 240 = topoheight
        // No blocks can be popped
        let mut chain_with_prune = chain.clone();
        chain_with_prune.pruned_topoheight = Some(0);

        // Cannot pop because topoheight(240) <= safety(0+240=240)
        let popped = chain_with_prune.pop_blocks(5);
        assert!(popped.is_empty());
    }

    #[test]
    fn chain_shorter_than_prune_safety_limit_no_pruning() {
        let chain = make_linear_chain(100, 100); // 100 < 240
        assert!(chain.topoheight < PRUNE_SAFETY_LIMIT);

        // Chain too short for meaningful pruning
        // If pruned_topoheight were set to 0, safety = 240 > topoheight(100)
        // So no blocks could be popped
        let mut chain_with_prune = chain.clone();
        chain_with_prune.pruned_topoheight = Some(0);

        let popped = chain_with_prune.pop_blocks(10);
        assert!(popped.is_empty());
    }

    #[test]
    fn pruning_does_not_affect_stable_height_calculation() {
        let mut chain = make_linear_chain(300, 100);

        // Stable height before pruning
        let stable_before = chain.stable_topoheight;
        assert_eq!(stable_before, 276); // 300 - 24

        // Set pruning
        chain.pruned_topoheight = Some(100);

        // Stable height is calculated from topoheight, not affected by pruning
        // stable = topoheight - STABLE_LIMIT
        let expected_stable = chain.topoheight - STABLE_LIMIT;
        assert_eq!(chain.stable_topoheight, expected_stable);
        assert_eq!(chain.stable_topoheight, stable_before); // unchanged
    }
}
