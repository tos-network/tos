// Dump range proof components for debugging
// Run: cd ~/tos/tck/crypto && cargo run --release --bin dump_proof

use bulletproofs::RangeProof;

fn main() {
    // First proof from rangeproofs.yaml (simple_42)
    let proof_hex = "5a4a3f154f54cb5a663ea204530f0d04c84968bb64e5fd1681e1ed90aceb78028ee99e6f1d72eab63cb7a397b90154700fa2b3a71f5e31c2c6a5ded2c5553510b84aced58e3ea30b6aff0f2d0916177552b81bf8866c7ead48d8131d2c522641ce5b5551c988676c164a920338b9b2d9733e5ca78b52e1ff4851cf5aef8578211d2580b2d6667465b29978c075cfebbd35c1ca39eb8d69183de63b8e7f16c20b5596620e10052390835711825c130cc86f4a520ec08dc7c41da02a1e7e07f1084d72f2f73999af4b1d2061089f5cd8aa8c41f346b4607d515d724b3da463b60c46e1f7caedc591c10a7187dee4f3fb036099a40e7430b110c56df965ac15fd0c0a9270eff2211357e92864f7d048a8fc486300dcb302f19db340c7c1f2238a6ea2a4496e4e22519cc776241f5592e1debc0f3066eb98727d210b837fec15230ed2881a21bc2fc58d86c96260cea75166f3e898a81168baa978c1f0ca5deab3208606adff49fe8bb9e67fbccfeda103b90a704ee0ae4c1431c2fac799ddc0a01bc2d17df446ee0a91c801d460db8db727b1704733363e562b8140f593efa9be0026cbfefa1379577d8ca1a93df556048ceaae76ec39c4dd1e072bf504092c0304d2854c8510a367c5bfa1cfcfed53aa709e469c3a54d17c559cd2fffbf6a56a1d1071a466043367ca887ad2dd627debad47c198d955c8046ae6051888ac82362b7251f156578b8a2b947ba0324af0ce2cfd157ee72582c49dbffa4edd59ba08704a54fd4bc03d65ba14ab658c6ca4cb15e11ed78e96589b2619ba64fdfd6e894a4286f654f5f1a973f956a7694a8a900fe962716078be844b40763d0f1313ba60fe34afe9e2c4c157e62b05ec9377279c3583622aad868263d07b0ccdbced27010c6a8005902fc1d36df7a70a977ba774eb5b944bb4229885309c7714bb7b9b02";

    let proof_bytes = hex::decode(proof_hex).unwrap();
    println!("Proof total size: {} bytes", proof_bytes.len());

    // Parse using bulletproofs crate
    let proof = RangeProof::from_bytes(&proof_bytes).expect("parse proof");

    // Access internal fields by serializing again
    let reserialized = proof.to_bytes();

    // The proof structure based on the crate:
    // A, S, T1, T2 (points) - 4*32 = 128 bytes
    // tx, tx_blinding, e_blinding (scalars) - 3*32 = 96 bytes
    // ipp_proof: L[0], R[0], ... L[n-1], R[n-1], a, b

    println!("\n--- Range Proof Components ---");
    println!("A:            {}", hex::encode(&reserialized[0..32]));
    println!("S:            {}", hex::encode(&reserialized[32..64]));
    println!("T1:           {}", hex::encode(&reserialized[64..96]));
    println!("T2:           {}", hex::encode(&reserialized[96..128]));
    println!("tx:           {}", hex::encode(&reserialized[128..160]));
    println!("tx_blinding:  {}", hex::encode(&reserialized[160..192]));
    println!("e_blinding:   {}", hex::encode(&reserialized[192..224]));

    println!("\n--- IPP Proof (offset 224) ---");
    let ipp_start = 224;
    let logn = 6; // for 64-bit

    for i in 0..logn {
        let l_start = ipp_start + i * 64;
        let r_start = l_start + 32;
        println!("L[{}]:         {}", i, hex::encode(&reserialized[l_start..l_start+32]));
        println!("R[{}]:         {}", i, hex::encode(&reserialized[r_start..r_start+32]));
    }

    let a_start = ipp_start + logn * 64;
    let b_start = a_start + 32;
    println!("a:            {}", hex::encode(&reserialized[a_start..a_start+32]));
    println!("b:            {}", hex::encode(&reserialized[b_start..b_start+32]));

    // Verify it re-serializes correctly
    assert_eq!(proof_bytes, reserialized, "Re-serialization doesn't match!");
    println!("\nRe-serialization matches original proof bytes.");
}
