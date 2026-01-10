//! # Factory Pattern Example
//!
//! This example demonstrates the Factory Pattern in smart contracts.
//! The Factory Pattern allows a contract to create instances of other contracts,
//! track them, and provide access control over the creation process.

#![no_std]
#![no_main]

use tako_sdk::*;

/// Address type (32-byte hash)
pub type Address = [u8; 32];

/// Contract hash (32-byte identifier)
pub type ContractHash = [u8; 32];

// Contract type constants
const CONTRACT_TYPE_TOKEN: u8 = 0;
const CONTRACT_TYPE_NFT: u8 = 1;
const CONTRACT_TYPE_MULTISIG: u8 = 2;
const CONTRACT_TYPE_GOVERNANCE: u8 = 3;
const CONTRACT_TYPE_CUSTOM: u8 = 255;

// Storage key prefixes
const KEY_ADMIN: &[u8] = b"admin";
const KEY_PAUSED: &[u8] = b"paused";
const KEY_COUNT: &[u8] = b"count";
const KEY_CONTRACT_PREFIX: &[u8] = b"con:"; // con:{hash} -> type, creator, active
const KEY_CREATOR_PREFIX: &[u8] = b"crt:"; // crt:{creator}{index} -> hash

// Error codes
define_errors! {
    FactoryPaused = 1301,
    ContractExists = 1302,
    ContractNotFound = 1303,
    NotAuthorized = 1304,
    AlreadyInactive = 1305,
    AlreadyActive = 1306,
    OnlyAdmin = 1307,
    AlreadyPaused = 1308,
    NotPaused = 1309,
    StorageError = 1310,
    InvalidInput = 1311,
}

entrypoint!(process_instruction);

fn process_instruction(input: &[u8]) -> entrypoint::Result<()> {
    if input.is_empty() {
        return Ok(());
    }

    match input[0] {
        // Initialize: [0, admin[32]]
        0 => {
            if input.len() < 33 {
                return Err(InvalidInput);
            }
            let mut admin = [0u8; 32];
            admin.copy_from_slice(&input[1..33]);
            init(&admin)
        }
        // Create contract: [1, creator[32], contract_type[1]]
        1 => {
            if input.len() < 34 {
                return Err(InvalidInput);
            }
            let mut creator = [0u8; 32];
            creator.copy_from_slice(&input[1..33]);
            let contract_type = input[33];
            create_contract(&creator, contract_type)
        }
        // Deactivate contract: [2, caller[32], contract_hash[32]]
        2 => {
            if input.len() < 65 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut contract_hash = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            contract_hash.copy_from_slice(&input[33..65]);
            deactivate_contract(&caller, &contract_hash)
        }
        // Reactivate contract: [3, caller[32], contract_hash[32]]
        3 => {
            if input.len() < 65 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut contract_hash = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            contract_hash.copy_from_slice(&input[33..65]);
            reactivate_contract(&caller, &contract_hash)
        }
        // Pause: [4, caller[32]]
        4 => {
            if input.len() < 33 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            pause(&caller)
        }
        // Unpause: [5, caller[32]]
        5 => {
            if input.len() < 33 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            unpause(&caller)
        }
        // Get contract count: [6]
        6 => {
            let count = read_u64(KEY_COUNT);
            set_return_data(&count.to_le_bytes());
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Initialize factory
fn init(admin: &Address) -> entrypoint::Result<()> {
    storage_write(KEY_ADMIN, admin).map_err(|_| StorageError)?;
    storage_write(KEY_PAUSED, &[0u8]).map_err(|_| StorageError)?;
    storage_write(KEY_COUNT, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    log("Factory initialized");
    Ok(())
}

/// Create a new contract instance
fn create_contract(creator: &Address, contract_type: u8) -> entrypoint::Result<()> {
    // Check if paused
    if is_paused() {
        return Err(FactoryPaused);
    }

    // Generate contract hash using block hash and count
    let count = read_u64(KEY_COUNT);
    let block_hash = get_block_hash();
    let contract_hash = generate_hash(creator, contract_type, count, &block_hash);

    // Check if contract already exists
    let contract_key = make_contract_key(&contract_hash);
    let mut buffer = [0u8; 1];
    if storage_read(&contract_key, &mut buffer) > 0 {
        return Err(ContractExists);
    }

    // Store contract info: [type, active, creator...]
    let mut contract_data = [0u8; 34]; // type + active + creator
    contract_data[0] = contract_type;
    contract_data[1] = 1; // active
    contract_data[2..34].copy_from_slice(creator);
    storage_write(&contract_key, &contract_data).map_err(|_| StorageError)?;

    // Update count
    storage_write(KEY_COUNT, &(count + 1).to_le_bytes()).map_err(|_| StorageError)?;

    // Return contract hash
    set_return_data(&contract_hash);

    log("Contract created");
    Ok(())
}

/// Deactivate a contract
fn deactivate_contract(caller: &Address, contract_hash: &ContractHash) -> entrypoint::Result<()> {
    let contract_key = make_contract_key(contract_hash);
    let mut contract_data = [0u8; 34];
    let len = storage_read(&contract_key, &mut contract_data);
    if len != 34 {
        return Err(ContractNotFound);
    }

    // Check authorization (creator or admin)
    let admin = read_admin()?;
    let mut creator = [0u8; 32];
    creator.copy_from_slice(&contract_data[2..34]);
    if *caller != creator && *caller != admin {
        return Err(NotAuthorized);
    }

    // Check if already inactive
    if contract_data[1] == 0 {
        return Err(AlreadyInactive);
    }

    // Deactivate
    contract_data[1] = 0;
    storage_write(&contract_key, &contract_data).map_err(|_| StorageError)?;

    log("Contract deactivated");
    Ok(())
}

/// Reactivate a contract
fn reactivate_contract(caller: &Address, contract_hash: &ContractHash) -> entrypoint::Result<()> {
    // Only admin can reactivate
    let admin = read_admin()?;
    if *caller != admin {
        return Err(OnlyAdmin);
    }

    let contract_key = make_contract_key(contract_hash);
    let mut contract_data = [0u8; 34];
    let len = storage_read(&contract_key, &mut contract_data);
    if len != 34 {
        return Err(ContractNotFound);
    }

    // Check if already active
    if contract_data[1] != 0 {
        return Err(AlreadyActive);
    }

    // Reactivate
    contract_data[1] = 1;
    storage_write(&contract_key, &contract_data).map_err(|_| StorageError)?;

    log("Contract reactivated");
    Ok(())
}

/// Pause factory
fn pause(caller: &Address) -> entrypoint::Result<()> {
    let admin = read_admin()?;
    if *caller != admin {
        return Err(OnlyAdmin);
    }

    if is_paused() {
        return Err(AlreadyPaused);
    }

    storage_write(KEY_PAUSED, &[1u8]).map_err(|_| StorageError)?;
    log("Factory paused");
    Ok(())
}

/// Unpause factory
fn unpause(caller: &Address) -> entrypoint::Result<()> {
    let admin = read_admin()?;
    if *caller != admin {
        return Err(OnlyAdmin);
    }

    if !is_paused() {
        return Err(NotPaused);
    }

    storage_write(KEY_PAUSED, &[0u8]).map_err(|_| StorageError)?;
    log("Factory unpaused");
    Ok(())
}

// Helper functions

fn read_admin() -> entrypoint::Result<Address> {
    let mut buffer = [0u8; 32];
    let len = storage_read(KEY_ADMIN, &mut buffer);
    if len != 32 {
        return Err(StorageError);
    }
    Ok(buffer)
}

fn is_paused() -> bool {
    let mut buffer = [0u8; 1];
    let len = storage_read(KEY_PAUSED, &mut buffer);
    len == 1 && buffer[0] != 0
}

fn read_u64(key: &[u8]) -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

fn make_contract_key(hash: &ContractHash) -> [u8; 36] {
    // con: (4) + hash (32) = 36
    let mut key = [0u8; 36];
    key[0..4].copy_from_slice(KEY_CONTRACT_PREFIX);
    key[4..36].copy_from_slice(hash);
    key
}

fn generate_hash(
    creator: &Address,
    contract_type: u8,
    count: u64,
    block_hash: &[u8; 32],
) -> ContractHash {
    // Simple hash: XOR creator with block_hash, add type and count
    let mut hash = [0u8; 32];
    for i in 0..32 {
        hash[i] = creator[i] ^ block_hash[i];
    }
    hash[0] ^= contract_type;
    hash[1] ^= (count & 0xFF) as u8;
    hash[2] ^= ((count >> 8) & 0xFF) as u8;
    hash
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
