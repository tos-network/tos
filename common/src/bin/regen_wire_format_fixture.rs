use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tos_common::{
    crypto::{
        proofs::{RangeProof, BP_GENS, BULLET_PROOF_SIZE, PC_GENS},
        Hash, Signature,
    },
    serializer::{Reader, Serializer},
    transaction::{
        FeeType, Reference, Transaction, TransactionType, TxVersion,
        TX_TYPE_OPCODE_SHIELD_TRANSFERS, TX_TYPE_OPCODE_UNO_TRANSFERS,
        TX_TYPE_OPCODE_UNSHIELD_TRANSFERS,
    },
};
use tos_crypto::{curve25519_dalek::Scalar, merlin::Transcript};

const LEGACY_TX_TYPE_OPCODE_UNO_TRANSFERS: u8 = 18;
const LEGACY_TX_TYPE_OPCODE_SHIELD_TRANSFERS: u8 = 19;
const LEGACY_TX_TYPE_OPCODE_UNSHIELD_TRANSFERS: u8 = 20;

#[derive(Debug, Deserialize, Serialize)]
struct WireFormatFixture {
    vectors: Vec<WireFormatVector>,
}

#[derive(Debug, Deserialize, Serialize)]
struct WireFormatVector {
    name: String,
    tx: serde_json::Value,
    expected_hex: String,
}

#[derive(Debug, Deserialize)]
struct TxHeaderFixture {
    version: u8,
    chain_id: u8,
    reference_hash: String,
    reference_topoheight: u64,
    signature: String,
}

fn repo_root_from_manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("common/ has a parent")
        .to_path_buf()
}

fn fixture_path() -> PathBuf {
    repo_root_from_manifest_dir().join("common/tests/wire_format.json")
}

fn parse_prefix(
    expected_hex: &str,
) -> Result<
    (
        TxVersion,
        u8,
        tos_common::crypto::elgamal::CompressedPublicKey,
        TransactionType,
        u64,
        FeeType,
        u64,
    ),
    tos_common::serializer::ReaderError,
> {
    let bytes =
        hex::decode(expected_hex).map_err(|_| tos_common::serializer::ReaderError::InvalidHex)?;
    let mut normalized = bytes.clone();

    // Backward-compat for fixture regeneration only:
    // old vectors may still use UNO/Shield/Unshield legacy opcodes 18/19/20.
    let mut probe = Reader::new(&normalized);
    let version = TxVersion::read(&mut probe)?;
    probe.context_mut().store(version);
    probe.read_u8()?; // chain_id
    let _ = tos_common::crypto::elgamal::CompressedPublicKey::read(&mut probe)?;
    let tx_opcode_offset = probe.total_read();
    let tx_opcode = probe.read_u8()?;
    normalized[tx_opcode_offset] = match tx_opcode {
        LEGACY_TX_TYPE_OPCODE_UNO_TRANSFERS => TX_TYPE_OPCODE_UNO_TRANSFERS,
        LEGACY_TX_TYPE_OPCODE_SHIELD_TRANSFERS => TX_TYPE_OPCODE_SHIELD_TRANSFERS,
        LEGACY_TX_TYPE_OPCODE_UNSHIELD_TRANSFERS => TX_TYPE_OPCODE_UNSHIELD_TRANSFERS,
        other => other,
    };

    let mut reader = Reader::new(&normalized);
    let version = TxVersion::read(&mut reader)?;
    // CiphertextValidityProof decoding depends on tx version.
    reader.context_mut().store(version);
    let chain_id = reader.read_u8()?;
    let source = tos_common::crypto::elgamal::CompressedPublicKey::read(&mut reader)?;
    let data = TransactionType::read(&mut reader)?;
    let fee = reader.read_u64()?;
    let fee_type = FeeType::read(&mut reader)?;
    let nonce = u64::read(&mut reader)?;
    Ok((version, chain_id, source, data, fee, fee_type, nonce))
}

fn dummy_range_proof() -> RangeProof {
    // Deterministic, not meant to correspond to the tx payload; only to satisfy codec parsing.
    let mut transcript = Transcript::new(b"wire_format_fixture_range_proof_v1");
    let values = vec![0u64, 1u64]; // power-of-two length
    let openings = vec![Scalar::from(7u64), Scalar::from(42u64)];

    let (rp, _commitments) = RangeProof::prove_multiple(
        &BP_GENS,
        &PC_GENS,
        &mut transcript,
        &values,
        &openings,
        BULLET_PROOF_SIZE,
    )
    .expect("range proof generation must succeed");
    rp
}

fn main() {
    let path = fixture_path();
    let raw = fs::read_to_string(&path).expect("read wire_format.json");
    let mut fixture: WireFormatFixture =
        serde_json::from_str(&raw).expect("parse wire_format.json");

    let mut updated = 0usize;

    for v in fixture.vectors.iter_mut() {
        let name = v.name.as_str();
        if name != "uno_transfers_basic"
            && name != "shield_transfers_basic"
            && name != "unshield_transfers_basic"
        {
            continue;
        }

        let hdr: TxHeaderFixture =
            serde_json::from_value(v.tx.clone()).expect("parse tx header json");
        let (version, chain_id, source, data, fee, fee_type, nonce) =
            parse_prefix(v.expected_hex.trim()).expect("parse tx prefix from expected_hex");

        // Sanity: header version/chain_id should match the prefix.
        assert_eq!(hdr.version, u8::from(version), "{name}: version mismatch");
        assert_eq!(hdr.chain_id, chain_id, "{name}: chain_id mismatch");

        let reference = Reference {
            hash: Hash::from_hex(hdr.reference_hash.trim()).expect("reference_hash hex"),
            topoheight: hdr.reference_topoheight,
        };
        let signature = Signature::from_hex(hdr.signature.trim()).expect("signature hex");

        let tx = match &data {
            TransactionType::UnoTransfers(_) => Transaction::new_with_uno(
                version,
                chain_id,
                source,
                data,
                fee,
                fee_type,
                nonce,
                Vec::new(),
                dummy_range_proof(),
                reference,
                None,
                signature,
            ),
            TransactionType::ShieldTransfers(_) | TransactionType::UnshieldTransfers(_) => {
                Transaction::new(
                    version, chain_id, source, data, fee, fee_type, nonce, reference, None,
                    signature,
                )
            }
            _ => panic!("{name}: unexpected tx type in fixture"),
        };

        v.expected_hex = tx.to_hex();
        updated += 1;
    }

    assert_eq!(
        updated, 3,
        "expected to patch exactly 3 vectors, patched={updated}"
    );

    let out = serde_json::to_string_pretty(&fixture).expect("serialize wire_format.json");
    fs::write(&path, format!("{out}\n")).expect("write wire_format.json");
    eprintln!("updated {updated} vectors in {}", path.display());
}
