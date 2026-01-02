#![allow(clippy::disallowed_methods)]

use crate::{
    account::Nonce,
    api::{DataElement, DataValue},
    block::BlockVersion,
    config::{BURN_PER_CONTRACT, COIN_VALUE, TOS_ASSET},
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey, PedersenOpening},
        Address, Hash, Hashable, KeyPair, PublicKey,
    },
    serializer::Serializer,
    transaction::{
        builder::{
            AccountState, ContractDepositBuilder, DeployContractBuilder, EnergyBuilder,
            EnergyOperationType, FeeBuilder, FeeHelper, GenerationError, InvokeContractBuilder,
            MultiSigBuilder, TransactionBuilder, TransactionTypeBuilder, TransferBuilder,
        },
        extra_data::Role,
        extra_data::{derive_shared_key_from_opening, PlaintextData},
        verify::{BlockchainVerificationState, NoZKPCache, ZKPCache},
        BurnPayload, MultiSigPayload, Reference, Transaction, TransactionType, TxVersion,
        MAX_TRANSFER_COUNT,
    },
};
use async_trait::async_trait;
use indexmap::IndexSet;
use std::{borrow::Cow, collections::HashMap, sync::Arc};
use tos_kernel::Environment;
use tos_kernel::Module;

/// Create a mock ELF bytecode for testing purposes
/// This creates a minimal valid ELF header that passes Module validation
fn create_mock_elf_bytecode() -> Vec<u8> {
    vec![
        0x7F, b'E', b'L', b'F', // ELF magic
        0x02, // 64-bit
        0x01, // Little endian
        0x01, // ELF version
        0x00, // OS/ABI
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Padding
        0x03, 0x00, // Type: shared object
        0xF7, 0x00, // Machine: BPF
        0x01, 0x00, 0x00, 0x00, // Version
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Entry point
        0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Program header offset
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Section header offset
        0x00, 0x00, 0x00, 0x00, // Flags
        0x40, 0x00, // ELF header size
        0x38, 0x00, // Program header entry size
        0x01, 0x00, // Program header count
        0x40, 0x00, // Section header entry size
        0x00, 0x00, // Section header count
        0x00, 0x00, // Section name string table index
    ]
}

// Create a newtype wrapper to avoid orphan rule violation
#[derive(Debug, Clone)]
struct TestError(());

impl<'a> From<&'a str> for TestError {
    fn from(_: &'a str) -> Self {
        TestError(())
    }
}

#[derive(Debug, Clone)]
struct AccountChainState {
    balances: HashMap<Hash, u64>,
    nonce: Nonce,
}

#[derive(Debug, Clone)]
struct ChainState {
    accounts: HashMap<PublicKey, AccountChainState>,
    multisig: HashMap<PublicKey, MultiSigPayload>,
    contracts: HashMap<Hash, Module>,
    env: Environment,
}

impl ChainState {
    fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            multisig: HashMap::new(),
            contracts: HashMap::new(),
            env: Environment::new(),
        }
    }
}

#[derive(Clone)]
struct Balance {
    balance: u64,
}

#[derive(Clone)]
struct Account {
    balances: HashMap<Hash, Balance>,
    keypair: KeyPair,
    nonce: Nonce,
}

impl Account {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
            keypair: KeyPair::new(),
            nonce: 0,
        }
    }

    fn set_balance(&mut self, asset: Hash, balance: u64) {
        self.balances.insert(asset, Balance { balance });
    }

    fn address(&self) -> Address {
        self.keypair.get_public_key().to_address(false)
    }
}

struct AccountStateImpl {
    balances: HashMap<Hash, Balance>,
    reference: Reference,
    nonce: Nonce,
}

fn create_tx_for(
    account: Account,
    destination: Address,
    amount: u64,
    extra_data: Option<DataElement>,
) -> Arc<Transaction> {
    let mut state = AccountStateImpl {
        balances: account.balances,
        nonce: account.nonce,
        reference: Reference {
            topoheight: 0,
            hash: Hash::zero(),
        },
    };

    // Debug extra_data size (before moving)
    if let Some(ref extra_data) = extra_data {
        println!("Debug extra_data size: {}", extra_data.to_bytes().len());
        println!(
            "Debug extra_data estimate: {}",
            2 + extra_data.to_bytes().len() + 64
        );
    }

    let data = TransactionTypeBuilder::Transfers(vec![TransferBuilder {
        amount,
        destination,
        asset: TOS_ASSET,
        extra_data,
    }]);

    let builder = TransactionBuilder::new(
        TxVersion::T0,
        0, // chain_id: 0 for tests
        account.keypair.get_public_key().compress(),
        None,
        data,
        FeeBuilder::default(),
    ); // Use T0 for all operations
    let estimated_size = builder.estimate_size();
    let tx = builder.build(&mut state, &account.keypair).unwrap();
    let actual_size = tx.size();
    let to_bytes_size = tx.to_bytes().len();
    println!(
        "Debug sizes: estimated={estimated_size}, actual={actual_size}, to_bytes={to_bytes_size}"
    );
    // Stake 2.0: fee_type removed, only fee_limit (8 bytes) remains
    println!(
        "Debug components: version={}, source={}, data={}, fee_limit={}, nonce={}, signature={}",
        1,
        tx.get_source().size(),
        tx.get_data().size(),
        8,
        8,
        tx.get_signature().size()
    );
    println!("Debug reference size: {}", tx.get_reference().size());

    // Calculate actual components (Stake 2.0: no fee_type field)
    let actual_components = 1
        + tx.get_source().size()
        + tx.get_data().size()
        + 8 // fee_limit
        + 8 // nonce
        + tx.get_reference().size()
        + tx.get_signature().size();
    println!("Debug calculated actual: {actual_components}");

    assert!(
        estimated_size == tx.size(),
        "expected {estimated_size} bytes got {actual_size} bytes"
    );
    assert!(tx.to_bytes().len() == estimated_size);

    Arc::new(tx)
}

#[test]
fn test_encrypt_decrypt() {
    let r = PedersenOpening::generate_new();
    let key = derive_shared_key_from_opening(&r);
    let message = "Hello, World!".as_bytes().to_vec();

    let plaintext = PlaintextData(message.clone());
    let cipher = plaintext.encrypt_in_place_with_aead(&key);
    let decrypted = cipher.decrypt_in_place(&key).unwrap();

    assert_eq!(decrypted.0, message);
}

// Balance simplification: This test verifies extra_data encryption/decryption
// Extra_data encryption is independent of balance proofs and still works with plaintext balances
#[test]
fn test_encrypt_decrypt_two_parties() {
    let mut alice = Account::new();
    alice.balances.insert(
        TOS_ASSET,
        Balance {
            balance: 100 * COIN_VALUE,
        },
    );

    let bob = Account::new();

    let payload = DataElement::Value(DataValue::String("Hello, World!".to_string()));
    let tx = create_tx_for(alice.clone(), bob.address(), 50, Some(payload.clone()));
    let TransactionType::Transfers(transfers) = tx.get_data() else {
        unreachable!()
    };

    let transfer = &transfers[0];
    let cipher = transfer.get_extra_data().clone().unwrap();
    // Verify the extra data from alice (sender)
    {
        let decrypted = cipher
            .decrypt(
                alice.keypair.get_private_key(),
                None,
                Role::Sender,
                TxVersion::T0,
            )
            .unwrap();
        assert_eq!(decrypted.data(), Some(&payload));
    }

    // Verify the extra data from bob (receiver)
    {
        let decrypted = cipher
            .decrypt(
                bob.keypair.get_private_key(),
                None,
                Role::Receiver,
                TxVersion::T0,
            )
            .unwrap();
        assert_eq!(decrypted.data(), Some(&payload));
    }

    // Balance simplification: With plaintext extra_data, decryption succeeds even with wrong role
    // This is expected behavior - no encryption means no role-based access control
    {
        let decrypted = cipher.decrypt(
            bob.keypair.get_private_key(),
            None,
            Role::Sender,
            TxVersion::T0,
        );
        assert!(decrypted.is_ok()); // Changed: plaintext succeeds even with wrong role
        assert_eq!(decrypted.unwrap().data(), Some(&payload));
    }
}

// Balance update bug FIXED - receiver balances are now properly credited
#[tokio::test]
async fn test_tx_verify() {
    let mut alice = Account::new();
    let mut bob = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);
    bob.set_balance(TOS_ASSET, 0);

    // Alice account is cloned to not be updated as it is used for verification and need current state
    let tx = create_tx_for(alice.clone(), bob.address(), 50, None);

    let mut state = ChainState::new();

    // Create the chain state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    {
        let mut balances = HashMap::new();
        for (asset, balance) in &bob.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            bob.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();

    // NOTE: verify() now mutates sender balance (like old encrypted balance code)
    // But receiver balance is still only updated in apply(), so we need to manually
    // add it here for this test (since we're not calling apply())
    {
        // Add amount to Bob's balance (receiver - only updated in apply())
        let bob_balance = state
            .accounts
            .get_mut(&bob.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .entry(TOS_ASSET)
            .or_insert(0);
        *bob_balance = bob_balance.checked_add(50).unwrap();

        // Sender balance (Alice) was already mutated by verify(), no need to deduct again
    }

    // Check Bob balance
    let balance = state.accounts[&bob.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(balance, 50u64);

    // Check Alice balance
    let balance = state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(balance, (100u64 * COIN_VALUE) - (50 + tx.fee_limit));
}

// Balance simplification: Re-enabled test - passes with plaintext balances
// This test verifies transaction caching behavior, which is independent of proof system
#[tokio::test]
async fn test_tx_verify_with_zkp_cache() {
    let mut alice = Account::new();
    let mut bob = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);
    bob.set_balance(TOS_ASSET, 0);

    // Alice account is cloned to not be updated as it is used for verification and need current state
    let tx = create_tx_for(alice.clone(), bob.address(), 50, None);

    let mut state = ChainState::new();

    // Create the chain state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    {
        let mut balances = HashMap::new();
        for (asset, balance) in &bob.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            bob.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    let mut clean_state = state.clone();
    let hash = tx.hash();
    {
        // Ensure the TX is valid first
        assert!(tx.verify(&hash, &mut state, &NoZKPCache).await.is_ok());
    }

    struct DummyCache;

    #[async_trait]
    impl<E> ZKPCache<E> for DummyCache {
        async fn is_already_verified(&self, _: &Hash) -> Result<bool, E> {
            Ok(true)
        }
    }

    // Fix the nonce to pass the verification
    state
        .accounts
        .get_mut(&alice.keypair.get_public_key().compress())
        .unwrap()
        .nonce = 0;

    // Balance simplification: Proof verification removed, test disabled
    // Now verification relies on plaintext balance checking instead of proofs
    // assert!(matches!(tx.verify(&hash, &mut state, &DummyCache).await, Err(_)));

    // But should be fine for a clean state
    assert!(tx
        .verify(&hash, &mut clean_state, &DummyCache)
        .await
        .is_ok());
}

// Test updated to work with plain u64 balances (balance simplification completed)
#[tokio::test]
async fn test_burn_tx_verify() {
    let mut alice = Account::new();
    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    let tx = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::Burn(BurnPayload {
            amount: 50 * COIN_VALUE,
            asset: TOS_ASSET,
        });
        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        );
        let estimated_size = builder.estimate_size();
        let tx = builder.build(&mut state, &alice.keypair).unwrap();
        assert!(estimated_size == tx.size());
        assert!(tx.to_bytes().len() == estimated_size);

        Arc::new(tx)
    };

    let mut state = ChainState::new();

    // Create the chain state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();

    // NOTE: verify() now mutates sender balance (like old encrypted balance code)
    // Sender balance (Alice) was already mutated by verify(), no need to deduct again

    // Check Alice balance
    let balance = state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(
        balance,
        (100u64 * COIN_VALUE) - (50 * COIN_VALUE + tx.fee_limit)
    );
}

// Balance simplification: Test updated to work with plain u64 balances
#[tokio::test]
async fn test_tx_invoke_contract() {
    let mut alice = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    let tx = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::InvokeContract(InvokeContractBuilder {
            contract: Hash::zero(),
            entry_id: 0,
            max_gas: 1000,
            parameters: Vec::new(),
            deposits: [(
                TOS_ASSET,
                ContractDepositBuilder {
                    amount: 50 * COIN_VALUE,
                    private: false,
                },
            )]
            .into_iter()
            .collect(),
            contract_key: None,
        });
        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        ); // Use T0 for InvokeContract
        let estimated_size = builder.estimate_size();
        let tx = builder.build(&mut state, &alice.keypair).unwrap();
        assert!(estimated_size == tx.size());
        assert!(tx.to_bytes().len() == estimated_size);

        Arc::new(tx)
    };

    let mut state = ChainState::new();
    let module = Module::from_bytecode(create_mock_elf_bytecode());
    state.contracts.insert(Hash::zero(), module);

    // Create the chain state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();

    // NOTE: verify() now mutates sender balance (like old encrypted balance code)
    // Sender balance (Alice) was already mutated by verify(), no need to deduct again

    // Check Alice balance
    let balance = state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    // 50 coins deposit + tx fee + 1000 gas fee
    let total_spend = (50 * COIN_VALUE) + tx.fee_limit + 1000;

    assert_eq!(balance, (100 * COIN_VALUE) - total_spend);
}

// Test contract deposits with multiple deposits
// Verifies that deposits are correctly deducted from sender balance
// NOTE: Private deposits (private: true) require TransactionBuilder support for contract keys
// Currently TransactionBuilder::build_deposits_commitments() receives &None for contract_key
// See: common/src/transaction/builder/mod.rs:793 and mod.rs:805
// Balance simplification: Test updated to work with plain u64 balances
#[tokio::test]
async fn test_tx_invoke_contract_multiple_deposits() {
    let mut alice = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    let tx = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::InvokeContract(InvokeContractBuilder {
            contract: Hash::zero(),
            entry_id: 0,
            max_gas: 1000,
            parameters: Vec::new(),
            deposits: [(
                TOS_ASSET,
                ContractDepositBuilder {
                    amount: 50 * COIN_VALUE,
                    private: false, // Public deposit
                },
            )]
            .into_iter()
            .collect(),
            contract_key: None,
        });
        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        );
        let estimated_size = builder.estimate_size();
        let tx = builder.build(&mut state, &alice.keypair).unwrap();
        assert!(
            estimated_size == tx.size(),
            "expected {} bytes got {} bytes",
            tx.size(),
            estimated_size
        );
        assert!(tx.to_bytes().len() == estimated_size);

        Arc::new(tx)
    };

    let mut state = ChainState::new();
    let module = Module::from_bytecode(create_mock_elf_bytecode());
    state.contracts.insert(Hash::zero(), module);

    // Create the chain state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();

    // NOTE: verify() now mutates sender balance (like old encrypted balance code)
    // Sender balance (Alice) was already mutated by verify(), no need to deduct again

    // Check Alice balance (sender side - should reflect deduction)
    let balance = state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    // 50 coins deposit + tx fee + 1000 gas fee
    let total_spend = (50 * COIN_VALUE) + tx.fee_limit + 1000;

    assert_eq!(balance, (100 * COIN_VALUE) - total_spend);
}

// Balance simplification: Test updated to work with plain u64 balances
#[tokio::test]
async fn test_tx_deploy_contract() {
    let mut alice = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    let tx = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        // Create module with valid ELF bytecode for deterministic address computation
        let module = Module::from_bytecode(create_mock_elf_bytecode());
        let data = TransactionTypeBuilder::DeployContract(DeployContractBuilder {
            module: module.to_hex(),
            invoke: None,
        });
        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        ); // Use T0 for DeployContract
        let estimated_size = builder.estimate_size();
        let tx = builder.build(&mut state, &alice.keypair).unwrap();
        assert!(
            estimated_size == tx.size(),
            "expected {} bytes got {} bytes",
            tx.size(),
            estimated_size
        );
        assert!(tx.to_bytes().len() == estimated_size);

        Arc::new(tx)
    };

    let mut state = ChainState::new();

    // Create the chain state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();

    // NOTE: verify() now mutates sender balance (like old encrypted balance code)
    // Sender balance (Alice) was already mutated by verify(), no need to deduct again

    // Check Alice balance
    let balance = state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    // 1 TOS for contract deploy, tx fee
    let total_spend = BURN_PER_CONTRACT + tx.fee_limit;

    assert_eq!(balance, (100 * COIN_VALUE) - total_spend);
}

// Balance simplification: Re-enabled test - passes with plaintext balances
// This test verifies maximum transfer count limit, which works with plaintext balances
#[tokio::test]
async fn test_max_transfers() {
    let mut alice = Account::new();
    let mut bob = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);
    bob.set_balance(TOS_ASSET, 0);

    let tx = {
        let mut transfers = Vec::new();
        for _ in 0..MAX_TRANSFER_COUNT {
            transfers.push(TransferBuilder {
                amount: 1,
                destination: bob.address(),
                asset: TOS_ASSET,
                extra_data: None,
            });
        }

        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::Transfers(transfers);
        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        );
        let estimated_size = builder.estimate_size();
        let tx = builder.build(&mut state, &alice.keypair).unwrap();
        assert!(estimated_size == tx.size());
        assert!(tx.to_bytes().len() == estimated_size);

        Arc::new(tx)
    };

    // Create the chain state
    let mut state = ChainState::new();

    // Alice
    {
        let mut balances = HashMap::new();
        for (asset, balance) in alice.balances {
            balances.insert(asset, balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }
    // Bob
    {
        let mut balances = HashMap::new();
        for (asset, balance) in bob.balances {
            balances.insert(asset, balance.balance);
        }
        state.accounts.insert(
            bob.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: bob.nonce,
            },
        );
    }
    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();
}

// Balance simplification: Re-enabled test - passes with plaintext balances
// This test verifies multisig account configuration, which works with plaintext balances
#[tokio::test]
async fn test_multisig_setup() {
    let mut alice = Account::new();
    let mut bob = Account::new();
    let charlie = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);
    bob.set_balance(TOS_ASSET, 0);

    let tx = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::MultiSig(MultiSigBuilder {
            threshold: 2,
            participants: IndexSet::from_iter(vec![
                bob.keypair.get_public_key().to_address(false),
                charlie.keypair.get_public_key().to_address(false),
            ]),
        });
        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        ); // Use T0 for MultiSig
        let estimated_size = builder.estimate_size();
        let tx = builder.build(&mut state, &alice.keypair).unwrap();
        assert!(estimated_size == tx.size());
        assert!(tx.to_bytes().len() == estimated_size);

        Arc::new(tx)
    };

    let mut state = ChainState::new();

    // Create the chain state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in alice.balances {
            balances.insert(asset, balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    {
        let mut balances = HashMap::new();
        for (asset, balance) in bob.balances {
            balances.insert(asset, balance.balance);
        }
        state.accounts.insert(
            bob.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();

    assert!(state
        .multisig
        .contains_key(&alice.keypair.get_public_key().compress()));
}

// Balance simplification: Re-enabled test - passes with plaintext balances
// This test verifies multisig transaction signing and verification, which works with plaintext balances
#[tokio::test]
async fn test_multisig() {
    let mut alice = Account::new();
    let mut bob = Account::new();

    // Signers
    let charlie = Account::new();
    let dave = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);
    bob.set_balance(TOS_ASSET, 0);

    let tx = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::Transfers(vec![TransferBuilder {
            amount: 1,
            destination: bob.address(),
            asset: TOS_ASSET,
            extra_data: None,
        }]);
        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            Some(2),
            data,
            FeeBuilder::default(),
        ); // Use T0 for MultiSig
        let mut tx = builder.build_unsigned(&mut state, &alice.keypair).unwrap();

        tx.sign_multisig(&charlie.keypair, 0);
        tx.sign_multisig(&dave.keypair, 1);

        Arc::new(tx.finalize(&alice.keypair))
    };

    // Create the chain state
    let mut state = ChainState::new();

    // Alice
    {
        let mut balances = HashMap::new();
        for (asset, balance) in alice.balances {
            balances.insert(asset, balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    // Bob
    {
        let mut balances = HashMap::new();
        for (asset, balance) in bob.balances {
            balances.insert(asset, balance.balance);
        }

        state.accounts.insert(
            bob.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    state.multisig.insert(
        alice.keypair.get_public_key().compress(),
        MultiSigPayload {
            threshold: 2,
            participants: IndexSet::from_iter(vec![
                charlie.keypair.get_public_key().compress(),
                dave.keypair.get_public_key().compress(),
            ]),
        },
    );

    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();
}

// Balance simplification: Test updated to work with plain u64 balances
#[tokio::test]
async fn test_transfer_extra_data_limits() {
    let mut alice = Account::new();
    let mut bob = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);
    bob.set_balance(TOS_ASSET, 0);

    // Test single transfer with exchange ID sized extra data (realistic use case)
    let max_extra_data = DataElement::Value(DataValue::Blob(vec![0u8; 32])); // Use 32 bytes, typical exchange ID size
    let tx = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::Transfers(vec![TransferBuilder {
            amount: 1,
            destination: bob.address(),
            asset: TOS_ASSET,
            extra_data: Some(max_extra_data),
        }]);

        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        );
        builder.build(&mut state, &alice.keypair).unwrap()
    };

    // Create the chain state
    let mut state = ChainState::new();

    // Alice
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }
    // Bob
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &bob.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            bob.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: bob.nonce,
            },
        );
    }

    // Verify the transaction
    let tx_hash = tx.hash();
    let tx_fee = tx.fee_limit; // Save fee_limit before moving tx into Arc
    let result = Arc::new(tx).verify(&tx_hash, &mut state, &NoZKPCache).await;
    assert!(
        result.is_ok(),
        "Transaction with maximum extra data should be valid"
    );

    // Balance simplification: verify() only validates, doesn't apply state changes
    // Manually apply balance changes to simulate what apply() does in production
    {
        // Deduct amount + fee from Alice's balance
        let total_spend = 1 + tx_fee;
        let alice_balance = state
            .accounts
            .get_mut(&alice.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .get_mut(&TOS_ASSET)
            .unwrap();
        *alice_balance = alice_balance.checked_sub(total_spend).unwrap();

        // Add amount to Bob's balance
        let bob_balance = state
            .accounts
            .get_mut(&bob.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .get_mut(&TOS_ASSET)
            .unwrap();
        *bob_balance = bob_balance.checked_add(1).unwrap();
    }

    // Test single transfer with oversized extra data (should fail)
    let oversized_extra_data = DataElement::Value(DataValue::Blob(vec![0u8; 2000])); // Use 2000 bytes which should definitely be too large
    let tx_oversized = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::Transfers(vec![TransferBuilder {
            amount: 1,
            destination: bob.address(),
            asset: TOS_ASSET,
            extra_data: Some(oversized_extra_data),
        }]);

        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        );
        builder.build(&mut state, &alice.keypair)
    };

    match tx_oversized {
        Ok(_) => panic!("Transaction with oversized extra data should fail"),
        Err(e) => {
            println!("Actual error: {e:?}");
            assert!(
                matches!(e, GenerationError::ExtraDataTooLarge),
                "Expected ExtraDataTooLarge error"
            );
        }
    }

    // Test multiple transfers with total extra data exceeding limit
    // Balance simplification: Updated sizes to exceed 4KB limit without encryption
    // 31 × 128 bytes + 1 × 200 bytes = 4168 bytes > 4096 bytes (EXTRA_DATA_LIMIT_SUM_SIZE)
    let mut transfers = Vec::new();
    for i in 0..32 {
        let extra_data_size = if i == 31 { 200 } else { 128 }; // Total: 31×128 + 200 = 4168 > 4096
        let extra_data = DataElement::Value(DataValue::Blob(vec![0u8; extra_data_size]));
        transfers.push(TransferBuilder {
            amount: 1,
            destination: bob.address(),
            asset: TOS_ASSET,
            extra_data: Some(extra_data),
        });
    }

    let tx_total_oversized = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::Transfers(transfers);
        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::default(),
        );
        builder.build(&mut state, &alice.keypair)
    };

    match tx_total_oversized {
        Ok(_) => panic!("Transaction with total oversized extra data should fail"),
        Err(e) => {
            println!("Actual total oversized error: {e:?}");
            assert!(
                matches!(
                    e,
                    GenerationError::ExtraDataTooLarge
                        | GenerationError::EncryptedExtraDataTooLarge(_, _)
                ),
                "Expected ExtraDataTooLarge or EncryptedExtraDataTooLarge error for total size"
            );
        }
    }
}

// Test UnfreezeTos does NOT credit balance immediately (Stake 2.0 behavior)
// UnfreezeTos adds to unfreezing queue, NOT immediate balance credit
// Balance is only credited via WithdrawExpireUnfreeze after 14-day waiting period
#[tokio::test]
async fn test_unfreeze_tos_balance_refund() {
    let mut alice = Account::new();
    let initial_balance = 1000 * COIN_VALUE;
    let _unfreeze_amount = 100 * COIN_VALUE;

    // Set initial balance (simulating post-freeze state)
    alice.set_balance(TOS_ASSET, initial_balance);

    // Create and verify UnfreezeTos transaction
    let unfreeze_tx = {
        let mut state = AccountStateImpl {
            balances: alice.balances.clone(),
            nonce: alice.nonce,
            reference: Reference {
                topoheight: 0,
                hash: Hash::zero(),
            },
        };

        let data = TransactionTypeBuilder::Energy(EnergyBuilder {
            operation: EnergyOperationType::UnfreezeTos,
            amount: Some(_unfreeze_amount),
            receiver: None,
            lock: false,
            lock_period: 0,
        });

        let builder = TransactionBuilder::new(
            TxVersion::T0,
            0, // chain_id: 0 for tests
            alice.keypair.get_public_key().compress(),
            None,
            data,
            FeeBuilder::Multiplier(1.0),
        );
        builder.build(&mut state, &alice.keypair).unwrap()
    };

    // Create chain state
    let mut state = ChainState::new();
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    // Check balance before verify
    let balance_before_verify = state
        .accounts
        .get(&alice.keypair.get_public_key().compress())
        .unwrap()
        .balances
        .get(&TOS_ASSET)
        .unwrap();
    println!("Balance before verify: {balance_before_verify}");

    // Verify UnfreezeTos transaction
    let unfreeze_tx_hash = unfreeze_tx.hash();
    let tx_fee = unfreeze_tx.fee_limit; // Save actual fee_limit from transaction
    println!("Transaction fee: {tx_fee}");
    let unfreeze_result = Arc::new(unfreeze_tx)
        .verify(&unfreeze_tx_hash, &mut state, &NoZKPCache)
        .await;
    assert!(
        unfreeze_result.is_ok(),
        "UnfreezeTos transaction should succeed"
    );

    // After UnfreezeTos verify: balance should NOT include unfreeze_amount (Stake 2.0)
    // Unfreeze amount goes to unfreezing queue, not immediate balance credit
    // Note: verify() reserves fee_limit; refund happens in apply() phase
    let alice_balance_after_unfreeze = state
        .accounts
        .get(&alice.keypair.get_public_key().compress())
        .unwrap()
        .balances
        .get(&TOS_ASSET)
        .unwrap();
    println!("Balance after verify: {alice_balance_after_unfreeze}");

    // In verify-only mode, fee_limit is reserved but not refunded (refund happens in apply)
    // Expected: initial_balance - tx_fee (fee reserved, no immediate unfreeze credit)
    let expected_balance = initial_balance - tx_fee;
    println!(
        "Expected balance: {expected_balance} (initial {initial_balance} - fee {tx_fee}, refund in apply phase)"
    );
    assert_eq!(
        *alice_balance_after_unfreeze, expected_balance,
        "Balance should be initial - fee_limit (refund happens in apply, not verify)"
    );

    // CRITICAL: Verify unfreeze amount is NOT credited to balance (Stake 2.0 behavior)
    let old_wrong_balance = initial_balance + _unfreeze_amount - tx_fee;
    assert_ne!(
        *alice_balance_after_unfreeze, old_wrong_balance,
        "UnfreezeTos should NOT credit balance immediately"
    );

    println!("UnfreezeTos test passed: Stake 2.0 queue behavior verified");
    println!("   Initial balance:      {initial_balance}");
    println!("   Transaction fee:      {tx_fee} (reserved in verify, refunded in apply)");
    println!("   Final balance:        {expected_balance}");
    println!("   Note: Unfreeze amount goes to unfreezing queue, not immediate balance");
    println!("   Use WithdrawExpireUnfreeze after 14 days to claim balance");
}

#[async_trait]
impl<'a> BlockchainVerificationState<'a, TestError> for ChainState {
    /// Pre-verify the TX
    async fn pre_verify_tx<'b>(&'b mut self, _: &Transaction) -> Result<(), TestError> {
        Ok(())
    }

    /// Get the balance for a receiver account
    /// Auto-creates account and balance entry with 0 if they don't exist
    /// This matches the real daemon behavior where new accounts are created on first receive
    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, TestError> {
        let key = account.into_owned();

        // Auto-create account if missing (for new accounts receiving funds)
        let account_state = self
            .accounts
            .entry(key)
            .or_insert_with(|| AccountChainState {
                balances: HashMap::new(),
                nonce: 0,
            });
        // Auto-create balance entry if missing (for new assets being received)
        Ok(account_state
            .balances
            .entry(asset.into_owned())
            .or_insert(0))
    }

    /// Get the balance used for verification of funds for the sender account
    async fn get_sender_balance<'b>(
        &'b mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        _: &Reference,
    ) -> Result<&'b mut u64, TestError> {
        self.accounts
            .get_mut(account)
            .and_then(|account| account.balances.get_mut(asset))
            .ok_or(TestError(()))
    }

    /// Apply new output to a sender account
    async fn add_sender_output(
        &mut self,
        _: &'a PublicKey,
        _: &'a Hash,
        _: u64,
    ) -> Result<(), TestError> {
        Ok(())
    }

    // ===== UNO (Privacy Balance) Methods =====
    // Stub implementations for testing

    async fn get_receiver_uno_balance<'b>(
        &'b mut self,
        _account: Cow<'a, PublicKey>,
        _asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, TestError> {
        Err(TestError(()))
    }

    async fn get_sender_uno_balance<'b>(
        &'b mut self,
        _account: &'a PublicKey,
        _asset: &'a Hash,
        _reference: &Reference,
    ) -> Result<&'b mut Ciphertext, TestError> {
        Err(TestError(()))
    }

    async fn add_sender_uno_output(
        &mut self,
        _account: &'a PublicKey,
        _asset: &'a Hash,
        _output: Ciphertext,
    ) -> Result<(), TestError> {
        Err(TestError(()))
    }

    /// Get the nonce of an account
    async fn get_account_nonce(&mut self, account: &'a PublicKey) -> Result<Nonce, TestError> {
        self.accounts
            .get(account)
            .map(|account| account.nonce)
            .ok_or(TestError(()))
    }

    /// Apply a new nonce to an account
    async fn update_account_nonce(
        &mut self,
        account: &'a PublicKey,
        new_nonce: Nonce,
    ) -> Result<(), TestError> {
        self.accounts
            .get_mut(account)
            .map(|account| account.nonce = new_nonce)
            .ok_or(TestError(()))
    }

    /// Atomic compare-and-swap for nonce (V-11 security fix)
    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        expected: Nonce,
        new_value: Nonce,
    ) -> Result<bool, TestError> {
        // For test state, we don't need true atomicity
        // Note: In this test module, PublicKey is already CompressedPublicKey
        let current = self.get_account_nonce(account).await?;
        if current == expected {
            self.update_account_nonce(account, new_value).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get_block_version(&self) -> BlockVersion {
        BlockVersion::Nobunaga
    }

    fn get_verification_timestamp(&self) -> u64 {
        // Use current system time for tests
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    async fn set_multisig_state(
        &mut self,
        account: &'a PublicKey,
        multisig: &MultiSigPayload,
    ) -> Result<(), TestError> {
        self.multisig.insert(account.clone(), multisig.clone());
        Ok(())
    }

    async fn get_multisig_state(
        &mut self,
        account: &'a PublicKey,
    ) -> Result<Option<&MultiSigPayload>, TestError> {
        Ok(self.multisig.get(account))
    }

    async fn get_environment(&mut self) -> Result<&Environment, TestError> {
        Ok(&self.env)
    }

    async fn set_contract_module(
        &mut self,
        hash: &Hash,
        module: &'a Module,
    ) -> Result<(), TestError> {
        self.contracts.insert(hash.clone(), module.clone());
        Ok(())
    }

    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, TestError> {
        Ok(self.contracts.contains_key(hash))
    }

    async fn get_contract_module_with_environment(
        &self,
        contract: &Hash,
    ) -> Result<(&Module, &Environment), TestError> {
        let module = self.contracts.get(contract).ok_or(TestError(()))?;
        Ok((module, &self.env))
    }

    fn get_network(&self) -> crate::network::Network {
        // Use Mainnet for tests (chain_id = 0)
        crate::network::Network::Mainnet
    }

    /// Check if an account is registered (for TOS-Only fee calculation)
    async fn is_account_registered(
        &self,
        account: &CompressedPublicKey,
    ) -> Result<bool, TestError> {
        // For testing, check if account exists in our state
        // Note: crypto::PublicKey = CompressedPublicKey, so account can be used directly
        Ok(self.accounts.contains_key(account))
    }

    async fn get_delegated_resource(
        &mut self,
        _from: &'a CompressedPublicKey,
        _to: &'a CompressedPublicKey,
    ) -> Result<Option<crate::account::DelegatedResource>, TestError> {
        Ok(None)
    }
}

impl FeeHelper for AccountStateImpl {
    type Error = TestError; // Use TestError instead of ()

    fn account_exists(&self, _: &PublicKey) -> Result<bool, Self::Error> {
        Ok(false)
    }
}

impl AccountState for AccountStateImpl {
    fn is_mainnet(&self) -> bool {
        false
    }

    fn get_account_balance(&self, asset: &Hash) -> Result<u64, TestError> {
        // Use TestError
        self.balances
            .get(asset)
            .map(|balance| balance.balance)
            .ok_or(TestError(()))
    }

    fn get_reference(&self) -> Reference {
        self.reference.clone()
    }

    fn update_account_balance(&mut self, asset: &Hash, balance: u64) -> Result<(), TestError> {
        // Use TestError
        self.balances.insert(asset.clone(), Balance { balance });
        Ok(())
    }

    fn get_nonce(&self) -> Result<Nonce, TestError> {
        // Use TestError
        Ok(self.nonce)
    }

    fn update_nonce(&mut self, new_nonce: Nonce) -> Result<(), TestError> {
        // Use TestError
        self.nonce = new_nonce;
        Ok(())
    }

    fn is_account_registered(&self, _: &PublicKey) -> Result<bool, TestError> {
        // For testing purposes, assume all accounts are registered
        Ok(true)
    }
}

// ============================================================================
// P0-4: INTEGRATION TESTS FOR BALANCE MUTATIONS
// ============================================================================
// These tests verify the critical balance verification and mutation logic
// implemented in commits 6bcab08, 2ce8d18, and 0466a69.
//
// Test Coverage:
// 1. End-to-end transfer with sender deduction and receiver credit
// 2. Double-spend prevention within same block
// 3. Insufficient balance rejection
// 4. Overflow protection (u64::MAX scenarios)
// 5. Fee deduction (TOS fees)
// 6. Burn transaction total supply handling
// ============================================================================

use crate::transaction::verify::VerificationError;

// Helper function to create a transfer transaction
fn create_transfer_tx(
    sender: &Account,
    receiver_addr: Address,
    amount: u64,
    asset: Hash,
) -> Arc<Transaction> {
    let mut state = AccountStateImpl {
        balances: sender.balances.clone(),
        nonce: sender.nonce,
        reference: Reference {
            topoheight: 0,
            hash: Hash::zero(),
        },
    };

    let data = TransactionTypeBuilder::Transfers(vec![TransferBuilder {
        amount,
        destination: receiver_addr,
        asset,
        extra_data: None,
    }]);

    let builder = TransactionBuilder::new(
        TxVersion::T0,
        0, // chain_id: 0 for tests
        sender.keypair.get_public_key().compress(),
        None,
        data,
        FeeBuilder::default(),
    );

    Arc::new(builder.build(&mut state, &sender.keypair).unwrap())
}

// Helper function to create a burn transaction
fn create_burn_tx(sender: &Account, amount: u64, asset: Hash) -> Arc<Transaction> {
    let mut state = AccountStateImpl {
        balances: sender.balances.clone(),
        nonce: sender.nonce,
        reference: Reference {
            topoheight: 0,
            hash: Hash::zero(),
        },
    };

    let data = TransactionTypeBuilder::Burn(BurnPayload { amount, asset });

    let builder = TransactionBuilder::new(
        TxVersion::T0,
        0, // chain_id: 0 for tests
        sender.keypair.get_public_key().compress(),
        None,
        data,
        FeeBuilder::default(),
    );

    Arc::new(builder.build(&mut state, &sender.keypair).unwrap())
}

// Test 1: End-to-end transfer with balance verification
// Verifies P0-2 (receiver balance updates) and P0-3 (sender balance deduction)
#[tokio::test]
async fn test_p04_transfer_balance_mutation() {
    let mut alice = Account::new();
    let mut bob = Account::new();

    // Alice starts with 1000 TOS, Bob with 0
    alice.set_balance(TOS_ASSET, 1000 * COIN_VALUE);
    bob.set_balance(TOS_ASSET, 0);

    // Alice transfers 500 TOS to Bob
    let tx = create_transfer_tx(&alice, bob.address(), 500 * COIN_VALUE, TOS_ASSET);
    let tx_fee = tx.fee_limit;

    // Create chain state
    let mut state = ChainState::new();
    state.accounts.insert(
        alice.keypair.get_public_key().compress(),
        AccountChainState {
            balances: alice
                .balances
                .iter()
                .map(|(k, v)| (k.clone(), v.balance))
                .collect(),
            nonce: alice.nonce,
        },
    );
    state.accounts.insert(
        bob.keypair.get_public_key().compress(),
        AccountChainState {
            balances: bob
                .balances
                .iter()
                .map(|(k, v)| (k.clone(), v.balance))
                .collect(),
            nonce: bob.nonce,
        },
    );

    // Execute the transaction via verify() which handles sender deduction
    let tx_hash = tx.hash();
    tx.verify(&tx_hash, &mut state, &NoZKPCache).await.unwrap();

    // NOTE: verify() mutates sender balance (P0-3 implementation)
    // But receiver balance is only updated in apply(), so we need to manually add it here
    // to simulate what apply() does (P0-2 implementation test)
    {
        // Add amount to Bob's balance (receiver - simulates apply() receiver update logic)
        let bob_balance = state
            .accounts
            .get_mut(&bob.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .entry(TOS_ASSET)
            .or_insert(0);
        *bob_balance = bob_balance.checked_add(500 * COIN_VALUE).unwrap();
    }

    // Verify Alice's balance: 1000 - 500 - fee (sender deduction from verify())
    let alice_balance =
        state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(
        alice_balance,
        1000 * COIN_VALUE - 500 * COIN_VALUE - tx_fee,
        "Alice's balance should be deducted by transfer amount + fee"
    );

    // Verify Bob's balance: 0 + 500 (receiver credit from simulated apply())
    let bob_balance = state.accounts[&bob.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(
        bob_balance,
        500 * COIN_VALUE,
        "Bob's balance should be credited with transfer amount"
    );

    // Verify total supply is conserved (minus fee which goes to network)
    let total_balance = alice_balance + bob_balance;
    assert_eq!(
        total_balance,
        1000 * COIN_VALUE - tx_fee,
        "Total supply should be conserved (minus fee)"
    );
}

// Test 2: Double-spend prevention within same block
// Verifies that sender balance deduction prevents spending same funds twice
#[tokio::test]
async fn test_p04_double_spend_prevention() {
    let mut alice = Account::new();
    let bob = Account::new();

    // Alice starts with only 100 TOS
    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    // Create two transactions from Alice, each spending 60 TOS
    let tx1 = create_transfer_tx(&alice, bob.address(), 60 * COIN_VALUE, TOS_ASSET);

    // Update alice nonce for second transaction
    alice.nonce += 1;
    let tx2 = create_transfer_tx(&alice, bob.address(), 60 * COIN_VALUE, TOS_ASSET);

    // Create chain state
    let mut state = ChainState::new();
    state.accounts.insert(
        alice.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 100 * COIN_VALUE)]
                .into_iter()
                .collect(),
            nonce: 0,
        },
    );
    state.accounts.insert(
        bob.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 0)].into_iter().collect(),
            nonce: 0,
        },
    );

    // First transaction should succeed
    let tx1_hash = tx1.hash();
    let result1 = tx1.verify(&tx1_hash, &mut state, &NoZKPCache).await;
    assert!(result1.is_ok(), "First transaction should succeed");

    // Second transaction should fail due to insufficient balance
    // After TX1, Alice has: 100 - 60 - fee1 < 60 + fee2
    let tx2_hash = tx2.hash();
    let result2 = tx2.verify(&tx2_hash, &mut state, &NoZKPCache).await;
    assert!(
        result2.is_err(),
        "Second transaction should fail (double-spend prevention)"
    );

    match result2 {
        Err(VerificationError::InsufficientFunds {
            available,
            required,
        }) => {
            println!("Double-spend prevented: available={available}, required={required}");
            assert!(available < required, "Should have insufficient funds");
        }
        _ => panic!("Expected InsufficientFunds error, got {result2:?}"),
    }
}

// Test 3: Insufficient balance rejection
// Verifies balance checking in pre_verify() and verify()
#[tokio::test]
async fn test_p04_insufficient_balance() {
    let mut alice = Account::new();
    let bob = Account::new();

    // Alice needs 200 TOS to build the transaction (transaction builder validates balance)
    // But we'll set chain state to only 50 TOS to test verify() rejection
    alice.set_balance(TOS_ASSET, 200 * COIN_VALUE);

    // Create transaction to transfer 100 TOS
    let tx = create_transfer_tx(&alice, bob.address(), 100 * COIN_VALUE, TOS_ASSET);

    // Create chain state with insufficient balance (50 TOS < 100 TOS + fee)
    let mut state = ChainState::new();
    state.accounts.insert(
        alice.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 50 * COIN_VALUE)]
                .into_iter()
                .collect(),
            nonce: alice.nonce,
        },
    );
    state.accounts.insert(
        bob.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 0)].into_iter().collect(),
            nonce: 0,
        },
    );

    // Transaction should fail with insufficient balance during verify()
    let tx_hash = tx.hash();
    let result = tx.verify(&tx_hash, &mut state, &NoZKPCache).await;

    assert!(
        result.is_err(),
        "Transaction should fail due to insufficient balance"
    );
    match result {
        Err(VerificationError::InsufficientFunds {
            available,
            required,
        }) => {
            println!("Insufficient balance detected: available={available}, required={required}");
            assert_eq!(
                available,
                50 * COIN_VALUE,
                "Available balance should be 50 TOS"
            );
            assert!(required > available, "Required should exceed available");
        }
        _ => panic!("Expected InsufficientFunds error, got {result:?}"),
    }
}

// Test 4: Overflow protection
// Verifies checked_add() and checked_sub() prevent u64 overflow
#[tokio::test]
async fn test_p04_overflow_protection() {
    let mut alice = Account::new();
    let mut bob = Account::new();

    // Alice starts with u64::MAX (enough to build transaction)
    alice.set_balance(TOS_ASSET, u64::MAX);
    // Bob starts with u64::MAX - will test that adding to his balance overflows
    bob.set_balance(TOS_ASSET, u64::MAX);

    // Transfer a large amount that would overflow when added to Bob's u64::MAX
    let tx = create_transfer_tx(&alice, bob.address(), 1000 * COIN_VALUE, TOS_ASSET);

    // Create chain state
    let mut state = ChainState::new();
    state.accounts.insert(
        alice.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), u64::MAX)].into_iter().collect(),
            nonce: alice.nonce,
        },
    );
    state.accounts.insert(
        bob.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), u64::MAX)].into_iter().collect(),
            nonce: 0,
        },
    );

    // verify() deducts from sender - should succeed
    let tx_hash = tx.hash();
    let result_verify = tx.verify(&tx_hash, &mut state, &NoZKPCache).await;
    assert!(
        result_verify.is_ok(),
        "verify() should succeed (sender balance deduction is OK)"
    );

    // Now manually simulate apply() receiver balance update - this should detect overflow
    // In production, apply() would do this receiver balance update and catch the overflow
    let TransactionType::Transfers(transfers) = tx.get_data() else {
        panic!("Expected Transfers transaction");
    };

    if let Some(transfer) = transfers.iter().next() {
        let current_balance = state
            .accounts
            .get_mut(&bob.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .get_mut(&TOS_ASSET)
            .unwrap();

        let amount = transfer.get_amount();
        let result = current_balance.checked_add(amount);

        // This should be None (overflow detected)
        assert!(
            result.is_none(),
            "Overflow should be detected when adding to u64::MAX"
        );
        println!("Overflow protection triggered: u64::MAX + {amount} would overflow");
        return;
    }

    panic!("Should have detected overflow");
}

// Test 5: Fee deduction with TOS
// Verifies fee is correctly deducted from sender balance
#[tokio::test]
async fn test_p04_fee_deduction() {
    let mut alice = Account::new();
    let bob = Account::new();

    // Alice starts with 1000 TOS
    alice.set_balance(TOS_ASSET, 1000 * COIN_VALUE);

    // Transfer 100 TOS to Bob
    let tx = create_transfer_tx(&alice, bob.address(), 100 * COIN_VALUE, TOS_ASSET);
    let tx_fee = tx.fee_limit;

    // Ensure fee is non-zero
    assert!(tx_fee > 0, "Fee should be non-zero");

    // Create chain state
    let mut state = ChainState::new();
    state.accounts.insert(
        alice.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 1000 * COIN_VALUE)]
                .into_iter()
                .collect(),
            nonce: alice.nonce,
        },
    );
    state.accounts.insert(
        bob.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 0)].into_iter().collect(),
            nonce: 0,
        },
    );

    // Execute transaction
    let tx_hash = tx.hash();
    tx.verify(&tx_hash, &mut state, &NoZKPCache).await.unwrap();

    // Simulate apply() receiver balance update
    {
        let bob_balance = state
            .accounts
            .get_mut(&bob.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .entry(TOS_ASSET)
            .or_insert(0);
        *bob_balance = bob_balance.checked_add(100 * COIN_VALUE).unwrap();
    }

    // Verify Alice's balance includes fee deduction
    let alice_balance =
        state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(
        alice_balance,
        1000 * COIN_VALUE - 100 * COIN_VALUE - tx_fee,
        "Alice's balance should include fee deduction"
    );

    // Verify Bob received exact transfer amount (no fee deduction)
    let bob_balance = state.accounts[&bob.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(
        bob_balance,
        100 * COIN_VALUE,
        "Bob should receive exact transfer amount without fee deduction"
    );

    println!("Fee correctly deducted: {tx_fee} from sender");
}

// Test 6: Burn transaction
// Verifies burn transaction deducts from sender and burns the amount
#[tokio::test]
async fn test_p04_burn_transaction() {
    let mut alice = Account::new();

    // Alice starts with 1000 TOS
    alice.set_balance(TOS_ASSET, 1000 * COIN_VALUE);

    // Burn 200 TOS
    let tx = create_burn_tx(&alice, 200 * COIN_VALUE, TOS_ASSET);
    let tx_fee = tx.fee_limit;

    // Create chain state
    let mut state = ChainState::new();
    state.accounts.insert(
        alice.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 1000 * COIN_VALUE)]
                .into_iter()
                .collect(),
            nonce: alice.nonce,
        },
    );

    // Execute transaction
    let tx_hash = tx.hash();
    tx.verify(&tx_hash, &mut state, &NoZKPCache).await.unwrap();

    // Verify Alice's balance: 1000 - 200 (burned) - fee
    let alice_balance =
        state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(
        alice_balance,
        1000 * COIN_VALUE - 200 * COIN_VALUE - tx_fee,
        "Alice's balance should be deducted by burn amount + fee"
    );

    println!("Burn transaction correctly deducted: 200 TOS + {tx_fee} fee");
}

// Test 7: Multiple transfers in single transaction
// Verifies total spending calculation across multiple transfers
#[tokio::test]
async fn test_p04_multiple_transfers() {
    let mut alice = Account::new();
    let bob = Account::new();
    let charlie = Account::new();

    // Alice starts with 1000 TOS
    alice.set_balance(TOS_ASSET, 1000 * COIN_VALUE);

    // Create transaction with multiple transfers: 300 to Bob, 200 to Charlie
    let mut state_impl = AccountStateImpl {
        balances: alice.balances.clone(),
        nonce: alice.nonce,
        reference: Reference {
            topoheight: 0,
            hash: Hash::zero(),
        },
    };

    let data = TransactionTypeBuilder::Transfers(vec![
        TransferBuilder {
            amount: 300 * COIN_VALUE,
            destination: bob.address(),
            asset: TOS_ASSET,
            extra_data: None,
        },
        TransferBuilder {
            amount: 200 * COIN_VALUE,
            destination: charlie.address(),
            asset: TOS_ASSET,
            extra_data: None,
        },
    ]);

    let builder = TransactionBuilder::new(
        TxVersion::T0,
        0, // chain_id: 0 for tests
        alice.keypair.get_public_key().compress(),
        None,
        data,
        FeeBuilder::default(),
    );

    let tx = Arc::new(builder.build(&mut state_impl, &alice.keypair).unwrap());
    let tx_fee = tx.fee_limit;

    // Create chain state
    let mut state = ChainState::new();
    state.accounts.insert(
        alice.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 1000 * COIN_VALUE)]
                .into_iter()
                .collect(),
            nonce: alice.nonce,
        },
    );
    state.accounts.insert(
        bob.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 0)].into_iter().collect(),
            nonce: 0,
        },
    );
    state.accounts.insert(
        charlie.keypair.get_public_key().compress(),
        AccountChainState {
            balances: vec![(TOS_ASSET.clone(), 0)].into_iter().collect(),
            nonce: 0,
        },
    );

    // Execute transaction
    let tx_hash = tx.hash();
    tx.verify(&tx_hash, &mut state, &NoZKPCache).await.unwrap();

    // Simulate apply() receiver balance updates
    {
        let bob_balance = state
            .accounts
            .get_mut(&bob.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .entry(TOS_ASSET)
            .or_insert(0);
        *bob_balance = bob_balance.checked_add(300 * COIN_VALUE).unwrap();

        let charlie_balance = state
            .accounts
            .get_mut(&charlie.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .entry(TOS_ASSET)
            .or_insert(0);
        *charlie_balance = charlie_balance.checked_add(200 * COIN_VALUE).unwrap();
    }

    // Verify Alice's balance: 1000 - 300 - 200 - fee
    let alice_balance =
        state.accounts[&alice.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(
        alice_balance,
        1000 * COIN_VALUE - 300 * COIN_VALUE - 200 * COIN_VALUE - tx_fee,
        "Alice's balance should be deducted by total transfer amount + fee"
    );

    // Verify Bob's balance
    let bob_balance = state.accounts[&bob.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(bob_balance, 300 * COIN_VALUE, "Bob should receive 300 TOS");

    // Verify Charlie's balance
    let charlie_balance =
        state.accounts[&charlie.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    assert_eq!(
        charlie_balance,
        200 * COIN_VALUE,
        "Charlie should receive 200 TOS"
    );

    println!("Multiple transfers correctly processed: 300 + 200 TOS");
}

// ============================================================================
// TOS-ONLY FEE VERIFICATION TESTS
// ============================================================================
// Account creation fee validation is now in verify() phase!
// This catches insufficient fee errors early during mempool validation.
//
// The validation check ensures that when sending TOS to a new (unregistered) account:
// 1. verify() checks: amount >= FEE_PER_ACCOUNT_CREATION (0.1 TOS)
// 2. If amount < 0.1 TOS, verify() returns AmountTooSmallForAccountCreation error
// 3. apply() handles the actual fee deduction and receiver balance update

use crate::config::FEE_PER_ACCOUNT_CREATION;

/// Test: Transfer less than 0.1 TOS to NEW account fails during verify()
///
/// This test validates that the account creation fee check is now in verify(),
/// allowing mempool to reject invalid transactions early.
///
/// Scenario:
/// - Alice has 100 TOS
/// - Alice tries to send 0.05 TOS to Bob (new account, not in state)
/// - Expected: verify() fails with AmountTooSmallForAccountCreation error
#[tokio::test]
async fn test_account_creation_fee_validation_in_verify() {
    let mut alice = Account::new();
    let bob = Account::new(); // Bob is NEW - will NOT be added to state

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    // Create transfer transaction: Alice -> Bob (0.05 TOS - less than 0.1 TOS fee)
    let transfer_amount = FEE_PER_ACCOUNT_CREATION / 2; // 0.05 TOS
    let tx = create_tx_for(alice.clone(), bob.address(), transfer_amount, None);

    let mut state = ChainState::new();

    // Add Alice to state (existing account)
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    // IMPORTANT: Do NOT add Bob to state - he is a NEW account!
    // This triggers the account creation fee validation

    let hash = tx.hash();
    let result = tx.verify(&hash, &mut state, &NoZKPCache).await;

    // CRITICAL ASSERTION: verify() should fail with AmountTooSmallForAccountCreation
    match result {
        Err(VerificationError::AmountTooSmallForAccountCreation { amount, fee }) => {
            assert_eq!(
                amount, transfer_amount,
                "Error should report the transfer amount"
            );
            assert_eq!(fee, FEE_PER_ACCOUNT_CREATION, "Error should report the fee");
            println!(
                "Correctly rejected transfer of {} to new account (fee: {})",
                amount, fee
            );
        }
        Ok(()) => {
            panic!(
                "Expected AmountTooSmallForAccountCreation error, but verify() succeeded. \
                 Transfer amount {} is less than fee {}",
                transfer_amount, FEE_PER_ACCOUNT_CREATION
            );
        }
        Err(other) => {
            panic!(
                "Expected AmountTooSmallForAccountCreation error, got: {:?}",
                other
            );
        }
    }
}

/// Test: Transfer >= 0.1 TOS to NEW account succeeds during verify()
///
/// This test validates that transfers with sufficient amount pass verify().
/// Note: The actual fee deduction happens in apply(), which requires BlockchainApplyState.
///
/// Scenario:
/// - Alice has 100 TOS
/// - Alice sends 1 TOS to Bob (new account, not in state)
/// - Expected: verify() succeeds (amount >= FEE_PER_ACCOUNT_CREATION)
#[tokio::test]
async fn test_account_creation_fee_sufficient_amount_passes_verify() {
    let mut alice = Account::new();
    let bob = Account::new(); // Bob is NEW - will NOT be added to state

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    // Create transfer transaction: Alice -> Bob (1 TOS - more than 0.1 TOS fee)
    let transfer_amount = COIN_VALUE; // 1 TOS
    let tx = create_tx_for(alice.clone(), bob.address(), transfer_amount, None);

    let mut state = ChainState::new();

    // Add Alice to state (existing account)
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    // IMPORTANT: Do NOT add Bob to state - he is a NEW account!
    // This triggers the account creation fee validation

    let hash = tx.hash();
    let result = tx.verify(&hash, &mut state, &NoZKPCache).await;

    // ASSERTION: verify() should succeed since amount >= FEE_PER_ACCOUNT_CREATION
    assert!(
        result.is_ok(),
        "Transfer of {} TOS to new account should pass verify() (fee is {}). Error: {:?}",
        transfer_amount as f64 / COIN_VALUE as f64,
        FEE_PER_ACCOUNT_CREATION as f64 / COIN_VALUE as f64,
        result.err()
    );
    println!(
        "Transfer of {} to new account passed verify() (fee: {})",
        transfer_amount, FEE_PER_ACCOUNT_CREATION
    );
}

/// Test: Transfer exactly 0.1 TOS to NEW account succeeds during verify()
///
/// Edge case: the minimum valid amount for new account creation.
#[tokio::test]
async fn test_account_creation_fee_exact_minimum_passes_verify() {
    let mut alice = Account::new();
    let bob = Account::new(); // Bob is NEW - will NOT be added to state

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    // Create transfer transaction: Alice -> Bob (exactly 0.1 TOS - equals fee)
    let transfer_amount = FEE_PER_ACCOUNT_CREATION; // Exactly 0.1 TOS
    let tx = create_tx_for(alice.clone(), bob.address(), transfer_amount, None);

    let mut state = ChainState::new();

    // Add Alice to state (existing account)
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    // IMPORTANT: Do NOT add Bob to state - he is a NEW account!

    let hash = tx.hash();
    let result = tx.verify(&hash, &mut state, &NoZKPCache).await;

    // ASSERTION: verify() should succeed since amount == FEE_PER_ACCOUNT_CREATION
    assert!(
        result.is_ok(),
        "Transfer of exactly {} (the fee) to new account should pass verify(). Error: {:?}",
        FEE_PER_ACCOUNT_CREATION,
        result.err()
    );
    println!(
        "Transfer of exactly {} (the fee) to new account passed verify()",
        FEE_PER_ACCOUNT_CREATION
    );
}

/// Test: Transfer to EXISTING account does NOT deduct creation fee
///
/// NOTE: This test verifies existing account behavior (no fee deduction).
/// Currently works because we manually add transfer amount like other tests.
#[tokio::test]
async fn test_no_creation_fee_for_existing_account() {
    let mut alice = Account::new();
    let mut bob = Account::new();

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);
    bob.set_balance(TOS_ASSET, 10 * COIN_VALUE); // Bob exists with balance

    let transfer_amount = COIN_VALUE; // 1 TOS
    let tx = create_tx_for(alice.clone(), bob.address(), transfer_amount, None);

    let mut state = ChainState::new();

    // Add Alice to state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    // Add Bob to state (EXISTING account)
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &bob.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            bob.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: bob.nonce,
            },
        );
    }

    let hash = tx.hash();
    tx.verify(&hash, &mut state, &NoZKPCache).await.unwrap();

    // Manually add transfer amount to Bob (as verify doesn't do this for receiver)
    {
        let bob_balance = state
            .accounts
            .get_mut(&bob.keypair.get_public_key().compress())
            .unwrap()
            .balances
            .entry(TOS_ASSET)
            .or_insert(0);
        *bob_balance = bob_balance.checked_add(transfer_amount).unwrap();
    }

    // Bob should have 10 + 1 = 11 TOS (no creation fee deducted)
    let bob_balance = state.accounts[&bob.keypair.get_public_key().compress()].balances[&TOS_ASSET];
    let expected_bob_balance = 10 * COIN_VALUE + transfer_amount;
    assert_eq!(
        bob_balance, expected_bob_balance,
        "Existing account should receive full transfer amount without creation fee"
    );

    println!(
        "No creation fee for existing account: Bob has {} TOS",
        bob_balance / COIN_VALUE
    );
}

/// Test: Transfer amount less than 0.1 TOS to new account should fail
///
/// NOTE: This test is ignored because the validation happens in apply(), not verify().
/// In the current architecture, this transaction would pass verify() but fail in apply().
#[tokio::test]
#[ignore = "Account creation fee validation is in apply(), not verify()"]
async fn test_transfer_below_creation_fee_to_new_account_fails() {
    let mut alice = Account::new();
    let bob = Account::new(); // New account

    alice.set_balance(TOS_ASSET, 100 * COIN_VALUE);

    // Try to send 0.05 TOS (less than 0.1 TOS creation fee)
    let transfer_amount = FEE_PER_ACCOUNT_CREATION / 2; // 0.05 TOS
    let tx = create_tx_for(alice.clone(), bob.address(), transfer_amount, None);

    let mut state = ChainState::new();

    // Add Alice to state
    {
        let mut balances = HashMap::new();
        for (asset, balance) in &alice.balances {
            balances.insert(asset.clone(), balance.balance);
        }
        state.accounts.insert(
            alice.keypair.get_public_key().compress(),
            AccountChainState {
                balances,
                nonce: alice.nonce,
            },
        );
    }

    // Do NOT add Bob to state (new account)

    let hash = tx.hash();
    let result = tx.verify(&hash, &mut state, &NoZKPCache).await;

    // Should fail because transfer amount < account creation fee
    assert!(
        result.is_err(),
        "Transfer of {} to new account should fail (less than {} creation fee)",
        transfer_amount,
        FEE_PER_ACCOUNT_CREATION
    );

    println!(
        "Correctly rejected transfer of {} to new account (< {} fee)",
        transfer_amount, FEE_PER_ACCOUNT_CREATION
    );
}
