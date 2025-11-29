use std::process::exit;
trait OrExit<T> {
    fn or_exit(self, msg: &str) -> T;
}
impl<T, E: std::fmt::Display> OrExit<T> for Result<T, E> {
    fn or_exit(self, msg: &str) -> T {
        match self {
            Ok(v) => v,
            Err(e) => {
                eprintln!("error: {}: {}", msg, e);
                exit(2)
            }
        }
    }
}
impl<T> OrExit<T> for Option<T> {
    fn or_exit(self, msg: &str) -> T {
        match self {
            Some(v) => v,
            None => {
                eprintln!("error: {}: none", msg);
                exit(2)
            }
        }
    }
}

use std::env;
use tos_common::{
    block::{Block, BlockHeader, BlockVersion},
    crypto::{Address, Hash, Hashable},
    immutable::Immutable,
    serializer::Serializer,
};

const EXTRA_NONCE_SIZE: usize = 32;

fn main() {
    let args: Vec<String> = env::args().collect();
    let network = if args.len() > 1 && args[1] == "testnet" {
        "testnet"
    } else {
        "mainnet"
    };

    // Use the developer address from configuration
    let dev_address = "tos1qsl6sj2u0gp37tr6drrq964rd4d8gnaxnezgytmt0cfltnp2wsgqqak28je";
    // SAFETY: Acceptable in build-time genesis generator with hardcoded address
    #[allow(clippy::disallowed_methods)]
    let address = Address::from_string(dev_address).or_exit("unwrap");
    let public_key = address.to_public_key();

    println!("Network: {network}");
    println!("Developer address: {dev_address}");
    println!("Developer public key: {}", public_key.to_hex());

    // Create genesis block header with different timestamps for different networks
    // VERSION UNIFICATION: All networks use Baseline version from genesis
    let (version, timestamp) = match network {
        "testnet" => (BlockVersion::Baseline, 1696132639000u64), // Testnet timestamp
        _ => (BlockVersion::Baseline, 1752336822401u64),         // Mainnet timestamp
    };

    let header = BlockHeader::new_simple(
        version,
        Vec::new(),              // parents (empty for genesis)
        timestamp,               // fixed timestamp
        [0u8; EXTRA_NONCE_SIZE], // extra nonce
        public_key,              // miner
        Hash::zero(),            // hash_merkle_root (empty for genesis)
    );

    // Create genesis block
    let block = Block::new(Immutable::Owned(header), Vec::new());
    let block_hash = block.hash();

    println!("\n=== New Genesis Block Information ===");
    println!("Block hex: {}", block.to_hex());
    println!("Block hash: {block_hash}");
    println!("Block hash (bytes): {:?}", block_hash.clone().to_bytes());

    // Verify block
    println!("\n=== Verification Information ===");
    println!("Block version: {:?}", block.get_version());
    println!("Block height: {}", block.get_blue_score());
    println!("Miner: {}", block.get_miner().to_hex());
    println!("Timestamp: {}", block.get_timestamp());
    println!("Transaction count: {}", block.get_transactions().len());

    println!("\n=== Configuration Update ===");
    println!("Please update the following content to daemon/src/config.rs:");
    match network {
        "testnet" => {
            println!(
                "const TESTNET_GENESIS_BLOCK: &str = \"{}\";",
                block.to_hex()
            );
            println!(
                "const TESTNET_GENESIS_BLOCK_HASH: Hash = Hash::new({:?});",
                block_hash.to_bytes()
            );
        }
        _ => {
            println!(
                "const MAINNET_GENESIS_BLOCK: &str = \"{}\";",
                block.to_hex()
            );
            println!(
                "const MAINNET_GENESIS_BLOCK_HASH: Hash = Hash::new({:?});",
                block_hash.to_bytes()
            );
        }
    }

    // Verify generated string
    println!("\n=== Generated String Verification ===");
    println!("String length: {}", block.to_hex().len());
    println!("String content: '{}'", block.to_hex());

    // Test parsing
    match Block::from_hex(&block.to_hex()) {
        Ok(parsed_block) => {
            println!("✅ String can be correctly parsed as block");
            println!("Parsed block hash: {}", parsed_block.hash());
        }
        Err(e) => {
            println!("❌ String cannot be parsed as block: {e:?}");
        }
    }
}
