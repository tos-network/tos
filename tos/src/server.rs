// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

#![deny(warnings)]

use tos::{config::*, network, transport};
use cores::{validator::*, base_types::*, validators::Validators};

use futures::future::join_all;
use log::*;
use structopt::StructOpt;
use tokio::runtime::Runtime;

#[allow(clippy::too_many_arguments)]
fn make_shard_server(
    local_ip_addr: &str,
    server_config_path: &str,
    validators_config_path: &str,
    initial_accounts_config_path: &str,
    buffer_size: usize,
    cross_shard_queue_size: usize,
    shard: u32,
) -> network::Server {
    let server_config =
        ValidatorServerConfig::read(server_config_path).expect("Fail to read server config");
    let validators_config =
        ValidatorsConfig::read(validators_config_path).expect("Fail to read validators config");
    let initial_accounts_config = InitialStateConfig::read(initial_accounts_config_path)
        .expect("Fail to read initial account config");

    let validators = Validators::new(validators_config.voting_rights());
    let shards = server_config.validator.shards;

    let mut state = ValidatorState::new_shard(
        validators,
        server_config.validator.address,
        server_config.key.copy(),
        shard,
        shards,
    );

    // Load initial states
    for (address, balance) in &initial_accounts_config.accounts {
        if ValidatorState::get_shard(shards, address) != shard {
            continue;
        }
        let client = AccountOffchainState {
            balance: *balance,
            nonce: Nonce::from(0),
            pending_confirmation: None,
            confirmed_log: Vec::new(),
            synchronization_log: Vec::new(),
            received_log: Vec::new(),
        };
        state.accounts.insert(*address, client);
    }

    network::Server::new(
        server_config.validator.network_protocol,
        local_ip_addr.to_string(),
        server_config.validator.port,
        state,
        buffer_size,
        cross_shard_queue_size,
    )
}

fn make_servers(
    local_ip_addr: &str,
    server_config_path: &str,
    validators_config_path: &str,
    initial_accounts_config_path: &str,
    buffer_size: usize,
    cross_shard_queue_size: usize,
) -> Vec<network::Server> {
    let server_config =
        ValidatorServerConfig::read(server_config_path).expect("Fail to read server config");
    let shards = server_config.validator.shards;

    let mut servers = Vec::new();
    for shard in 0..shards {
        servers.push(make_shard_server(
            local_ip_addr,
            server_config_path,
            validators_config_path,
            initial_accounts_config_path,
            buffer_size,
            cross_shard_queue_size,
            shard,
        ))
    }
    servers
}

#[derive(StructOpt)]
#[structopt(
    name = "Tos Server",
    about = "A byzantine fault tolerant payments sidechain with low-latency finality and high throughput"
)]
struct ServerOpt {
    /// Path to the file containing the server configuration of this Tos validator (including its secret key)
    #[structopt(long)]
    server: String,

    /// Subcommands. Acceptable values are run and generate.
    #[structopt(subcommand)]
    cmd: ServerCommands,
}

#[derive(StructOpt)]
enum ServerCommands {
    /// Runs a service for each shard of the Tos validator")
    #[structopt(name = "run")]
    Run {
        /// Maximum size of datagrams received and sent (bytes)
        #[structopt(long, default_value = transport::DEFAULT_MAX_DATAGRAM_SIZE)]
        buffer_size: usize,

        /// Number of cross shards messages allowed before blocking the main server loop
        #[structopt(long, default_value = "1000")]
        cross_shard_queue_size: usize,

        /// Path to the file containing the public description of all validators in this Tos validators
        #[structopt(long)]
        validators: String,

        /// Path to the file describing the initial user accounts
        #[structopt(long)]
        initial_accounts: String,

        /// Runs a specific shard (from 0 to shards-1)
        #[structopt(long)]
        shard: Option<u32>,
    },

    /// Generate a new server configuration and output its public description
    #[structopt(name = "generate")]
    Generate {
        /// Chooses a network protocol between Udp and Tcp
        #[structopt(long, default_value = "Udp")]
        protocol: transport::NetworkProtocol,

        /// Sets the public name of the host
        #[structopt(long)]
        host: String,

        /// Sets the base port, i.e. the port on which the server listens for the first shard
        #[structopt(long)]
        port: u32,

        /// Number of shards for this validator
        #[structopt(long)]
        shards: u32,
    },
}

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let options = ServerOpt::from_args();

    let server_config_path = &options.server;

    match options.cmd {
        ServerCommands::Run {
            buffer_size,
            cross_shard_queue_size,
            validators,
            initial_accounts,
            shard,
        } => {
            // Run the server
            let servers = match shard {
                Some(shard) => {
                    info!("Running shard number {}", shard);
                    let server = make_shard_server(
                        "0.0.0.0", // Allow local IP address to be different from the public one.
                        server_config_path,
                        &validators,
                        &initial_accounts,
                        buffer_size,
                        cross_shard_queue_size,
                        shard,
                    );
                    vec![server]
                }
                None => {
                    info!("Running all shards");
                    make_servers(
                        "0.0.0.0", // Allow local IP address to be different from the public one.
                        server_config_path,
                        &validators,
                        &initial_accounts,
                        buffer_size,
                        cross_shard_queue_size,
                    )
                }
            };

            let mut rt = Runtime::new().unwrap();
            let mut handles = Vec::new();
            for server in servers {
                handles.push(async move {
                    let spawned_server = match server.spawn().await {
                        Ok(server) => server,
                        Err(err) => {
                            error!("Failed to start server: {}", err);
                            return;
                        }
                    };
                    if let Err(err) = spawned_server.join().await {
                        error!("Server ended with an error: {}", err);
                    }
                });
            }
            rt.block_on(join_all(handles));
        }

        ServerCommands::Generate {
            protocol,
            host,
            port,
            shards,
        } => {
            let (address, key) = get_key_pair();
            let validator = ValidatorConfig {
                network_protocol: protocol,
                address,
                host,
                port: port,
                shards: shards,
            };
            let server = ValidatorServerConfig { validator, key };
            server
                .write(server_config_path)
                .expect("Unable to write server config file");
            info!("Wrote server config file");
            server.validator.print();
        }
    }
}
