//! # Multisig Wallet Contract
//!
//! A multi-signature wallet that requires multiple owners to approve transactions.

#![no_std]
#![no_main]

use tako_sdk::*;

pub type Address = [u8; 32];
pub type TxId = u64;

// Transaction status
const STATUS_PENDING: u8 = 0;
const STATUS_EXECUTED: u8 = 1;
const STATUS_CANCELLED: u8 = 2;

// Storage keys
const KEY_THRESHOLD: &[u8] = b"thresh";
const KEY_OWNER_COUNT: &[u8] = b"oc";
const KEY_TX_COUNTER: &[u8] = b"txc";
const KEY_BALANCE: &[u8] = b"bal";
const KEY_OWNER_PREFIX: &[u8] = b"own:"; // own:{address} -> 1 (is owner)
const KEY_TX_PREFIX: &[u8] = b"tx:"; // tx:{id} -> to, value, status, approvals
const KEY_APPROVAL_PREFIX: &[u8] = b"appr:"; // appr:{tx_id}{owner} -> 1 (approved)

// Error codes
define_errors! {
    OnlyOwner = 1501,
    TxNotFound = 1502,
    TxNotPending = 1503,
    AlreadyApproved = 1504,
    NotApproved = 1505,
    InsufficientApprovals = 1506,
    InsufficientBalance = 1507,
    InvalidRecipient = 1508,
    StorageError = 1509,
    InvalidInput = 1510,
    InvalidThreshold = 1511,
}

entrypoint!(process_instruction);

fn process_instruction(input: &[u8]) -> entrypoint::Result<()> {
    if input.is_empty() {
        return Ok(());
    }

    match input[0] {
        // Init: [0, threshold[4], owner_count[4], owners[32 * count]...]
        0 => {
            if input.len() < 9 {
                return Err(InvalidInput);
            }
            let threshold = u32::from_le_bytes(input[1..5].try_into().unwrap());
            let owner_count = u32::from_le_bytes(input[5..9].try_into().unwrap()) as usize;
            if input.len() < 9 + owner_count * 32 {
                return Err(InvalidInput);
            }
            let mut owners = [[0u8; 32]; 10]; // Max 10 owners
            for i in 0..owner_count.min(10) {
                owners[i].copy_from_slice(&input[9 + i * 32..9 + (i + 1) * 32]);
            }
            init(threshold, owner_count as u32, &owners[..owner_count])
        }
        // Propose: [1, caller[32], to[32], value[8]]
        1 => {
            if input.len() < 73 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut to = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            to.copy_from_slice(&input[33..65]);
            let value = u64::from_le_bytes(input[65..73].try_into().unwrap());
            propose_transaction(&caller, &to, value)
        }
        // Approve: [2, caller[32], tx_id[8]]
        2 => {
            if input.len() < 41 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            let tx_id = u64::from_le_bytes(input[33..41].try_into().unwrap());
            approve_transaction(&caller, tx_id)
        }
        // Execute: [3, caller[32], tx_id[8]]
        3 => {
            if input.len() < 41 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            let tx_id = u64::from_le_bytes(input[33..41].try_into().unwrap());
            execute_transaction(&caller, tx_id)
        }
        // Deposit: [4, amount[8]]
        4 => {
            if input.len() < 9 {
                return Err(InvalidInput);
            }
            let amount = u64::from_le_bytes(input[1..9].try_into().unwrap());
            deposit(amount)
        }
        // Cancel: [5, caller[32], tx_id[8]]
        5 => {
            if input.len() < 41 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            let tx_id = u64::from_le_bytes(input[33..41].try_into().unwrap());
            cancel_transaction(&caller, tx_id)
        }
        _ => Ok(()),
    }
}

fn init(threshold: u32, owner_count: u32, owners: &[[u8; 32]]) -> entrypoint::Result<()> {
    if threshold == 0 || threshold > owner_count {
        return Err(InvalidThreshold);
    }

    storage_write(KEY_THRESHOLD, &threshold.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_OWNER_COUNT, &owner_count.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_TX_COUNTER, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_BALANCE, &0u64.to_le_bytes()).map_err(|_| StorageError)?;

    for owner in owners {
        let key = make_owner_key(owner);
        storage_write(&key, &[1u8]).map_err(|_| StorageError)?;
    }

    log("Multisig wallet initialized");
    Ok(())
}

fn propose_transaction(caller: &Address, to: &Address, value: u64) -> entrypoint::Result<()> {
    if !is_owner(caller) {
        return Err(OnlyOwner);
    }

    if *to == [0u8; 32] {
        return Err(InvalidRecipient);
    }

    let tx_id = read_u64(KEY_TX_COUNTER);

    // Store tx: [to(32), value(8), status(1), approvals(4)] = 45 bytes
    let mut tx_data = [0u8; 45];
    tx_data[0..32].copy_from_slice(to);
    tx_data[32..40].copy_from_slice(&value.to_le_bytes());
    tx_data[40] = STATUS_PENDING;
    // approvals starts at 0 (bytes 41-44)

    let key = make_tx_key(tx_id);
    storage_write(&key, &tx_data).map_err(|_| StorageError)?;

    storage_write(KEY_TX_COUNTER, &(tx_id + 1).to_le_bytes()).map_err(|_| StorageError)?;

    set_return_data(&tx_id.to_le_bytes());
    log("Transaction proposed");
    Ok(())
}

fn approve_transaction(caller: &Address, tx_id: TxId) -> entrypoint::Result<()> {
    if !is_owner(caller) {
        return Err(OnlyOwner);
    }

    let tx_key = make_tx_key(tx_id);
    let mut tx_data = [0u8; 45];
    let len = storage_read(&tx_key, &mut tx_data);
    if len != 45 {
        return Err(TxNotFound);
    }

    if tx_data[40] != STATUS_PENDING {
        return Err(TxNotPending);
    }

    // Check if already approved
    let approval_key = make_approval_key(tx_id, caller);
    let mut approval_buffer = [0u8; 1];
    if storage_read(&approval_key, &mut approval_buffer) > 0 && approval_buffer[0] != 0 {
        return Err(AlreadyApproved);
    }

    // Record approval
    storage_write(&approval_key, &[1u8]).map_err(|_| StorageError)?;

    // Increment approval count
    let approvals = u32::from_le_bytes(tx_data[41..45].try_into().unwrap());
    tx_data[41..45].copy_from_slice(&(approvals + 1).to_le_bytes());
    storage_write(&tx_key, &tx_data).map_err(|_| StorageError)?;

    log("Transaction approved");
    Ok(())
}

fn execute_transaction(caller: &Address, tx_id: TxId) -> entrypoint::Result<()> {
    if !is_owner(caller) {
        return Err(OnlyOwner);
    }

    let tx_key = make_tx_key(tx_id);
    let mut tx_data = [0u8; 45];
    let len = storage_read(&tx_key, &mut tx_data);
    if len != 45 {
        return Err(TxNotFound);
    }

    if tx_data[40] != STATUS_PENDING {
        return Err(TxNotPending);
    }

    // Check approvals
    let approvals = u32::from_le_bytes(tx_data[41..45].try_into().unwrap());
    let threshold = read_u32(KEY_THRESHOLD);
    if approvals < threshold {
        return Err(InsufficientApprovals);
    }

    // Check balance
    let value = u64::from_le_bytes(tx_data[32..40].try_into().unwrap());
    let balance = read_u64(KEY_BALANCE);
    if balance < value {
        return Err(InsufficientBalance);
    }

    // Deduct balance
    storage_write(KEY_BALANCE, &(balance - value).to_le_bytes()).map_err(|_| StorageError)?;

    // Mark as executed
    tx_data[40] = STATUS_EXECUTED;
    storage_write(&tx_key, &tx_data).map_err(|_| StorageError)?;

    log("Transaction executed");
    Ok(())
}

fn cancel_transaction(caller: &Address, tx_id: TxId) -> entrypoint::Result<()> {
    if !is_owner(caller) {
        return Err(OnlyOwner);
    }

    let tx_key = make_tx_key(tx_id);
    let mut tx_data = [0u8; 45];
    let len = storage_read(&tx_key, &mut tx_data);
    if len != 45 {
        return Err(TxNotFound);
    }

    if tx_data[40] != STATUS_PENDING {
        return Err(TxNotPending);
    }

    tx_data[40] = STATUS_CANCELLED;
    storage_write(&tx_key, &tx_data).map_err(|_| StorageError)?;

    log("Transaction cancelled");
    Ok(())
}

fn deposit(amount: u64) -> entrypoint::Result<()> {
    let balance = read_u64(KEY_BALANCE);
    storage_write(KEY_BALANCE, &(balance + amount).to_le_bytes()).map_err(|_| StorageError)?;
    log("Deposit received");
    Ok(())
}

// Helper functions

fn is_owner(address: &Address) -> bool {
    let key = make_owner_key(address);
    let mut buffer = [0u8; 1];
    let len = storage_read(&key, &mut buffer);
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

fn read_u32(key: &[u8]) -> u32 {
    let mut buffer = [0u8; 4];
    let len = storage_read(key, &mut buffer);
    if len == 4 {
        u32::from_le_bytes(buffer)
    } else {
        0
    }
}

fn make_owner_key(address: &Address) -> [u8; 36] {
    let mut key = [0u8; 36];
    key[0..4].copy_from_slice(KEY_OWNER_PREFIX);
    key[4..36].copy_from_slice(address);
    key
}

fn make_tx_key(tx_id: TxId) -> [u8; 11] {
    let mut key = [0u8; 11];
    key[0..3].copy_from_slice(KEY_TX_PREFIX);
    key[3..11].copy_from_slice(&tx_id.to_le_bytes());
    key
}

fn make_approval_key(tx_id: TxId, owner: &Address) -> [u8; 45] {
    let mut key = [0u8; 45];
    key[0..5].copy_from_slice(KEY_APPROVAL_PREFIX);
    key[5..13].copy_from_slice(&tx_id.to_le_bytes());
    key[13..45].copy_from_slice(owner);
    key
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
