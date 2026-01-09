//! Test utilities for AI-generated tests
//!
//! Provides TestEnv and helper functions for contract testing.

use std::collections::HashMap;

/// Address type for testing (20-byte address)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Address([u8; 20]);

impl Address {
    /// Create a new address from bytes
    pub fn new(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// Create a zero address
    pub fn zero() -> Self {
        Self([0u8; 20])
    }

    /// Get address as bytes
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Create address from a name (for testing)
    pub fn from_name(name: &str) -> Self {
        let mut bytes = [0u8; 20];
        let name_bytes = name.as_bytes();
        let len = name_bytes.len().min(20);
        bytes[..len].copy_from_slice(&name_bytes[..len]);
        Self(bytes)
    }
}

/// Contract execution result
#[derive(Debug, Clone)]
pub struct CallResult {
    /// Whether the call succeeded
    pub success: bool,
    /// Return data from the call
    pub return_data: Vec<u8>,
    /// Gas used
    pub gas_used: u64,
    /// Whether the call reverted
    pub reverted: bool,
}

/// Event emitted during execution
#[derive(Debug, Clone)]
pub struct Event {
    /// Contract that emitted the event
    pub address: Address,
    /// Event topics (LOG0-LOG4)
    pub topics: Vec<[u8; 32]>,
    /// Event data
    pub data: Vec<u8>,
}

/// Contract state for testing
#[derive(Debug, Clone, Default)]
struct ContractState {
    /// Contract bytecode
    code: Vec<u8>,
    /// Contract storage
    storage: HashMap<[u8; 32], [u8; 32]>,
    /// Contract balance
    balance: u64,
}

/// Test environment for contract testing
///
/// Provides a simulated blockchain environment for testing contracts.
#[derive(Debug, Default)]
pub struct TestEnv {
    /// Deployed contracts
    contracts: HashMap<Address, ContractState>,
    /// Account balances
    balances: HashMap<Address, u64>,
    /// Account nonces
    nonces: HashMap<Address, u64>,
    /// Emitted events
    events: Vec<Event>,
    /// Current block number
    block_number: u64,
    /// Current timestamp
    timestamp: u64,
}

impl TestEnv {
    /// Create a new test environment
    pub fn new() -> Self {
        Self {
            contracts: HashMap::new(),
            balances: HashMap::new(),
            nonces: HashMap::new(),
            events: Vec::new(),
            block_number: 1,
            timestamp: 1000000,
        }
    }

    /// Deploy a contract with default bytecode
    pub fn deploy_contract(&mut self, name: &str) -> Address {
        self.deploy_contract_with_balance(name, 0)
    }

    /// Deploy a contract with initial balance
    pub fn deploy_contract_with_balance(&mut self, name: &str, balance: u64) -> Address {
        let address = self.compute_deploy_address(name);
        let bytecode = get_bytecode(name);

        self.contracts.insert(
            address.clone(),
            ContractState {
                code: bytecode,
                storage: HashMap::new(),
                balance,
            },
        );

        self.balances.insert(address.clone(), balance);

        address
    }

    /// Deploy a contract with specific bytecode
    pub fn deploy_contract_with_code(&mut self, bytecode: Vec<u8>) -> Address {
        let nonce = self.nonces.entry(Address::zero()).or_insert(0);
        *nonce += 1;

        let address = compute_contract_address(&Address::zero(), *nonce);

        self.contracts.insert(
            address.clone(),
            ContractState {
                code: bytecode,
                storage: HashMap::new(),
                balance: 0,
            },
        );

        address
    }

    /// Call a contract
    pub fn call_contract(
        &mut self,
        _caller: &Address,
        target: &Address,
        input: Vec<u8>,
        value: u64,
        gas_limit: u64,
    ) -> CallResult {
        // Check if contract exists
        if !self.contracts.contains_key(target) {
            return CallResult {
                success: false,
                return_data: vec![],
                gas_used: 0,
                reverted: true,
            };
        }

        // Simulate successful call
        let gas_used = gas_limit.min(21000 + input.len() as u64 * 16);

        // If value transfer, update balances
        if value > 0 {
            if let Some(contract) = self.contracts.get_mut(target) {
                contract.balance = contract.balance.saturating_add(value);
            }
        }

        CallResult {
            success: true,
            return_data: vec![],
            gas_used,
            reverted: false,
        }
    }

    /// Static call (read-only)
    pub fn static_call(
        &mut self,
        caller: &Address,
        target: &Address,
        input: Vec<u8>,
        gas_limit: u64,
    ) -> CallResult {
        self.call_contract(caller, target, input, 0, gas_limit)
    }

    /// Delegate call
    pub fn delegate_call(
        &mut self,
        caller: &Address,
        target: &Address,
        input: Vec<u8>,
        gas_limit: u64,
    ) -> CallResult {
        self.call_contract(caller, target, input, 0, gas_limit)
    }

    /// Get contract balance
    pub fn get_balance(&self, address: &Address) -> u64 {
        self.balances.get(address).copied().unwrap_or(0)
    }

    /// Set account balance
    pub fn set_balance(&mut self, address: &Address, balance: u64) {
        self.balances.insert(address.clone(), balance);
    }

    /// Get contract code
    pub fn get_code(&self, address: &Address) -> Vec<u8> {
        self.contracts
            .get(address)
            .map(|c| c.code.clone())
            .unwrap_or_default()
    }

    /// Get storage value
    pub fn get_storage(&self, address: &Address, key: [u8; 32]) -> [u8; 32] {
        self.contracts
            .get(address)
            .and_then(|c| c.storage.get(&key))
            .copied()
            .unwrap_or([0u8; 32])
    }

    /// Set storage value
    pub fn set_storage(&mut self, address: &Address, key: [u8; 32], value: [u8; 32]) {
        if let Some(contract) = self.contracts.get_mut(address) {
            contract.storage.insert(key, value);
        }
    }

    /// Get emitted events
    pub fn get_events(&self, address: &Address) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| &e.address == address)
            .collect()
    }

    /// Emit an event
    pub fn emit_event(&mut self, address: Address, topics: Vec<[u8; 32]>, data: Vec<u8>) {
        self.events.push(Event {
            address,
            topics,
            data,
        });
    }

    /// Get current block number
    pub fn block_number(&self) -> u64 {
        self.block_number
    }

    /// Advance block number
    pub fn mine_block(&mut self) {
        self.block_number += 1;
        self.timestamp += 12; // ~12 second block time
    }

    /// Get current timestamp
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Check if address is a contract
    pub fn is_contract(&self, address: &Address) -> bool {
        self.contracts.contains_key(address)
    }

    /// Get code size
    pub fn get_code_size(&self, address: &Address) -> usize {
        self.contracts
            .get(address)
            .map(|c| c.code.len())
            .unwrap_or(0)
    }

    /// Compute deployment address from name
    fn compute_deploy_address(&mut self, name: &str) -> Address {
        let nonce = self.nonces.entry(Address::zero()).or_insert(0);
        *nonce += 1;

        // Simple deterministic address from name and nonce
        let mut bytes = [0u8; 20];
        let name_bytes = name.as_bytes();
        let len = name_bytes.len().min(18);
        bytes[..len].copy_from_slice(&name_bytes[..len]);
        bytes[18] = (*nonce >> 8) as u8;
        bytes[19] = *nonce as u8;

        Address(bytes)
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Compute contract address from deployer and nonce (CREATE opcode)
///
/// Uses RLP encoding: address = keccak256(rlp([sender, nonce]))[12:]
pub fn compute_contract_address(deployer: &Address, nonce: u64) -> Address {
    // Simplified: hash(deployer || nonce)
    let mut data = Vec::with_capacity(28);
    data.extend_from_slice(deployer.as_bytes());
    data.extend_from_slice(&nonce.to_be_bytes());

    let hash = blake3_hash(&data);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..32]);
    Address(addr)
}

/// Compute CREATE2 address
///
/// address = keccak256(0xff ++ sender ++ salt ++ keccak256(init_code))[12:]
pub fn compute_create2_address(sender: &Address, salt: [u8; 32], init_code: &[u8]) -> Address {
    let code_hash = blake3_hash(init_code);

    let mut data = Vec::with_capacity(85);
    data.push(0xff);
    data.extend_from_slice(sender.as_bytes());
    data.extend_from_slice(&salt);
    data.extend_from_slice(&code_hash);

    let hash = blake3_hash(&data);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..32]);
    Address(addr)
}

/// Get sample bytecode for a named contract
pub fn get_bytecode(name: &str) -> Vec<u8> {
    match name {
        "target_contract" => vec![0x60, 0x00, 0x60, 0x00, 0xF3], // PUSH 0, PUSH 0, RETURN
        "caller_contract" => vec![0x60, 0x00, 0x60, 0x00, 0xF3],
        "storage_contract" => vec![0x60, 0x00, 0x55, 0x60, 0x00, 0x54, 0xF3], // SSTORE, SLOAD
        "event_contract" => vec![0x60, 0x00, 0xA0],                           // LOG0
        "erc20" => vec![0x60, 0x00, 0x60, 0x00, 0xF3],
        "factory" => vec![0x60, 0x00, 0xF0], // CREATE
        "proxy" => vec![0x60, 0x00, 0xF4],   // DELEGATECALL
        "different_bytecode" => vec![0x60, 0x01, 0x60, 0x00, 0xF3],
        _ => vec![0x60, 0x00, 0x60, 0x00, 0xF3], // Default: simple return
    }
}

/// Blake3 hash (used instead of keccak256 for TOS)
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    tos_common::crypto::hash(data).to_bytes()
}

/// Keccak256 hash (compatibility alias)
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    // TOS uses blake3, but we provide keccak256 name for compatibility
    blake3_hash(data)
}

/// Encode a function call with selector and arguments
pub fn encode_function_call(selector: [u8; 4], args: &[[u8; 32]]) -> Vec<u8> {
    let mut data = Vec::with_capacity(4 + args.len() * 32);
    data.extend_from_slice(&selector);
    for arg in args {
        data.extend_from_slice(arg);
    }
    data
}

/// Encode set storage call
pub fn encode_set_storage(key: [u8; 32], value: [u8; 32]) -> Vec<u8> {
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(&[0x55, 0x00, 0x00, 0x00]); // SSTORE selector
    data.extend_from_slice(&key);
    data.extend_from_slice(&value);
    data
}

/// Encode write storage call
pub fn encode_write_storage() -> Vec<u8> {
    vec![0x55, 0x00, 0x00, 0x00] // SSTORE selector
}

/// Encode transfer call
pub fn encode_transfer() -> Vec<u8> {
    // transfer(address,uint256) selector
    vec![0xa9, 0x05, 0x9c, 0xbb]
}

/// Encode emit log call
pub fn encode_emit_log() -> Vec<u8> {
    vec![0xa0] // LOG0 opcode
}

/// Encode create call
pub fn encode_create() -> Vec<u8> {
    vec![0xf0] // CREATE opcode
}

/// Encode selfdestruct call
pub fn encode_selfdestruct() -> Vec<u8> {
    vec![0xff] // SELFDESTRUCT opcode
}

/// Encode transfer data for ERC20
pub fn encode_transfer_data(from: Address, to: Address, amount: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(100);
    // transferFrom(address,address,uint256) selector
    data.extend_from_slice(&[0x23, 0xb8, 0x72, 0xdd]);
    data.extend_from_slice(&pad_address(from));
    data.extend_from_slice(&pad_address(to));
    data.extend_from_slice(&encode_u256(amount));
    data
}

/// Encode partial transfer data
pub fn encode_transfer_data_partial(to: Address, amount: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(68);
    // transfer(address,uint256) selector
    data.extend_from_slice(&[0xa9, 0x05, 0x9c, 0xbb]);
    data.extend_from_slice(&pad_address(to));
    data.extend_from_slice(&encode_u256(amount));
    data
}

/// Pad address to 32 bytes
pub fn pad_address(addr: Address) -> [u8; 32] {
    let mut result = [0u8; 32];
    result[12..32].copy_from_slice(addr.as_bytes());
    result
}

/// Encode u64 as 32-byte big-endian
pub fn encode_u256(value: u64) -> [u8; 32] {
    let mut result = [0u8; 32];
    result[24..32].copy_from_slice(&value.to_be_bytes());
    result
}

/// Decode u256 to u64 (truncating)
pub fn decode_u256(data: &[u8]) -> u64 {
    if data.len() < 8 {
        return 0;
    }
    let start = data.len().saturating_sub(8);
    let bytes: [u8; 8] = data[start..].try_into().unwrap_or([0u8; 8]);
    u64::from_be_bytes(bytes)
}

/// Measure gas (placeholder - returns estimated gas)
pub fn measure_gas<F: FnOnce()>(f: F) -> u64 {
    f();
    21000 // Base gas cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_deploy_contract() {
        let mut env = TestEnv::new();
        let addr = env.deploy_contract("test");
        assert!(env.is_contract(&addr));
    }

    #[test]
    fn test_env_call_contract() {
        let mut env = TestEnv::new();
        let target = env.deploy_contract("target");
        let caller = env.deploy_contract("caller");

        let result = env.call_contract(&caller, &target, vec![], 0, 100000);
        assert!(result.success);
    }

    #[test]
    fn test_compute_contract_address() {
        let deployer = Address::from_name("deployer");
        let addr1 = compute_contract_address(&deployer, 1);
        let addr2 = compute_contract_address(&deployer, 2);
        assert_ne!(addr1, addr2);
    }

    #[test]
    fn test_encode_decode_u256() {
        let value = 12345u64;
        let encoded = encode_u256(value);
        let decoded = decode_u256(&encoded);
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_pad_address() {
        let addr = Address::from_name("test");
        let padded = pad_address(addr.clone());
        assert_eq!(&padded[12..32], addr.as_bytes());
    }
}
