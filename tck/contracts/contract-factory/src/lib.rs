//! # Contract Factory Example
//!
//! This contract demonstrates a factory pattern for deploying contracts,
//! similar to Ethereum's CREATE2 opcode. Since TAKO VM doesn't support
//! in-contract deployment (by design for security), this factory uses an
//! off-chain deployment service pattern.
//!
//! ## Architecture
//!
//! ```text
//! User → Factory Contract (on-chain)
//!   ↓ emit DeploymentRequested event
//!   ↓
//! Off-chain Service (watches events)
//!   ↓ sends DeployContract transaction
//!   ↓
//! TOS Blockchain → New Contract Deployed
//! ```
//!
//! ## Features
//!
//! - **Deterministic Addresses**: Uses CREATE2-style address calculation
//! - **Event-Driven**: Off-chain service listens for deployment events
//! - **Gas Efficient**: No need to store bytecode on-chain
//! - **Flexible**: Supports any contract template
//!
//! ## Usage
//!
//! 1. Deploy this factory contract once
//! 2. Run the off-chain deployment service (see README.md)
//! 3. Call `request_deployment()` to create new contracts
//! 4. Off-chain service deploys the actual contract

#![no_std]
#![no_main]

use core::panic::PanicInfo;

// ============================================================================
// Storage Layout
// ============================================================================

/// Storage key prefixes
const PREFIX_DEPLOYMENT_RECORD: u8 = 0x01;
const PREFIX_OWNER: u8 = 0x02;
const PREFIX_TEMPLATE_HASH: u8 = 0x03;
const PREFIX_DEPLOYMENT_COUNT: u8 = 0x04;
const PREFIX_DEPLOYMENT_FEE: u8 = 0x05;

/// Maximum deployment fee (10 TOS = 10_000_000_000 nanoTOS)
const MAX_DEPLOYMENT_FEE: u64 = 10_000_000_000;

// ============================================================================
// Data Structures
// ============================================================================

/// Deployment record stored on-chain
#[repr(C)]
struct DeploymentRecord {
    deployer: [u8; 32],         // Who requested the deployment
    contract_address: [u8; 32], // Predicted contract address
    bytecode_hash: [u8; 32],    // Hash of the bytecode to deploy
    timestamp: u64,             // When deployment was requested
    deployed: bool,             // Whether deployment completed
}

// ============================================================================
// External Functions (Syscalls)
// ============================================================================

extern "C" {
    fn storage_read(
        key_ptr: *const u8,
        key_len: u64,
        value_ptr: *mut u8,
        value_len: u64,
    ) -> u64;
    fn storage_write(
        key_ptr: *const u8,
        key_len: u64,
        value_ptr: *const u8,
        value_len: u64,
    ) -> u64;
    fn get_caller(output: *mut u8) -> u64;
    fn get_contract_hash(output: *mut u8) -> u64;
    fn get_timestamp() -> u64;
    fn get_call_value() -> u64;
    fn get_input_data(data_ptr: *mut u8, data_len: u64) -> u64;
    fn emit_event(
        topic_ptr: *const u8,
        topic_len: u64,
        data_ptr: *const u8,
        data_len: u64,
    ) -> u64;
    fn keccak256(input_ptr: *const u8, input_len: u64, output_ptr: *mut u8) -> u64;
    fn panic(msg_ptr: *const u8, msg_len: u64) -> !;
    fn set_return_data(data_ptr: *const u8, data_len: u64) -> u64;
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Panic handler
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe {
        panic(b"Contract panicked\0".as_ptr(), 18);
    }
}

/// Copy memory (since we're in no_std)
unsafe fn memcpy(dest: *mut u8, src: *const u8, n: usize) {
    for i in 0..n {
        *dest.add(i) = *src.add(i);
    }
}

/// Compare two byte arrays
unsafe fn memcmp(a: *const u8, b: *const u8, n: usize) -> bool {
    for i in 0..n {
        if *a.add(i) != *b.add(i) {
            return false;
        }
    }
    true
}

/// Fill memory with zeros
unsafe fn memset(dest: *mut u8, val: u8, n: usize) {
    for i in 0..n {
        *dest.add(i) = val;
    }
}

// ============================================================================
// Storage Access Functions
// ============================================================================

/// Read owner address from storage
unsafe fn read_owner(owner: *mut [u8; 32]) -> bool {
    let key = [PREFIX_OWNER];
    let result = storage_read(key.as_ptr(), 1, owner as *mut u8, 32);
    result == 0
}

/// Write owner address to storage
unsafe fn write_owner(owner: *const [u8; 32]) {
    let key = [PREFIX_OWNER];
    storage_write(key.as_ptr(), 1, owner as *const u8, 32);
}

/// Read template bytecode hash
unsafe fn read_template_hash(hash: *mut [u8; 32]) -> bool {
    let key = [PREFIX_TEMPLATE_HASH];
    let result = storage_read(key.as_ptr(), 1, hash as *mut u8, 32);
    result == 0
}

/// Write template bytecode hash
unsafe fn write_template_hash(hash: *const [u8; 32]) {
    let key = [PREFIX_TEMPLATE_HASH];
    storage_write(key.as_ptr(), 1, hash as *const u8, 32);
}

/// Read deployment count
unsafe fn read_deployment_count() -> u64 {
    let key = [PREFIX_DEPLOYMENT_COUNT];
    let mut count: u64 = 0;
    let result = storage_read(key.as_ptr(), 1, &mut count as *mut u64 as *mut u8, 8);
    if result == 0 {
        count
    } else {
        0
    }
}

/// Write deployment count
unsafe fn write_deployment_count(count: u64) {
    let key = [PREFIX_DEPLOYMENT_COUNT];
    storage_write(key.as_ptr(), 1, &count as *const u64 as *const u8, 8);
}

/// Read deployment fee
unsafe fn read_deployment_fee() -> u64 {
    let key = [PREFIX_DEPLOYMENT_FEE];
    let mut fee: u64 = 0;
    let result = storage_read(key.as_ptr(), 1, &mut fee as *mut u64 as *mut u8, 8);
    if result == 0 {
        fee
    } else {
        0
    }
}

/// Write deployment fee
unsafe fn write_deployment_fee(fee: u64) {
    let key = [PREFIX_DEPLOYMENT_FEE];
    storage_write(key.as_ptr(), 1, &fee as *const u64 as *const u8, 8);
}

/// Read deployment record
unsafe fn read_deployment_record(salt: *const [u8; 32], record: *mut DeploymentRecord) -> bool {
    let mut key = [0u8; 33];
    key[0] = PREFIX_DEPLOYMENT_RECORD;
    memcpy(key.as_mut_ptr().add(1), salt as *const u8, 32);

    let result = storage_read(
        key.as_ptr(),
        33,
        record as *mut u8,
        core::mem::size_of::<DeploymentRecord>() as u64,
    );
    result == 0
}

/// Write deployment record
unsafe fn write_deployment_record(salt: *const [u8; 32], record: *const DeploymentRecord) {
    let mut key = [0u8; 33];
    key[0] = PREFIX_DEPLOYMENT_RECORD;
    memcpy(key.as_mut_ptr().add(1), salt as *const u8, 32);

    storage_write(
        key.as_ptr(),
        33,
        record as *const u8,
        core::mem::size_of::<DeploymentRecord>() as u64,
    );
}

// ============================================================================
// CREATE2 Address Calculation
// ============================================================================

/// Compute CREATE2-style deterministic address
/// address = keccak256(0xFF || factory_address || salt || bytecode_hash)
unsafe fn compute_create2_address(
    salt: *const [u8; 32],
    bytecode_hash: *const [u8; 32],
    output: *mut [u8; 32],
) {
    let mut factory_address = [0u8; 32];
    get_contract_hash(factory_address.as_mut_ptr());

    // Build preimage: 0xFF || factory || salt || bytecode_hash
    let mut preimage = [0u8; 97]; // 1 + 32 + 32 + 32
    preimage[0] = 0xFF;
    memcpy(preimage.as_mut_ptr().add(1), factory_address.as_ptr(), 32);
    memcpy(preimage.as_mut_ptr().add(33), salt as *const u8, 32);
    memcpy(
        preimage.as_mut_ptr().add(65),
        bytecode_hash as *const u8,
        32,
    );

    // Hash to get address
    keccak256(preimage.as_ptr(), 97, output as *mut u8);
}

// ============================================================================
// Contract Entry Points
// ============================================================================

/// Initialize the factory
///
/// # Input Format
/// * First 32 bytes: template bytecode hash
/// * Next 8 bytes: deployment fee (in nanoTOS)
///
/// # Security
/// - Owner is automatically set to contract deployer (caller)
/// - Template hash must be provided and non-zero
/// - Fee must not exceed MAX_DEPLOYMENT_FEE
#[no_mangle]
pub extern "C" fn constructor() {
    unsafe {
        // Read input data: template_hash (32 bytes) + fee (8 bytes)
        let mut input = [0u8; 40];
        let actual_len = get_input_data(input.as_mut_ptr(), 40);

        if actual_len < 40 {
            panic(b"Constructor requires 40 bytes input\0".as_ptr(), 36);
        }

        // Set caller as owner
        let mut owner = [0u8; 32];
        get_caller(owner.as_mut_ptr());
        write_owner(&owner);

        // Read and validate template hash (first 32 bytes)
        let mut template_hash = [0u8; 32];
        memcpy(template_hash.as_mut_ptr(), input.as_ptr(), 32);

        // Validate template hash is not zero
        let mut is_zero = true;
        for i in 0..32 {
            if template_hash[i] != 0 {
                is_zero = false;
                break;
            }
        }
        if is_zero {
            panic(b"Template hash cannot be zero\0".as_ptr(), 28);
        }

        write_template_hash(&template_hash);

        // Read and validate deployment fee (next 8 bytes)
        let mut fee_bytes = [0u8; 8];
        memcpy(fee_bytes.as_mut_ptr(), input.as_ptr().add(32), 8);
        let fee = u64::from_le_bytes(fee_bytes);

        if fee > MAX_DEPLOYMENT_FEE {
            panic(b"Fee exceeds maximum\0".as_ptr(), 19);
        }

        write_deployment_fee(fee);

        // Initialize deployment count
        write_deployment_count(0);

        // Emit initialization event
        let mut event_data = [0u8; 72]; // owner (32) + template_hash (32) + fee (8)
        memcpy(event_data.as_mut_ptr(), owner.as_ptr(), 32);
        memcpy(event_data.as_mut_ptr().add(32), template_hash.as_ptr(), 32);
        memcpy(event_data.as_mut_ptr().add(64), fee_bytes.as_ptr(), 8);

        emit_event(
            b"FactoryInitialized\0".as_ptr(),
            18,
            event_data.as_ptr(),
            72,
        );
    }
}

/// Set template bytecode hash (owner only)
///
/// # Input
/// * 32 bytes: bytecode hash
///
/// # Security
/// - Only owner can call this function
/// - Template hash must be non-zero
/// - Emits TemplateHashUpdated event
#[no_mangle]
pub extern "C" fn set_template_hash() {
    unsafe {
        // Check caller is owner
        let mut caller = [0u8; 32];
        get_caller(caller.as_mut_ptr());

        let mut owner = [0u8; 32];
        if !read_owner(&mut owner) {
            panic(b"Owner not set\0".as_ptr(), 14);
        }

        if !memcmp(caller.as_ptr(), owner.as_ptr(), 32) {
            panic(b"Only owner can set template\0".as_ptr(), 28);
        }

        // Read template hash from input
        let mut template_hash = [0u8; 32];
        let actual_len = get_input_data(template_hash.as_mut_ptr(), 32);

        if actual_len < 32 {
            panic(b"Template hash requires 32 bytes\0".as_ptr(), 31);
        }

        // Validate template hash is not zero
        let mut is_zero = true;
        for i in 0..32 {
            if template_hash[i] != 0 {
                is_zero = false;
                break;
            }
        }
        if is_zero {
            panic(b"Template hash cannot be zero\0".as_ptr(), 28);
        }

        write_template_hash(&template_hash);

        // Emit event
        emit_event(
            b"TemplateHashUpdated\0".as_ptr(),
            19,
            template_hash.as_ptr(),
            32,
        );

        // Set return value (success = 1)
        let success: u64 = 1;
        set_return_data(&success as *const u64 as *const u8, 8);
    }
}

/// Set deployment fee (owner only)
///
/// # Input
/// * 8 bytes: fee amount in nanoTOS (little-endian)
///
/// # Security
/// - Only owner can call this function
/// - Fee must not exceed MAX_DEPLOYMENT_FEE
/// - Emits DeploymentFeeUpdated event
#[no_mangle]
pub extern "C" fn set_deployment_fee() {
    unsafe {
        // Check caller is owner
        let mut caller = [0u8; 32];
        get_caller(caller.as_mut_ptr());

        let mut owner = [0u8; 32];
        if !read_owner(&mut owner) {
            panic(b"Owner not set\0".as_ptr(), 14);
        }

        if !memcmp(caller.as_ptr(), owner.as_ptr(), 32) {
            panic(b"Only owner can set fee\0".as_ptr(), 22);
        }

        // Read fee from input
        let mut fee_bytes = [0u8; 8];
        let actual_len = get_input_data(fee_bytes.as_mut_ptr(), 8);

        if actual_len < 8 {
            panic(b"Fee requires 8 bytes\0".as_ptr(), 20);
        }

        let fee = u64::from_le_bytes(fee_bytes);

        if fee > MAX_DEPLOYMENT_FEE {
            panic(b"Fee exceeds maximum\0".as_ptr(), 19);
        }

        write_deployment_fee(fee);

        // Emit event
        emit_event(
            b"DeploymentFeeUpdated\0".as_ptr(),
            20,
            fee_bytes.as_ptr(),
            8,
        );

        // Set return value (success = 1)
        let success: u64 = 1;
        set_return_data(&success as *const u64 as *const u8, 8);
    }
}

/// Request contract deployment
///
/// # Input
/// * First 32 bytes: salt (for deterministic address)
/// * Next 32 bytes: bytecode hash
///
/// # Output
/// * 32 bytes: predicted contract address
///
/// # Events
/// Emits "DeploymentRequested" event with:
/// - deployer address (32 bytes)
/// - salt (32 bytes)
/// - bytecode_hash (32 bytes)
/// - predicted_address (32 bytes)
#[no_mangle]
pub extern "C" fn request_deployment() {
    unsafe {
        // Get caller
        let mut caller = [0u8; 32];
        get_caller(caller.as_mut_ptr());

        // Check deployment fee
        let fee = read_deployment_fee();
        if fee > 0 {
            let value = get_call_value();
            if value < fee {
                panic(b"Insufficient deployment fee\0".as_ptr(), 27);
            }
        }

        // Read input: salt (32 bytes) + bytecode_hash (32 bytes)
        let mut input = [0u8; 64];
        let actual_len = get_input_data(input.as_mut_ptr(), 64);

        if actual_len < 64 {
            panic(b"request_deployment requires 64 bytes\0".as_ptr(), 36);
        }

        // Parse salt (first 32 bytes)
        let mut salt = [0u8; 32];
        memcpy(salt.as_mut_ptr(), input.as_ptr(), 32);

        // Parse bytecode_hash (next 32 bytes)
        let mut bytecode_hash = [0u8; 32];
        memcpy(bytecode_hash.as_mut_ptr(), input.as_ptr().add(32), 32);

        // Validate bytecode_hash is not zero
        let mut is_zero = true;
        for i in 0..32 {
            if bytecode_hash[i] != 0 {
                is_zero = false;
                break;
            }
        }
        if is_zero {
            panic(b"Bytecode hash cannot be zero\0".as_ptr(), 28);
        }

        // Validate bytecode_hash matches template
        let mut template_hash = [0u8; 32];
        if !read_template_hash(&mut template_hash) {
            panic(b"Template hash not set\0".as_ptr(), 21);
        }

        if !memcmp(bytecode_hash.as_ptr(), template_hash.as_ptr(), 32) {
            panic(b"Bytecode hash must match template\0".as_ptr(), 34);
        }

        // Compute predicted address
        let mut predicted_address = [0u8; 32];
        compute_create2_address(&salt, &bytecode_hash, &mut predicted_address);

        // Check if salt already used (deployed OR pending)
        // SECURITY: Reject duplicate salts to prevent front-running attacks
        // Salt must be globally unique per factory to ensure deterministic addresses
        let mut existing_record = DeploymentRecord {
            deployer: [0u8; 32],
            contract_address: [0u8; 32],
            bytecode_hash: [0u8; 32],
            timestamp: 0,
            deployed: false,
        };

        if read_deployment_record(&salt, &mut existing_record) {
            // Salt already exists - reject outright for security
            // This prevents:
            // 1. Front-running attacks (attacker using victim's salt)
            // 2. Accidental overwrites of pending deployments
            // 3. Confusion about who owns a particular deployment
            panic(b"Salt already used\0".as_ptr(), 17);
        }

        // Create deployment record
        let timestamp = get_timestamp();
        let record = DeploymentRecord {
            deployer: caller,
            contract_address: predicted_address,
            bytecode_hash,
            timestamp,
            deployed: false,
        };

        // Store record
        write_deployment_record(&salt, &record);

        // Increment deployment count
        let count = read_deployment_count();
        write_deployment_count(count + 1);

        // Emit event for off-chain service
        let mut event_data = [0u8; 128]; // deployer + salt + bytecode_hash + predicted_address
        memcpy(event_data.as_mut_ptr(), caller.as_ptr(), 32);
        memcpy(event_data.as_mut_ptr().add(32), salt.as_ptr(), 32);
        memcpy(event_data.as_mut_ptr().add(64), bytecode_hash.as_ptr(), 32);
        memcpy(
            event_data.as_mut_ptr().add(96),
            predicted_address.as_ptr(),
            32,
        );

        emit_event(
            b"DeploymentRequested\0".as_ptr(),
            19,
            event_data.as_ptr(),
            128,
        );

        // Return predicted address to caller
        set_return_data(predicted_address.as_ptr(), 32);
    }
}

/// Mark deployment as completed (called by off-chain service after deployment)
///
/// # Input
/// * 32 bytes: salt
#[no_mangle]
pub extern "C" fn mark_deployed() {
    unsafe {
        // Get caller
        let mut caller = [0u8; 32];
        get_caller(caller.as_mut_ptr());

        // Check caller is owner (only off-chain service should call this)
        let mut owner = [0u8; 32];
        if !read_owner(&mut owner) {
            panic(b"Owner not set\0".as_ptr(), 14);
        }

        if !memcmp(caller.as_ptr(), owner.as_ptr(), 32) {
            panic(b"Only owner can mark deployed\0".as_ptr(), 29);
        }

        // Read salt from input (32 bytes)
        let mut salt = [0u8; 32];
        let actual_len = get_input_data(salt.as_mut_ptr(), 32);

        if actual_len < 32 {
            panic(b"mark_deployed requires 32 bytes salt\0".as_ptr(), 37);
        }

        // Read existing record
        let mut record = DeploymentRecord {
            deployer: [0u8; 32],
            contract_address: [0u8; 32],
            bytecode_hash: [0u8; 32],
            timestamp: 0,
            deployed: false,
        };

        if !read_deployment_record(&salt, &mut record) {
            panic(b"Deployment not found\0".as_ptr(), 20);
        }

        // Mark as deployed
        record.deployed = true;
        write_deployment_record(&salt, &record);

        // Emit event
        emit_event(b"DeploymentCompleted\0".as_ptr(), 19, salt.as_ptr(), 32);
    }
}

/// Get deployment count (view function)
///
/// # Output
/// * 8 bytes: total number of deployments requested
#[no_mangle]
pub extern "C" fn get_deployment_count() -> u64 {
    unsafe { read_deployment_count() }
}

/// Get deployment fee (view function)
///
/// # Output
/// * 8 bytes: deployment fee in nanoTOS
#[no_mangle]
pub extern "C" fn get_deployment_fee() -> u64 {
    unsafe { read_deployment_fee() }
}

/// Check if deployment is completed
///
/// # Input
/// * 32 bytes: salt
///
/// # Output
/// * 1 byte: 1 if deployed, 0 otherwise
#[no_mangle]
pub extern "C" fn is_deployed() -> u64 {
    unsafe {
        // Read salt from input (32 bytes)
        let mut salt = [0u8; 32];
        let actual_len = get_input_data(salt.as_mut_ptr(), 32);

        if actual_len < 32 {
            panic(b"is_deployed requires 32 bytes salt\0".as_ptr(), 35);
        }

        let mut record = DeploymentRecord {
            deployer: [0u8; 32],
            contract_address: [0u8; 32],
            bytecode_hash: [0u8; 32],
            timestamp: 0,
            deployed: false,
        };

        if read_deployment_record(&salt, &mut record) {
            if record.deployed {
                return 1;
            }
        }
        0
    }
}
