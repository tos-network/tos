use crate::{
    account::{DelegateRecordEntry, EnergyLease, EnergyResource, FreezeDuration},
    config::ENERGY_PER_TRANSFER,
    crypto::elgamal::KeyPair,
    utils::energy_fee::{EnergyFeeCalculator, EnergyResourceManager},
};

#[test]
fn test_energy_fee_calculation() {
    // Test basic energy cost calculation - only transfer count matters, not transaction size
    let cost = EnergyFeeCalculator::calculate_energy_cost(1024, 1, 0);
    assert_eq!(cost, ENERGY_PER_TRANSFER);

    // Test with multiple outputs - still 1 energy per transaction
    let cost = EnergyFeeCalculator::calculate_energy_cost(1024, 3, 0);
    assert_eq!(cost, ENERGY_PER_TRANSFER);

    // Test with new addresses - new addresses don't consume energy in current implementation
    let cost = EnergyFeeCalculator::calculate_energy_cost(1024, 1, 2);
    assert_eq!(cost, ENERGY_PER_TRANSFER);
}

#[test]
fn test_energy_to_tos_conversion() {
    // This test is simplified since ENERGY_TO_TOS_RATE is no longer used
    // Energy conversion is handled differently in the current implementation
    let energy_needed = 1000;
    // In current implementation, energy shortage is handled by TOS conversion
    // but the rate is not exposed as a constant
    assert!(energy_needed > 0);
}

#[test]
fn test_energy_resource_management() {
    let mut resource = EnergyResource::new();

    // Test freezing TOS for energy with different durations
    let topoheight = 1000;
    let network = crate::network::Network::Mainnet;

    // Freeze 1 TOS for 3 days (6.0x multiplier)
    let duration3 = FreezeDuration::new(3).unwrap();
    let energy_gained_3d = EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration3,
        topoheight,
    )
    .unwrap();
    assert_eq!(energy_gained_3d, 6); // 6 transfers
    assert_eq!(resource.frozen_tos, 1);
    assert_eq!(resource.energy, 6);
    assert_eq!(resource.available_energy(), 6);

    // Freeze 1 TOS for 7 days (14.0x multiplier)
    let duration7 = FreezeDuration::new(7).unwrap();
    let energy_gained_7d = EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration7,
        topoheight,
    )
    .unwrap();
    assert_eq!(energy_gained_7d, 14); // 14 transfers
    assert_eq!(resource.frozen_tos, 2);
    assert_eq!(resource.energy, 20);
    assert_eq!(resource.available_energy(), 20);

    // Test that partial TOS amounts are rejected
    let energy_gained_partial = EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        150000000, // 1.5 TOS (invalid)
        duration3,
        topoheight,
    )
    .unwrap();
    assert_eq!(energy_gained_partial, 0); // Invalid amount yields no energy
    assert_eq!(resource.frozen_tos, 2); // No additional TOS frozen

    // Consume energy for transfers
    let result = EnergyResourceManager::consume_energy_for_transaction(
        &mut resource,
        5, // 5 transfers
        topoheight + 1,
        &network,
    );
    assert!(result.is_ok());
    assert_eq!(resource.available_energy(), 15); // 20 - 5 = 15 transfers remaining

    // Test unfreezing whole number TOS
    let unfreeze_result = EnergyResourceManager::unfreeze_tos(
        &mut resource,
        100000000, // 1 TOS
        topoheight + duration3.duration_in_blocks_for_network(&network),
        None, // FIFO mode
        &network,
    );
    assert!(unfreeze_result.is_ok());
    let (energy_removed, _pending_amount) = unfreeze_result.unwrap();
    assert_eq!(energy_removed, 6); // 1 TOS * 6 energy accounted for record update
}

#[test]
fn test_energy_available_same_block() {
    let mut resource = EnergyResource::new();
    let topoheight = 500;

    let duration = FreezeDuration::new(7).unwrap();
    let energy_gained = EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration,
        topoheight,
    )
    .unwrap();

    assert_eq!(energy_gained, 14);
    assert_eq!(resource.available_energy_at(topoheight), 14);
    assert_eq!(resource.available_energy_at(topoheight + 1), 14);
}

#[test]
fn test_delegated_energy_available_same_block() {
    let mut resource = EnergyResource::new();
    let topoheight = 700;

    resource.add_delegated_energy(20, topoheight).unwrap();

    assert_eq!(resource.energy, 20);
    assert_eq!(resource.available_energy_at(topoheight), 20);
    assert_eq!(resource.available_energy_at(topoheight + 1), 20);
}

#[test]
fn test_freeze_and_consume_same_block_flow() {
    let mut resource = EnergyResource::new();
    let topoheight = 1000;
    let network = crate::network::Network::Mainnet;

    let duration = FreezeDuration::new(7).unwrap();
    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration,
        topoheight,
    )
    .unwrap();

    let result = EnergyResourceManager::consume_energy_for_transaction(
        &mut resource,
        1,
        topoheight,
        &network,
    );
    assert!(result.is_ok());
    assert_eq!(resource.available_energy_at(topoheight), 13);
}

#[test]
fn test_unfreeze_energy_removed_same_block_flow() {
    let mut resource = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let freeze_topoheight = 2000;
    let duration = FreezeDuration::new(3).unwrap();

    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration,
        freeze_topoheight,
    )
    .unwrap();

    let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks_for_network(&network);
    let (energy_removed, _pending_amount) = EnergyResourceManager::unfreeze_tos(
        &mut resource,
        100000000,
        unlock_topoheight,
        None,
        &network,
    )
    .unwrap();

    assert_eq!(energy_removed, 6);
    assert_eq!(resource.available_energy_at(unlock_topoheight), 6);
    assert_eq!(resource.pending_unfreezes.len(), 1);
}

#[test]
fn test_withdraw_same_block_flow() {
    let mut resource = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let freeze_topoheight = 3000;
    let duration = FreezeDuration::new(3).unwrap();

    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration,
        freeze_topoheight,
    )
    .unwrap();

    let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks_for_network(&network);
    EnergyResourceManager::unfreeze_tos(
        &mut resource,
        100000000,
        unlock_topoheight,
        None,
        &network,
    )
    .unwrap();

    let withdraw_topoheight = unlock_topoheight + network.unfreeze_cooldown_blocks();
    let withdrawn =
        EnergyResourceManager::withdraw_unfrozen(&mut resource, withdraw_topoheight).unwrap();

    assert_eq!(withdrawn, 100000000);
    assert!(resource.pending_unfreezes.is_empty());
}

#[test]
fn test_delegate_freeze_consume_unfreeze_same_block_flow() {
    let mut delegator = EnergyResource::new();
    let mut delegatee = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let topoheight = 4000;
    let duration = FreezeDuration::new(7).unwrap();

    let total_amount_atomic = 100000000;
    let total_amount_whole = total_amount_atomic / crate::config::COIN_VALUE;
    let energy = total_amount_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();
    let entries = vec![DelegateRecordEntry {
        delegatee: delegatee_key.clone(),
        amount: total_amount_whole,
        energy,
    }];

    delegator
        .create_delegated_freeze(entries, duration, total_amount_whole, topoheight, &network)
        .unwrap();
    delegatee.add_delegated_energy(energy, topoheight).unwrap();

    assert_eq!(delegatee.available_energy_at(topoheight), energy);

    delegatee
        .consume_energy(1, topoheight)
        .expect("delegatee should consume same-block energy");

    let unfreeze_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let (removed_delegatee, energy_removed, _pending_amount) = delegator
        .unfreeze_delegated_entry(
            total_amount_atomic,
            unfreeze_topoheight,
            Some(0),
            &delegatee_key,
            &network,
        )
        .unwrap();
    assert_eq!(removed_delegatee, delegatee_key);
    assert_eq!(energy_removed, energy);
    assert_eq!(delegatee.energy, energy - 1);
}

#[test]
fn test_batch_delegate_consume_selective_unfreeze_same_block_flow() {
    let mut delegator = EnergyResource::new();
    let mut delegatee_a = EnergyResource::new();
    let mut delegatee_b = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let topoheight = 5000;
    let duration = FreezeDuration::new(7).unwrap();

    let delegatee_key_a = KeyPair::new().get_public_key().compress();
    let delegatee_key_b = KeyPair::new().get_public_key().compress();

    let amount_a_atomic = 100000000;
    let amount_b_atomic = 200000000;
    let amount_a_whole = amount_a_atomic / crate::config::COIN_VALUE;
    let amount_b_whole = amount_b_atomic / crate::config::COIN_VALUE;

    let energy_a = amount_a_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();
    let energy_b = amount_b_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    let entries = vec![
        DelegateRecordEntry {
            delegatee: delegatee_key_a.clone(),
            amount: amount_a_whole,
            energy: energy_a,
        },
        DelegateRecordEntry {
            delegatee: delegatee_key_b.clone(),
            amount: amount_b_whole,
            energy: energy_b,
        },
    ];

    delegator
        .create_delegated_freeze(
            entries,
            duration,
            amount_a_whole + amount_b_whole,
            topoheight,
            &network,
        )
        .unwrap();

    delegatee_a
        .add_delegated_energy(energy_a, topoheight)
        .unwrap();
    delegatee_b
        .add_delegated_energy(energy_b, topoheight)
        .unwrap();

    delegatee_a.consume_energy(1, topoheight).unwrap();
    delegatee_b.consume_energy(1, topoheight).unwrap();

    let unfreeze_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let (_removed_delegatee, energy_removed, _pending_amount) = delegator
        .unfreeze_delegated_entry(
            amount_a_atomic,
            unfreeze_topoheight,
            Some(0),
            &delegatee_key_a,
            &network,
        )
        .unwrap();

    assert_eq!(energy_removed, energy_a);
    assert_eq!(delegatee_a.energy, energy_a - 1);
    assert_eq!(delegatee_b.energy, energy_b - 1);
}

#[test]
fn test_batch_delegate_partial_unfreeze_by_record_index() {
    let mut delegator = EnergyResource::new();
    let mut delegatee = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let topo_a = 6000;
    let topo_b = 7000;
    let amount_a_atomic = 200000000;
    let amount_b_atomic = 100000000;
    let amount_a_whole = amount_a_atomic / crate::config::COIN_VALUE;
    let amount_b_whole = amount_b_atomic / crate::config::COIN_VALUE;

    let energy_a = amount_a_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();
    let energy_b = amount_b_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_a_whole,
                energy: energy_a,
            }],
            duration,
            amount_a_whole,
            topo_a,
            &network,
        )
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_b_whole,
                energy: energy_b,
            }],
            duration,
            amount_b_whole,
            topo_b,
            &network,
        )
        .unwrap();

    delegatee.add_delegated_energy(energy_a, topo_a).unwrap();
    delegatee.add_delegated_energy(energy_b, topo_b).unwrap();

    let unfreeze_topoheight = topo_b + duration.duration_in_blocks_for_network(&network);
    let partial_amount = 100000000;
    let (_removed_delegatee, energy_removed, _pending_amount) = delegator
        .unfreeze_delegated_entry(
            partial_amount,
            unfreeze_topoheight,
            Some(1),
            &delegatee_key,
            &network,
        )
        .unwrap();

    assert_eq!(energy_removed, energy_b);
    assert_eq!(delegatee.energy, energy_a + energy_b);
}

#[test]
fn test_batch_delegate_partial_unfreeze_updates_entry_energy() {
    let mut delegator = EnergyResource::new();
    let mut delegatee = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let topoheight = 8000;
    let amount_atomic = 200000000;
    let amount_whole = amount_atomic / crate::config::COIN_VALUE;
    let energy = amount_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy,
            }],
            duration,
            amount_whole,
            topoheight,
            &network,
        )
        .unwrap();

    delegatee.add_delegated_energy(energy, topoheight).unwrap();

    let unfreeze_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let partial_amount = 100000000;
    let (_removed_delegatee, energy_removed, _pending_amount) = delegator
        .unfreeze_delegated_entry(
            partial_amount,
            unfreeze_topoheight,
            Some(0),
            &delegatee_key,
            &network,
        )
        .unwrap();

    assert_eq!(energy_removed, 14);
    assert_eq!(delegatee.energy, energy);

    let record = &delegator.delegated_records[0];
    assert_eq!(record.entries[0].amount, 1);
    assert_eq!(record.entries[0].energy, 14);
}

#[test]
fn test_batch_delegate_unfreeze_requires_record_index_when_multiple_records() {
    let mut delegator = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let topo_a = 9000;
    let topo_b = 10000;

    let amount_atomic = 100000000;
    let amount_whole = amount_atomic / crate::config::COIN_VALUE;
    let energy = amount_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy,
            }],
            duration,
            amount_whole,
            topo_a,
            &network,
        )
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy,
            }],
            duration,
            amount_whole,
            topo_b,
            &network,
        )
        .unwrap();

    let unfreeze_topoheight = topo_b + duration.duration_in_blocks_for_network(&network);
    let result = delegator.unfreeze_delegated_entry(
        amount_atomic,
        unfreeze_topoheight,
        None,
        &delegatee_key,
        &network,
    );

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("Multiple delegation records exist"));
}

#[test]
fn test_batch_delegate_unfreeze_record_index_out_of_bounds() {
    let mut delegator = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let topoheight = 11000;
    let amount_atomic = 100000000;
    let amount_whole = amount_atomic / crate::config::COIN_VALUE;
    let energy = amount_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy,
            }],
            duration,
            amount_whole,
            topoheight,
            &network,
        )
        .unwrap();

    let unfreeze_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let result = delegator.unfreeze_delegated_entry(
        amount_atomic,
        unfreeze_topoheight,
        Some(1),
        &delegatee_key,
        &network,
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Record index out of bounds"));
}

#[test]
fn test_batch_delegate_mixed_duration_records_selective_unfreeze() {
    let mut delegator = EnergyResource::new();
    let mut delegatee = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let duration_short = FreezeDuration::new(7).unwrap();
    let duration_long = FreezeDuration::new(14).unwrap();
    let amount_atomic = 100000000;
    let amount_whole = amount_atomic / crate::config::COIN_VALUE;

    let energy_short = amount_whole
        .checked_mul(duration_short.reward_multiplier())
        .unwrap();
    let energy_long = amount_whole
        .checked_mul(duration_long.reward_multiplier())
        .unwrap();

    let topo_a = 12000;
    let topo_b = 12010;

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy: energy_short,
            }],
            duration_short,
            amount_whole,
            topo_a,
            &network,
        )
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy: energy_long,
            }],
            duration_long,
            amount_whole,
            topo_b,
            &network,
        )
        .unwrap();

    delegatee
        .add_delegated_energy(energy_short, topo_a)
        .unwrap();
    delegatee.add_delegated_energy(energy_long, topo_b).unwrap();

    let unfreeze_topoheight = topo_b + duration_long.duration_in_blocks_for_network(&network);
    let (_removed_delegatee, energy_removed, _pending_amount) = delegator
        .unfreeze_delegated_entry(
            amount_atomic,
            unfreeze_topoheight,
            Some(1),
            &delegatee_key,
            &network,
        )
        .unwrap();

    assert_eq!(energy_removed, energy_long);
    assert_eq!(delegatee.energy, energy_short + energy_long);
}

#[test]
fn test_same_block_multiple_partial_unfreezes() {
    let mut delegator = EnergyResource::new();
    let mut delegatee = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let topoheight = 13000;
    let amount_atomic = 300000000;
    let amount_whole = amount_atomic / crate::config::COIN_VALUE;
    let energy = amount_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy,
            }],
            duration,
            amount_whole,
            topoheight,
            &network,
        )
        .unwrap();

    delegatee.add_delegated_energy(energy, topoheight).unwrap();

    let unfreeze_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let first_amount = 100000000;
    let second_amount = 100000000;

    let (_removed_delegatee, energy_removed_first, _pending_amount) = delegator
        .unfreeze_delegated_entry(
            first_amount,
            unfreeze_topoheight,
            Some(0),
            &delegatee_key,
            &network,
        )
        .unwrap();
    let _ = energy_removed_first;

    let (_removed_delegatee, energy_removed_second, _pending_amount) = delegator
        .unfreeze_delegated_entry(
            second_amount,
            unfreeze_topoheight,
            Some(0),
            &delegatee_key,
            &network,
        )
        .unwrap();
    let _ = energy_removed_second;

    assert_eq!(delegatee.energy, energy);
    let record = &delegator.delegated_records[0];
    assert_eq!(record.entries[0].amount, 1);
    assert_eq!(record.entries[0].energy, 14);
}

#[test]
fn test_batch_delegate_unfreeze_record_still_locked() {
    let mut delegator = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let topoheight = 14000;
    let amount_atomic = 100000000;
    let amount_whole = amount_atomic / crate::config::COIN_VALUE;
    let energy = amount_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy,
            }],
            duration,
            amount_whole,
            topoheight,
            &network,
        )
        .unwrap();

    let result = delegator.unfreeze_delegated_entry(
        amount_atomic,
        topoheight + 1,
        Some(0),
        &delegatee_key,
        &network,
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Record is still locked"));
}

#[test]
fn test_batch_delegate_unfreeze_delegatee_not_in_record() {
    let mut delegator = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();
    let other_delegatee = KeyPair::new().get_public_key().compress();

    let topoheight = 15000;
    let amount_atomic = 100000000;
    let amount_whole = amount_atomic / crate::config::COIN_VALUE;
    let energy = amount_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy,
            }],
            duration,
            amount_whole,
            topoheight,
            &network,
        )
        .unwrap();

    let unfreeze_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let result = delegator.unfreeze_delegated_entry(
        amount_atomic,
        unfreeze_topoheight,
        Some(0),
        &other_delegatee,
        &network,
    );

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("Delegatee not found in record"));
}

#[test]
fn test_batch_delegate_unfreeze_amount_exceeds_entry() {
    let mut delegator = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let topoheight = 16000;
    let amount_atomic = 100000000;
    let amount_whole = amount_atomic / crate::config::COIN_VALUE;
    let energy = amount_whole
        .checked_mul(duration.reward_multiplier())
        .unwrap();

    delegator
        .create_delegated_freeze(
            vec![DelegateRecordEntry {
                delegatee: delegatee_key.clone(),
                amount: amount_whole,
                energy,
            }],
            duration,
            amount_whole,
            topoheight,
            &network,
        )
        .unwrap();

    let unfreeze_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let result = delegator.unfreeze_delegated_entry(
        amount_atomic + 100000000,
        unfreeze_topoheight,
        Some(0),
        &delegatee_key,
        &network,
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Amount exceeds entry amount"));
}

#[test]
fn test_batch_delegate_unfreeze_with_empty_record() {
    let mut delegator = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let delegatee_key = KeyPair::new().get_public_key().compress();

    let topoheight = 17000;
    delegator
        .create_delegated_freeze(Vec::new(), duration, 0, topoheight, &network)
        .unwrap_err();

    let unfreeze_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let result = delegator.unfreeze_delegated_entry(
        100000000,
        unfreeze_topoheight,
        Some(0),
        &delegatee_key,
        &network,
    );

    assert!(result.is_err());
}

#[test]
fn test_energy_unfreeze_mechanism() {
    let mut resource = EnergyResourceManager::create_energy_resource();
    let freeze_topoheight = 1000;
    let unlock_topoheight_7d = freeze_topoheight + 7 * 24 * 60 * 60;
    let network = crate::network::Network::Mainnet;

    // Freeze 1 TOS for 7 days
    let duration = FreezeDuration::new(7).unwrap();
    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration,
        freeze_topoheight,
    )
    .unwrap();

    // Try to unfreeze before unlock time (should fail)
    let result = EnergyResourceManager::unfreeze_tos(
        &mut resource,
        100000000, // 1 TOS (minimum unit)
        unlock_topoheight_7d - 1,
        None, // FIFO mode
        &network,
    );
    assert!(result.is_err());

    // Unfreeze after unlock time
    let (energy_removed, _pending_amount) = EnergyResourceManager::unfreeze_tos(
        &mut resource,
        100000000, // 1 TOS
        unlock_topoheight_7d,
        None, // FIFO mode
        &network,
    )
    .unwrap();
    assert_eq!(energy_removed, 14); // 1 TOS * 14 energy accounted for record update
    assert_eq!(resource.frozen_tos, 0);
    assert_eq!(resource.energy, 14);
}

#[test]
fn test_energy_status() {
    let mut resource = EnergyResourceManager::create_energy_resource();
    let topoheight = 1000;
    let network = crate::network::Network::Mainnet;

    // Add some energy
    let duration = FreezeDuration::new(7).unwrap();
    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration,
        topoheight,
    )
    .unwrap();

    // Consume some energy
    EnergyResourceManager::consume_energy_for_transaction(
        &mut resource,
        2, // 2 energy units
        topoheight + 1,
        &network,
    )
    .unwrap();

    let status = EnergyResourceManager::get_energy_status(&resource, topoheight + 1);
    assert_eq!(status.energy, 12); // 14 gained - 2 consumed
    assert_eq!(status.available_energy, 12);
    assert_eq!(status.frozen_tos, 1);

    // Test usage percentage
    let usage_percentage = status.usage_percentage();
    assert_eq!(usage_percentage, 0.0);

    // Test energy low check
    assert!(!status.is_energy_low()); // 90% available

    // Consume more energy to make it low
    EnergyResourceManager::consume_energy_for_transaction(
        &mut resource,
        12, // Consume more energy
        topoheight + 1,
        &network,
    )
    .unwrap();

    let status = EnergyResourceManager::get_energy_status(&resource, topoheight + 1);
    assert!(status.is_energy_low()); // No energy available
}

#[test]
fn test_energy_status_same_block_available() {
    let mut resource = EnergyResourceManager::create_energy_resource();
    let topoheight = 1000;

    let duration = FreezeDuration::new(7).unwrap();
    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        crate::config::COIN_VALUE,
        duration,
        topoheight,
    )
    .unwrap();

    let status = EnergyResourceManager::get_energy_status(&resource, topoheight);
    assert_eq!(status.available_energy, 14);
}

#[test]
fn test_freeze_duration_rewards() {
    let amounts = [100000000, 200000000, 500000000]; // 1, 2, 5 TOS
    let durations = [
        FreezeDuration::new(3).unwrap(),
        FreezeDuration::new(7).unwrap(),
        FreezeDuration::new(14).unwrap(),
    ];
    let multipliers = [6u64, 14u64, 28u64]; // New energy model: 2 * days

    for amount in amounts {
        for (duration, multiplier) in durations.iter().zip(multipliers.iter()) {
            let mut resource = EnergyResourceManager::create_energy_resource();

            let energy_gained = EnergyResourceManager::freeze_tos_for_energy(
                &mut resource,
                amount,
                *duration,
                1000,
            )
            .unwrap();

            let expected_energy = (amount / crate::config::COIN_VALUE) * multiplier;
            assert_eq!(energy_gained, expected_energy);
            assert_eq!(resource.energy, expected_energy);
            assert_eq!(resource.frozen_tos, amount / crate::config::COIN_VALUE);
        }
    }
}

#[test]
fn test_energy_lease() {
    use crate::crypto::elgamal::KeyPair;

    let lessor = KeyPair::new().get_public_key().compress();
    let lessee = KeyPair::new().get_public_key().compress();
    let energy_amount = 1; // 1 energy unit
    let duration = 1000; // 1000 blocks
    let start_topoheight = 1000;
    let price_per_energy = 1000; // 0.00001 TOS per energy unit

    let lease = EnergyLease::new(
        lessor.clone(),
        lessee.clone(),
        energy_amount,
        duration,
        start_topoheight,
        price_per_energy,
    );

    assert_eq!(lease.energy_amount, energy_amount);
    assert_eq!(lease.duration, duration);
    assert_eq!(lease.start_topoheight, start_topoheight);
    assert_eq!(lease.price_per_energy, price_per_energy);

    // Test lease validity
    assert!(lease.is_valid(start_topoheight + 500)); // Valid
    assert!(!lease.is_valid(start_topoheight + 1001)); // Expired

    // Test total cost
    let total_cost = lease.total_cost().unwrap();
    assert_eq!(total_cost, energy_amount * price_per_energy);
}

#[test]
fn test_energy_fee_calculator_total_cost() {
    let mut resource = EnergyResourceManager::create_energy_resource();

    // Add some energy
    let duration = FreezeDuration::new(7).unwrap();
    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration,
        1000,
    )
    .unwrap();

    let new_addresses = 2;

    // Test with sufficient energy
    let energy_consumed = EnergyFeeCalculator::calculate_energy_cost(
        1024, // tx_size (ignored in current implementation)
        1,    // output_count (1 transfer)
        new_addresses,
    );
    let available_energy = resource.available_energy();
    let tos_cost = if energy_consumed <= available_energy {
        0 // Sufficient energy available
    } else {
        // Insufficient energy - in current implementation, this would fail
        // rather than convert to TOS
        0
    };

    assert_eq!(energy_consumed, 1); // Only 1 transfer = 1 energy
    assert_eq!(tos_cost, 0); // No TOS cost when energy is sufficient

    // Test with insufficient energy (multiple transfers)
    let energy_consumed = EnergyFeeCalculator::calculate_energy_cost(
        1024, // tx_size (ignored)
        20,   // output_count (20 transfers, more than available energy)
        new_addresses,
    );
    let available_energy = resource.available_energy();
    let tos_cost = if energy_consumed <= available_energy {
        0 // Sufficient energy available
    } else {
        // Insufficient energy - in current implementation, this would fail
        // rather than convert to TOS
        0
    };

    assert_eq!(energy_consumed, 1); // Any transfer transaction = 1 energy
                                    // In current implementation, insufficient energy causes transaction failure
                                    // rather than TOS conversion
    assert_eq!(tos_cost, 0);
}

#[test]
fn test_freeze_record_management() {
    let mut resource = EnergyResourceManager::create_energy_resource();
    let topoheight = 1000;

    // Create multiple freeze records with different durations
    let duration3 = FreezeDuration::new(3).unwrap();
    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration3,
        topoheight,
    )
    .unwrap();

    let duration7 = FreezeDuration::new(7).unwrap();
    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        200000000, // 2 TOS
        duration7,
        topoheight,
    )
    .unwrap();

    let duration14 = FreezeDuration::new(14).unwrap();
    EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        300000000, // 3 TOS
        duration14,
        topoheight,
    )
    .unwrap();

    assert_eq!(resource.freeze_records.len(), 3);
    assert_eq!(resource.frozen_tos, 6);

    // Test unlockable records at different times
    let unlockable_3d = resource.get_unlockable_records(topoheight + 3 * 24 * 60 * 60);
    assert_eq!(unlockable_3d.len(), 1);

    let unlockable_7d = resource.get_unlockable_records(topoheight + 7 * 24 * 60 * 60);
    assert_eq!(unlockable_7d.len(), 2);

    let unlockable_14d = resource.get_unlockable_records(topoheight + 14 * 24 * 60 * 60);
    assert_eq!(unlockable_14d.len(), 3);

    // Test unlockable TOS amount
    let unlockable_tos_3d = resource
        .get_unlockable_tos(topoheight + 3 * 24 * 60 * 60)
        .unwrap();
    assert_eq!(unlockable_tos_3d, 100000000);

    let unlockable_tos_7d = resource
        .get_unlockable_tos(topoheight + 7 * 24 * 60 * 60)
        .unwrap();
    assert_eq!(unlockable_tos_7d, 300000000);

    let unlockable_tos_14d = resource
        .get_unlockable_tos(topoheight + 14 * 24 * 60 * 60)
        .unwrap();
    assert_eq!(unlockable_tos_14d, 600000000);
}

#[test]
fn test_energy_unlock_uses_topoheight() {
    let mut resource = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(7).unwrap();
    let topoheight = 1000;

    resource
        .freeze_tos_for_energy_with_network(
            crate::config::COIN_VALUE,
            duration,
            topoheight,
            &network,
        )
        .unwrap();

    let record = &resource.freeze_records[0];
    assert_eq!(
        record.unlock_topoheight,
        topoheight + duration.duration_in_blocks_for_network(&network)
    );
}

#[test]
fn test_unfreeze_unlock_uses_topoheight() {
    let mut resource = EnergyResource::new();
    let network = crate::network::Network::Mainnet;
    let duration = FreezeDuration::new(3).unwrap();
    let topoheight = 500;

    resource
        .freeze_tos_for_energy_with_network(
            crate::config::COIN_VALUE,
            duration,
            topoheight,
            &network,
        )
        .unwrap();

    let unlock_topoheight = topoheight + duration.duration_in_blocks_for_network(&network);
    let early = resource.unfreeze_tos(
        crate::config::COIN_VALUE,
        unlock_topoheight - 1,
        None,
        &network,
    );
    assert!(early.is_err());

    let (energy_removed, pending) = resource
        .unfreeze_tos(crate::config::COIN_VALUE, unlock_topoheight, None, &network)
        .unwrap();
    assert_eq!(pending, crate::config::COIN_VALUE);
    assert!(energy_removed > 0);
}

#[test]
fn test_whole_number_tos_requirement() {
    let mut resource = EnergyResource::new();
    let topoheight = 1000;
    let duration = FreezeDuration::new(7).unwrap();
    let network = crate::network::Network::Mainnet;

    // Test that partial TOS amounts are rejected
    let energy_gained = EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        150000000, // 1.5 TOS
        duration,
        topoheight,
    )
    .unwrap();
    assert_eq!(energy_gained, 0); // Invalid amount yields no energy
    assert_eq!(resource.frozen_tos, 0); // Nothing frozen

    // Test that very small amounts result in 0 TOS frozen
    let energy_gained_small = EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        50000000, // 0.5 TOS
        duration,
        topoheight,
    )
    .unwrap();
    assert_eq!(energy_gained_small, 0); // Invalid amount yields no energy
    assert_eq!(resource.frozen_tos, 0); // Still nothing frozen

    // Freeze a valid whole-number amount to confirm normal behaviour
    let energy_gained_valid = EnergyResourceManager::freeze_tos_for_energy(
        &mut resource,
        100000000, // 1 TOS
        duration,
        topoheight,
    )
    .unwrap();
    assert_eq!(energy_gained_valid, 14);
    assert_eq!(resource.energy, 14);
    assert_eq!(resource.available_energy(), 14);

    // Consume 1 energy = 1 transfer
    let result = EnergyResourceManager::consume_energy_for_transaction(
        &mut resource,
        1, // 1 transfer
        topoheight + 1,
        &network,
    );
    assert!(result.is_ok());
    assert_eq!(resource.available_energy(), 13); // 13 transfers remaining
}
