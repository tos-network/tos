// gen_contract_vectors.rs - Generate TCK test vectors for InvokeContract (Type 3) and DeployContract (Type 4)
//
// Wire formats:
//
// InvokeContractPayload (Type 3):
//   contract:    Hash (32 bytes)
//   deposits:    u8 count + [asset(32) + ContractDeposit(1+8)]...
//   entry_id:    u16 BE
//   max_gas:     u64 BE
//   parameters:  u8 count + [ValueCell]...
//
// DeployContractPayload (Type 4):
//   module:      u32 len BE + bytecode (must start with 0x7F 'E' 'L' 'F')
//   invoke:      Option<InvokeConstructorPayload>
//                  - None: u8 = 0
//                  - Some: u8 = 1 + InvokeConstructorPayload
//
// InvokeConstructorPayload:
//   max_gas:     u64 BE
//   deposits:    u8 count + [asset(32) + ContractDeposit(1+8)]...
//
// ContractDeposit:
//   type_tag:    u8 = 0 (PlainText variant)
//   amount:      u64 BE
//
// Primitive types (tag + value):
//   0: Null
//   1: U8(u8)
//   2: U16(u16 BE)
//   3: U32(u32 BE)
//   4: U64(u64 BE)
//   5: U128(u128 BE)
//   6: U256(32 bytes BE)
//   7: Boolean(u8 0/1)
//   8: String(u16 len BE + bytes)
//
// ValueCell types (tag + content):
//   0: Default(Primitive)
//   1: Bytes(u32 len BE + bytes)
//   2: Object(u32 len BE + ValueCell[])
//   3: Map(u32 len BE + [key_ValueCell + value_ValueCell]...)

use serde::Serialize;
use std::fs::File;
use std::io::Write;
use tos_common::crypto::Hash;
use tos_common::serializer::Serializer;
use tos_common::transaction::{
    ContractDeposit, DeployContractPayload, Deposits, InvokeConstructorPayload,
    InvokeContractPayload,
};
use tos_kernel::{Module, Primitive, ValueCell};

#[derive(Serialize)]
struct InvokeContractVector {
    name: String,
    description: String,
    contract_hex: String,
    deposits_count: usize,
    entry_id: u16,
    max_gas: u64,
    parameters_count: usize,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct DeployContractVector {
    name: String,
    description: String,
    module_len: usize,
    has_invoke: bool,
    invoke_max_gas: Option<u64>,
    invoke_deposits_count: Option<usize>,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct ContractVectors {
    description: String,
    invoke_contract_vectors: Vec<InvokeContractVector>,
    deploy_contract_vectors: Vec<DeployContractVector>,
}

fn hash_from_bytes(bytes: &[u8; 32]) -> Hash {
    Hash::new(*bytes)
}

fn main() {
    let mut invoke_vectors = Vec::new();
    let mut deploy_vectors = Vec::new();

    // ========== InvokeContract Vectors (Type 3) ==========

    // Vector 1: Minimal invoke - no deposits, no parameters
    {
        let contract = hash_from_bytes(&[0x11u8; 32]);
        let contract_hex = hex::encode(contract.as_bytes());
        let payload = InvokeContractPayload {
            contract,
            deposits: Deposits::new(),
            entry_id: 0,
            max_gas: 1000000,
            parameters: Vec::new(),
        };
        let wire = payload.to_bytes();
        invoke_vectors.push(InvokeContractVector {
            name: "minimal_invoke".to_string(),
            description: "Minimal invoke with no deposits and no parameters".to_string(),
            contract_hex,
            deposits_count: 0,
            entry_id: 0,
            max_gas: 1000000,
            parameters_count: 0,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 2: Invoke with single deposit
    {
        let contract = hash_from_bytes(&[0x22u8; 32]);
        let contract_hex = hex::encode(contract.as_bytes());
        let asset = hash_from_bytes(&[0xAAu8; 32]);
        let mut deposits = Deposits::new();
        deposits.insert(asset, ContractDeposit::new(5000000000)); // 50 TOS
        let payload = InvokeContractPayload {
            contract,
            deposits,
            entry_id: 1,
            max_gas: 2000000,
            parameters: Vec::new(),
        };
        let wire = payload.to_bytes();
        invoke_vectors.push(InvokeContractVector {
            name: "invoke_with_deposit".to_string(),
            description: "Invoke with single asset deposit".to_string(),
            contract_hex,
            deposits_count: 1,
            entry_id: 1,
            max_gas: 2000000,
            parameters_count: 0,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 3: Invoke with multiple deposits
    {
        let contract = hash_from_bytes(&[0x33u8; 32]);
        let contract_hex = hex::encode(contract.as_bytes());
        let asset1 = hash_from_bytes(&[0xA1u8; 32]);
        let asset2 = hash_from_bytes(&[0xA2u8; 32]);
        let mut deposits = Deposits::new();
        deposits.insert(asset1, ContractDeposit::new(1000000000));
        deposits.insert(asset2, ContractDeposit::new(2000000000));
        let payload = InvokeContractPayload {
            contract,
            deposits,
            entry_id: 100,
            max_gas: 5000000,
            parameters: Vec::new(),
        };
        let wire = payload.to_bytes();
        invoke_vectors.push(InvokeContractVector {
            name: "invoke_multi_deposit".to_string(),
            description: "Invoke with multiple asset deposits".to_string(),
            contract_hex,
            deposits_count: 2,
            entry_id: 100,
            max_gas: 5000000,
            parameters_count: 0,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 4: Invoke with primitive parameters (U64, Boolean)
    {
        let contract = hash_from_bytes(&[0x44u8; 32]);
        let contract_hex = hex::encode(contract.as_bytes());
        let parameters = vec![
            ValueCell::Default(Primitive::U64(12345678)),
            ValueCell::Default(Primitive::Boolean(true)),
            ValueCell::Default(Primitive::U8(42)),
        ];
        let payload = InvokeContractPayload {
            contract,
            deposits: Deposits::new(),
            entry_id: 5,
            max_gas: 3000000,
            parameters,
        };
        let wire = payload.to_bytes();
        invoke_vectors.push(InvokeContractVector {
            name: "invoke_with_primitives".to_string(),
            description: "Invoke with primitive parameters (U64, Boolean, U8)".to_string(),
            contract_hex,
            deposits_count: 0,
            entry_id: 5,
            max_gas: 3000000,
            parameters_count: 3,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 5: Invoke with string parameter
    {
        let contract = hash_from_bytes(&[0x55u8; 32]);
        let contract_hex = hex::encode(contract.as_bytes());
        let parameters = vec![ValueCell::Default(Primitive::String(
            "Hello, Contract!".to_string(),
        ))];
        let payload = InvokeContractPayload {
            contract,
            deposits: Deposits::new(),
            entry_id: 10,
            max_gas: 1500000,
            parameters,
        };
        let wire = payload.to_bytes();
        invoke_vectors.push(InvokeContractVector {
            name: "invoke_with_string".to_string(),
            description: "Invoke with string parameter".to_string(),
            contract_hex,
            deposits_count: 0,
            entry_id: 10,
            max_gas: 1500000,
            parameters_count: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 6: Invoke with Bytes parameter
    {
        let contract = hash_from_bytes(&[0x66u8; 32]);
        let contract_hex = hex::encode(contract.as_bytes());
        let parameters = vec![ValueCell::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF])];
        let payload = InvokeContractPayload {
            contract,
            deposits: Deposits::new(),
            entry_id: 20,
            max_gas: 1000000,
            parameters,
        };
        let wire = payload.to_bytes();
        invoke_vectors.push(InvokeContractVector {
            name: "invoke_with_bytes".to_string(),
            description: "Invoke with Bytes parameter".to_string(),
            contract_hex,
            deposits_count: 0,
            entry_id: 20,
            max_gas: 1000000,
            parameters_count: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 7: Invoke with Object (array of values)
    {
        let contract = hash_from_bytes(&[0x77u8; 32]);
        let contract_hex = hex::encode(contract.as_bytes());
        let parameters = vec![ValueCell::Object(vec![
            ValueCell::Default(Primitive::U32(100)),
            ValueCell::Default(Primitive::U32(200)),
            ValueCell::Default(Primitive::U32(300)),
        ])];
        let payload = InvokeContractPayload {
            contract,
            deposits: Deposits::new(),
            entry_id: 30,
            max_gas: 2000000,
            parameters,
        };
        let wire = payload.to_bytes();
        invoke_vectors.push(InvokeContractVector {
            name: "invoke_with_object".to_string(),
            description: "Invoke with Object parameter (array of U32)".to_string(),
            contract_hex,
            deposits_count: 0,
            entry_id: 30,
            max_gas: 2000000,
            parameters_count: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 8: Complex invoke with deposits and parameters
    {
        let contract = hash_from_bytes(&[0x88u8; 32]);
        let contract_hex = hex::encode(contract.as_bytes());
        let asset = hash_from_bytes(&[0xBBu8; 32]);
        let mut deposits = Deposits::new();
        deposits.insert(asset, ContractDeposit::new(10000000000)); // 100 TOS
        let parameters = vec![
            ValueCell::Default(Primitive::U64(999)),
            ValueCell::Default(Primitive::String("transfer".to_string())),
            ValueCell::Bytes(vec![0x01, 0x02, 0x03, 0x04, 0x05]),
        ];
        let payload = InvokeContractPayload {
            contract,
            deposits,
            entry_id: 50,
            max_gas: 10000000,
            parameters,
        };
        let wire = payload.to_bytes();
        invoke_vectors.push(InvokeContractVector {
            name: "invoke_complex".to_string(),
            description: "Complex invoke with deposit and multiple parameters".to_string(),
            contract_hex,
            deposits_count: 1,
            entry_id: 50,
            max_gas: 10000000,
            parameters_count: 3,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // ========== DeployContract Vectors (Type 4) ==========

    // Create minimal valid ELF bytecode (just the magic header + padding)
    fn make_minimal_elf(extra_size: usize) -> Vec<u8> {
        let mut bytecode = vec![0x7F, b'E', b'L', b'F']; // ELF magic
        // Add minimal ELF header fields (simplified - just padding for test)
        bytecode.extend(vec![0x00; 12]); // class, encoding, version, os/abi, padding
        bytecode.extend(vec![0x02, 0x00]); // e_type: ET_EXEC
        bytecode.extend(vec![0xF3, 0x00]); // e_machine: EM_BPF (0xF3)
        bytecode.extend(vec![0x01, 0x00, 0x00, 0x00]); // e_version
        bytecode.extend(vec![0x00; extra_size]); // Additional padding
        bytecode
    }

    // Vector 1: Deploy without invoke
    {
        let bytecode = make_minimal_elf(16);
        let module = Module::from_bytecode(bytecode.clone());
        let payload = DeployContractPayload {
            module,
            invoke: None,
        };
        let wire = payload.to_bytes();
        deploy_vectors.push(DeployContractVector {
            name: "deploy_no_invoke".to_string(),
            description: "Deploy contract without constructor invocation".to_string(),
            module_len: bytecode.len(),
            has_invoke: false,
            invoke_max_gas: None,
            invoke_deposits_count: None,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 2: Deploy with invoke (no deposits)
    {
        let bytecode = make_minimal_elf(32);
        let module = Module::from_bytecode(bytecode.clone());
        let payload = DeployContractPayload {
            module,
            invoke: Some(InvokeConstructorPayload {
                max_gas: 5000000,
                deposits: Deposits::new(),
            }),
        };
        let wire = payload.to_bytes();
        deploy_vectors.push(DeployContractVector {
            name: "deploy_with_invoke".to_string(),
            description: "Deploy contract with constructor (no deposits)".to_string(),
            module_len: bytecode.len(),
            has_invoke: true,
            invoke_max_gas: Some(5000000),
            invoke_deposits_count: Some(0),
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 3: Deploy with invoke and deposit
    {
        let bytecode = make_minimal_elf(48);
        let module = Module::from_bytecode(bytecode.clone());
        let asset = hash_from_bytes(&[0xCCu8; 32]);
        let mut deposits = Deposits::new();
        deposits.insert(asset, ContractDeposit::new(50000000000)); // 500 TOS
        let payload = DeployContractPayload {
            module,
            invoke: Some(InvokeConstructorPayload {
                max_gas: 20000000,
                deposits,
            }),
        };
        let wire = payload.to_bytes();
        deploy_vectors.push(DeployContractVector {
            name: "deploy_with_deposit".to_string(),
            description: "Deploy contract with constructor and initial deposit".to_string(),
            module_len: bytecode.len(),
            has_invoke: true,
            invoke_max_gas: Some(20000000),
            invoke_deposits_count: Some(1),
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 4: Deploy with larger bytecode
    {
        let bytecode = make_minimal_elf(256);
        let module = Module::from_bytecode(bytecode.clone());
        let payload = DeployContractPayload {
            module,
            invoke: Some(InvokeConstructorPayload {
                max_gas: 100000000,
                deposits: Deposits::new(),
            }),
        };
        let wire = payload.to_bytes();
        deploy_vectors.push(DeployContractVector {
            name: "deploy_larger_module".to_string(),
            description: "Deploy contract with larger bytecode module".to_string(),
            module_len: bytecode.len(),
            has_invoke: true,
            invoke_max_gas: Some(100000000),
            invoke_deposits_count: Some(0),
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 5: Deploy with multiple deposits
    {
        let bytecode = make_minimal_elf(64);
        let module = Module::from_bytecode(bytecode.clone());
        let asset1 = hash_from_bytes(&[0xD1u8; 32]);
        let asset2 = hash_from_bytes(&[0xD2u8; 32]);
        let mut deposits = Deposits::new();
        deposits.insert(asset1, ContractDeposit::new(10000000000));
        deposits.insert(asset2, ContractDeposit::new(20000000000));
        let payload = DeployContractPayload {
            module,
            invoke: Some(InvokeConstructorPayload {
                max_gas: 30000000,
                deposits,
            }),
        };
        let wire = payload.to_bytes();
        deploy_vectors.push(DeployContractVector {
            name: "deploy_multi_deposit".to_string(),
            description: "Deploy contract with multiple initial deposits".to_string(),
            module_len: bytecode.len(),
            has_invoke: true,
            invoke_max_gas: Some(30000000),
            invoke_deposits_count: Some(2),
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Build output
    let vectors = ContractVectors {
        description: "TCK test vectors for InvokeContract (Type 3) and DeployContract (Type 4)"
            .to_string(),
        invoke_contract_vectors: invoke_vectors,
        deploy_contract_vectors: deploy_vectors,
    };

    // Write YAML output
    let yaml = serde_yaml::to_string(&vectors).expect("Failed to serialize");
    let mut file = File::create("contract.yaml").expect("Failed to create file");
    file.write_all(yaml.as_bytes())
        .expect("Failed to write file");

    println!("Generated contract.yaml with:");
    println!(
        "  - {} InvokeContract vectors",
        vectors.invoke_contract_vectors.len()
    );
    println!(
        "  - {} DeployContract vectors",
        vectors.deploy_contract_vectors.len()
    );
}
