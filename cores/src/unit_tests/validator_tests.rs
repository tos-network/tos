// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn test_handle_transfer_tx_bad_signature() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));
    let transfer_tx = init_transfer_tx(sender, &sender_key, recipient, Amount::from(5));
    let (_unknown_address, unknown_key) = get_key_pair();
    let mut bad_signature_transfer_tx = transfer_tx.clone();
    bad_signature_transfer_tx.signature = Signature::new(&transfer_tx.transfer, &unknown_key);
    assert!(validator_state
        .handle_transfer_tx(bad_signature_transfer_tx)
        .is_err());
    assert!(validator_state
        .accounts
        .get(&sender)
        .unwrap()
        .pending_confirmation
        .is_none());
}

#[test]
fn test_handle_transfer_tx_zero_amount() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));
    let transfer_tx = init_transfer_tx(sender, &sender_key, recipient, Amount::from(5));

    // test transfer non-positive amount
    let mut zero_amount_transfer = transfer_tx.transfer;
    zero_amount_transfer.amount = Amount::zero();
    let zero_amount_transfer_tx = Transaction::new(zero_amount_transfer, &sender_key);
    assert!(validator_state
        .handle_transfer_tx(zero_amount_transfer_tx)
        .is_err());
    assert!(validator_state
        .accounts
        .get(&sender)
        .unwrap()
        .pending_confirmation
        .is_none());
}

#[test]
fn test_handle_transfer_tx_unknown_sender() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));
    let transfer_tx = init_transfer_tx(sender, &sender_key, recipient, Amount::from(5));
    let (unknown_address, unknown_key) = get_key_pair();

    let mut unknown_sender_transfer = transfer_tx.transfer;
    unknown_sender_transfer.sender = unknown_address;
    let unknown_sender_transfer_tx = Transaction::new(unknown_sender_transfer, &unknown_key);
    assert!(validator_state
        .handle_transfer_tx(unknown_sender_transfer_tx)
        .is_err());
    assert!(validator_state
        .accounts
        .get(&sender)
        .unwrap()
        .pending_confirmation
        .is_none());
}

#[test]
fn test_handle_transfer_tx_bad_nonce() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let validator_state = init_state_with_account(sender, Balance::from(5));
    let transfer_tx = init_transfer_tx(sender, &sender_key, recipient, Amount::from(5));

    let mut nonce_state = validator_state;
    let nonce_state_sender_account =
        nonce_state.accounts.get_mut(&sender).unwrap();
    nonce_state_sender_account.nonce =
        nonce_state_sender_account
            .nonce
            .increment()
            .unwrap();
    assert!(nonce_state
        .handle_transfer_tx(transfer_tx)
        .is_err());
    assert!(nonce_state
        .accounts
        .get(&sender)
        .unwrap()
        .pending_confirmation
        .is_none());
}

#[test]
fn test_handle_transfer_tx_exceed_balance() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));
    let transfer_tx = init_transfer_tx(sender, &sender_key, recipient, Amount::from(1000));
    assert!(validator_state
        .handle_transfer_tx(transfer_tx)
        .is_err());
    assert!(validator_state
        .accounts
        .get(&sender)
        .unwrap()
        .pending_confirmation
        .is_none());
}

#[test]
fn test_handle_transfer_tx_ok() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));
    let transfer_tx = init_transfer_tx(sender, &sender_key, recipient, Amount::from(5));

    let account_info = validator_state
        .handle_transfer_tx(transfer_tx)
        .unwrap();
    let pending_confirmation = validator_state
        .accounts
        .get(&sender)
        .unwrap()
        .pending_confirmation
        .clone()
        .unwrap();
    assert_eq!(
        account_info.pending_confirmation.unwrap(),
        pending_confirmation
    );
}

#[test]
fn test_handle_transfer_tx_double_spend() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));
    let transfer_tx = init_transfer_tx(sender, &sender_key, recipient, Amount::from(5));

    let signed_tx = validator_state
        .handle_transfer_tx(transfer_tx.clone())
        .unwrap();
    let double_spend_signed_tx = validator_state
        .handle_transfer_tx(transfer_tx)
        .unwrap();
    assert_eq!(signed_tx, double_spend_signed_tx);
}

#[test]
fn test_handle_confirmation_tx_unknown_sender() {
    let recipient = dbg_addr(2);
    let (sender, sender_key) = get_key_pair();
    let mut validator_state = init_state();
    let certified_transfer_tx = init_certified_transfer_tx(
        sender,
        &sender_key,
        recipient,
        Amount::from(5),
        &validator_state,
    );

    assert!(validator_state
        .handle_confirmation_tx(ConfirmationTx::new(certified_transfer_tx))
        .is_ok());
    assert!(validator_state.accounts.get(&recipient).is_some());
}

#[test]
fn test_handle_confirmation_tx_bad_nonce() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));
    let sender_account = validator_state.accounts.get_mut(&sender).unwrap();
    sender_account.nonce = sender_account.nonce.increment().unwrap();
    // let old_account = sender_account;

    let old_balance;
    let old_seq_num;
    {
        let old_account = validator_state.accounts.get_mut(&sender).unwrap();
        old_balance = old_account.balance;
        old_seq_num = old_account.nonce;
    }

    let certified_transfer_tx = init_certified_transfer_tx(
        sender,
        &sender_key,
        recipient,
        Amount::from(5),
        &validator_state,
    );
    // Replays are ignored.
    assert!(validator_state
        .handle_confirmation_tx(ConfirmationTx::new(certified_transfer_tx))
        .is_ok());
    let new_account = validator_state.accounts.get_mut(&sender).unwrap();
    assert_eq!(old_balance, new_account.balance);
    assert_eq!(old_seq_num, new_account.nonce);
    assert_eq!(new_account.confirmed_log, Vec::new());
    assert!(validator_state.accounts.get(&recipient).is_none());
}

#[test]
fn test_handle_confirmation_tx_exceed_balance() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));

    let certified_transfer_tx = init_certified_transfer_tx(
        sender,
        &sender_key,
        recipient,
        Amount::from(1000),
        &validator_state,
    );
    assert!(validator_state
        .handle_confirmation_tx(ConfirmationTx::new(certified_transfer_tx))
        .is_ok());
    let new_account = validator_state.accounts.get(&sender).unwrap();
    assert_eq!(Balance::from(-995), new_account.balance);
    assert_eq!(Nonce::from(1), new_account.nonce);
    assert_eq!(new_account.confirmed_log.len(), 1);
    assert!(validator_state.accounts.get(&recipient).is_some());
}

#[test]
fn test_handle_confirmation_tx_receiver_balance_overflow() {
    let (sender, sender_key) = get_key_pair();
    let (recipient, _) = get_key_pair();
    let mut validator_state = init_state_with_accounts(vec![
        (sender, Balance::from(1)),
        (recipient, Balance::max()),
    ]);

    let certified_transfer_tx = init_certified_transfer_tx(
        sender,
        &sender_key,
        recipient,
        Amount::from(1),
        &validator_state,
    );
    assert!(validator_state
        .handle_confirmation_tx(ConfirmationTx::new(certified_transfer_tx))
        .is_ok());
    let new_sender_account = validator_state.accounts.get(&sender).unwrap();
    assert_eq!(Balance::from(0), new_sender_account.balance);
    assert_eq!(
        Nonce::from(1),
        new_sender_account.nonce
    );
    assert_eq!(new_sender_account.confirmed_log.len(), 1);
    let new_recipient_account = validator_state.accounts.get(&recipient).unwrap();
    assert_eq!(Balance::max(), new_recipient_account.balance);
}

#[test]
fn test_handle_confirmation_tx_receiver_equal_sender() {
    let (address, key) = get_key_pair();
    let mut validator_state = init_state_with_account(address, Balance::from(1));

    let certified_transfer_tx = init_certified_transfer_tx(
        address,
        &key,
        address,
        Amount::from(10),
        &validator_state,
    );
    assert!(validator_state
        .handle_confirmation_tx(ConfirmationTx::new(certified_transfer_tx))
        .is_ok());
    let account = validator_state.accounts.get(&address).unwrap();
    assert_eq!(Balance::from(1), account.balance);
    assert_eq!(Nonce::from(1), account.nonce);
    assert_eq!(account.confirmed_log.len(), 1);
}

#[test]
fn test_handle_cross_shard_recipient_commit() {
    let (sender, sender_key) = get_key_pair();
    let (recipient, _) = get_key_pair();
    // Sender has no account on this shard.
    let mut validator_state = init_state_with_account(recipient, Balance::from(1));
    let certified_transfer_tx = init_certified_transfer_tx(
        sender,
        &sender_key,
        recipient,
        Amount::from(10),
        &validator_state,
    );
    assert!(validator_state
        .handle_cross_shard_recipient_commit(certified_transfer_tx)
        .is_ok());
    let account = validator_state.accounts.get(&recipient).unwrap();
    assert_eq!(Balance::from(11), account.balance);
    assert_eq!(Nonce::from(0), account.nonce);
    assert_eq!(account.confirmed_log.len(), 0);
}

#[test]
fn test_handle_confirmation_tx_ok() {
    let (sender, sender_key) = get_key_pair();
    let recipient = dbg_addr(2);
    let mut validator_state = init_state_with_account(sender, Balance::from(5));
    let certified_transfer_tx = init_certified_transfer_tx(
        sender,
        &sender_key,
        recipient,
        Amount::from(5),
        &validator_state,
    );

    let old_account = validator_state.accounts.get_mut(&sender).unwrap();
    let mut nonce = old_account.nonce;
    nonce = nonce.increment().unwrap();
    let mut remaining_balance = old_account.balance;
    remaining_balance = remaining_balance
        .try_sub(certified_transfer_tx.value.transfer.amount.into())
        .unwrap();

    let (info, _) = validator_state
        .handle_confirmation_tx(ConfirmationTx::new(certified_transfer_tx.clone()))
        .unwrap();
    assert_eq!(sender, info.sender);
    assert_eq!(remaining_balance, info.balance);
    assert_eq!(nonce, info.nonce);
    assert_eq!(None, info.pending_confirmation);
    assert_eq!(
        validator_state.accounts.get(&sender).unwrap().confirmed_log,
        vec![certified_transfer_tx.clone()]
    );

    let recipient_account = validator_state.accounts.get(&recipient).unwrap();
    assert_eq!(
        recipient_account.balance,
        certified_transfer_tx.value.transfer.amount.into()
    );

    let info_request = AccountInfoRequest {
        sender: recipient,
        request_nonce: None,
        request_received_transfers_excluding_first_nth: Some(0),
    };
    let response = validator_state
        .handle_account_info_request(info_request)
        .unwrap();
    assert_eq!(response.requested_received_transfers.len(), 1);
    assert_eq!(
        response.requested_received_transfers[0]
            .value
            .transfer
            .amount,
        Amount::from(5)
    );
}

#[test]
fn test_handle_primary_synchronization_tx_update() {
    let mut state = init_state();
    let mut updated_transaction_index = state.last_transaction_index;
    let address = dbg_addr(1);
    let tx = init_primary_synchronization_tx(address);

    assert!(state
        .handle_primary_synchronization_tx(tx.clone())
        .is_ok());
    updated_transaction_index = updated_transaction_index.increment().unwrap();
    assert_eq!(state.last_transaction_index, updated_transaction_index);
    let account = state.accounts.get(&address).unwrap();
    assert_eq!(account.balance, tx.amount.into());
    assert_eq!(state.accounts.len(), 1);
}

#[test]
fn test_handle_primary_synchronization_tx_double_spend() {
    let mut state = init_state();
    let mut updated_transaction_index = state.last_transaction_index;
    let address = dbg_addr(1);
    let tx = init_primary_synchronization_tx(address);

    assert!(state
        .handle_primary_synchronization_tx(tx.clone())
        .is_ok());
    updated_transaction_index = updated_transaction_index.increment().unwrap();
    // Replays are ignored.
    assert!(state
        .handle_primary_synchronization_tx(tx.clone())
        .is_ok());
    assert_eq!(state.last_transaction_index, updated_transaction_index);
    let account = state.accounts.get(&address).unwrap();
    assert_eq!(account.balance, tx.amount.into());
    assert_eq!(state.accounts.len(), 1);
}

#[test]
fn test_account_state_ok() {
    let sender = dbg_addr(1);
    let validator_state = init_state_with_account(sender, Balance::from(5));
    assert_eq!(
        validator_state.accounts.get(&sender).unwrap(),
        validator_state.account_state(&sender).unwrap()
    );
}

#[test]
fn test_account_state_unknown_account() {
    let sender = dbg_addr(1);
    let unknown_address = dbg_addr(99);
    let validator_state = init_state_with_account(sender, Balance::from(5));
    assert!(validator_state.account_state(&unknown_address).is_err());
}

#[test]
fn test_get_shards() {
    let shards = 16u32;
    let mut found = vec![false; shards as usize];
    let mut left = shards;
    loop {
        let (address, _) = get_key_pair();
        let shard = ValidatorState::get_shard(shards, &address) as usize;
        println!("found {}", shard);
        if !found[shard] {
            found[shard] = true;
            left -= 1;
            if left == 0 {
                break;
            }
        }
    }
}

// helpers

#[cfg(test)]
fn init_state() -> ValidatorState {
    let (validator_address, validator_key) = get_key_pair();
    let mut validators = BTreeMap::new();
    validators.insert(
        /* address */ validator_address,
        /* voting right */ 1,
    );
    let validators = Validators::new(validators);
    ValidatorState::new(validators, validator_address, validator_key)
}

#[cfg(test)]
fn init_state_with_accounts<I: IntoIterator<Item = (Address, Balance)>>(
    balances: I,
) -> ValidatorState {
    let mut state = init_state();
    for (address, balance) in balances {
        let account = state
            .accounts
            .entry(address)
            .or_insert_with(AccountOffchainState::new);
        account.balance = balance;
    }
    state
}

#[cfg(test)]
fn init_state_with_account(address: Address, balance: Balance) -> ValidatorState {
    init_state_with_accounts(std::iter::once((address, balance)))
}

#[cfg(test)]
fn init_transfer_tx(
    sender: Address,
    secret: &KeyPair,
    recipient: Address,
    amount: Amount,
) -> Transaction {
    let transfer = Transfer {
        sender,
        recipient,
        amount,
        nonce: Nonce::new(),
        user_data: UserData::default(),
    };
    Transaction::new(transfer, secret)
}

#[cfg(test)]
fn init_certified_transfer_tx(
    sender: Address,
    secret: &KeyPair,
    recipient: Address,
    amount: Amount,
    validator_state: &ValidatorState,
) -> CertifiedTransaction {
    let transfer_tx = init_transfer_tx(sender, secret, recipient, amount);
    let vote = SignedTransaction::new(
        transfer_tx.clone(),
        validator_state.name,
        &validator_state.secret,
    );
    let mut builder =
        SignatureAggregator::try_new(transfer_tx, &validator_state.validators).unwrap();
    builder
        .append(vote.validator, vote.signature)
        .unwrap()
        .unwrap()
}

#[cfg(test)]
fn init_primary_synchronization_tx(recipient: Address) -> PrimarySynchronizationTx {
    let mut transaction_index = VersionNumber::new();
    transaction_index = transaction_index.increment().unwrap();
    PrimarySynchronizationTx {
        recipient,
        amount: Amount::from(5),
        transaction_index,
    }
}
