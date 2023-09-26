// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use crate::transport::Protocol;
use cores::{
    base_types::*,
    client::ClientState,
    messages::CertifiedTransaction,
};

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorConfig {
    pub protocol: Protocol,
    #[serde(
        serialize_with = "address_as_base58",
        deserialize_with = "address_from_base58"
    )]
    pub address: Address,
    pub host: String,
    pub port: u32,
    pub shards: u32,
}

impl ValidatorConfig {
    pub fn print(&self) {
        let data = serde_json::to_string(self).unwrap();
        println!("{}", data);
    }
}

#[derive(Serialize, Deserialize)]
pub struct ValidatorServerConfig {
    pub validator: ValidatorConfig,
    pub key: KeyPair,
}

impl ValidatorServerConfig {
    pub fn read(path: &str) -> Result<Self, std::io::Error> {
        let data = fs::read(path)?;
        Ok(serde_json::from_slice(data.as_slice())?)
    }

    pub fn write(&self, path: &str) -> Result<(), std::io::Error> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;
        let mut writer = BufWriter::new(file);
        let data = serde_json::to_string_pretty(self).unwrap();
        writer.write_all(data.as_ref())?;
        writer.write_all(b"\n")?;
        Ok(())
    }
}

pub struct ValidatorsConfig {
    pub validators: Vec<ValidatorConfig>,
}

impl ValidatorsConfig {
    pub fn read(path: &str) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let stream = serde_json::Deserializer::from_reader(reader).into_iter();
        Ok(Self {
            validators: stream.filter_map(Result::ok).collect(),
        })
    }

    pub fn write(&self, path: &str) -> Result<(), std::io::Error> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;
        let mut writer = BufWriter::new(file);
        for config in &self.validators {
            serde_json::to_writer(&mut writer, config)?;
            writer.write_all(b"\n")?;
        }
        Ok(())
    }

    pub fn voting_rights(&self) -> BTreeMap<ValidatorName, usize> {
        let mut map = BTreeMap::new();
        for validator in &self.validators {
            map.insert(validator.address, 1);
        }
        map
    }
}

#[derive(Serialize, Deserialize)]
pub struct UserAccount {
    #[serde(
        serialize_with = "address_as_base58",
        deserialize_with = "address_from_base58"
    )]
    pub address: Address,
    pub key: KeyPair,
    pub nonce: Nonce,
    pub balance: Balance,
    pub sent: Vec<CertifiedTransaction>,
    pub received: Vec<CertifiedTransaction>,
}

impl UserAccount {
    pub fn new(balance: Balance) -> Self {
        let (address, key) = get_key_pair();
        Self {
            address,
            key,
            nonce: Nonce::new(),
            balance,
            sent: Vec::new(),
            received: Vec::new(),
        }
    }
}

pub struct AccountsConfig {
    accounts: BTreeMap<Address, UserAccount>,
}

impl AccountsConfig {
    pub fn get(&self, address: &Address) -> Option<&UserAccount> {
        self.accounts.get(address)
    }

    pub fn insert(&mut self, account: UserAccount) {
        self.accounts.insert(account.address, account);
        //self.write_account(&account);
    }

    pub fn num_accounts(&self) -> usize {
        self.accounts.len()
    }

    pub fn accounts_mut(&mut self) -> impl Iterator<Item = &mut UserAccount> {
        self.accounts.values_mut()
    }

    pub fn update_from_state<A>(&mut self, state: &ClientState<A>) {
        let account = self
            .accounts
            .get_mut(&state.address())
            .expect("Updated account should already exist");
        account.nonce = state.nonce();
        account.balance = state.balance();
        account.sent = state.sent().clone();
        account.received = state.received().cloned().collect();
        //self.write_account(&account);
    }

    pub fn update_for_received_transfer(&mut self, certificate: CertifiedTransaction) {
        let transfer = &certificate.value.transfer;
        let recipient = &transfer.recipient;
        if let Some(config) = self.accounts.get_mut(recipient) {
            if let Err(position) = config
                .received
                .binary_search_by_key(&certificate.key(), CertifiedTransaction::key)
            {
                config.balance = config.balance.try_add(transfer.amount.into()).unwrap();
                config.received.insert(position, certificate);
                //self.write_account(&config);
            }
        }
    }

    pub fn read_or_create(path: &str) -> Result<Self, std::io::Error> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(path)?;
        let reader = BufReader::new(file);
        let stream = serde_json::Deserializer::from_reader(reader).into_iter();
        Ok(Self {
            accounts: stream
                .filter_map(Result::ok)
                .map(|account: UserAccount| (account.address, account))
                .collect(),
        })
    }

    pub fn write(&self, path: &str) -> Result<(), std::io::Error> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;
        let mut writer = BufWriter::new(file);
        for account in self.accounts.values() {
            serde_json::to_writer(&mut writer, account)?;
            writer.write_all(b"\n")?;
        }
        Ok(())
    }
}

pub struct InitialStateConfig {
    pub accounts: Vec<(Address, Balance)>,
}

impl InitialStateConfig {
    pub fn read(path: &str) -> Result<Self, failure::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut accounts = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let elements = line.split(':').collect::<Vec<_>>();
            if elements.len() != 2 {
                failure::bail!("expecting two columns separated with ':'")
            }
            let address = decode_address(elements[0])?;
            let balance = elements[1].parse()?;
            accounts.push((address, balance));
        }
        Ok(Self { accounts })
    }

    pub fn write(&self, path: &str) -> Result<(), std::io::Error> {
        let file = OpenOptions::new().create(true).write(true).open(path)?;
        let mut writer = BufWriter::new(file);
        for (address, balance) in &self.accounts {
            writeln!(writer, "{}:{}", encode_address(address), balance)?;
        }
        Ok(())
    }
}
