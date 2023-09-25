// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::collections::BTreeMap;

#[test]
fn test_signed_values() {
    let mut validators = BTreeMap::new();
    let (a1, sec1) = get_key_pair();
    let (a2, sec2) = get_key_pair();
    let (a3, sec3) = get_key_pair();

    validators.insert(/* address */ a1, /* voting right */ 1);
    validators.insert(/* address */ a2, /* voting right */ 0);
    let validators = Validators::new(validators);

    let transfer = Transfer {
        sender: a1,
        recipient: a2,
        amount: Amount::from(1),
        nonce: Nonce::new(),
        user_data: UserData::default(),
    };
    let tx = Transaction::new(transfer.clone(), &sec1);
    let bad_tx = Transaction::new(transfer, &sec2);

    let v = SignedTransaction::new(tx.clone(), a1, &sec1);
    assert!(v.check(&validators).is_ok());

    let v = SignedTransaction::new(tx.clone(), a2, &sec2);
    assert!(v.check(&validators).is_err());

    let v = SignedTransaction::new(tx, a3, &sec3);
    assert!(v.check(&validators).is_err());

    let v = SignedTransaction::new(bad_tx, a1, &sec1);
    assert!(v.check(&validators).is_err());
}

#[test]
fn test_certificates() {
    let (a1, sec1) = get_key_pair();
    let (a2, sec2) = get_key_pair();
    let (a3, sec3) = get_key_pair();

    let mut validators = BTreeMap::new();
    validators.insert(/* address */ a1, /* voting right */ 1);
    validators.insert(/* address */ a2, /* voting right */ 1);
    let validators = Validators::new(validators);

    let transfer = Transfer {
        sender: a1,
        recipient: a2,
        amount: Amount::from(1),
        nonce: Nonce::new(),
        user_data: UserData::default(),
    };
    let tx = Transaction::new(transfer.clone(), &sec1);
    let bad_tx = Transaction::new(transfer, &sec2);

    let v1 = SignedTransaction::new(tx.clone(), a1, &sec1);
    let v2 = SignedTransaction::new(tx.clone(), a2, &sec2);
    let v3 = SignedTransaction::new(tx.clone(), a3, &sec3);

    let mut builder = SignatureAggregator::try_new(tx.clone(), &validators).unwrap();
    assert!(builder
        .append(v1.validator, v1.signature)
        .unwrap()
        .is_none());
    let mut c = builder.append(v2.validator, v2.signature).unwrap().unwrap();
    assert!(c.check(&validators).is_ok());
    c.signatures.pop();
    assert!(c.check(&validators).is_err());

    let mut builder = SignatureAggregator::try_new(tx, &validators).unwrap();
    assert!(builder
        .append(v1.validator, v1.signature)
        .unwrap()
        .is_none());
    assert!(builder.append(v3.validator, v3.signature).is_err());

    assert!(SignatureAggregator::try_new(bad_tx, &validators).is_err());
}
