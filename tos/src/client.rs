// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

#![deny(warnings)]

use tos::{config::*, network, transport};
use cores::{
    validator::*, base_types::*, client::*, validators::Validators, messages::*, serialize::*,
};

use bytes::Bytes;
use futures::stream::StreamExt;
use log::*;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use structopt::StructOpt;
use tokio::runtime::Runtime;

fn make_validator_clients(
    validators_config: &ValidatorsConfig,
    buffer_size: usize,
    send_timeout: std::time::Duration,
    recv_timeout: std::time::Duration,
) -> HashMap<ValidatorName, network::Client> {
    let mut validator_clients = HashMap::new();
    for config in &validators_config.validators {
        let config = config.clone();
        let client = network::Client::new(
            config.protocol,
            config.host,
            config.port,
            config.shards,
            buffer_size,
            send_timeout,
            recv_timeout,
        );
        validator_clients.insert(config.address, client);
    }
    validator_clients
}

fn make_validator_mass_clients(
    validators_config: &ValidatorsConfig,
    buffer_size: usize,
    send_timeout: std::time::Duration,
    recv_timeout: std::time::Duration,
    max_in_flight: u64,
) -> Vec<(u32, network::MassClient)> {
    let mut validator_clients = Vec::new();
    for config in &validators_config.validators {
        let client = network::MassClient::new(
            config.protocol,
            config.host.clone(),
            config.port,
            buffer_size,
            send_timeout,
            recv_timeout,
            max_in_flight / config.shards as u64, // Distribute window to diff shards
        );
        validator_clients.push((config.shards, client));
    }
    validator_clients
}

fn make_client_state(
    accounts: &mut AccountsConfig,
    validators_config: &ValidatorsConfig,
    address: Address,
    buffer_size: usize,
    send_timeout: std::time::Duration,
    recv_timeout: std::time::Duration,
) -> ClientState<network::Client> {
    let account = accounts.get(&address).expect("Unknown account");
    let validators = Validators::new(validators_config.voting_rights());
    let validator_clients =
        make_validator_clients(validators_config, buffer_size, send_timeout, recv_timeout);
    ClientState::new(
        address,
        account.key.copy(),
        validators,
        validator_clients,
        account.nonce,
        account.sent.clone(),
        account.received.clone(),
        account.balance,
    )
}

/// Make one transfer tx per account, up to `max_txs` transfers.
fn make_benchmark_transfer_txs(
    accounts_config: &mut AccountsConfig,
    max_txs: usize,
) -> (Vec<Transaction>, Vec<(Address, Bytes)>) {
    let mut txs = Vec::new();
    let mut serialized_txs = Vec::new();
    // TODO: deterministic sequence of txs to recover from interrupted benchmarks.
    let mut next_recipient = get_key_pair().0;
    for account in accounts_config.accounts_mut() {
        let transfer = Transfer {
            sender: account.address,
            recipient: next_recipient,
            amount: Amount::from(1),
            nonce: account.nonce,
            user_data: UserData::default(),
        };
        debug!("Preparing transfer tx: {:?}", transfer);
        account.nonce = account.nonce.increment().unwrap();
        next_recipient = account.address;
        let tx = Transaction::new(transfer.clone(), &account.key);
        txs.push(tx.clone());
        let serialized_tx = serialize_transfer_tx(&tx);
        serialized_txs.push((account.address, serialized_tx.into()));
        if serialized_txs.len() >= max_txs {
            break;
        }
    }
    (txs, serialized_txs)
}

/// Try to make certificates from txs and server configs
fn make_benchmark_certificates_from_txs_and_server_configs(
    txs: Vec<Transaction>,
    server_config: Vec<&str>,
) -> Vec<(Address, Bytes)> {
    let mut keys = Vec::new();
    for file in server_config {
        let server_config = ValidatorServerConfig::read(file).expect("Fail to read server config");
        keys.push((server_config.validator.address, server_config.key));
    }
    let validators = Validators {
        voting_rights: keys.iter().map(|(k, _)| (*k, 1)).collect(),
        total_votes: keys.len(),
    };
    assert!(
        keys.len() >= validators.quorum_threshold(),
        "Not enough server configs were provided with --server-configs"
    );
    let mut serialized_certificates = Vec::new();
    for tx in txs {
        let mut certificate = CertifiedTransaction {
            value: tx.clone(),
            signatures: Vec::new(),
        };
        for i in 0..validators.quorum_threshold() {
            let (pubx, secx) = keys.get(i).unwrap();
            let sig = Signature::new(&certificate.value.transfer, secx);
            certificate.signatures.push((*pubx, sig));
        }
        let serialized_certificate = serialize_cert(&certificate);
        serialized_certificates.push((tx.transfer.sender, serialized_certificate.into()));
    }
    serialized_certificates
}

/// Try to aggregate votes into certificates.
fn make_benchmark_certificates_from_votes(
    validators_config: &ValidatorsConfig,
    votes: Vec<SignedTransaction>,
) -> Vec<(Address, Bytes)> {
    let validators = Validators::new(validators_config.voting_rights());
    let mut aggregators = HashMap::new();
    let mut certificates = Vec::new();
    let mut done_senders = HashSet::new();
    for vote in votes {
        // We aggregate votes indexed by sender.
        let address = vote.value.transfer.sender;
        if done_senders.contains(&address) {
            continue;
        }
        debug!(
            "Processing vote on {}'s transfer by {}",
            encode_address(&address),
            encode_address(&vote.validator)
        );
        let value = vote.value;
        let aggregator = aggregators
            .entry(address)
            .or_insert_with(|| SignatureAggregator::try_new(value, &validators).unwrap());
        match aggregator.append(vote.validator, vote.signature) {
            Ok(Some(certificate)) => {
                debug!("Found certificate: {:?}", certificate);
                let buf = serialize_cert(&certificate);
                certificates.push((address, buf.into()));
                done_senders.insert(address);
            }
            Ok(None) => {
                debug!("Added one vote");
            }
            Err(error) => {
                error!("Failed to aggregate vote: {}", error);
            }
        }
    }
    certificates
}

/// Broadcast a bulk of requests to each validator.
async fn mass_broadcast_txs(
    phase: &'static str,
    validators_config: &ValidatorsConfig,
    buffer_size: usize,
    send_timeout: std::time::Duration,
    recv_timeout: std::time::Duration,
    max_in_flight: u64,
    txs: Vec<(Address, Bytes)>,
) -> Vec<Bytes> {
    let time_start = Instant::now();
    info!("Broadcasting {} {} txs", txs.len(), phase);
    let validator_clients = make_validator_mass_clients(
        validators_config,
        buffer_size,
        send_timeout,
        recv_timeout,
        max_in_flight,
    );
    let mut streams = Vec::new();
    for (shards, client) in validator_clients {
        // Re-index txs by shard for this particular validator client.
        let mut sharded_requests = HashMap::new();
        for (address, buf) in &txs {
            let shard = ValidatorState::get_shard(shards, address);
            sharded_requests
                .entry(shard)
                .or_insert_with(Vec::new)
                .push(buf.clone());
        }
        streams.push(client.run(sharded_requests));
    }
    let responses = futures::stream::select_all(streams).concat().await;
    let time_elapsed = time_start.elapsed();
    warn!(
        "Received {} responses in {} ms.",
        responses.len(),
        time_elapsed.as_millis()
    );
    warn!(
        "Estimated server throughput: {} {} txs per sec",
        (txs.len() as u128) * 1_000_000 / time_elapsed.as_micros(),
        phase
    );
    responses
}

fn mass_update_recipients(
    accounts_config: &mut AccountsConfig,
    certificates: Vec<(Address, Bytes)>,
) {
    for (_sender, buf) in certificates {
        if let Ok(SerializedMessage::Cert(certificate)) = deserialize_message(&buf[..]) {
            accounts_config.update_for_received_transfer(*certificate);
        }
    }
}

fn deserialize_response(response: &[u8]) -> Option<AccountInfoResponse> {
    match deserialize_message(response) {
        Ok(SerializedMessage::InfoResp(info)) => Some(*info),
        Ok(SerializedMessage::Error(error)) => {
            error!("Received error value: {}", error);
            None
        }
        Ok(_) => {
            error!("Unexpected return value");
            None
        }
        Err(error) => {
            error!(
                "Unexpected error: {} while deserializing {:?}",
                error, response
            );
            None
        }
    }
}

#[derive(StructOpt)]
#[structopt(
    name = "Tos Client",
    about = "A Byzantine fault tolerant payments sidechain with low-latency finality and high throughput"
)]
struct ClientOpt {
    /// Sets the file describing the public configurations of all validators
    #[structopt(long)]
    validators: String,

    /// Timeout for sending queries (us)
    #[structopt(long, default_value = "4000000")]
    send_timeout: u64,

    /// Timeout for receiving responses (us)
    #[structopt(long, default_value = "4000000")]
    recv_timeout: u64,

    /// Maximum size of datagrams received and sent (bytes)
    #[structopt(long, default_value = transport::DEFAULT_MAX_DATAGRAM_SIZE)]
    buffer_size: usize,

    /// Subcommands. Acceptable values are transfer, query_balance, benchmark, and create_accounts.
    #[structopt(subcommand)]
    cmd: ClientCommands,
}

#[derive(StructOpt)]
enum ClientCommands {
    /// Transfer funds
    #[structopt(name = "transfer")]
    Transfer {
        /// Sending address (must be one of our accounts)
        #[structopt(long)]
        from: String,

        /// Recipient address
        #[structopt(long)]
        to: String,

        /// Amount to transfer
        amount: u64,
    },

    /// Obtain the spendable balance
    #[structopt(name = "query_balance")]
    QueryBalance {
        /// Address of the account
        address: String,
    },

    /// Send one transfer per account in bulk mode
    #[structopt(name = "benchmark")]
    Benchmark {
        /// Maximum number of requests in flight
        #[structopt(long, default_value = "200")]
        max_in_flight: u64,

        /// Use a subset of the accounts to generate N transfers
        #[structopt(long)]
        max_txs: Option<usize>,

        /// Use server configuration files to generate certificates (instead of aggregating received votes).
        #[structopt(long)]
        server_configs: Option<Vec<String>>,
    },

    /// Create new user accounts and print the public keys
    #[structopt(name = "create_accounts")]
    CreateAccounts {
        /// known initial balance of the account
        #[structopt(long, default_value = "0")]
        initial_funding: Balance,

        /// Number of additional accounts to create
        num: u32,
    },
}

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let options = ClientOpt::from_args();

    let send_timeout = Duration::from_micros(options.send_timeout);
    let recv_timeout = Duration::from_micros(options.recv_timeout);
    let validators_config_path = &options.validators;
    let buffer_size = options.buffer_size;

    let mut accounts_config = 
        AccountsConfig::read_or_create().expect("Unable to read user accounts");
    let validators_config =
        ValidatorsConfig::read(validators_config_path).expect("Unable to read validators config file");

    match options.cmd {
        ClientCommands::Transfer { from, to, amount } => {
            let sender = decode_address(&from).expect("Failed to decode sender's address");
            let recipient = decode_address(&to).expect("Failed to decode recipient's address");
            let amount = Amount::from(amount);

            let mut rt = Runtime::new().unwrap();
            rt.block_on(async move {
                let mut client_state = make_client_state(
                    &mut accounts_config,
                    &validators_config,
                    sender,
                    buffer_size,
                    send_timeout,
                    recv_timeout,
                );
                info!("Starting transfer");
                let time_start = Instant::now();
                let cert = client_state
                    .transfer_to_tos(amount, recipient, UserData::default())
                    .await
                    .unwrap();
                let time_total = time_start.elapsed().as_micros();
                info!("Transfer confirmed after {} us", time_total);
                println!("{:?}", cert);
                accounts_config.update_from_state(&client_state);
                info!("Updating recipient's local balance");
                let mut recipient_client_state = make_client_state(
                    &mut accounts_config,
                    &validators_config,
                    recipient,
                    buffer_size,
                    send_timeout,
                    recv_timeout,
                );
                recipient_client_state
                    .receive_from_tos(cert)
                    .await
                    .unwrap();
                accounts_config.update_from_state(&recipient_client_state);
                info!("Saved user account states");
            });
        }

        ClientCommands::QueryBalance { address } => {
            let user_address = decode_address(&address).expect("Failed to decode address");

            let mut rt = Runtime::new().unwrap();
            rt.block_on(async move {
                let mut client_state = make_client_state(
                    &mut accounts_config,
                    &validators_config,
                    user_address,
                    buffer_size,
                    send_timeout,
                    recv_timeout,
                );
                info!("Starting balance query");
                let time_start = Instant::now();
                let amount = client_state.get_spendable_amount().await.unwrap();
                let time_total = time_start.elapsed().as_micros();
                info!("Balance confirmed after {} us", time_total);
                println!("{:?}", amount);
                accounts_config.update_from_state(&client_state);
                info!("Saved client account state");
            });
        }

        ClientCommands::Benchmark {
            max_in_flight,
            max_txs,
            server_configs,
        } => {
            let max_txs = max_txs.unwrap_or_else(|| accounts_config.num_accounts());

            let mut rt = Runtime::new().unwrap();
            rt.block_on(async move {
                warn!("Starting benchmark phase 1 (transfer txs)");
                let (txs, serialize_txs) =
                    make_benchmark_transfer_txs(&mut accounts_config, max_txs);
                let responses = mass_broadcast_txs(
                    "transfer",
                    &validators_config,
                    buffer_size,
                    send_timeout,
                    recv_timeout,
                    max_in_flight,
                    serialize_txs,
                )
                .await;
                let votes: Vec<_> = responses
                    .into_iter()
                    .filter_map(|buf| {
                        deserialize_response(&buf[..]).and_then(|info| info.pending_confirmation)
                    })
                    .collect();
                warn!("Received {} valid votes.", votes.len());

                warn!("Starting benchmark phase 2 (confirmation txs)");
                let certificates = if let Some(files) = server_configs {
                    warn!("Using server configs provided by --server-configs");
                    let files = files.iter().map(AsRef::as_ref).collect();
                    make_benchmark_certificates_from_txs_and_server_configs(txs, files)
                } else {
                    warn!("Using validators config");
                    make_benchmark_certificates_from_votes(&validators_config, votes)
                };
                let responses = mass_broadcast_txs(
                    "confirmation",
                    &validators_config,
                    buffer_size,
                    send_timeout,
                    recv_timeout,
                    max_in_flight,
                    certificates.clone(),
                )
                .await;
                let mut confirmed = HashSet::new();
                let num_valid =
                    responses
                        .iter()
                        .fold(0, |acc, buf| match deserialize_response(&buf[..]) {
                            Some(info) => {
                                confirmed.insert(info.sender);
                                acc + 1
                            }
                            None => acc,
                        });
                warn!(
                    "Received {} valid confirmations for {} transfers.",
                    num_valid,
                    confirmed.len()
                );

                warn!("Updating local state of user accounts");
                // Make sure that the local balances are accurate so that future
                // balance checks of the non-mass client pass.
                mass_update_recipients(&mut accounts_config, certificates);
                info!("Saved client account state");
            });
        }

        ClientCommands::CreateAccounts {
            initial_funding,
            num,
        } => {
            let num_accounts: u32 = num;
            for _ in 0..num_accounts {
                let account = UserAccount::new(initial_funding);
                println!("{}:{}", encode_address(&account.address), initial_funding);
                accounts_config.insert(account);
            }
        }
    }
}
