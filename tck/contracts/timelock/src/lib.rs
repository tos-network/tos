//! # Timelock Contract
//!
//! A timelock contract that delays execution of transactions for a specified period.

#![no_std]
#![no_main]

use tako_sdk::*;

pub type Address = [u8; 32];
pub type TxId = [u8; 32];

// Transaction status
const STATUS_PENDING: u8 = 0;
const STATUS_READY: u8 = 1;
const STATUS_EXECUTED: u8 = 2;
const STATUS_CANCELLED: u8 = 3;

// Storage keys
const KEY_ADMIN: &[u8] = b"admin";
const KEY_MIN_DELAY: &[u8] = b"minD";
const KEY_TX_PREFIX: &[u8] = b"tx:"; // tx:{id} -> target, value, eta, status

// Error codes
define_errors! {
    OnlyAdmin = 1701,
    TxNotFound = 1702,
    TxNotReady = 1703,
    TxAlreadyExecuted = 1704,
    TxNotPending = 1705,
    TooEarly = 1706,
    DelayTooShort = 1707,
    StorageError = 1708,
    InvalidInput = 1709,
}

entrypoint!(process_instruction);

fn process_instruction(input: &[u8]) -> entrypoint::Result<()> {
    if input.is_empty() {
        return Ok(());
    }

    match input[0] {
        // Init: [0, admin[32], min_delay[8]]
        0 => {
            if input.len() < 41 {
                return Err(InvalidInput);
            }
            let mut admin = [0u8; 32];
            admin.copy_from_slice(&input[1..33]);
            let min_delay = u64::from_le_bytes(input[33..41].try_into().unwrap());
            init(&admin, min_delay)
        }
        // Queue: [1, caller[32], target[32], value[8], delay[8], current_time[8]]
        1 => {
            if input.len() < 89 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut target = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            target.copy_from_slice(&input[33..65]);
            let value = u64::from_le_bytes(input[65..73].try_into().unwrap());
            let delay = u64::from_le_bytes(input[73..81].try_into().unwrap());
            let current_time = u64::from_le_bytes(input[81..89].try_into().unwrap());
            queue_transaction(&caller, &target, value, delay, current_time)
        }
        // Execute: [2, caller[32], tx_id[32], current_time[8]]
        2 => {
            if input.len() < 73 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut tx_id = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            tx_id.copy_from_slice(&input[33..65]);
            let current_time = u64::from_le_bytes(input[65..73].try_into().unwrap());
            execute_transaction(&caller, &tx_id, current_time)
        }
        // Cancel: [3, caller[32], tx_id[32]]
        3 => {
            if input.len() < 65 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut tx_id = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            tx_id.copy_from_slice(&input[33..65]);
            cancel_transaction(&caller, &tx_id)
        }
        // Update min delay: [4, caller[32], new_delay[8]]
        4 => {
            if input.len() < 41 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            let new_delay = u64::from_le_bytes(input[33..41].try_into().unwrap());
            update_min_delay(&caller, new_delay)
        }
        _ => Ok(()),
    }
}

fn init(admin: &Address, min_delay: u64) -> entrypoint::Result<()> {
    storage_write(KEY_ADMIN, admin).map_err(|_| StorageError)?;
    storage_write(KEY_MIN_DELAY, &min_delay.to_le_bytes()).map_err(|_| StorageError)?;
    log("Timelock initialized");
    Ok(())
}

fn queue_transaction(
    caller: &Address,
    target: &Address,
    value: u64,
    delay: u64,
    current_time: u64,
) -> entrypoint::Result<()> {
    let admin = read_admin()?;
    if *caller != admin {
        return Err(OnlyAdmin);
    }

    let min_delay = read_u64(KEY_MIN_DELAY);
    if delay < min_delay {
        return Err(DelayTooShort);
    }

    // Generate tx_id from parameters
    let tx_id = generate_tx_id(target, value, current_time);
    let eta = current_time + delay;

    // Store tx: [target(32), value(8), eta(8), status(1)] = 49 bytes
    let mut tx_data = [0u8; 49];
    tx_data[0..32].copy_from_slice(target);
    tx_data[32..40].copy_from_slice(&value.to_le_bytes());
    tx_data[40..48].copy_from_slice(&eta.to_le_bytes());
    tx_data[48] = STATUS_PENDING;

    let tx_key = make_tx_key(&tx_id);
    storage_write(&tx_key, &tx_data).map_err(|_| StorageError)?;

    set_return_data(&tx_id);
    log("Transaction queued");
    Ok(())
}

fn execute_transaction(
    caller: &Address,
    tx_id: &TxId,
    current_time: u64,
) -> entrypoint::Result<()> {
    let admin = read_admin()?;
    if *caller != admin {
        return Err(OnlyAdmin);
    }

    let tx_key = make_tx_key(tx_id);
    let mut tx_data = [0u8; 49];
    let len = storage_read(&tx_key, &mut tx_data);
    if len != 49 {
        return Err(TxNotFound);
    }

    let status = tx_data[48];
    if status == STATUS_EXECUTED {
        return Err(TxAlreadyExecuted);
    }
    if status != STATUS_PENDING {
        return Err(TxNotPending);
    }

    let eta = u64::from_le_bytes(tx_data[40..48].try_into().unwrap());
    if current_time < eta {
        return Err(TooEarly);
    }

    // Mark as executed
    tx_data[48] = STATUS_EXECUTED;
    storage_write(&tx_key, &tx_data).map_err(|_| StorageError)?;

    log("Transaction executed");
    Ok(())
}

fn cancel_transaction(caller: &Address, tx_id: &TxId) -> entrypoint::Result<()> {
    let admin = read_admin()?;
    if *caller != admin {
        return Err(OnlyAdmin);
    }

    let tx_key = make_tx_key(tx_id);
    let mut tx_data = [0u8; 49];
    let len = storage_read(&tx_key, &mut tx_data);
    if len != 49 {
        return Err(TxNotFound);
    }

    if tx_data[48] != STATUS_PENDING {
        return Err(TxNotPending);
    }

    // Mark as cancelled
    tx_data[48] = STATUS_CANCELLED;
    storage_write(&tx_key, &tx_data).map_err(|_| StorageError)?;

    log("Transaction cancelled");
    Ok(())
}

fn update_min_delay(caller: &Address, new_delay: u64) -> entrypoint::Result<()> {
    let admin = read_admin()?;
    if *caller != admin {
        return Err(OnlyAdmin);
    }

    storage_write(KEY_MIN_DELAY, &new_delay.to_le_bytes()).map_err(|_| StorageError)?;
    log("Min delay updated");
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

fn read_u64(key: &[u8]) -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

fn generate_tx_id(target: &Address, value: u64, timestamp: u64) -> TxId {
    // Simple hash: XOR target bytes with value and timestamp
    let mut id = [0u8; 32];
    id.copy_from_slice(target);
    let value_bytes = value.to_le_bytes();
    let time_bytes = timestamp.to_le_bytes();
    for i in 0..8 {
        id[i] ^= value_bytes[i];
        id[i + 8] ^= time_bytes[i];
    }
    id
}

fn make_tx_key(tx_id: &TxId) -> [u8; 35] {
    let mut key = [0u8; 35];
    key[0..3].copy_from_slice(KEY_TX_PREFIX);
    key[3..35].copy_from_slice(tx_id);
    key
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
