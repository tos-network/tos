// Verify range proofs from YAML test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin verify_rangeproofs

use bulletproofs::{BulletproofGens, PedersenGens, RangeProof};
use curve25519_dalek_ng::ristretto::CompressedRistretto;

fn main() {
    let pc_gens = PedersenGens::default();
    let bp_gens_64 = BulletproofGens::new(64, 1);
    let bp_gens_32 = BulletproofGens::new(32, 1);

    // Test vectors from rangeproofs.yaml
    let test_vectors = vec![
        ("simple_42", 64, "e01a1a667661d62623d9ed0d5a04c0308d3e63f448b7cd225738b55d3f062b03",
         "5a4a3f154f54cb5a663ea204530f0d04c84968bb64e5fd1681e1ed90aceb78028ee99e6f1d72eab63cb7a397b90154700fa2b3a71f5e31c2c6a5ded2c5553510b84aced58e3ea30b6aff0f2d0916177552b81bf8866c7ead48d8131d2c522641ce5b5551c988676c164a920338b9b2d9733e5ca78b52e1ff4851cf5aef8578211d2580b2d6667465b29978c075cfebbd35c1ca39eb8d69183de63b8e7f16c20b5596620e10052390835711825c130cc86f4a520ec08dc7c41da02a1e7e07f1084d72f2f73999af4b1d2061089f5cd8aa8c41f346b4607d515d724b3da463b60c46e1f7caedc591c10a7187dee4f3fb036099a40e7430b110c56df965ac15fd0c0a9270eff2211357e92864f7d048a8fc486300dcb302f19db340c7c1f2238a6ea2a4496e4e22519cc776241f5592e1debc0f3066eb98727d210b837fec15230ed2881a21bc2fc58d86c96260cea75166f3e898a81168baa978c1f0ca5deab3208606adff49fe8bb9e67fbccfeda103b90a704ee0ae4c1431c2fac799ddc0a01bc2d17df446ee0a91c801d460db8db727b1704733363e562b8140f593efa9be0026cbfefa1379577d8ca1a93df556048ceaae76ec39c4dd1e072bf504092c0304d2854c8510a367c5bfa1cfcfed53aa709e469c3a54d17c559cd2fffbf6a56a1d1071a466043367ca887ad2dd627debad47c198d955c8046ae6051888ac82362b7251f156578b8a2b947ba0324af0ce2cfd157ee72582c49dbffa4edd59ba08704a54fd4bc03d65ba14ab658c6ca4cb15e11ed78e96589b2619ba64fdfd6e894a4286f654f5f1a973f956a7694a8a900fe962716078be844b40763d0f1313ba60fe34afe9e2c4c157e62b05ec9377279c3583622aad868263d07b0ccdbced27010c6a8005902fc1d36df7a70a977ba774eb5b944bb4229885309c7714bb7b9b02"),
        ("zero_value", 64, "9a0dd785f756a0180db89d9b074f9e16309187cbf87aff17e339ca0e4d539d6e",
         "84911f3e10774b2298595784d5acddb217fded10316ff2a85658a8d2cd0ad772fe16cdfe7f4819f63164be810cc41d41d8fda3888b4a4a4dc21c138a700d591abedea9fa9d6e789695a4f8d75be3c338022ee0f32df28c0b53c8b241d4131d3caa78437558d38255ff254b65c2085bb77a776ae3ff1b43389731ccb6db1f26436dbe4bb3459cea41e278575d3dbaeadbbad666e3a85f2ad6b5e3f02402ec2b0180b2837c1a1ba09606c297d63d6a80873d354df4964b1690e1b662582e7b0a0858bbb14364d20166310f4e28f321b482dae8f31cd6fda367e2c8bec27b3a0d0cc6abb3c66d6e41c9c4ca593a2ba29bfbbc5457a011d120a3103f430c82e3451600e54e72bfbf8eaf3fde953e00c1554219473638906b60604896727c3bd9a54b1a763d002d38d17795faabaf76fb0114b79bfb2f8673b9268850a17fe4b5ae594adcfa30a6f54d3e55d791988a25cb894e5fb12cbfb89d8b6d231ff59b770f4aa8fa848632f10e0f19e538d3c67f5b7f3676fef7b2f8fc2734e071578816d446467cc6452356a27fb4969c6563e2f66df5ae40622da16894371c720f0dd2ab7648082f08ba97b93b0db5d8e4a030e68f377c6b5e8b0966e25487c87bad76dc69468361fbc420b57b5d1fe4e38d701ec00b6e7abbca9446e6624fb660ecd06c070ee363469b2b06e0458cf1a2078d94515e491cf84d62db9f002e012454c2886984ce7681e788f3f7b65a18c24c5e5a507d307ecb3515ee26a554b0172873613f56dff5102a9c97e097b873b0a317953989a3e25068fece031a539970e33f202bb2200b02e3b1204c61cdfdb662270e740b528b81540dd0196b9f6cf3aae90f5d5e1e70b60963b87011337c04d67e7bbe5a62d75568cdd39db6d3586244f4fe0aa6b5ede289a71502179313cc727d9124727c552a0d5ec20589ae2b64a1da4300"),
        ("bit32_million", 32, "a830882a457e95c81a6f8207cdde034418e3a518a5b606b1fd778baf8f49240b",
         "be6b495596ac1c3dd9db8176dcbb71a3d9f416490a82e606c061c94d5abe6930f86520ca95173ca7d66a2e603d388de0c4620e70763f335fa15d70dfba008357169641dd31d0cc0cccd44327c5aee17de30b8ae11bf00b953a834ec6f2d3324c1cc26f79a34ae5679cc607444ccac2675f9c89c9be9d20cbf34ff05816f4ab129904c1000e27d507da6f6a542c15dff7184379d605c6df2f848c67dd67f3ec0d942934f3a1a6ab8f2786b0cf9704b253b2a595c76086c87017c8b8269836db0f88ca1b2b1dab859bace7523bab16563ce1ffa031a7da104c24bd2e1f7664900378c01236c4853ebe6b2cec963dcab7d0d3692e7db323a4d2e8894346c0eab731ca07b3295c73d983c0e99762f5dedf2a8bd7bf14427b35e947068be09f572b54cc53e8fb9f864bc2cdde2af868af68f28e825ce7e6ccf2cbd13fd99476361f4760a7b3edde7d14576ef8149c03a4302ef93d6643a27a35cd3cb0634857cebd3c86c041693683596b36c4ebdaf7510b0af19adc12799de092cbab3d2ed1055e2c4e17d8da9dfe940cd7cf8a5585de8975e8795f92a2c34cdab109d25a454f3c39b49762eb65eb6f4f1536c21ec82a6d68d14e9876e944305f6bb88af5ee2ada2fe8f46d5f448c534fca54f44583574040ea240d0780593b3ce2510d254fd70c1b9e56655cd5531cb82a7c2b4b5e7d79fd2a516b3406f1adffbf7401fd3d949b28caf79e606ff6f535528533800913ee794cfe5f32c03e02c65e57865e9e592e5d6e5db9ae58572e5f42ede9d3d7c0942dbf7dcd86a7b8dcfccf9049cb3b9ca7098655e7266824bd1a95e3e54bd4c6419b7d0c975c10c050cc872a1de36e19ad00"),
    ];

    let mut passed = 0;
    let mut failed = 0;

    for (name, bit_length, commitment_hex, proof_hex) in test_vectors {
        let commitment_bytes = hex::decode(commitment_hex).unwrap();
        let commitment = CompressedRistretto::from_slice(&commitment_bytes);

        let proof_bytes = hex::decode(proof_hex).unwrap();
        let proof = RangeProof::from_bytes(&proof_bytes).expect("parse proof");

        let bp_gens = if bit_length == 32 { &bp_gens_32 } else { &bp_gens_64 };

        // Fresh transcript for verification
        let mut transcript = merlin::Transcript::new(b"RangeProofTest");

        let result = proof.verify_single(
            bp_gens,
            &pc_gens,
            &mut transcript,
            &commitment,
            bit_length,
        );

        match result {
            Ok(_) => {
                println!("PASS: {} (verified OK)", name);
                passed += 1;
            }
            Err(e) => {
                println!("FAIL: {} - {:?}", name, e);
                failed += 1;
            }
        }
    }

    println!("\n{} passed, {} failed", passed, failed);
}
