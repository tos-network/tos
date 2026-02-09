use std::{fs, path::PathBuf};

use serde::Deserialize;
use tos_common::{
    serializer::{Reader, ReaderError, Serializer},
    transaction::{FeeType, Transaction, TransactionType},
};

#[derive(Debug, Deserialize)]
struct WireFormatFixture {
    vectors: Vec<WireFormatVector>,
}

#[derive(Debug, Deserialize)]
struct WireFormatVector {
    name: String,
    tx: serde_json::Value,
    expected_hex: String,
}

#[derive(Debug, Deserialize)]
struct TxHeaderFixture {
    version: u8,
    chain_id: u8,
    source: String,
    tx_type: String,
    nonce: u64,
    fee: u64,
    fee_type: u8,
    #[allow(dead_code)]
    payload: serde_json::Value,
    reference_hash: String,
    reference_topoheight: u64,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct ConformanceVectorsFile {
    test_vectors: Vec<ConformanceTestVector>,
}

#[derive(Debug, Deserialize)]
struct ConformanceTestVector {
    name: String,
    input: ConformanceInput,
    expected: ConformanceExpected,
}

#[derive(Debug, Deserialize)]
struct ConformanceInput {
    kind: String,
    wire_hex: String,
}

#[derive(Debug, Deserialize)]
struct ConformanceExpected {
    success: bool,
}

fn repo_root_from_manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("common/ has a parent")
        .to_path_buf()
}

fn tos_spec_wire_fixture_path() -> PathBuf {
    repo_root_from_manifest_dir().join("common/tests/wire_format.json")
}

fn tos_spec_wire_negative_vectors_path() -> PathBuf {
    repo_root_from_manifest_dir().join("common/tests/wire_format_negative.json")
}

fn decode_tx_strict(hex_str: &str) -> Result<Transaction, ReaderError> {
    let bytes = match hex::decode(hex_str) {
        Ok(b) => b,
        Err(_) => return Err(ReaderError::InvalidHex),
    };
    let mut reader = Reader::new(&bytes);
    let tx = <Transaction as Serializer>::read(&mut reader)?;
    if reader.size() != 0 {
        // Non-empty remainder => trailing bytes. Mirror conformance "strict" semantics.
        return Err(ReaderError::InvalidSize);
    }
    Ok(tx)
}

fn fee_type_id(ft: &FeeType) -> u8 {
    match ft {
        FeeType::TOS => 0,
        FeeType::Energy => 1,
        FeeType::UNO => 2,
    }
}

fn tx_type_name(data: &TransactionType) -> &'static str {
    match data {
        TransactionType::Transfers(_) => "transfers",
        TransactionType::Burn(_) => "burn",
        // Spec uses `multisig` (not the serde `multi_sig` rename_all output).
        TransactionType::MultiSig(_) => "multisig",
        TransactionType::InvokeContract(_) => "invoke_contract",
        TransactionType::DeployContract(_) => "deploy_contract",
        TransactionType::Energy(_) => "energy",
        TransactionType::AgentAccount(_) => "agent_account",
        TransactionType::UnoTransfers(_) => "uno_transfers",
        TransactionType::ShieldTransfers(_) => "shield_transfers",
        TransactionType::UnshieldTransfers(_) => "unshield_transfers",
        TransactionType::RegisterName(_) => "register_name",
    }
}

#[test]
fn spec_wire_format_vectors_roundtrip_and_headers() {
    let path = tos_spec_wire_fixture_path();
    assert!(path.exists(), "missing fixture: {}", path.display());

    let raw = fs::read_to_string(&path).expect("read wire_format.json");
    let fixture: WireFormatFixture = serde_json::from_str(&raw).expect("parse wire_format.json");

    // TX types removed during codebase slimdown now return InvalidValue.
    // Skip vectors whose tx_type is no longer supported.
    let removed_types: &[&str] = &[
        "ephemeral_message",
        "bind_referrer",
        "batch_referral_reward",
        "create_escrow",
        "deposit_escrow",
        "release_escrow",
        "refund_escrow",
        "challenge_escrow",
        "dispute_escrow",
        "appeal_escrow",
        "submit_verdict",
        "submit_verdict_by_juror",
        "register_arbiter",
        "update_arbiter",
        "slash_arbiter",
        "request_arbiter_exit",
        "withdraw_arbiter_stake",
        "cancel_arbiter_exit",
        "commit_arbitration_open",
        "commit_vote_request",
        "commit_selection_commitment",
        "commit_juror_vote",
        "set_kyc",
        "revoke_kyc",
        "renew_kyc",
        "transfer_kyc",
        "appeal_kyc",
        "bootstrap_committee",
        "register_committee",
        "update_committee",
        "emergency_suspend",
    ];

    let mut decode_failures: Vec<(String, String)> = Vec::new();

    for v in fixture.vectors {
        // Skip vectors for removed TX types
        if removed_types
            .iter()
            .any(|prefix| v.name.starts_with(prefix))
        {
            continue;
        }
        let expected_hex = v.expected_hex.trim().to_ascii_lowercase();
        let bytes = hex::decode(&expected_hex)
            .unwrap_or_else(|e| panic!("{}: invalid hex in expected_hex: {e}", v.name));
        let mut reader = Reader::new(&bytes);
        let tx = match <Transaction as Serializer>::read(&mut reader) {
            Ok(tx) => {
                let rem = reader.size();
                if rem != 0 {
                    decode_failures.push((
                        v.name.clone(),
                        format!("trailing_bytes rem={rem} total_bytes={}", bytes.len()),
                    ));
                    continue;
                }
                tx
            }
            Err(e) => {
                decode_failures.push((v.name.clone(), format!("{e:?}")));
                continue;
            }
        };

        assert_eq!(
            tx.to_hex(),
            expected_hex,
            "{}: re-encoded hex mismatch",
            v.name
        );

        let hdr: TxHeaderFixture =
            serde_json::from_value(v.tx).unwrap_or_else(|e| panic!("{}: bad tx json: {e}", v.name));

        assert_eq!(
            u8::from(tx.get_version()),
            hdr.version,
            "{}: version",
            v.name
        );
        assert_eq!(tx.get_chain_id(), hdr.chain_id, "{}: chain_id", v.name);
        assert_eq!(tx.get_nonce(), hdr.nonce, "{}: nonce", v.name);
        assert_eq!(tx.get_fee(), hdr.fee, "{}: fee", v.name);
        assert_eq!(
            fee_type_id(tx.get_fee_type()),
            hdr.fee_type,
            "{}: fee_type",
            v.name
        );
        assert_eq!(
            tx.get_source().to_hex(),
            hdr.source.to_ascii_lowercase(),
            "{}: source",
            v.name
        );
        assert_eq!(
            tx_type_name(tx.get_data()),
            hdr.tx_type.as_str(),
            "{}: tx_type",
            v.name
        );
        assert_eq!(
            tx.get_reference().hash.to_hex(),
            hdr.reference_hash.to_ascii_lowercase(),
            "{}: reference_hash",
            v.name
        );
        assert_eq!(
            tx.get_reference().topoheight,
            hdr.reference_topoheight,
            "{}: reference_topoheight",
            v.name
        );
        assert_eq!(
            tx.get_signature().to_hex(),
            hdr.signature.to_ascii_lowercase(),
            "{}: signature",
            v.name
        );
    }

    assert!(
        decode_failures.is_empty(),
        "wire_format.json contains {} vectors that do not strict-decode: {:?}",
        decode_failures.len(),
        decode_failures
    );
}

#[test]
fn spec_wire_format_negative_vectors_rejected() {
    let path = tos_spec_wire_negative_vectors_path();
    assert!(path.exists(), "missing fixture: {}", path.display());

    let raw = fs::read_to_string(&path).expect("read wire_format_negative.json");
    let file: ConformanceVectorsFile =
        serde_json::from_str(&raw).expect("parse wire_format_negative.json");

    for tv in file.test_vectors {
        if tv.input.kind != "tx" || tv.expected.success {
            continue;
        }
        let hex = tv.input.wire_hex.trim().to_ascii_lowercase();
        let ok = decode_tx_strict(&hex).is_ok();
        assert!(
            !ok,
            "{}: expected strict decode to fail but it succeeded",
            tv.name
        );
    }
}
