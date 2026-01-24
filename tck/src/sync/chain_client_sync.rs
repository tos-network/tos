// Layer 1.5: ChainClient Chain & Sync Tests
//
// Tests chain-building, state consistency, and finality on a single-node
// ChainClient, filling the gap between Layer 1 (pure component) and Layer 3
// (multi-node LocalTosNetwork).

#[cfg(test)]
mod tests {
    use crate::tier1_5::block_warp::BlockWarp;
    use crate::tier1_5::{
        AutoMineConfig, ChainClient, ChainClientConfig, ConfirmationDepth, GenesisAccount,
        TransactionError,
    };
    use crate::tier1_component::TestTransaction;
    use tos_common::crypto::Hash;

    fn addr(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    fn tx_hash(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    // 1. mine_blocks(10) -> topoheight == 10
    #[tokio::test]
    async fn test_mine_blocks_advances_topoheight() {
        let alice = addr(1);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();
        assert_eq!(client.topoheight(), 0);

        let hashes = client.mine_blocks(10).await.unwrap();
        assert_eq!(hashes.len(), 10);
        assert_eq!(client.topoheight(), 10);

        // All block hashes should be unique
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(hashes[i], hashes[j]);
            }
        }
    }

    // 2. warp(50) ok, warp(30) -> TargetBehindCurrent error
    #[tokio::test]
    async fn test_warp_to_topoheight_and_reject_backward() {
        let alice = addr(1);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();

        // Forward warp succeeds
        client.warp_to_topoheight(50).await.unwrap();
        assert_eq!(client.current_topoheight(), 50);

        // Backward warp fails
        let result = client.warp_to_topoheight(30).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                crate::tier1_5::block_warp::WarpError::TargetBehindCurrent {
                    target: 30,
                    current: 50
                }
            ),
            "Expected TargetBehindCurrent, got: {:?}",
            err
        );

        // Topoheight unchanged after failed warp
        assert_eq!(client.current_topoheight(), 50);
    }

    // 3. 5 txs in 5 blocks -> nonce = 5, stale nonce rejected
    #[tokio::test]
    async fn test_nonce_monotonic_across_blocks() {
        let alice = addr(1);
        let bob = addr(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        // Process 5 transactions in 5 separate blocks
        for nonce in 1..=5u64 {
            let tx = TestTransaction {
                hash: tx_hash(90 + nonce as u8),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 100,
                fee: 10,
                nonce,
            };
            let result = client.process_transaction(tx).await.unwrap();
            assert!(result.success, "tx with nonce {} failed", nonce);
        }

        // Nonce should be 5
        let alice_nonce = client.get_nonce(&alice).await.unwrap();
        assert_eq!(alice_nonce, 5);
        assert_eq!(client.topoheight(), 5);

        // Stale nonce (3) should be rejected
        let stale_tx = TestTransaction {
            hash: tx_hash(200),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 50,
            fee: 10,
            nonce: 3,
        };
        let result = client.process_transaction(stale_tx).await.unwrap();
        assert!(!result.success);
        assert!(matches!(
            result.error,
            Some(TransactionError::InvalidNonce {
                expected: 6,
                provided: 3
            })
        ));
    }

    // 4. Process tx (nonce 1), mine, replay nonce 1 -> InvalidNonce
    #[tokio::test]
    async fn test_stale_nonce_rejected_after_advancement() {
        let alice = addr(1);
        let bob = addr(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        // Process first transaction
        let tx1 = TestTransaction {
            hash: tx_hash(99),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1000,
            fee: 10,
            nonce: 1,
        };
        let result = client.process_transaction(tx1).await.unwrap();
        assert!(result.success);

        // Replay same nonce
        let tx_replay = TestTransaction {
            hash: tx_hash(100),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 500,
            fee: 10,
            nonce: 1,
        };
        let replay_result = client.process_transaction(tx_replay).await.unwrap();
        assert!(!replay_result.success);
        assert!(matches!(
            replay_result.error,
            Some(TransactionError::InvalidNonce {
                expected: 2,
                provided: 1
            })
        ));
    }

    // 5. 3 accounts, 10 transfers, sum(balances) + fees == initial_supply
    #[tokio::test]
    async fn test_balance_conservation_across_chain() {
        let alice = addr(1);
        let bob = addr(2);
        let charlie = addr(3);

        let initial_alice: u64 = 500_000;
        let initial_bob: u64 = 300_000;
        let initial_charlie: u64 = 200_000;
        let initial_supply = initial_alice + initial_bob + initial_charlie;

        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), initial_alice))
            .with_account(GenesisAccount::new(bob.clone(), initial_bob))
            .with_account(GenesisAccount::new(charlie.clone(), initial_charlie))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        let fee: u64 = 10;
        let mut total_fees: u64 = 0;

        // 10 transfers in a round-robin pattern: alice->bob, bob->charlie, charlie->alice, ...
        let senders = [&alice, &bob, &charlie];
        let recipients = [&bob, &charlie, &alice];

        for i in 0..10u64 {
            let sender_idx = (i as usize) % 3;
            let sender = senders[sender_idx].clone();
            let recipient = recipients[sender_idx].clone();

            let nonce = client.get_nonce(&sender).await.unwrap() + 1;
            let tx = TestTransaction {
                hash: tx_hash(50 + i as u8),
                sender: sender.clone(),
                recipient,
                amount: 1000,
                fee,
                nonce,
            };

            let result = client.process_transaction(tx).await.unwrap();
            assert!(result.success, "Transfer {} failed", i);
            total_fees = total_fees.saturating_add(fee);
        }

        // Verify conservation: sum(balances) + total_fees == initial_supply
        let final_alice = client.get_balance(&alice).await.unwrap();
        let final_bob = client.get_balance(&bob).await.unwrap();
        let final_charlie = client.get_balance(&charlie).await.unwrap();

        let final_sum = final_alice
            .saturating_add(final_bob)
            .saturating_add(final_charlie);
        assert_eq!(
            final_sum + total_fees,
            initial_supply,
            "Balance conservation violated: {} + {} != {}",
            final_sum,
            total_fees,
            initial_supply
        );
    }

    // 6. process_transaction_with_depth(Stable) mines 10 extra blocks
    #[tokio::test]
    async fn test_confirmation_depth_stable_mines_extra() {
        let alice = addr(1);
        let bob = addr(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();
        assert_eq!(client.topoheight(), 0);

        let tx = TestTransaction {
            hash: tx_hash(99),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 5000,
            fee: 10,
            nonce: 1,
        };

        let result = client
            .process_transaction_with_depth(tx, ConfirmationDepth::Stable)
            .await
            .unwrap();
        assert!(result.success);

        // 1 block for the tx + 10 blocks for Stable depth
        assert_eq!(client.topoheight(), 11);
    }

    // 7. force_set_balance(50000) -> transfer 40000 succeeds
    #[tokio::test]
    async fn test_force_set_balance_enables_large_transfer() {
        let alice = addr(1);
        let bob = addr(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 100))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        // Initially alice can't send 40000
        let tx_fail = TestTransaction {
            hash: tx_hash(98),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 40_000,
            fee: 10,
            nonce: 1,
        };
        let fail_result = client.process_transaction(tx_fail).await.unwrap();
        assert!(!fail_result.success);

        // Force-set balance
        client.force_set_balance(&alice, 50_000).await.unwrap();
        let balance = client.get_balance(&alice).await.unwrap();
        assert_eq!(balance, 50_000);

        // Now transfer 40000 succeeds
        let tx_ok = TestTransaction {
            hash: tx_hash(99),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 40_000,
            fee: 10,
            nonce: 1, // Still nonce 1 since first tx failed
        };
        let ok_result = client.process_transaction(tx_ok).await.unwrap();
        assert!(ok_result.success);

        let bob_balance = client.get_balance(&bob).await.unwrap();
        assert_eq!(bob_balance, 40_000);
    }

    // 8. force_set_nonce(10) -> tx nonce 11 ok, nonce 1 rejected
    #[tokio::test]
    async fn test_force_set_nonce_changes_expected() {
        let alice = addr(1);
        let bob = addr(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        // Force-set nonce to 10
        client.force_set_nonce(&alice, 10).await.unwrap();
        let nonce = client.get_nonce(&alice).await.unwrap();
        assert_eq!(nonce, 10);

        // Nonce 1 should be rejected (expected = stored+1 = 11)
        let tx_stale = TestTransaction {
            hash: tx_hash(98),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100,
            fee: 10,
            nonce: 1,
        };
        let stale_result = client.process_transaction(tx_stale).await.unwrap();
        assert!(!stale_result.success);
        assert!(matches!(
            stale_result.error,
            Some(TransactionError::InvalidNonce {
                expected: 11,
                provided: 1
            })
        ));

        // Nonce 11 should succeed
        let tx_ok = TestTransaction {
            hash: tx_hash(99),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 100,
            fee: 10,
            nonce: 11,
        };
        let ok_result = client.process_transaction(tx_ok).await.unwrap();
        assert!(ok_result.success);

        let final_nonce = client.get_nonce(&alice).await.unwrap();
        assert_eq!(final_nonce, 11);
    }

    // 9. mine 3 blocks -> get_tips() contains last hash
    #[tokio::test]
    async fn test_get_tips_returns_latest_block() {
        let alice = addr(1);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();

        let hashes = client.mine_blocks(3).await.unwrap();
        let last_hash = hashes.last().unwrap().clone();

        let tips = client.get_tips().await.unwrap();
        assert!(
            tips.contains(&last_hash),
            "Tips {:?} does not contain last mined block {:?}",
            tips,
            last_hash
        );
    }

    // 10. mine blocks -> get_block_at_topoheight(N) -> correct BlockInfo
    #[tokio::test]
    async fn test_block_at_topoheight_returns_correct_info() {
        let alice = addr(1);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();

        // Mine 5 blocks
        let hashes = client.mine_blocks(5).await.unwrap();

        // Query each block by topoheight
        for (i, expected_hash) in hashes.iter().enumerate() {
            let topo = (i as u64) + 1; // blocks are at topo 1..=5
            let block_info = client.get_block_at_topoheight(topo).await.unwrap();
            assert_eq!(
                block_info.hash, *expected_hash,
                "Block at topo {} has wrong hash",
                topo
            );
            assert_eq!(block_info.topoheight, topo);
        }

        // Query beyond chain should fail
        let err = client.get_block_at_topoheight(100).await;
        assert!(err.is_err());
    }

    // 11. unknown addr -> balance 0, after transfer -> balance > 0 and account exists
    #[tokio::test]
    async fn test_account_exists_after_first_receive() {
        let alice = addr(1);
        let unknown = addr(99);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        // Unknown address has zero balance before any transfer
        let balance_before = client.get_balance(&unknown).await.unwrap();
        assert_eq!(balance_before, 0);

        // Transfer to unknown address creates the account
        let tx = TestTransaction {
            hash: tx_hash(99),
            sender: alice.clone(),
            recipient: unknown.clone(),
            amount: 1000,
            fee: 10,
            nonce: 1,
        };
        let result = client.process_transaction(tx).await.unwrap();
        assert!(result.success);

        // Now the address has a balance and account_exists returns true
        let exists_after = client.account_exists(&unknown).await.unwrap();
        assert!(exists_after);

        let balance_after = client.get_balance(&unknown).await.unwrap();
        assert_eq!(balance_after, 1000);
    }

    // 12. A->B (block1), B->C (block2), verify intermediate balances
    #[tokio::test]
    async fn test_chain_of_custody_sequential_transfers() {
        let alice = addr(1);
        let bob = addr(2);
        let charlie = addr(3);

        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 100_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_account(GenesisAccount::new(charlie.clone(), 0))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        // Block 1: Alice -> Bob (50000)
        let tx1 = TestTransaction {
            hash: tx_hash(90),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 50_000,
            fee: 10,
            nonce: 1,
        };
        let r1 = client.process_transaction(tx1).await.unwrap();
        assert!(r1.success);
        assert_eq!(client.topoheight(), 1);

        // Intermediate check
        let bob_mid = client.get_balance(&bob).await.unwrap();
        assert_eq!(bob_mid, 50_000);
        let charlie_mid = client.get_balance(&charlie).await.unwrap();
        assert_eq!(charlie_mid, 0);

        // Block 2: Bob -> Charlie (30000)
        let tx2 = TestTransaction {
            hash: tx_hash(91),
            sender: bob.clone(),
            recipient: charlie.clone(),
            amount: 30_000,
            fee: 10,
            nonce: 1, // Bob's first tx, nonce = stored(0)+1 = 1
        };
        let r2 = client.process_transaction(tx2).await.unwrap();
        assert!(r2.success);
        assert_eq!(client.topoheight(), 2);

        // Final state
        let alice_final = client.get_balance(&alice).await.unwrap();
        let bob_final = client.get_balance(&bob).await.unwrap();
        let charlie_final = client.get_balance(&charlie).await.unwrap();

        assert_eq!(alice_final, 100_000 - 50_000 - 10);
        assert_eq!(bob_final, 50_000 - 30_000 - 10);
        assert_eq!(charlie_final, 30_000);
    }

    // 13. process tx -> get_tx_result(hash) -> matches original result
    #[tokio::test]
    async fn test_get_tx_result_returns_historical() {
        let alice = addr(1);
        let bob = addr(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        let tx = TestTransaction {
            hash: tx_hash(42),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 7777,
            fee: 50,
            nonce: 1,
        };
        let tx_hash_val = tx.hash.clone();

        let original_result = client.process_transaction(tx).await.unwrap();
        assert!(original_result.success);

        // Retrieve from tx log
        let historical = client.get_tx_result(&tx_hash_val).await;
        assert!(historical.is_some());

        let historical = historical.unwrap();
        assert_eq!(historical.success, original_result.success);
        assert_eq!(historical.tx_hash, original_result.tx_hash);
        assert_eq!(historical.block_hash, original_result.block_hash);
        assert_eq!(historical.topoheight, original_result.topoheight);
        assert_eq!(historical.gas_used, original_result.gas_used);
        assert_eq!(historical.new_nonce, original_result.new_nonce);

        // Non-existent hash returns None
        let missing = client.get_tx_result(&addr(255)).await;
        assert!(missing.is_none());
    }
}
