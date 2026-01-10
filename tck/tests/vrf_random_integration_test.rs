#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
// File: testing-framework/tests/vrf_random_integration_test.rs
//
// VRF Randomness Integration Test
//
// This test executes a contract that calls `vrf_random` and `vrf_public_key`.
// It is skipped if the fixture is not built yet.

use std::path::Path;

use tos_common::block::compute_vrf_input;
use tos_common::contract::ContractCache;
use tos_common::crypto::{hash, Hash, KeyPair};
use tos_daemon::{
    tako_integration::TakoExecutor,
    vrf::{VrfData, VrfKeyManager, VrfOutput, VrfProof, VrfPublicKey},
};
use tos_kernel::ValueCell;
use tos_tck::utilities::create_contract_test_storage;

fn keypair_to_hash(keypair: &KeyPair) -> Hash {
    Hash::new(*keypair.get_public_key().compress().as_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use core::fmt::Write;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

fn cache_get_bytes(cache: &ContractCache, key: &[u8]) -> Option<Vec<u8>> {
    let key_cell = ValueCell::Bytes(key.to_vec());
    cache.storage.get(&key_cell).and_then(|(_, value)| {
        value
            .as_ref()
            .and_then(|cell| cell.as_bytes().ok())
            .cloned()
    })
}

async fn execute_vrf_contract(
    bytecode: &[u8],
    vrf_data: &VrfData,
    block_hash: &Hash,
    miner_public_key: &[u8; 32],
) -> ContractCache {
    let contract_path = "tests/fixtures/vrf_random.so";
    if !Path::new(contract_path).exists() {
        eprintln!(
            "Skipping VRF test: missing {contract_path}. Build /Users/tomisetsu/tako/examples/vrf-random and copy the .so here."
        );
        return ContractCache::default();
    }

    let owner = KeyPair::new();
    let storage = create_contract_test_storage(&owner, 10_000_000)
        .await
        .expect("Failed to create test storage");

    let contract_hash = Hash::zero();
    let tx_sender = keypair_to_hash(&owner);

    let mut storage_write = storage.write().await;
    let result = TakoExecutor::execute_with_vrf(
        bytecode,
        &mut *storage_write,
        1,
        &contract_hash,
        block_hash,
        1,
        1_704_067_200,
        &Hash::zero(),
        &tx_sender,
        &[],
        None,
        Some(vrf_data),
        Some(miner_public_key),
    )
    .expect("Contract execution failed");

    assert_eq!(result.return_value, 0);
    result.cache
}

#[tokio::test]
async fn test_vrf_random_syscall_in_contract() {
    let contract_path = "tests/fixtures/vrf_random.so";
    if !Path::new(contract_path).exists() {
        eprintln!(
            "Skipping VRF test: missing {contract_path}. Build /Users/tomisetsu/tako/examples/vrf-random and copy the .so here."
        );
        return;
    }

    let bytecode = std::fs::read(contract_path).expect("Failed to read vrf_random.so");
    let block_hash = Hash::new([9u8; 32]);
    let miner_keypair = KeyPair::new();
    let miner_compressed = miner_keypair.get_public_key().compress();
    let miner_public_key = miner_compressed.as_bytes();
    let chain_id: u64 = 3; // Devnet
    let vrf_manager = VrfKeyManager::new();
    let vrf_data: VrfData = vrf_manager
        .sign(
            chain_id,
            block_hash.as_bytes(),
            &miner_compressed,
            &miner_keypair,
        )
        .expect("Failed to sign VRF data");

    let cache = execute_vrf_contract(&bytecode, &vrf_data, &block_hash, miner_public_key).await;

    let random = cache_get_bytes(&cache, b"vrf_random").expect("Missing vrf_random");
    let pre_output = cache_get_bytes(&cache, b"vrf_pre_output").expect("Missing vrf_pre_output");
    let proof = cache_get_bytes(&cache, b"vrf_proof").expect("Missing vrf_proof");
    let public_key = cache_get_bytes(&cache, b"vrf_public_key").expect("Missing vrf_public_key");
    let stored_block_hash =
        cache_get_bytes(&cache, b"vrf_block_hash").expect("Missing vrf_block_hash");
    let verified = cache_get_bytes(&cache, b"vrf_verified").expect("Missing vrf_verified");

    println!("vrf_random: {}", to_hex(&random));
    println!("vrf_pre_output: {}", to_hex(&pre_output));
    println!("vrf_proof: {}", to_hex(&proof));
    println!("vrf_public_key: {}", to_hex(&public_key));
    println!("vrf_block_hash: {}", to_hex(&stored_block_hash));

    assert_eq!(random.len(), 32);
    assert_eq!(pre_output.len(), 32);
    assert_eq!(proof.len(), 64);
    assert_eq!(public_key.len(), 32);
    assert_eq!(stored_block_hash.len(), 32);
    assert_eq!(verified, vec![1u8]);
    assert_eq!(stored_block_hash, block_hash.as_bytes().to_vec());

    let mut derive_input = Vec::with_capacity(
        b"TOS-VRF-DERIVE".len() + pre_output.len() + block_hash.as_bytes().len(),
    );
    derive_input.extend_from_slice(b"TOS-VRF-DERIVE");
    derive_input.extend_from_slice(&pre_output);
    derive_input.extend_from_slice(block_hash.as_bytes());
    let expected_random = hash(&derive_input);
    assert_eq!(random, expected_random.as_bytes().to_vec());

    let pk = VrfPublicKey::from_bytes(&public_key).expect("Invalid VRF public key");
    let output = VrfOutput::from_bytes(&pre_output).expect("Invalid VRF output");
    let proof = VrfProof::from_bytes(&proof).expect("Invalid VRF proof");

    // Compute VRF input with identity binding (same as signing)
    let vrf_input = compute_vrf_input(block_hash.as_bytes(), &miner_compressed);

    pk.verify(&vrf_input, &output, &proof)
        .expect("VRF verification failed");
}

#[tokio::test]
async fn test_vrf_random_syscall_fixed_key_deterministic() {
    let contract_path = "tests/fixtures/vrf_random.so";
    if !Path::new(contract_path).exists() {
        eprintln!(
            "Skipping VRF test: missing {contract_path}. Build /Users/tomisetsu/tako/examples/vrf-random and copy the .so here."
        );
        return;
    }

    let bytecode = std::fs::read(contract_path).expect("Failed to read vrf_random.so");
    let block_hash = Hash::new([9u8; 32]);
    let miner_keypair = KeyPair::new();
    let miner_compressed = miner_keypair.get_public_key().compress();
    let miner_public_key = miner_compressed.as_bytes();
    let chain_id: u64 = 3; // Devnet
    let fixed_secret = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    let vrf_manager = VrfKeyManager::from_hex(fixed_secret).expect("Invalid fixed VRF secret");
    let vrf_data: VrfData = vrf_manager
        .sign(
            chain_id,
            block_hash.as_bytes(),
            &miner_compressed,
            &miner_keypair,
        )
        .expect("Failed to sign VRF data");

    let cache_a = execute_vrf_contract(&bytecode, &vrf_data, &block_hash, miner_public_key).await;
    let cache_b = execute_vrf_contract(&bytecode, &vrf_data, &block_hash, miner_public_key).await;

    let random_a = cache_get_bytes(&cache_a, b"vrf_random").expect("Missing vrf_random");
    let random_b = cache_get_bytes(&cache_b, b"vrf_random").expect("Missing vrf_random");
    let pre_output_a =
        cache_get_bytes(&cache_a, b"vrf_pre_output").expect("Missing vrf_pre_output");
    let pre_output_b =
        cache_get_bytes(&cache_b, b"vrf_pre_output").expect("Missing vrf_pre_output");
    let proof_a = cache_get_bytes(&cache_a, b"vrf_proof").expect("Missing vrf_proof");
    let proof_b = cache_get_bytes(&cache_b, b"vrf_proof").expect("Missing vrf_proof");
    let public_key_a =
        cache_get_bytes(&cache_a, b"vrf_public_key").expect("Missing vrf_public_key");
    let public_key_b =
        cache_get_bytes(&cache_b, b"vrf_public_key").expect("Missing vrf_public_key");

    assert_eq!(random_a, random_b);
    assert_eq!(pre_output_a, pre_output_b);
    assert_eq!(proof_a, proof_b);
    assert_eq!(public_key_a, public_key_b);

    let mut derive_input = Vec::with_capacity(
        b"TOS-VRF-DERIVE".len() + pre_output_a.len() + block_hash.as_bytes().len(),
    );
    derive_input.extend_from_slice(b"TOS-VRF-DERIVE");
    derive_input.extend_from_slice(&pre_output_a);
    derive_input.extend_from_slice(block_hash.as_bytes());
    let expected_random = hash(&derive_input);

    assert_eq!(random_a, expected_random.as_bytes().to_vec());
    assert_eq!(pre_output_a, vrf_data.output.as_bytes().to_vec());
    assert_eq!(proof_a, vrf_data.proof.to_bytes().to_vec());
    assert_eq!(public_key_a, vrf_data.public_key.as_bytes().to_vec());
}
