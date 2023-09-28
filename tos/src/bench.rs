// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

#![deny(warnings)]

use tos::{network, transport};
use cores::{validator::*, base_types::*, validators::*, messages::*, serialize::*};

use bytes::Bytes;
use futures::stream::StreamExt;
use log::*;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use structopt::StructOpt;
use tokio::{runtime::Builder, time};

use std::thread;

#[derive(Debug, Clone, StructOpt)]
#[structopt(
    name = "Tos Benchmark",
    about = "Local end-to-end test and benchmark of the Tos protocol"
)]
struct ClientServerBenchmark {
    /// Choose a network protocol between Udp and Tcp
    #[structopt(long, default_value = "udp")]
    protocol: transport::Protocol,
    /// Hostname
    #[structopt(long, default_value = "127.0.0.1")]
    host: String,
    /// Base port number
    #[structopt(long, default_value = "9555")]
    port: u32,
    /// Size of the Tos validators
    #[structopt(long, default_value = "4")]
    validators_size: usize,
    /// Number of shards per Tos validator
    #[structopt(long, default_value = "15")]
    shards: u32,
    /// Maximum number of requests in flight (0 for blocking client)
    #[structopt(long, default_value = "1000")]
    max_in_flight: usize,
    /// Number of accounts and transactions used in the benchmark
    #[structopt(long, default_value = "40000")]
    num_accounts: usize,
    /// Timeout for sending queries (us)
    #[structopt(long, default_value = "4000000")]
    send_timeout_us: u64,
    /// Timeout for receiving responses (us)
    #[structopt(long, default_value = "4000000")]
    recv_timeout_us: u64,
    /// Maximum size of datagrams received and sent (bytes)
    #[structopt(long, default_value = transport::DEFAULT_MAX_DATAGRAM_SIZE)]
    buffer_size: usize,
    /// Number of cross shards messages allowed before blocking the main server loop
    #[structopt(long, default_value = "1")]
    cross_shard_queue_size: usize,
}

fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let benchmark = ClientServerBenchmark::from_args();

    let (states, txs) = benchmark.make_structures();

    // Start the servers on the thread pool
    for state in states {
        // Make special single-core runtime for each server
        let b = benchmark.clone();
        thread::spawn(move || {
            let mut runtime = Builder::new()
                .enable_all()
                .basic_scheduler()
                .thread_stack_size(15 * 1024 * 1024)
                .build()
                .unwrap();

            runtime.block_on(async move {
                let server = b.spawn_server(state).await;
                if let Err(err) = server.join().await {
                    error!("Server ended with an error: {}", err);
                }
            });
        });
    }

    let mut runtime = Builder::new()
        .enable_all()
        .basic_scheduler()
        .thread_stack_size(15 * 1024 * 1024)
        .build()
        .unwrap();
    runtime.block_on(benchmark.launch_client(txs));
}

impl ClientServerBenchmark {
    fn make_structures(&self) -> (Vec<ValidatorState>, Vec<(u32, Bytes)>) {
        info!("Preparing accounts.");
        let mut keys = Vec::new();
        for _ in 0..self.validators_size {
            keys.push(get_key_pair());
        }
        let validators = Validators {
            voting_rights: keys.iter().map(|(k, _)| (*k, 1)).collect(),
            total_votes: self.validators_size,
        };

        // Pick an validator and create one state per shard.
        let (public_auth0, secret_auth0) = keys.pop().unwrap();
        let mut states = Vec::new();
        for i in 0..self.shards {
            let state = ValidatorState::new_shard(
                validators.clone(),
                public_auth0,
                secret_auth0.copy(),
                i as u32,
                self.shards,
            );
            states.push(state);
        }

        // Seed user accounts.
        let mut account_keys = Vec::new();
        for _ in 0..self.num_accounts {
            let keypair = get_key_pair();
            let i = ValidatorState::get_shard(self.shards, &keypair.0) as usize;
            assert!(states[i].in_shard(&keypair.0));
            let client = AccountOffchainState {
                balance: Balance::from(Amount::from(100)),
                nonce: Nonce::from(0),
                pending_confirmation: None,
                confirmed_log: Vec::new(),
                synchronization_log: Vec::new(),
                received_log: Vec::new(),
            };
            states[i].accounts.insert(keypair.0, client);
            account_keys.push(keypair);
        }

        info!("Preparing transactions.");
        // Make one transaction per account (transfer tx + confirmation).
        let mut txs: Vec<(u32, Bytes)> = Vec::new();
        let mut next_recipient = get_key_pair().0;
        for (pubx, secx) in account_keys.iter() {
            let transfer = Transfer {
                sender: *pubx,
                recipient: next_recipient,
                amount: Amount::from(50),
                nonce: Nonce::from(0),
                user_data: UserData::default(),
            };
            next_recipient = *pubx;
            let tx = Transaction::new(transfer.clone(), secx);
            let shard = ValidatorState::get_shard(self.shards, pubx);

            // Serialize tx
            let bufx = serialize_transfer_tx(&tx);
            assert!(!bufx.is_empty());

            // Make certificate
            let mut certificate = CertifiedTransaction {
                value: tx,
                signatures: Vec::new(),
            };
            for i in 0..validators.quorum_threshold() {
                let (pubx, secx) = keys.get(i).unwrap();
                let sig = Signature::new(&certificate.value.transfer, secx);
                certificate.signatures.push((*pubx, sig));
            }

            let bufx2 = serialize_cert(&certificate);
            assert!(!bufx2.is_empty());

            txs.push((shard, bufx2.into()));
            txs.push((shard, bufx.into()));
        }

        (states, txs)
    }

    async fn spawn_server(&self, state: ValidatorState) -> transport::SpawnedServer {
        let server = network::Server::new(
            self.protocol,
            self.host.clone(),
            self.port,
            state,
            self.buffer_size,
            self.cross_shard_queue_size,
        );
        server.spawn().await.unwrap()
    }

    async fn launch_client(&self, mut txs: Vec<(u32, Bytes)>) {
        time::delay_for(Duration::from_millis(1000)).await;

        let items_number = txs.len() / 2;
        let time_start = Instant::now();

        let max_in_flight = (self.max_in_flight / self.shards as usize) as usize;
        info!("Set max_in_flight per shard to {}", max_in_flight);

        info!("Sending requests.");
        if self.max_in_flight > 0 {
            let mass_client = network::MassClient::new(
                self.protocol,
                self.host.clone(),
                self.port,
                self.buffer_size,
                Duration::from_micros(self.send_timeout_us),
                Duration::from_micros(self.recv_timeout_us),
                max_in_flight as u64,
            );
            let mut sharded_requests = HashMap::new();
            for (shard, buf) in txs.iter().rev() {
                sharded_requests
                    .entry(*shard)
                    .or_insert_with(Vec::new)
                    .push(buf.clone());
            }
            let responses = mass_client.run(sharded_requests).concat().await;
            info!("Received {} responses.", responses.len(),);
        } else {
            // Use actual client core
            let mut client = network::Client::new(
                self.protocol,
                self.host.clone(),
                self.port,
                self.shards,
                self.buffer_size,
                Duration::from_micros(self.send_timeout_us),
                Duration::from_micros(self.recv_timeout_us),
            );

            while !txs.is_empty() {
                if txs.len() % 1000 == 0 {
                    info!("Process message {}...", txs.len());
                }
                let (shard, tx) = txs.pop().unwrap();
                let status = client.send_recv_bytes(shard, tx.to_vec()).await;
                match status {
                    Ok(info) => {
                        debug!("Query response: {:?}", info);
                    }
                    Err(error) => {
                        error!("Failed to execute tx: {}", error);
                    }
                }
            }
        }

        let time_total = time_start.elapsed().as_micros();
        warn!(
            "Total time: {}ms, items: {}, tx/sec: {}",
            time_total,
            items_number,
            1_000_000.0 * (items_number as f64) / (time_total as f64)
        );
    }
}
