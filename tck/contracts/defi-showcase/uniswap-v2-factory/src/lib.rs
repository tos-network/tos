//! # Uniswap V2 Factory - TAKO Implementation
//!
//! Decentralized exchange factory for creating token pair contracts.
//! Demonstrates CREATE2-style deterministic deployment in TAKO.
//!
//! ## Key Features
//!
//! - **Pair Creation**: Create liquidity pools for token pairs
//! - **Deterministic Addresses**: Pairs have predictable addresses
//! - **Fee Management**: Configurable protocol fees
//! - **Pair Registry**: Track all created pairs with enumeration support
//!
//! ## Completeness Status
//!
//! **Current Implementation Level: ~90%**
//!
//! ✅ **Implemented Features:**
//! - Pair creation with deterministic addresses
//! - Token sorting (token0 < token1)
//! - Duplicate pair prevention
//! - Zero address validation
//! - AllPairs array storage and enumeration
//! - GetPair bidirectional lookup
//! - SetFeeTo permission control
//! - SetFeeToSetter role transfer
//! - AllPairsLength query
//! - FeeTo query
//!
//! ❌ **Known Limitations:**
//! - Mock caller (Address::new([1u8; 32])) - requires TAKO runtime integration
//! - No event emission (PairCreated) - requires TAKO event system
//! - No actual pair contract deployment - only address generation
//! - Limited to hash-based pair IDs (no true CREATE2 bytecode deployment)
//!
//! ## Rust Advantages
//!
//! - Hash-based deterministic pair IDs
//! - Type-safe token pair ordering
//! - Efficient storage with minimal overhead

#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use tako_macros::event;
use tako_sdk::{
    debug_log, get_input_data, storage_read, storage_write, Address, MAX_VALUE_SIZE, SUCCESS,
};

type AccountId = Address;
type TakoResult<T> = Result<T, u64>;

// TakoError type for error handling
#[repr(u64)]
#[derive(Debug, Clone, Copy)]
pub enum TakoError {
    StorageError = 1,
    InvalidInput = 2,
    Unauthorized = 3,
    AlreadyExists = 4,
    NotFound = 5,
    CustomError(u64),
}

impl From<TakoError> for u64 {
    fn from(err: TakoError) -> u64 {
        match err {
            TakoError::StorageError => 1,
            TakoError::InvalidInput => 2,
            TakoError::Unauthorized => 3,
            TakoError::AlreadyExists => 4,
            TakoError::NotFound => 5,
            TakoError::CustomError(code) => code,
        }
    }
}

// Maximum storage buffer size (optimized for memory efficiency)
// Factory data: pair_id (32) + length (8) = 40 bytes max
const STORAGE_BUFFER_SIZE: usize = 256;

// Storage helper functions
fn get_storage(key: &[u8]) -> TakoResult<Vec<u8>> {
    let mut buffer = vec![0u8; STORAGE_BUFFER_SIZE];
    match storage_read(key, &mut buffer) {
        SUCCESS => {
            // Find actual data length (trim zeros)
            let len = buffer
                .iter()
                .rposition(|&b| b != 0)
                .map(|i| i + 1)
                .unwrap_or(0);
            buffer.truncate(len);
            Ok(buffer)
        }
        _ => Err(TakoError::NotFound.into()),
    }
}

fn set_storage(key: &[u8], value: &[u8]) -> TakoResult<()> {
    storage_write(key, value).map_err(|_| TakoError::StorageError.into())
}

// Blake3 hash function
fn blake3_hash(data: &[u8]) -> [u8; 32] {
    tako_sdk::syscalls::blake3(data)
}

const KEY_FEE_TO: &[u8] = b"fee_to";
const KEY_FEE_TO_SETTER: &[u8] = b"fee_to_setter";
const KEY_ALL_PAIRS_LENGTH: &[u8] = b"all_pairs_length";

/// Get storage key for pair by index
fn pair_index_key(index: u64) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[..8].copy_from_slice(b"pair_idx");
    key[8..].copy_from_slice(&index.to_le_bytes());
    key
}

/// Get pair storage key from token addresses
fn pair_key(token_a: &AccountId, token_b: &AccountId) -> [u8; 72] {
    let mut key = [0u8; 72];
    key[..8].copy_from_slice(b"pair_key");

    // Sort tokens (token0 < token1)
    let (token0, token1) = if token_a.0 < token_b.0 {
        (token_a, token_b)
    } else {
        (token_b, token_a)
    };

    key[8..40].copy_from_slice(&token0.0);
    key[40..72].copy_from_slice(&token1.0);
    key
}

// ============================================================================
// Event Definitions
// ============================================================================

/// PairCreated event
/// Emitted when a new liquidity pair is created
#[event]
pub struct PairCreated {
    pub token0: Address,
    pub token1: Address,
    pub pair: Address,
    pub pair_count: u64,
}

/// Contract instructions
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Instruction {
    Initialize = 0,
    CreatePair = 1,
    SetFeeTo = 2,
    SetFeeToSetter = 3,

    // Queries
    GetPair = 100,
    AllPairsLength = 101,
    FeeTo = 102,
    GetPairAtIndex = 103,
}

impl Instruction {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Initialize),
            1 => Some(Self::CreatePair),
            2 => Some(Self::SetFeeTo),
            3 => Some(Self::SetFeeToSetter),
            100 => Some(Self::GetPair),
            101 => Some(Self::AllPairsLength),
            102 => Some(Self::FeeTo),
            103 => Some(Self::GetPairAtIndex),
            _ => None,
        }
    }
}

/// Get the caller's account ID from transaction context
///
/// Uses TAKO runtime syscall `get_caller` to retrieve the direct caller address.
/// Used for access control in `set_fee_to()` and `set_fee_to_setter()`.
///
/// # Returns
/// The 32-byte address of the account that directly invoked this contract
fn get_caller() -> AccountId {
    let caller_bytes = tako_sdk::syscalls::get_caller();
    Address::new(caller_bytes)
}

/// Initialize factory
fn initialize(_input: &[u8]) -> TakoResult<Vec<u8>> {
    debug_log!("UniswapV2Factory: Initializing");

    // Check if already initialized
    if get_storage(KEY_FEE_TO_SETTER).is_ok() {
        return Err(TakoError::CustomError(999).into());
    }

    let caller = get_caller();
    set_storage(KEY_FEE_TO_SETTER, &caller.0)?;
    set_storage(KEY_ALL_PAIRS_LENGTH, &[0u8; 8])?;

    debug_log!("UniswapV2Factory: Initialized successfully");
    Ok(vec![1])
}

/// Create a new pair
fn create_pair(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 64 {
        return Err(TakoError::InvalidInput.into());
    }

    let token_a = match Address::from_slice(&input[..32]) {
        Some(addr) => addr,
        None => return Err(1u64),
    };
    let token_b = match Address::from_slice(&input[32..64]) {
        Some(addr) => addr,
        None => return Err(1u64),
    };

    // Sort tokens to ensure token0 < token1
    let (token0, token1) = if token_a.0 < token_b.0 {
        (token_a, token_b)
    } else {
        (token_b, token_a)
    };

    // Zero address check (Solidity: require(token0 != address(0)))
    if token0.0 == [0u8; 32] {
        return Err(TakoError::CustomError(5).into()); // Zero address
    }

    // Check tokens are different
    if token0.0 == token1.0 {
        return Err(TakoError::CustomError(2).into()); // Identical addresses
    }

    // Check pair doesn't exist
    let key = pair_key(&token0, &token1);
    if get_storage(&key).is_ok() {
        return Err(TakoError::CustomError(3).into()); // Pair exists
    }

    // Create deterministic pair ID using Blake3 (equivalent to CREATE2)
    let mut hash_input = [0u8; 64];
    hash_input[..32].copy_from_slice(&token0.0);
    hash_input[32..].copy_from_slice(&token1.0);

    let pair_id = blake3_hash(&hash_input);

    // Store pair in mapping (token0, token1 -> pair)
    set_storage(&key, &pair_id)?;

    // Get current pair count
    let length_bytes = get_storage(KEY_ALL_PAIRS_LENGTH).unwrap_or_default();
    let mut length_arr = [0u8; 8];
    if !length_bytes.is_empty() {
        length_arr.copy_from_slice(&length_bytes[..8]);
    }
    let length = u64::from_le_bytes(length_arr);

    // Store pair in allPairs array (index -> pair)
    let idx_key = pair_index_key(length);
    set_storage(&idx_key, &pair_id)?;

    // Increment pair count
    let new_pair_count = length + 1;
    set_storage(KEY_ALL_PAIRS_LENGTH, &new_pair_count.to_le_bytes())?;

    // Convert pair_id to AccountId for event
    let pair_id_account = Address::new(pair_id);

    // Emit PairCreated event
    PairCreated {
        token0,
        token1,
        pair: pair_id_account,
        pair_count: new_pair_count,
    }
    .emit()
    .ok();

    debug_log!("UniswapV2Factory: Created pair");

    Ok(pair_id.to_vec())
}

/// Set fee recipient
fn set_fee_to(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    // Require fee_to_setter permission
    let caller = get_caller();
    let setter_bytes = get_storage(KEY_FEE_TO_SETTER)?;
    let setter = match Address::from_slice(&setter_bytes[..32]) {
        Some(addr) => addr,
        None => return Err(1u64),
    };

    if caller != setter {
        return Err(TakoError::CustomError(4).into()); // Unauthorized
    }

    let fee_to = match Address::from_slice(&input[..32]) {
        Some(addr) => addr,
        None => return Err(1u64),
    };

    set_storage(KEY_FEE_TO, &fee_to.0)?;

    debug_log!("UniswapV2Factory: Updated fee recipient");

    Ok(vec![1])
}

/// Set fee to setter (transfer feeToSetter role)
fn set_fee_to_setter(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    // Require current fee_to_setter permission
    let caller = get_caller();
    let setter_bytes = get_storage(KEY_FEE_TO_SETTER)?;
    let setter = match Address::from_slice(&setter_bytes[..32]) {
        Some(addr) => addr,
        None => return Err(1u64),
    };

    if caller != setter {
        return Err(TakoError::CustomError(4).into()); // Unauthorized
    }

    let new_setter = match Address::from_slice(&input[..32]) {
        Some(addr) => addr,
        None => return Err(1u64),
    };

    set_storage(KEY_FEE_TO_SETTER, &new_setter.0)?;

    debug_log!("UniswapV2Factory: Updated fee_to_setter");

    Ok(vec![1])
}

/// Query pair address (FIX: Added token sorting per audit)
fn get_pair(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 64 {
        return Err(TakoError::InvalidInput.into());
    }

    let token_a = match Address::from_slice(&input[..32]) {
        Some(addr) => addr,
        None => return Err(1u64),
    };
    let token_b = match Address::from_slice(&input[32..64]) {
        Some(addr) => addr,
        None => return Err(1u64),
    };

    // FIX: Sort tokens before lookup (audit finding)
    // This ensures bidirectional lookup works regardless of input order
    let key = pair_key(&token_a, &token_b);
    let pair_bytes = get_storage(&key)?;

    Ok(pair_bytes)
}

/// Query all pairs length
fn all_pairs_length(_input: &[u8]) -> TakoResult<Vec<u8>> {
    let length_bytes = get_storage(KEY_ALL_PAIRS_LENGTH)?;
    Ok(length_bytes)
}

/// Query fee recipient
fn fee_to(_input: &[u8]) -> TakoResult<Vec<u8>> {
    let fee_to_bytes = get_storage(KEY_FEE_TO).unwrap_or_else(|_| vec![0u8; 32]);
    Ok(fee_to_bytes)
}

/// Get pair by index (additional helper for enumeration)
fn get_pair_at_index(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 8 {
        return Err(TakoError::InvalidInput.into());
    }

    let index = u64::from_le_bytes([
        input[0], input[1], input[2], input[3], input[4], input[5], input[6], input[7],
    ]);

    let idx_key = pair_index_key(index);
    let pair_bytes = get_storage(&idx_key)?;

    Ok(pair_bytes)
}

/// Main entrypoint
const MAX_INPUT_SIZE: usize = 128; // Reduced from 1024 to minimize stack usage

#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    // Read input data via syscall
    let mut buffer = [0u8; MAX_INPUT_SIZE];
    let len = get_input_data(&mut buffer);
    let input = &buffer[..len as usize];

    match process_instruction(input) {
        Ok(_) => 0,
        Err(e) => e as u64,
    }
}

fn process_instruction(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.is_empty() {
        return Err(TakoError::InvalidInput.into());
    }

    let instruction = match Instruction::from_u8(input[0]) {
        Some(instr) => instr,
        None => return Err(0u64),
    };

    let args = if input.len() > 1 { &input[1..] } else { &[] };

    match instruction {
        Instruction::Initialize => initialize(args),
        Instruction::CreatePair => create_pair(args),
        Instruction::SetFeeTo => set_fee_to(args),
        Instruction::SetFeeToSetter => set_fee_to_setter(args),
        Instruction::GetPair => get_pair(args),
        Instruction::AllPairsLength => all_pairs_length(args),
        Instruction::FeeTo => fee_to(args),
        Instruction::GetPairAtIndex => get_pair_at_index(args),
    }
}

// ============================================================================
// Required for no_std target
// ============================================================================

use core::alloc::{GlobalAlloc, Layout};

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
