# TOS DoS Protection Guide

**Version**: 1.0
**Date**: 2025-11-30
**Status**: Security Documentation

---

## 1. Overview

This document describes the DoS (Denial of Service) protection mechanisms in TOS and provides guidance for operators on securing their nodes.

---

## 2. Network Layer Protection

### 2.1 Connection Limits

| Parameter | Default | Config Flag | Description |
|-----------|---------|-------------|-------------|
| `max_peers` | 32 | `--max-peers` | Maximum total peer connections |
| `max_outgoing_peers` | 8 | `--p2p-max-outgoing-peers` | Maximum outgoing connections |
| `concurrency_task_count_limit` | 16 | `--p2p-concurrency-task-count-limit` | Concurrent connection tasks |

### 2.2 Peer Banning

| Parameter | Default | Config Flag | Description |
|-----------|---------|-------------|-------------|
| `fail_count_limit` | 5 | `--p2p-fail-count-limit` | Failures before temp ban |
| `temp_ban_duration` | 15m | `--p2p-temp-ban-duration` | Temporary ban duration |

**Automatic banning triggers**:
- Invalid block submissions
- Protocol violations
- Excessive request failures

### 2.3 Chain Sync Protection

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_chain_response_size` | 128 | Max blocks per sync response |

**Protection mechanisms**:
- Response size validation
- Block hash verification before processing
- Sequential validation (default) or parallel with verification

---

## 3. Consensus Layer Protection

### 3.1 Block Validation Limits

| Parameter | Value | Description |
|-----------|-------|-------------|
| `MAX_BLOCK_SIZE` | 1 MB | Maximum block size |
| `TIPS_LIMIT` | 32 | Maximum parent tips per block |
| `MAX_TRANSACTIONS_PER_BLOCK` | 10,000 | Transaction limit |

### 3.2 DAG Resource Limits

| Operation | Protection |
|-----------|------------|
| GHOSTDAG computation | Limited by TIPS_LIMIT |
| Reachability queries | Interval tree O(1) lookups |
| Mergeset calculation | Bounded by K parameter |

### 3.3 Timestamp Validation

Blocks with invalid timestamps are rejected:
- Must be greater than all parent timestamps
- Must be greater than median of parent timestamps
- Future timestamp tolerance: configurable

---

## 4. Execution Layer Protection

### 4.1 Parallel Execution Limits

| Parameter | Default | Description |
|-----------|---------|-------------|
| Semaphore limit | CPU cores | Max concurrent execution tasks |
| Task timeout | Configurable | Per-task execution timeout |

### 4.2 Transaction Validation

| Protection | Description |
|------------|-------------|
| Nonce validation | Prevents replay attacks |
| Balance checks | Prevents overdraft |
| Gas limits | Prevents infinite loops |

---

## 5. RPC Layer Protection

### 5.1 WebSocket Limits

| Parameter | Default | Config Flag |
|-----------|---------|-------------|
| `ws_max_message_size` | 1 MB | `--rpc-ws-max-message-size` |
| `ws_max_subscriptions` | 100 | `--rpc-ws-max-subscriptions` |
| `ws_max_connections_per_minute` | 100 | `--rpc-ws-max-connections-per-minute` |
| `ws_max_messages_per_second` | 10 | `--rpc-ws-max-messages-per-second` |

### 5.2 GetWork Rate Limiting

| Parameter | Default | Config Flag |
|-----------|---------|-------------|
| `rate_limit_ms` | 500 | `--getwork-rate-limit-ms` |

---

## 6. Storage Protection

### 6.1 Database Limits

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_open_files` | 1024 | RocksDB file handles |
| `cache_size` | 64 MB | Block cache size |

### 6.2 Pruning

| Parameter | Default | Description |
|-----------|---------|-------------|
| `PRUNE_SAFETY_LIMIT` | 80 | Minimum blocks to keep |

Enable auto-pruning to prevent disk exhaustion:
```bash
--auto-prune-keep-n-blocks 1000
```

---

## 7. Recommended Production Settings

### 7.1 High-Security Node

```bash
tos_daemon \
  --network mainnet \
  --max-peers 16 \
  --p2p-max-outgoing-peers 4 \
  --p2p-fail-count-limit 3 \
  --p2p-temp-ban-duration 30m \
  --rpc-ws-max-connections-per-minute 50 \
  --rpc-ws-max-messages-per-second 5
```

### 7.2 Public RPC Node

```bash
tos_daemon \
  --network mainnet \
  --max-peers 64 \
  --rpc-ws-max-connections-per-minute 200 \
  --rpc-ws-max-subscriptions 50 \
  --rpc-ws-allowed-origins "https://yourdomain.com"
```

---

## 8. Monitoring Recommendations

### 8.1 Prometheus Metrics

Enable metrics endpoint:
```bash
--prometheus-enable --prometheus-route /metrics
```

Key metrics to monitor:
- `tos_peer_count` - Connected peers
- `tos_block_height` - Chain height
- `tos_mempool_size` - Pending transactions
- `tos_orphan_rate` - Block orphan percentage

### 8.2 Alert Thresholds

| Metric | Warning | Critical |
|--------|---------|----------|
| Peer count | < 4 | < 2 |
| Block time deviation | > 2x target | > 5x target |
| Orphan rate | > 10% | > 25% |
| Memory usage | > 80% | > 95% |

---

## 9. Known Attack Vectors

### 9.1 Block Flooding

**Attack**: Send many invalid blocks to consume CPU/bandwidth
**Mitigation**:
- Peer banning after failures
- PoW verification before full validation
- Rate limiting

### 9.2 Large Block Attack

**Attack**: Submit maximum-size blocks with complex transactions
**Mitigation**:
- MAX_BLOCK_SIZE limit
- Transaction count limits
- Gas limits per block

### 9.3 DAG Complexity Attack

**Attack**: Create complex DAG structure with many tips
**Mitigation**:
- TIPS_LIMIT bounds parent count
- GHOSTDAG_K limits anticone processing
- Tip difficulty validation (91% rule)

### 9.4 Timestamp Manipulation

**Attack**: Manipulate timestamps to affect difficulty
**Mitigation**:
- IQR-based DAA (uses 25%-75% percentile)
- Strict timestamp ordering rules
- Bounded adjustment ratios

---

## 10. Emergency Procedures

### 10.1 Under Active Attack

1. **Reduce peer count**:
   ```bash
   # Restart with reduced connections
   --max-peers 8 --p2p-max-outgoing-peers 2
   ```

2. **Enable strict banning**:
   ```bash
   --p2p-fail-count-limit 2 --p2p-temp-ban-duration 1h
   ```

3. **Use exclusive nodes only**:
   ```bash
   --exclusive-nodes "trusted-node-1:8080,trusted-node-2:8080"
   ```

### 10.2 Recovery

1. Stop daemon
2. Check logs for attack patterns
3. Add attacker IPs to firewall blocklist
4. Restart with adjusted parameters
5. Monitor metrics closely

---

## References

- [TOS Consensus Specification](./CONSENSUS_SPECIFICATION.md)
- [Security Audit Report](../memo/03-Security-Audits/)
- [GHOSTDAG Paper](https://eprint.iacr.org/2018/104.pdf)

---

## Changelog

- **v1.0 (2025-11-30)**: Initial documentation based on security audit F-03
