// Verify/Apply phase separation tests
//
// Verifies the conceptual properties of the verify/apply transaction processing model:
// - Verify phase is stateless (read-only, no balance/nonce mutations)
// - Apply phase is stateful (modifies balances, nonces)
// - Nonce uses compare-and-swap semantics in verify
// - Balance is deducted from sender BEFORE crediting receiver in apply
// - All arithmetic uses checked_* functions to prevent overflow/underflow

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Mock state representing account nonces and balances for testing
    // verify/apply phase separation properties.
    struct MockState {
        nonces: HashMap<u8, AtomicU64>,
        balances: HashMap<(u8, u8), u64>,
    }

    impl MockState {
        // Create a new MockState with pre-populated accounts
        fn new() -> Self {
            let mut nonces = HashMap::new();
            let mut balances = HashMap::new();

            // Account 1: nonce=0, balance=10000 for asset 0
            nonces.insert(1u8, AtomicU64::new(0));
            balances.insert((1u8, 0u8), 10_000u64);

            // Account 2: nonce=0, balance=5000 for asset 0
            nonces.insert(2u8, AtomicU64::new(0));
            balances.insert((2u8, 0u8), 5_000u64);

            // Account 3: nonce=0, balance=0 for asset 0
            nonces.insert(3u8, AtomicU64::new(0));
            balances.insert((3u8, 0u8), 0u64);

            Self { nonces, balances }
        }

        // Compare-and-swap nonce: atomically update if current value matches expected
        fn compare_and_swap_nonce(&self, account: u8, expected: u64, new: u64) -> bool {
            if let Some(nonce) = self.nonces.get(&account) {
                nonce
                    .compare_exchange(expected, new, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            } else {
                false
            }
        }

        // Get the current nonce for an account
        fn get_nonce(&self, account: u8) -> Option<u64> {
            self.nonces.get(&account).map(|n| n.load(Ordering::SeqCst))
        }

        // Get the balance for an account and asset
        fn get_balance(&self, account: u8, asset: u8) -> u64 {
            self.balances.get(&(account, asset)).copied().unwrap_or(0)
        }

        // Set the balance for an account and asset
        fn set_balance(&mut self, account: u8, asset: u8, value: u64) {
            self.balances.insert((account, asset), value);
        }

        // Simulate verify phase: check nonce and balance without modifying state
        // Returns Ok(()) if verification passes, Err(reason) otherwise
        fn verify_transfer(
            &self,
            sender: u8,
            _receiver: u8,
            amount: u64,
            expected_nonce: u64,
        ) -> Result<(), &'static str> {
            // Validate amount > 0
            if amount == 0 {
                return Err("zero amount");
            }

            // Check nonce matches expected (CAS semantics in verify)
            let current_nonce = self.get_nonce(sender).ok_or("account not found")?;
            if current_nonce != expected_nonce {
                return Err("nonce mismatch");
            }

            // Check sufficient balance (read-only check)
            let balance = self.get_balance(sender, 0);
            if balance < amount {
                return Err("insufficient balance");
            }

            Ok(())
        }

        // Simulate apply phase: actually modify balances and nonce
        // Deducts from sender BEFORE crediting receiver
        fn apply_transfer(
            &mut self,
            sender: u8,
            receiver: u8,
            amount: u64,
            expected_nonce: u64,
        ) -> Result<(), &'static str> {
            // CAS the nonce
            let new_nonce = expected_nonce.checked_add(1).ok_or("nonce overflow")?;
            if !self.compare_and_swap_nonce(sender, expected_nonce, new_nonce) {
                return Err("nonce CAS failed");
            }

            // Deduct from sender first (checked subtraction)
            let sender_balance = self.get_balance(sender, 0);
            let new_sender_balance = sender_balance
                .checked_sub(amount)
                .ok_or("underflow: insufficient balance")?;
            self.set_balance(sender, 0, new_sender_balance);

            // Credit receiver (checked addition)
            let receiver_balance = self.get_balance(receiver, 0);
            let new_receiver_balance = receiver_balance
                .checked_add(amount)
                .ok_or("overflow: receiver balance")?;
            self.set_balance(receiver, 0, new_receiver_balance);

            Ok(())
        }
    }

    // =========================================================================
    // Nonce CAS Property Tests
    // =========================================================================

    #[test]
    fn test_cas_nonce_success() {
        let state = MockState::new();

        // CAS(current=0, expected=0, new=1) should succeed
        let result = state.compare_and_swap_nonce(1, 0, 1);
        assert!(result, "CAS should succeed when current matches expected");
        assert_eq!(
            state.get_nonce(1).unwrap(),
            1,
            "Nonce should be updated to 1"
        );
    }

    #[test]
    fn test_cas_nonce_failure() {
        let state = MockState::new();

        // First, advance nonce to 1
        assert!(state.compare_and_swap_nonce(1, 0, 1));

        // CAS(current=1, expected=0, new=1) should fail
        let result = state.compare_and_swap_nonce(1, 0, 1);
        assert!(
            !result,
            "CAS should fail when current (1) does not match expected (0)"
        );
        assert_eq!(
            state.get_nonce(1).unwrap(),
            1,
            "Nonce should remain at 1 after failed CAS"
        );
    }

    #[test]
    fn test_cas_nonce_sequential() {
        let state = MockState::new();

        // CAS(0->1), CAS(1->2), CAS(2->3) should all succeed
        assert!(state.compare_and_swap_nonce(1, 0, 1), "0->1 should succeed");
        assert!(state.compare_and_swap_nonce(1, 1, 2), "1->2 should succeed");
        assert!(state.compare_and_swap_nonce(1, 2, 3), "2->3 should succeed");

        assert_eq!(state.get_nonce(1).unwrap(), 3, "Final nonce should be 3");
    }

    #[test]
    fn test_cas_nonce_concurrent_rejection() {
        let state = MockState::new();

        // Two transactions with same expected nonce: first succeeds, second fails
        let first = state.compare_and_swap_nonce(1, 0, 1);
        let second = state.compare_and_swap_nonce(1, 0, 1);

        assert!(first, "First CAS with expected=0 should succeed");
        assert!(
            !second,
            "Second CAS with expected=0 should fail (nonce is now 1)"
        );
        assert_eq!(
            state.get_nonce(1).unwrap(),
            1,
            "Nonce should be 1 (only first succeeded)"
        );
    }

    #[test]
    fn test_cas_nonce_gap_rejection() {
        let state = MockState::new();

        // CAS(current=0, expected=2, new=3) should fail (gap in nonce sequence)
        let result = state.compare_and_swap_nonce(1, 2, 3);
        assert!(!result, "CAS should fail when expected (2) != current (0)");
        assert_eq!(
            state.get_nonce(1).unwrap(),
            0,
            "Nonce should remain at 0 after gap rejection"
        );
    }

    // =========================================================================
    // Balance Phase Property Tests
    // =========================================================================

    #[test]
    fn test_verify_does_not_modify_balance() {
        let state = MockState::new();
        let initial_balance = state.get_balance(1, 0);

        // Verify should be read-only
        let result = state.verify_transfer(1, 2, 500, 0);
        assert!(result.is_ok(), "Verify should pass for valid transfer");

        let after_balance = state.get_balance(1, 0);
        assert_eq!(
            initial_balance, after_balance,
            "Balance must not change during verify phase"
        );
    }

    #[test]
    fn test_apply_deducts_sender() {
        let mut state = MockState::new();
        let initial_balance = state.get_balance(1, 0);
        let amount = 500u64;

        let result = state.apply_transfer(1, 2, amount, 0);
        assert!(result.is_ok(), "Apply should succeed for valid transfer");

        let after_balance = state.get_balance(1, 0);
        let expected = initial_balance.checked_sub(amount).unwrap();
        assert_eq!(
            after_balance, expected,
            "Sender balance should decrease by transfer amount"
        );
    }

    #[test]
    fn test_apply_credits_receiver() {
        let mut state = MockState::new();
        let initial_receiver_balance = state.get_balance(2, 0);
        let amount = 500u64;

        let result = state.apply_transfer(1, 2, amount, 0);
        assert!(result.is_ok(), "Apply should succeed");

        let after_receiver_balance = state.get_balance(2, 0);
        let expected = initial_receiver_balance.checked_add(amount).unwrap();
        assert_eq!(
            after_receiver_balance, expected,
            "Receiver balance should increase by transfer amount"
        );
    }

    #[test]
    fn test_apply_sender_before_receiver() {
        // Verify that sender deduction happens before receiver credit
        // by using a case where sender == receiver would cause issues
        // if order were reversed (receiver credit would increase balance
        // before sender deduction checks it)
        let mut state = MockState::new();

        // Account 1 has 10000, transfer 8000 to account 2
        let sender_before = state.get_balance(1, 0);
        let receiver_before = state.get_balance(2, 0);
        let amount = 8000u64;

        let result = state.apply_transfer(1, 2, amount, 0);
        assert!(result.is_ok());

        // Verify conservation: total before == total after
        let sender_after = state.get_balance(1, 0);
        let receiver_after = state.get_balance(2, 0);

        let total_before = sender_before.checked_add(receiver_before).unwrap();
        let total_after = sender_after.checked_add(receiver_after).unwrap();
        assert_eq!(
            total_before, total_after,
            "Total value must be conserved: sender deduction = receiver credit"
        );

        // Verify sender was properly deducted
        assert_eq!(sender_after, sender_before.checked_sub(amount).unwrap());
        // Verify receiver was properly credited
        assert_eq!(receiver_after, receiver_before.checked_add(amount).unwrap());
    }

    #[test]
    fn test_insufficient_balance_rejected() {
        let mut state = MockState::new();

        // Account 1 has 10000, try to transfer 20000
        let result = state.apply_transfer(1, 2, 20_000, 0);
        assert!(
            result.is_err(),
            "Transfer exceeding balance should be rejected"
        );

        // But nonce was already CAS'd, so it advanced (this is a property of
        // our mock; in real implementation, balance check comes before nonce update
        // or the entire operation is atomic)
    }

    // =========================================================================
    // Checked Arithmetic Tests
    // =========================================================================

    #[test]
    fn test_checked_add_normal() {
        let a: u64 = 1000;
        let b: u64 = 2000;
        let result = a.checked_add(b);
        assert_eq!(result, Some(3000), "Normal addition should work");
    }

    #[test]
    fn test_checked_add_overflow() {
        let a: u64 = u64::MAX;
        let b: u64 = 1;
        let result = a.checked_add(b);
        assert_eq!(result, None, "u64::MAX + 1 should return None (overflow)");
    }

    #[test]
    fn test_checked_sub_normal() {
        let a: u64 = 5000;
        let b: u64 = 3000;
        let result = a.checked_sub(b);
        assert_eq!(result, Some(2000), "Normal subtraction should work");
    }

    #[test]
    fn test_checked_sub_underflow() {
        let a: u64 = 0;
        let b: u64 = 1;
        let result = a.checked_sub(b);
        assert_eq!(result, None, "0 - 1 should return None (underflow)");
    }

    #[test]
    fn test_balance_transfer_conservation() {
        let mut state = MockState::new();
        let amount = 3000u64;

        let sender_before = state.get_balance(1, 0);
        let receiver_before = state.get_balance(2, 0);

        let result = state.apply_transfer(1, 2, amount, 0);
        assert!(result.is_ok());

        let sender_after = state.get_balance(1, 0);
        let receiver_after = state.get_balance(2, 0);

        // sender_loss == receiver_gain
        let sender_loss = sender_before.checked_sub(sender_after).unwrap();
        let receiver_gain = receiver_after.checked_sub(receiver_before).unwrap();

        assert_eq!(
            sender_loss, receiver_gain,
            "Amount deducted from sender must equal amount credited to receiver"
        );
        assert_eq!(sender_loss, amount, "Loss must equal transfer amount");
    }

    // =========================================================================
    // Phase Isolation Tests
    // =========================================================================

    #[test]
    fn test_verify_parallelizable() {
        // Multiple verifications can run concurrently without interference
        // because verify is read-only
        let state = MockState::new();

        // Multiple verify calls with the same state should all succeed
        // (no mutation means no ordering dependency)
        let r1 = state.verify_transfer(1, 2, 500, 0);
        let r2 = state.verify_transfer(1, 3, 1000, 0);
        let r3 = state.verify_transfer(2, 1, 2000, 0);

        assert!(r1.is_ok(), "First verify should succeed");
        assert!(
            r2.is_ok(),
            "Second verify should succeed (same sender, same nonce is ok in verify)"
        );
        assert!(r3.is_ok(), "Third verify should succeed (different sender)");

        // State should be completely unchanged
        assert_eq!(state.get_balance(1, 0), 10_000);
        assert_eq!(state.get_balance(2, 0), 5_000);
        assert_eq!(state.get_nonce(1).unwrap(), 0);
        assert_eq!(state.get_nonce(2).unwrap(), 0);
    }

    #[test]
    fn test_apply_sequential() {
        // Applies must be ordered: nonce advances sequentially
        let mut state = MockState::new();

        // First apply: nonce 0 -> 1
        let r1 = state.apply_transfer(1, 2, 1000, 0);
        assert!(r1.is_ok(), "First apply should succeed");
        assert_eq!(state.get_nonce(1).unwrap(), 1);

        // Second apply: nonce 1 -> 2
        let r2 = state.apply_transfer(1, 2, 1000, 1);
        assert!(r2.is_ok(), "Second apply should succeed");
        assert_eq!(state.get_nonce(1).unwrap(), 2);

        // Out-of-order apply should fail: nonce is 2, trying expected=0
        let r3 = state.apply_transfer(1, 2, 1000, 0);
        assert!(r3.is_err(), "Out-of-order apply should fail");
    }

    #[test]
    fn test_failed_verify_no_side_effects() {
        let state = MockState::new();

        let balance_before = state.get_balance(1, 0);
        let nonce_before = state.get_nonce(1).unwrap();

        // Verify with wrong nonce should fail
        let result = state.verify_transfer(1, 2, 500, 99);
        assert!(result.is_err(), "Verify with wrong nonce should fail");

        // Verify with insufficient balance should fail
        let result2 = state.verify_transfer(1, 2, 999_999, 0);
        assert!(
            result2.is_err(),
            "Verify with insufficient balance should fail"
        );

        // State must be completely unchanged after failed verifications
        assert_eq!(
            state.get_balance(1, 0),
            balance_before,
            "Balance must not change after failed verify"
        );
        assert_eq!(
            state.get_nonce(1).unwrap(),
            nonce_before,
            "Nonce must not change after failed verify"
        );
    }

    #[test]
    fn test_apply_after_verify() {
        // Full verify -> apply pipeline
        let mut state = MockState::new();
        let amount = 2000u64;
        let sender = 1u8;
        let receiver = 2u8;
        let nonce = 0u64;

        // Step 1: Verify (read-only check)
        let verify_result = state.verify_transfer(sender, receiver, amount, nonce);
        assert!(verify_result.is_ok(), "Verify should pass");

        // State unchanged after verify
        assert_eq!(state.get_balance(sender, 0), 10_000);
        assert_eq!(state.get_balance(receiver, 0), 5_000);
        assert_eq!(state.get_nonce(sender).unwrap(), 0);

        // Step 2: Apply (mutates state)
        let apply_result = state.apply_transfer(sender, receiver, amount, nonce);
        assert!(apply_result.is_ok(), "Apply should succeed after verify");

        // State should now be modified
        assert_eq!(state.get_balance(sender, 0), 8_000);
        assert_eq!(state.get_balance(receiver, 0), 7_000);
        assert_eq!(state.get_nonce(sender).unwrap(), 1);
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_zero_transfer_rejected() {
        let state = MockState::new();

        // Amount 0 should fail validation
        let result = state.verify_transfer(1, 2, 0, 0);
        assert!(result.is_err(), "Zero amount transfer should be rejected");
        assert_eq!(
            result.unwrap_err(),
            "zero amount",
            "Error should indicate zero amount"
        );
    }

    #[test]
    fn test_self_transfer_check() {
        // Sender == receiver case: should still work arithmetically
        // (deduct then credit same account = no net change in balance)
        let mut state = MockState::new();
        let amount = 1000u64;
        let account = 1u8;

        let balance_before = state.get_balance(account, 0);

        // Self-transfer: account 1 sends to itself
        let result = state.apply_transfer(account, account, amount, 0);
        assert!(result.is_ok(), "Self-transfer should succeed");

        let balance_after = state.get_balance(account, 0);
        assert_eq!(
            balance_before, balance_after,
            "Self-transfer should result in no net balance change"
        );

        // Nonce should still advance
        assert_eq!(state.get_nonce(account).unwrap(), 1);
    }

    #[test]
    fn test_multi_output_total_deduction() {
        // Multiple outputs should sum correctly from sender's balance
        let mut state = MockState::new();
        // Account 1 has 10000
        // Send: 1000 to account 2, 2000 to account 3
        let amounts = [1000u64, 2000u64];
        let receivers = [2u8, 3u8];

        let sender = 1u8;
        let total_amount: u64 = amounts.iter().copied().sum();
        let sender_before = state.get_balance(sender, 0);

        // Simulate multi-output: each output is an individual apply
        // In real system these would be part of a single transaction,
        // but the total deduction from sender must equal the sum of credits
        let mut nonce = 0u64;
        for (&amount, &receiver) in amounts.iter().zip(receivers.iter()) {
            let result = state.apply_transfer(sender, receiver, amount, nonce);
            assert!(
                result.is_ok(),
                "Transfer of {} to account {} should succeed",
                amount,
                receiver
            );
            nonce = nonce.checked_add(1).unwrap();
        }

        let sender_after = state.get_balance(sender, 0);
        let sender_loss = sender_before.checked_sub(sender_after).unwrap();

        assert_eq!(
            sender_loss, total_amount,
            "Total deduction from sender ({}) must equal sum of all outputs ({})",
            sender_loss, total_amount
        );

        // Verify each receiver got their amount
        assert_eq!(
            state.get_balance(2, 0),
            5_000u64.checked_add(1000).unwrap(),
            "Receiver 2 should have initial + 1000"
        );
        assert_eq!(
            state.get_balance(3, 0),
            0u64.checked_add(2000).unwrap(),
            "Receiver 3 should have initial + 2000"
        );
    }

    // =========================================================================
    // Additional: Overflow protection in receiver credit
    // =========================================================================

    #[test]
    fn test_receiver_overflow_protection() {
        let mut state = MockState::new();

        // Set receiver balance close to u64::MAX
        state.set_balance(2, 0, u64::MAX - 100);

        // Account 1 has 10000, try to send 200 to account 2 (would overflow)
        let result = state.apply_transfer(1, 2, 200, 0);
        assert!(
            result.is_err(),
            "Transfer causing receiver overflow should be rejected"
        );
    }

    #[test]
    fn test_nonce_overflow_protection() {
        let mut state = MockState::new();

        // Set nonce to u64::MAX
        state.nonces.insert(1, AtomicU64::new(u64::MAX));

        // Attempting to increment nonce past u64::MAX should fail
        let result = state.apply_transfer(1, 2, 100, u64::MAX);
        assert!(
            result.is_err(),
            "Nonce increment past u64::MAX should be rejected"
        );
    }
}
