// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use crate::transport::*;
use cores::{validator::*, base_types::*, client::*, error::*, messages::*, serialize::*};

use bytes::Bytes;
use futures::{channel::mpsc, future::FutureExt, sink::SinkExt, stream::StreamExt};
use log::*;
use std::io;
use tokio::time;

pub struct Server {
    protocol: Protocol,
    address: String,
    port: u32,
    state: ValidatorState,
    buffer_size: usize,
    cross_shard_queue_size: usize,
    // Stats
    packets_processed: u64,
    user_errors: u64,
}

impl Server {
    pub fn new(
        protocol: Protocol,
        address: String,
        port: u32,
        state: ValidatorState,
        buffer_size: usize,
        cross_shard_queue_size: usize,
    ) -> Self {
        Self {
            protocol,
            address,
            port,
            state,
            buffer_size,
            cross_shard_queue_size,
            packets_processed: 0,
            user_errors: 0,
        }
    }

    pub fn packets_processed(&self) -> u64 {
        self.packets_processed
    }

    pub fn user_errors(&self) -> u64 {
        self.user_errors
    }

    async fn forward_cross_shard_queries(
        protocol: Protocol,
        address: String,
        port: u32,
        this_shard: ShardId,
        mut receiver: mpsc::Receiver<(Vec<u8>, ShardId)>,
    ) {
        let mut pool = protocol
            .make_outgoing_connection_pool()
            .await
            .expect("Initialization should not fail");

        let mut queries_sent = 0u64;
        while let Some((buf, shard)) = receiver.next().await {
            // Send cross-shard query.
            let remote_address = format!("{}:{}", address, port + shard);
            let status = pool.send_data_to(&buf, &remote_address).await;
            if let Err(error) = status {
                error!("Failed to send cross-shard query: {}", error);
            } else {
                debug!("Sent cross shard query: {} -> {}", this_shard, shard);
                queries_sent += 1;
                if queries_sent % 2000 == 0 {
                    info!(
                        "{}:{} (shard {}) has sent {} cross-shard queries",
                        address,
                        port + this_shard,
                        this_shard,
                        queries_sent
                    );
                }
            }
        }
    }

    pub async fn spawn(self) -> Result<SpawnedServer, io::Error> {
        info!(
            "Listening to {} traffic on {}:{}",
            self.protocol,
            self.address,
            self.port + self.state.shard_id
        );
        let address = format!(
            "{}:{}",
            self.address,
            self.port + self.state.shard_id
        );

        let (cross_shard_sender, cross_shard_receiver) = mpsc::channel(self.cross_shard_queue_size);
        tokio::spawn(Self::forward_cross_shard_queries(
            self.protocol,
            self.address.clone(),
            self.port,
            self.state.shard_id,
            cross_shard_receiver,
        ));

        let buffer_size = self.buffer_size;
        let protocol = self.protocol;
        let state = RunningServerState {
            server: self,
            cross_shard_sender,
        };
        // Launch server for the appropriate protocol.
        protocol.spawn_server(&address, state, buffer_size).await
    }
}

struct RunningServerState {
    server: Server,
    cross_shard_sender: mpsc::Sender<(Vec<u8>, ShardId)>,
}

impl MessageHandler for RunningServerState {
    fn handle_message<'a>(
        &'a mut self,
        buffer: &'a [u8],
    ) -> futures::future::BoxFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            let result = deserialize_message(buffer);
            let reply = match result {
                Err(_) => Err(TosError::InvalidDecoding),
                Ok(result) => {
                    match result {
                        SerializedMessage::Tx(message) => self
                            .server
                            .state
                            .handle_transfer_tx(*message)
                            .map(|info| Some(serialize_info_response(&info))),
                        SerializedMessage::Cert(message) => {
                            let confirmation_tx = ConfirmationTx {
                                ctx: message.as_ref().clone(),
                            };
                            match self
                                .server
                                .state
                                .handle_confirmation_tx(confirmation_tx)
                            {
                                Ok((info, send_shard)) => {
                                    // Send a message to other shard
                                    if let Some(cross_shard_update) = send_shard {
                                        let shard = cross_shard_update.shard_id;
                                        let tmp_out = serialize_cross_shard(&message);
                                        debug!(
                                            "Scheduling cross shard query: {} -> {}",
                                            self.server.state.shard_id, shard
                                        );
                                        self.cross_shard_sender
                                            .send((tmp_out, shard))
                                            .await
                                            .expect("internal channel should not fail");
                                    };

                                    // Response
                                    Ok(Some(serialize_info_response(&info)))
                                }
                                Err(error) => Err(error),
                            }
                        }
                        SerializedMessage::InfoReq(message) => self
                            .server
                            .state
                            .handle_account_info_request(*message)
                            .map(|info| Some(serialize_info_response(&info))),
                        SerializedMessage::CrossShard(message) => {
                            match self
                                .server
                                .state
                                .handle_cross_shard_recipient_commit(*message)
                            {
                                Ok(_) => Ok(None), // Nothing to reply
                                Err(error) => {
                                    error!("Failed to handle cross-shard query: {}", error);
                                    Ok(None) // Nothing to reply
                                }
                            }
                        }
                        _ => Err(TosError::UnexpectedMessage),
                    }
                }
            };

            self.server.packets_processed += 1;
            if self.server.packets_processed % 5000 == 0 {
                info!(
                    "{}:{} (shard {}) has processed {} packets",
                    self.server.address,
                    self.server.port + self.server.state.shard_id,
                    self.server.state.shard_id,
                    self.server.packets_processed
                );
            }

            match reply {
                Ok(x) => x,
                Err(error) => {
                    warn!("User query failed: {}", error);
                    self.server.user_errors += 1;
                    Some(serialize_error(&error))
                }
            }
        })
    }
}

#[derive(Clone)]
pub struct Client {
    protocol: Protocol,
    address: String,
    port: u32,
    shards: u32,
    buffer_size: usize,
    send_timeout: std::time::Duration,
    recv_timeout: std::time::Duration,
}

impl Client {
    pub fn new(
        protocol: Protocol,
        address: String,
        port: u32,
        shards: u32,
        buffer_size: usize,
        send_timeout: std::time::Duration,
        recv_timeout: std::time::Duration,
    ) -> Self {
        Self {
            protocol,
            address,
            port,
            shards,
            buffer_size,
            send_timeout,
            recv_timeout,
        }
    }

    async fn send_recv_bytes_internal(
        &mut self,
        shard: ShardId,
        buf: Vec<u8>,
    ) -> Result<Vec<u8>, io::Error> {
        let address = format!("{}:{}", self.address, self.port + shard);
        let mut stream = self
            .protocol
            .connect(address, self.buffer_size)
            .await?;
        // Send message
        time::timeout(self.send_timeout, stream.write_data(&buf)).await??;
        // Wait for reply
        time::timeout(self.recv_timeout, stream.read_data()).await?
    }

    pub async fn send_recv_bytes(
        &mut self,
        shard: ShardId,
        buf: Vec<u8>,
    ) -> Result<AccountInfoResponse, TosError> {
        match self.send_recv_bytes_internal(shard, buf).await {
            Err(error) => Err(TosError::ClientIoError {
                error: format!("{}", error),
            }),
            Ok(response) => {
                // Parse reply
                match deserialize_message(&response[..]) {
                    Ok(SerializedMessage::InfoResp(resp)) => Ok(*resp),
                    Ok(SerializedMessage::Error(error)) => Err(*error),
                    Err(_) => Err(TosError::InvalidDecoding),
                    _ => Err(TosError::UnexpectedMessage),
                }
            }
        }
    }
}

impl ValidatorClient for Client {
    /// Initiate a new transfer to a Tos or Primary account.
    fn handle_transfer_tx(
        &mut self,
        tx: Transaction,
    ) -> AsyncResult<AccountInfoResponse, TosError> {
        Box::pin(async move {
            let shard = ValidatorState::get_shard(self.shards, &tx.transfer.sender);
            self.send_recv_bytes(shard, serialize_transfer_tx(&tx))
                .await
        })
    }

    /// Confirm a transfer to a Tos or Primary account.
    fn handle_confirmation_tx(
        &mut self,
        tx: ConfirmationTx,
    ) -> AsyncResult<AccountInfoResponse, TosError> {
        Box::pin(async move {
            let shard = ValidatorState::get_shard(
                self.shards,
                &tx.ctx.value.transfer.sender,
            );
            self.send_recv_bytes(shard, serialize_cert(&tx.ctx))
                .await
        })
    }

    /// Handle information requests for this account.
    fn handle_account_info_request(
        &mut self,
        request: AccountInfoRequest,
    ) -> AsyncResult<AccountInfoResponse, TosError> {
        Box::pin(async move {
            let shard = ValidatorState::get_shard(self.shards, &request.sender);
            self.send_recv_bytes(shard, serialize_info_request(&request))
                .await
        })
    }
}

#[derive(Clone)]
pub struct MassClient {
    protocol: Protocol,
    address: String,
    port: u32,
    buffer_size: usize,
    send_timeout: std::time::Duration,
    recv_timeout: std::time::Duration,
    max_in_flight: u64,
}

impl MassClient {
    pub fn new(
        protocol: Protocol,
        address: String,
        port: u32,
        buffer_size: usize,
        send_timeout: std::time::Duration,
        recv_timeout: std::time::Duration,
        max_in_flight: u64,
    ) -> Self {
        Self {
            protocol,
            address,
            port,
            buffer_size,
            send_timeout,
            recv_timeout,
            max_in_flight,
        }
    }

    async fn run_shard(&self, shard: u32, requests: Vec<Bytes>) -> Result<Vec<Bytes>, io::Error> {
        let address = format!("{}:{}", self.address, self.port + shard);
        let mut stream = self
            .protocol
            .connect(address, self.buffer_size)
            .await?;
        let mut requests = requests.iter();
        let mut in_flight: u64 = 0;
        let mut responses = Vec::new();

        loop {
            while in_flight < self.max_in_flight {
                let request = match requests.next() {
                    None => {
                        if in_flight == 0 {
                            return Ok(responses);
                        }
                        // No more entries to send.
                        break;
                    }
                    Some(request) => request,
                };
                let status = time::timeout(self.send_timeout, stream.write_data(request)).await;
                if let Err(error) = status {
                    error!("Failed to send request: {}", error);
                    continue;
                }
                in_flight += 1;
            }
            if requests.len() % 5000 == 0 && requests.len() > 0 {
                info!("In flight {} Remaining {}", in_flight, requests.len());
            }
            match time::timeout(self.recv_timeout, stream.read_data()).await {
                Ok(Ok(buffer)) => {
                    in_flight -= 1;
                    responses.push(Bytes::from(buffer));
                }
                Ok(Err(error)) => {
                    if error.kind() == io::ErrorKind::UnexpectedEof {
                        info!("Socket closed by server");
                        return Ok(responses);
                    }
                    error!("Received error response: {}", error);
                }
                Err(error) => {
                    error!(
                        "Timeout while receiving response: {} (in flight: {})",
                        error, in_flight
                    );
                }
            }
        }
    }

    /// Spin off one task for each shard based on this validator client.
    pub fn run<I>(&self, sharded_requests: I) -> impl futures::stream::Stream<Item = Vec<Bytes>>
    where
        I: IntoIterator<Item = (ShardId, Vec<Bytes>)>,
    {
        let handles = futures::stream::FuturesUnordered::new();
        for (shard, requests) in sharded_requests {
            let client = self.clone();
            handles.push(
                tokio::spawn(async move {
                    info!(
                        "Sending {} requests to {}:{} (shard {})",
                        client.protocol,
                        client.address,
                        client.port + shard,
                        shard
                    );
                    let responses = client
                        .run_shard(shard, requests)
                        .await
                        .unwrap_or_else(|_| Vec::new());
                    info!(
                        "Done sending {} requests to {}:{} (shard {})",
                        client.protocol,
                        client.address,
                        client.port + shard,
                        shard
                    );
                    responses
                })
                .then(|x| async { x.unwrap_or_else(|_| Vec::new()) }),
            );
        }
        handles
    }
}
