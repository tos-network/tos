// Check bulletproofs generators match Avatar
// Run: cd ~/tos/tck/crypto && cargo run --release --bin check_gens

use bulletproofs::PedersenGens;
use curve25519_dalek_ng::ristretto::RistrettoPoint;
use sha3::{Shake256, digest::{ExtendableOutput, Update, XofReader}};

// Recreate bulletproofs' GeneratorsChain
fn generators_chain(label: &[u8], count: usize) -> Vec<RistrettoPoint> {
    let mut shake = Shake256::default();
    shake.update(b"GeneratorsChain");
    shake.update(label);
    let mut reader = shake.finalize_xof();

    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let mut uniform_bytes = [0u8; 64];
        reader.read(&mut uniform_bytes);
        result.push(RistrettoPoint::from_uniform_bytes(&uniform_bytes));
    }
    result
}

fn main() {
    let pc_gens = PedersenGens::default();

    // Print compressed form of B (G)
    println!("Rust B (G):         {}", hex::encode(pc_gens.B.compress().as_bytes()));

    // Print compressed form of B_blinding (H)
    println!("Rust B_blinding (H): {}", hex::encode(pc_gens.B_blinding.compress().as_bytes()));

    // Avatar's generators (from at_rangeproofs_table_ref.c)
    println!();
    println!("Avatar G: e2f2ae0a6abc4e71a884a961c500515f58e30b6aa582dd8db6a65945e08d2d76");
    println!("Avatar H: 8c9240b456a9e6dc65c377a1048d745f94a08cdb7f44cbcd7b46f34048871134");

    // Generate BulletproofGens G_vec for party 0
    // Label is "G" + little-endian u32 party index
    let label_g: [u8; 5] = [b'G', 0, 0, 0, 0];
    let g_points = generators_chain(&label_g, 4);

    println!("\n--- BulletproofGens G[0..4] (regenerated) ---");
    for (i, g) in g_points.iter().enumerate() {
        println!("G[{}]: {}", i, hex::encode(g.compress().as_bytes()));
    }

    println!("\n--- Avatar generators_G[0..4] ---");
    println!("G[0]: e4d549716460013e71c032240c93ea1b1969cbc9e89c5d6b43adbf6c1df10724");
    println!("G[1]: d6728b558a7b439c64bc077828560391e30b589314a999648d5f8cb471725f04");
    println!("G[2]: 76c764f6854b782e38c65391d9cbdc5b6393b951ec450ddf75d05aab183e855c");
    println!("G[3]: ee2b3649b4a4b5e3a7c0cb8abd1efd953e4b568a9beec95fcd52c67ff4cc6d2e");

    // Generate H_vec for party 0
    let label_h: [u8; 5] = [b'H', 0, 0, 0, 0];
    let h_points = generators_chain(&label_h, 4);

    println!("\n--- BulletproofGens H[0..4] (regenerated) ---");
    for (i, h) in h_points.iter().enumerate() {
        println!("H[{}]: {}", i, hex::encode(h.compress().as_bytes()));
    }
}
