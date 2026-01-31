// Generate Ed25519 point operation test vectors for Avatar AVX2 compatibility testing
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_ed25519_point_vectors

use curve25519_dalek_ng::edwards::EdwardsPoint;
use curve25519_dalek_ng::scalar::Scalar;
use curve25519_dalek_ng::constants::ED25519_BASEPOINT_POINT;

fn main() {
    println!("# Ed25519 Point Operation Test Vectors");
    println!("# Generated from curve25519_dalek_ng for Avatar AVX2 compatibility testing");
    println!("");

    // Base point B
    let b = ED25519_BASEPOINT_POINT;
    println!("base_point_compressed: {}", hex::encode(b.compress().as_bytes()));

    // 2*B
    let two_b = b + b;
    println!("2B_compressed: {}", hex::encode(two_b.compress().as_bytes()));

    // 3*B
    let three_b = two_b + b;
    println!("3B_compressed: {}", hex::encode(three_b.compress().as_bytes()));

    // 4*B
    let four_b = two_b + two_b;
    println!("4B_compressed: {}", hex::encode(four_b.compress().as_bytes()));

    // 7*B
    let seven_b = four_b + three_b;
    println!("7B_compressed: {}", hex::encode(seven_b.compress().as_bytes()));

    // 8*B
    let eight_b = four_b + four_b;
    println!("8B_compressed: {}", hex::encode(eight_b.compress().as_bytes()));

    // 100*B
    let scalar_100 = Scalar::from(100u64);
    let hundred_b = b * scalar_100;
    println!("100B_compressed: {}", hex::encode(hundred_b.compress().as_bytes()));

    // 256*B
    let scalar_256 = Scalar::from(256u64);
    let twofivesix_b = b * scalar_256;
    println!("256B_compressed: {}", hex::encode(twofivesix_b.compress().as_bytes()));

    // Random scalar test
    let scalar_bytes: [u8; 32] = [
        0x42, 0x42, 0x42, 0x42, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let scalar = Scalar::from_bytes_mod_order(scalar_bytes);
    let random_b = b * scalar;
    println!("");
    println!("# Scalar multiplication test");
    println!("scalar_hex: {}", hex::encode(&scalar_bytes));
    println!("result_compressed: {}", hex::encode(random_b.compress().as_bytes()));
}
