# TOS Daemon RPC API Reference

**Last Updated:** October 14, 2025
**Version:** Compatible with TIP-2 (GHOSTDAG Implementation)

## Table of Contents

1. [Overview](#overview)
2. [Security Warning](#security-warning)
3. [JSON-RPC Protocol](#json-rpc-protocol)
4. [Network & Version APIs](#network--version-apis)
5. [Block APIs](#block-apis)
6. [Balance & Account APIs](#balance--account-apis)
7. [Transaction APIs](#transaction-apis)
8. [Asset APIs](#asset-apis)
9. [Mining APIs](#mining-apis)
10. [Mempool APIs](#mempool-apis)
11. [P2P & Network APIs](#p2p--network-apis)
12. [Utility APIs](#utility-apis)
13. [Contract APIs](#contract-apis)
14. [Multisig APIs](#multisig-apis)
15. [Energy System APIs](#energy-system-apis)
16. [AI Mining APIs](#ai-mining-apis)
17. [Error Codes](#error-codes)

---

## Overview

This document provides complete reference for all TOS Daemon RPC APIs. The daemon exposes a JSON-RPC 2.0 interface for blockchain interaction.

**Default Endpoint:** `http://127.0.0.1:8080/json_rpc`

**Supported Networks:**
- `mainnet` - Production network
- `testnet` - Testing network
- `devnet` - Development network

---

## Security Warning

🔒 **CRITICAL**: TOS RPC endpoints do NOT have built-in authentication.

### Production Requirements

1. ✅ **REQUIRED**: Firewall with IP whitelist
2. ✅ **REQUIRED**: Reverse proxy with authentication (nginx + basic auth)
3. ✅ **REQUIRED**: TLS/SSL encryption
4. ⚠️ **RECOMMENDED**: VPN for administrative access
5. ⚠️ **RECOMMENDED**: Rate limiting

### Local Development

```bash
# Bind to localhost only
tos_daemon --rpc-bind 127.0.0.1:8080

# Never expose RPC directly to the internet!
```

---

## JSON-RPC Protocol

All requests follow JSON-RPC 2.0 specification.

### Request Format

```json
{
  "jsonrpc": "2.0",
  "method": "method_name",
  "params": [...],
  "id": 1
}
```

### Response Format

**Success:**
```json
{
  "jsonrpc": "2.0",
  "result": {...},
  "id": 1
}
```

**Error:**
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32600,
    "message": "Invalid request"
  },
  "id": 1
}
```

---

## Network & Version APIs

### get_version

Get daemon version information.

**Parameters:** None

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_version",
  "params": [],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": "1.0.0",
  "id": 1
}
```

### get_info

Get comprehensive network information.

**Parameters:** None

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_info",
  "params": [],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "network": "devnet",
    "version": "1.0.0",
    "block_version": 0,

    // GHOSTDAG (TIP-2)
    "blue_score": 45123,
    "topoheight": 45123,
    "stable_blue_score": 45113,
    "pruned_topoheight": 44123,
    "top_block_hash": "0x1234...abcd",

    // BPS Metrics (TIP-2)
    "bps": 1.0,
    "actual_bps": 0.95,
    "block_time_target": 1000,
    "average_block_time": 1053,

    // Supply & Economics
    "circulating_supply": 15000000000000,
    "emitted_supply": 15000000000000,
    "burned_supply": 0,
    "maximum_supply": 18400000000000000,

    // Mining
    "difficulty": 123456,
    "block_reward": 500000000,
    "dev_reward": 50000000,
    "miner_reward": 450000000,

    // Network
    "mempool_size": 5
  },
  "id": 1
}
```

**Key Fields (TIP-2):**
- `blue_score`: DAG depth position (replaces legacy `height`)
- `topoheight`: Sequential storage index (0, 1, 2, 3, ...)
- `stable_blue_score`: Blue score confirmed with high probability (finality)
- `pruned_topoheight`: Oldest topoheight still available (if pruning enabled)

**BPS Metrics (Blocks Per Second System):**
- `bps`: Target blocks per second (configured value, typically 1.0 for TOS)
  - Calculated as: `1000.0 / block_time_target`
  - For OneBps configuration: 1000ms target = 1.0 BPS
  - This is the desired network throughput
- `actual_bps`: Actual blocks per second (measured performance)
  - Calculated as: `1000.0 / average_block_time`
  - Based on average of last 50 blocks
  - Shows real network performance
  - Deviation from `bps` indicates DAA (Difficulty Adjustment Algorithm) is actively adjusting
- `block_time_target`: Target time between blocks in milliseconds (typically 1000ms)
- `average_block_time`: Actual average time between last 50 blocks in milliseconds

**BPS Monitoring:**
- When `actual_bps` ≈ `bps`: Network is operating at target performance
- When `actual_bps` < `bps`: Blocks slower than target (DAA will reduce difficulty)
- When `actual_bps` > `bps`: Blocks faster than target (DAA will increase difficulty)

### get_blue_score

Get current blue score (DAG depth).

**Parameters:** None

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_blue_score",
  "params": [],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 45123,
  "id": 1
}
```

### get_topoheight

Get current topoheight (sequential index).

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 45123,
  "id": 1
}
```

### get_stable_blue_score

Get stable blue score (finalized chain position).

**Parameters:** None

**Aliases:** `get_stableheight`

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 45113,
  "id": 1
}
```

### get_stable_topoheight

Get stable topoheight (finalized storage index).

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 45113,
  "id": 1
}
```

### get_pruned_topoheight

Get pruned topoheight (earliest available block).

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 44123,
  "id": 1
}
```

### get_difficulty

Get current mining difficulty.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 123456,
  "id": 1
}
```

### get_tips

Get current DAG tips (block hashes with no children).

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    "0x1234...abcd",
    "0x5678...ef01"
  ],
  "id": 1
}
```

### get_hard_forks

Get hard fork activation information.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "version": 0,
      "height": 0,
      "block_time_target": 1000,
      "pow_algorithm": "tos/v1"
    }
  ],
  "id": 1
}
```

### get_dev_fee_thresholds

Get developer fee schedule by height.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {"height": 0, "fee_percentage": 10},
    {"height": 1000000, "fee_percentage": 5},
    {"height": 2000000, "fee_percentage": 2}
  ],
  "id": 1
}
```

### get_size_on_disk

Get blockchain database size on disk.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": "1.2 GB",
  "id": 1
}
```

---

## Block APIs

### get_block_at_topoheight

Get block at specific topoheight (TIP-2).

**Parameters:**
- `topoheight` (integer): Sequential block index

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_block_at_topoheight",
  "params": [45123],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "hash": "0x1234...abcd",
    "topoheight": 45123,
    "block_type": "Normal",
    "header": {
      "version": 0,
      "blue_score": 45123,
      "blue_work": "0x1234567890abcdef",
      "daa_score": 45120,
      "parents_by_level": [
        ["0xabcd...1234", "0xef01...5678"]
      ],
      "timestamp": 1697654400,
      "nonce": 123456,
      "extra_nonce": "0x00...00",
      "bits": 520159231,
      "miner": "tos1abc...xyz",
      "hash_merkle_root": "0x9876...4321",
      "pruning_point": "0x0000...0000"
    },
    "transactions": [...],
    "total_size_in_bytes": 1024,
    "difficulty": 123456,
    "reward": 500000000,
    "dev_reward": 50000000,
    "miner_reward": 450000000,
    "total_fees": 1000000
  },
  "id": 1
}
```

**Key Fields:**
- `parents_by_level`: DAG parent structure (TIP-2)
  - `[0]` = direct parents
  - `[1]` = grandparents not in level 0
- `blue_score`: DAG depth position
- `blue_work`: Cumulative work (U256)

### get_blocks_at_blue_score

Get all blocks at a specific blue score (multiple blocks in DAG).

**Parameters:**
- `blue_score` (integer): DAG depth

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_blocks_at_blue_score",
  "params": [45123],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "hash": "0x1234...abcd",
      "topoheight": 45123,
      ...
    },
    {
      "hash": "0x5678...ef01",
      "topoheight": 45124,
      ...
    }
  ],
  "id": 1
}
```

### get_block_by_hash

Get block by hash.

**Parameters:**
- `hash` (string): Block hash

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_block_by_hash",
  "params": ["0x1234...abcd"],
  "id": 1
}
```

### get_top_block

Get highest block in DAG.

**Parameters:** None

**Response:** Same as `get_block_by_hash`

### get_blocks_range_by_topoheight

Get range of blocks by topoheight.

**Parameters:**
- `start_topoheight` (integer)
- `end_topoheight` (integer)

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_blocks_range_by_topoheight",
  "params": [45100, 45110],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {...}, // block at 45100
    {...}, // block at 45101
    ...
  ],
  "id": 1
}
```

### get_blocks_range_by_blue_score

Get range of blocks by blue score.

**Parameters:**
- `start_blue_score` (integer)
- `end_blue_score` (integer)

### get_dag_order

Get DAG topological order information.

**Parameters:**
- `hash` (string): Block hash

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "topoheight": 45123,
    "blue_score": 45123
  },
  "id": 1
}
```

---

## Balance & Account APIs

### get_balance

Get account balance (at latest topoheight).

**Parameters:**
- `address` (string): Account address
- `asset` (string, optional): Asset hash (default: TOS)

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_balance",
  "params": ["tos1abc...xyz"],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "balance": 1000000000,
    "topoheight": 45123
  },
  "id": 1
}
```

### get_balance_at_topoheight

Get balance at specific topoheight (TIP-2).

**Parameters:**
- `address` (string)
- `topoheight` (integer)
- `asset` (string, optional)

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_balance_at_topoheight",
  "params": ["tos1abc...xyz", 45000],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "balance": 950000000,
    "topoheight": 45000
  },
  "id": 1
}
```

### get_stable_balance

Get balance at stable topoheight (finalized).

**Parameters:**
- `address` (string)
- `asset` (string, optional)

### has_balance

Check if account has any balance.

**Parameters:**
- `address` (string)
- `asset` (string, optional)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 1
}
```

### get_nonce

Get account nonce (transaction counter).

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 42,
  "id": 1
}
```

### get_nonce_at_topoheight

Get nonce at specific topoheight.

**Parameters:**
- `address` (string)
- `topoheight` (integer)

### has_nonce

Check if account has nonce.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 1
}
```

### get_account_history

Get transaction history for account.

**Parameters:**
- `address` (string)
- `minimum_topoheight` (integer, optional)
- `maximum_topoheight` (integer, optional)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "hash": "0x1234...abcd",
      "topoheight": 45120,
      "block_timestamp": 1697654400
    },
    ...
  ],
  "id": 1
}
```

### get_account_assets

Get all assets held by account.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "asset": "0x0000...0000",
      "balance": 1000000000
    },
    {
      "asset": "0xabcd...1234",
      "balance": 500
    }
  ],
  "id": 1
}
```

### get_accounts

Get list of accounts with balance.

**Parameters:**
- `skip` (integer, optional)
- `maximum` (integer, optional)
- `minimum_balance` (integer, optional)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    "tos1abc...xyz",
    "tos1def...uvw",
    ...
  ],
  "id": 1
}
```

### is_account_registered

Check if account is registered on chain.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 1
}
```

### get_account_registration_topoheight

Get topoheight when account was first registered.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 1234,
  "id": 1
}
```

### count_accounts

Get total number of registered accounts.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 10523,
  "id": 1
}
```

---

## Transaction APIs

### submit_transaction

Submit signed transaction to network.

**Parameters:**
- `data` (string): Hex-encoded transaction data

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "submit_transaction",
  "params": ["0x0123456789abcdef..."],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": "0x1234...abcd",
  "id": 1
}
```

### get_transaction

Get transaction by hash.

**Parameters:**
- `hash` (string): Transaction hash

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_transaction",
  "params": ["0x1234...abcd"],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "hash": "0x1234...abcd",
    "source": "tos1abc...xyz",
    "data": {
      "Transfer": [
        {
          "amount": 1000000000,
          "asset": "0x0000...0000",
          "to": "tos1def...uvw"
        }
      ]
    },
    "fee": 1000,
    "nonce": 42,
    "signature": "0x...",
    "version": 0,
    "in_mempool": false,
    "executed_in_block": "0xabcd...1234",
    "first_seen": 1697654400
  },
  "id": 1
}
```

### get_transactions

Get multiple transactions.

**Parameters:**
- `tx_hashes` (array): Array of transaction hashes

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {...},
    {...}
  ],
  "id": 1
}
```

### get_transactions_summary

Get transaction summary (without full data).

**Parameters:**
- `tx_hashes` (array)

### get_transaction_executor

Get transaction execution details.

**Parameters:**
- `hash` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "hash": "0xabcd...1234",
    "topoheight": 45120
  },
  "id": 1
}
```

### is_tx_executed_in_block

Check if transaction was executed in specific block.

**Parameters:**
- `tx_hash` (string)
- `block_hash` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 1
}
```

### count_transactions

Get total number of transactions.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 523456,
  "id": 1
}
```

---

## Asset APIs

### get_asset

Get asset information.

**Parameters:**
- `asset` (string): Asset hash

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_asset",
  "params": ["0xabcd...1234"],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "asset": "0xabcd...1234",
    "decimals": 8,
    "ticker": "MYTOKEN"
  },
  "id": 1
}
```

### get_asset_supply

Get total supply of asset.

**Parameters:**
- `asset` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 1000000000000,
  "id": 1
}
```

### get_assets

Get list of registered assets.

**Parameters:**
- `skip` (integer, optional)
- `maximum` (integer, optional)
- `minimum_topoheight` (integer, optional)
- `maximum_topoheight` (integer, optional)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "asset": "0xabcd...1234",
      "decimals": 8,
      "ticker": "MYTOKEN",
      "topoheight": 1234
    },
    ...
  ],
  "id": 1
}
```

### count_assets

Get total number of assets.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": 42,
  "id": 1
}
```

---

## Mining APIs

⚠️ These APIs require mining to be enabled (`--enable-mining` flag).

### get_block_template

Get block template for mining.

**Parameters:**
- `address` (string): Miner address

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "get_block_template",
  "params": ["tos1abc...xyz"],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "template": "0x...",
    "algorithm": "tos/v1",
    "difficulty": 123456,
    "height": 45124,
    "topoheight": 45124
  },
  "id": 1
}
```

### get_miner_work

Get mining work (for miners).

**Parameters:**
- `address` (string): Miner address

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "miner_work": "0x...",
    "algorithm": "tos/v1",
    "difficulty": 123456,
    "height": 45124,
    "topoheight": 45124
  },
  "id": 1
}
```

### submit_block

Submit mined block.

**Parameters:**
- `block_data` (string): Hex-encoded block data

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "submit_block",
  "params": ["0x0123456789abcdef..."],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "accepted": true,
    "block_hash": "0x1234...abcd"
  },
  "id": 1
}
```

---

## Mempool APIs

### get_mempool

Get all transactions in mempool.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "hash": "0x1234...abcd",
      "fee": 1000,
      "size": 256
    },
    ...
  ],
  "id": 1
}
```

### get_mempool_summary

Get mempool summary statistics.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "count": 5,
    "total_size_bytes": 1280,
    "total_fees": 5000
  },
  "id": 1
}
```

### get_mempool_cache

Get cached mempool data.

**Parameters:** None

### get_estimated_fee_rates

Get estimated fee rates for transaction priority.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "low": 100,
    "medium": 500,
    "high": 1000
  },
  "id": 1
}
```

---

## P2P & Network APIs

### p2p_status

Get P2P network status.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "peer_count": 8,
    "max_peers": 32,
    "our_id": "0x1234...abcd"
  },
  "id": 1
}
```

### get_peers

Get list of connected peers.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "addr": "192.168.1.100:2125",
      "peer_id": 1,
      "tag": "Default",
      "blue_score": 45123,
      "topoheight": 45123,
      "last_ping": 50,
      "is_priority": false,
      "cumulative_difficulty": 0,
      "connected_on": 1697654000,
      "bytes_recv": 1024000,
      "bytes_sent": 512000,
      "version": "1.0.0",
      "pruned_topoheight": 44123
    },
    ...
  ],
  "id": 1
}
```

### get_p2p_block_propagation

Get block propagation statistics.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "average_propagation_time_ms": 150,
    "blocks_received": 1000,
    "blocks_sent": 500
  },
  "id": 1
}
```

---

## Utility APIs

### validate_address

Validate address format.

**Parameters:**
- `address` (string)

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "validate_address",
  "params": ["tos1abc...xyz"],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "is_valid": true,
    "is_integrated": false
  },
  "id": 1
}
```

### split_address

Split integrated address into base address and data.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "address": "tos1abc...xyz",
    "integrated_data": "0x..."
  },
  "id": 1
}
```

### extract_key_from_address

Extract public key from address.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": "0x1234...abcd",
  "id": 1
}
```

### make_integrated_address

Create integrated address.

**Parameters:**
- `address` (string)
- `data` (string): Hex data to integrate

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "make_integrated_address",
  "params": ["tos1abc...xyz", "0x1234"],
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": "tosi1abc...xyz",
  "id": 1
}
```

### decrypt_extra_data

Decrypt extra data from transaction.

**Parameters:**
- `ciphertext` (string)
- `address` (string)

---

## Contract APIs

### get_contract_outputs

Get contract outputs.

**Parameters:**
- `hash` (string): Contract hash

### get_contract_module

Get contract module code.

**Parameters:**
- `hash` (string)

### get_contract_data

Get contract data.

**Parameters:**
- `hash` (string)
- `key` (string, optional)

### get_contract_data_at_topoheight

Get contract data at specific topoheight.

**Parameters:**
- `hash` (string)
- `topoheight` (integer)
- `key` (string, optional)

### get_contract_balance

Get contract balance.

**Parameters:**
- `hash` (string)
- `asset` (string, optional)

### get_contract_balance_at_topoheight

Get contract balance at specific topoheight.

**Parameters:**
- `hash` (string)
- `topoheight` (integer)
- `asset` (string, optional)

### get_contract_assets

Get all assets held by contract.

**Parameters:**
- `hash` (string)

### count_contracts

Get total number of contracts.

**Parameters:** None

---

## Multisig APIs

### get_multisig

Get multisig configuration for address.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "threshold": 2,
    "participants": [
      "tos1abc...xyz",
      "tos1def...uvw",
      "tos1ghi...rst"
    ]
  },
  "id": 1
}
```

### get_multisig_at_topoheight

Get multisig configuration at specific topoheight.

**Parameters:**
- `address` (string)
- `topoheight` (integer)

### has_multisig

Check if address has multisig configuration.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 1
}
```

### has_multisig_at_topoheight

Check multisig at specific topoheight.

**Parameters:**
- `address` (string)
- `topoheight` (integer)

---

## Energy System APIs

### get_energy

Get energy information for address.

**Parameters:**
- `address` (string)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "frozen_balance": 1000000000,
    "energy_available": 50000,
    "energy_used": 1000,
    "unlock_timestamp": 1697740800
  },
  "id": 1
}
```

---

## AI Mining APIs

### get_ai_mining_state

Get AI mining state for address.

**Parameters:**
- `address` (string)

### get_ai_mining_state_at_topoheight

Get AI mining state at specific topoheight.

**Parameters:**
- `address` (string)
- `topoheight` (integer)

### has_ai_mining_state_at_topoheight

Check if address has AI mining state at topoheight.

**Parameters:**
- `address` (string)
- `topoheight` (integer)

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 1
}
```

### get_ai_mining_statistics

Get AI mining network statistics.

**Parameters:** None

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "active_tasks": 50,
    "total_miners": 200,
    "total_rewards_distributed": 100000000000
  },
  "id": 1
}
```

### get_ai_mining_task

Get AI mining task details.

**Parameters:**
- `task_id` (string)

### get_ai_mining_miner

Get AI miner information.

**Parameters:**
- `address` (string)

### get_ai_mining_active_tasks

Get list of active AI mining tasks.

**Parameters:**
- `skip` (integer, optional)
- `maximum` (integer, optional)

---

## Error Codes

| Code | Message | Description |
|------|---------|-------------|
| -32700 | Parse error | Invalid JSON |
| -32600 | Invalid Request | Invalid JSON-RPC |
| -32601 | Method not found | Method doesn't exist |
| -32602 | Invalid params | Invalid method parameters |
| -32603 | Internal error | Internal server error |
| -32000 | Server error | Custom server error |

---

## Version History

### TIP-2 (GHOSTDAG Implementation)

**New APIs:**
- `get_blocks_at_blue_score`
- `get_balance_at_topoheight`
- `get_nonce_at_topoheight`
- `get_stable_blue_score` / `get_stable_topoheight`

**Modified APIs:**
- `get_info`: Added `bps`, `actual_bps`, `blue_score`, `topoheight`
- `get_block_*`: Added `parents_by_level`, `blue_work`, `blue_score`
- All `*_at_height` APIs renamed to `*_at_topoheight`

**Deprecated APIs:**
- None (backward compatibility maintained)

---

## Support

For issues or questions:
1. Check this reference
2. Review [TIP-2 Specification](../TIPs/TIP-2.md)
3. Open GitHub issue

---

**End of Document**
