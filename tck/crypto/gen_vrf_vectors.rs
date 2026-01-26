//! Generate VRF test vectors for cross-language testing
//!
//! This generates test vectors using schnorrkel VRF implementation
//! with TOS domain separator "TOS-VRF-v1".

use schnorrkel::{
    context::SigningContext,
    MiniSecretKey,
};

/// TOS VRF signing context domain separator
const TOS_VRF_CONTEXT: &[u8] = b"TOS-VRF-v1";

fn main() {
    println!("# VRF Test Vectors");
    println!("# Generated from schnorrkel VRF implementation");
    println!("# Domain separator: TOS-VRF-v1");
    println!();

    // Test vector 1: Known secret key
    let secret_bytes: [u8; 32] = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60,
        0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
        0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19,
        0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
    ];

    let mini = MiniSecretKey::from_bytes(&secret_bytes).unwrap();
    let keypair = mini.expand_to_keypair(schnorrkel::ExpansionMode::Ed25519);
    let public_key = keypair.public.to_bytes();

    let ctx = SigningContext::new(TOS_VRF_CONTEXT);

    println!("test_vectors:");
    println!();

    // Vector 1: empty input
    {
        let input: &[u8] = b"";
        let (inout, proof, _) = keypair.vrf_sign(ctx.bytes(input));
        let output = inout.to_preout().to_bytes();
        let proof_bytes = proof.to_bytes();

        println!("  - name: \"empty_input\"");
        println!("    secret_key: \"{}\"", hex::encode(&secret_bytes));
        println!("    public_key: \"{}\"", hex::encode(&public_key));
        println!("    input: \"\"");
        println!("    output: \"{}\"", hex::encode(&output));
        println!("    proof: \"{}\"", hex::encode(&proof_bytes));
        println!();
    }

    // Vector 2: simple message
    {
        let input = b"test message";
        let (inout, proof, _) = keypair.vrf_sign(ctx.bytes(input));
        let output = inout.to_preout().to_bytes();
        let proof_bytes = proof.to_bytes();

        println!("  - name: \"simple_message\"");
        println!("    secret_key: \"{}\"", hex::encode(&secret_bytes));
        println!("    public_key: \"{}\"", hex::encode(&public_key));
        println!("    input: \"{}\"", hex::encode(input));
        println!("    output: \"{}\"", hex::encode(&output));
        println!("    proof: \"{}\"", hex::encode(&proof_bytes));
        println!();
    }

    // Vector 3: block hash style input
    {
        let input: [u8; 32] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let (inout, proof, _) = keypair.vrf_sign(ctx.bytes(&input));
        let output = inout.to_preout().to_bytes();
        let proof_bytes = proof.to_bytes();

        println!("  - name: \"block_hash\"");
        println!("    secret_key: \"{}\"", hex::encode(&secret_bytes));
        println!("    public_key: \"{}\"", hex::encode(&public_key));
        println!("    input: \"{}\"", hex::encode(&input));
        println!("    output: \"{}\"", hex::encode(&output));
        println!("    proof: \"{}\"", hex::encode(&proof_bytes));
        println!();
    }

    // Vector 4: different key
    {
        let secret_bytes2: [u8; 32] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        ];
        let mini2 = MiniSecretKey::from_bytes(&secret_bytes2).unwrap();
        let keypair2 = mini2.expand_to_keypair(schnorrkel::ExpansionMode::Ed25519);
        let public_key2 = keypair2.public.to_bytes();

        let input = b"VRF test";
        let (inout, proof, _) = keypair2.vrf_sign(ctx.bytes(input));
        let output = inout.to_preout().to_bytes();
        let proof_bytes = proof.to_bytes();

        println!("  - name: \"different_key\"");
        println!("    secret_key: \"{}\"", hex::encode(&secret_bytes2));
        println!("    public_key: \"{}\"", hex::encode(&public_key2));
        println!("    input: \"{}\"", hex::encode(input));
        println!("    output: \"{}\"", hex::encode(&output));
        println!("    proof: \"{}\"", hex::encode(&proof_bytes));
    }
}
