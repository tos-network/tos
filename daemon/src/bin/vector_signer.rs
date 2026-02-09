use serde::Serialize;
use std::str::FromStr;
use tos_common::crypto::elgamal::{KeyPair, PrivateKey, PublicKey};
use tos_common::crypto::Hash;
use tos_common::serializer::Serializer;
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::{FeeType, Reference, TransactionType, TransferPayload, TxVersion};
use tos_crypto::curve25519_dalek::scalar::Scalar;

#[derive(Serialize)]
struct Output {
    accounts: Vec<Account>,
    cases: Vec<Case>,
}

#[derive(Serialize)]
struct Case {
    name: String,
    signature: String,
}

#[derive(Serialize)]
struct Account {
    name: String,
    private_key: String,
    public_key: String,
    address: String,
}

fn keypair_from_byte(byte: u8) -> KeyPair {
    let mut bytes = [0u8; 32];
    bytes[0] = byte;
    let scalar = Scalar::from_bytes_mod_order(bytes);
    let privkey = PrivateKey::from_scalar(scalar);
    KeyPair::from_private_key(privkey)
}

fn sign_tx(
    keypair: &KeyPair,
    chain_id: u8,
    version: TxVersion,
    fee: u64,
    fee_type: &FeeType,
    nonce: u64,
    reference: Reference,
    asset: Hash,
    dest: &PublicKey,
    amount: u64,
) -> String {
    let payload = TransferPayload::new(asset, dest.compress(), amount, None);
    let unsigned = UnsignedTransaction::new_with_fee_type(
        version,
        chain_id,
        keypair.get_public_key().compress(),
        TransactionType::Transfers(vec![payload]),
        fee,
        fee_type.clone(),
        nonce,
        reference,
    );
    let tx = unsigned.finalize(keypair);
    hex::encode(tx.get_signature().to_bytes())
}

fn main() {
    let roles = [
        ("Miner", 1u8),
        ("Alice", 2u8),
        ("Bob", 3u8),
        ("Carol", 4u8),
        ("Dave", 5u8),
        ("Eve", 6u8),
        ("Frank", 7u8),
        ("Grace", 8u8),
        ("Heidi", 9u8),
        ("Ivan", 10u8),
    ];

    let mut accounts = Vec::new();
    let mut keypairs = std::collections::HashMap::new();
    for (name, byte) in roles {
        let keypair = keypair_from_byte(byte);
        let priv_hex = hex::encode(keypair.get_private_key().to_bytes());
        let pub_hex = hex::encode(keypair.get_public_key().compress().as_bytes());
        accounts.push(Account {
            name: name.to_string(),
            private_key: priv_hex,
            public_key: pub_hex.clone(),
            address: pub_hex,
        });
        keypairs.insert(name, keypair);
    }

    let sender = keypairs.get("Alice").expect("Alice keypair");
    let receiver = keypairs.get("Bob").expect("Bob keypair");

    let chain_id = 3u8;
    let version = TxVersion::T1;
    let fee = 100000u64;
    let fee_type = FeeType::TOS;
    let reference = Reference {
        hash: Hash::from_str("0000000000000000000000000000000000000000000000000000000000000000")
            .unwrap(),
        topoheight: 0,
    };
    let asset =
        Hash::from_str("0000000000000000000000000000000000000000000000000000000000000000").unwrap();

    let dest_pub = receiver.get_public_key().clone();

    let mut cases = vec![
        Case {
            name: "transfer_success".to_string(),
            signature: sign_tx(
                sender,
                chain_id,
                version,
                fee,
                &fee_type,
                5,
                reference.clone(),
                asset.clone(),
                &dest_pub,
                100000,
            ),
        },
        Case {
            name: "nonce_too_high".to_string(),
            signature: sign_tx(
                sender,
                chain_id,
                version,
                fee,
                &fee_type,
                100,
                reference.clone(),
                asset.clone(),
                &dest_pub,
                100000,
            ),
        },
        Case {
            name: "insufficient_balance_execution_failure".to_string(),
            signature: sign_tx(
                sender, chain_id, version, fee, &fee_type, 5, reference, asset, &dest_pub, 2000000,
            ),
        },
    ];

    let invalid_fee_sig = sign_tx(
        sender,
        chain_id,
        version,
        0,
        &fee_type,
        5,
        Reference {
            hash: Hash::from_str(
                "0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            topoheight: 0,
        },
        Hash::from_str("0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
        &dest_pub,
        100000,
    );
    cases.push(Case {
        name: "invalid_fee".to_string(),
        signature: invalid_fee_sig,
    });

    let invalid_chain_sig = sign_tx(
        sender,
        4,
        version,
        fee,
        &fee_type,
        5,
        Reference {
            hash: Hash::from_str(
                "0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            topoheight: 0,
        },
        Hash::from_str("0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
        &dest_pub,
        100000,
    );
    cases.push(Case {
        name: "invalid_chain_id".to_string(),
        signature: invalid_chain_sig,
    });

    let mut invalid_sig = cases
        .iter()
        .find(|c| c.name == "transfer_success")
        .expect("transfer_success signature")
        .signature
        .clone();
    if let Some(_last) = invalid_sig.as_bytes().last().copied() {
        let mut bytes = hex::decode(invalid_sig).unwrap();
        let last_idx = bytes.len() - 1;
        bytes[last_idx] ^= 0x01;
        invalid_sig = hex::encode(bytes);
    }
    cases.push(Case {
        name: "invalid_signature".to_string(),
        signature: invalid_sig,
    });

    let out = Output { accounts, cases };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
