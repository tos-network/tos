# TOS Daemon Synchronization Options

## Overview

The TOS daemon provides several synchronization and storage optimization options to improve performance and reduce resource usage.

## Sync Options

### Boost Sync (`--allow-boost-sync`)

**Recommended: ✅ Enable**

Boost sync enables parallel blockchain synchronization, significantly reducing the time required for initial blockchain sync and catching up with the network.

**Benefits:**
- Faster initial blockchain synchronization
- Improved parallel processing of blocks
- Reduced sync time from hours to minutes
- Better network utilization

**Usage:**
```bash
./tos_daemon --allow-boost-sync
```

### Auto Pruning (`--auto-prune-keep-n-blocks`)

**Recommended: ✅ Enable with 2000 blocks**

Auto pruning automatically removes old blockchain data while keeping the most recent N blocks, significantly reducing storage requirements.

**Benefits:**
- Dramatically reduced storage footprint (from GBs to MBs)
- Maintains essential blockchain data for validation
- Automatic cleanup without manual intervention
- Faster startup times

**Configuration:**
```bash
./tos_daemon --auto-prune-keep-n-blocks 2000
```

**Storage Impact:**
- Without pruning: Full blockchain history (~GB scale)
- With pruning (2000 blocks): ~50-100 MB storage
- Keeps last 2000 blocks for validation and query purposes

## Recommended Configuration

### Mainnet Configuration
For mainnet TOS daemon deployments:

```bash
./tos_daemon \
    --rpc-bind-address 0.0.0.0:8080 \
    --p2p-bind-address 0.0.0.0:2125 \
    --allow-boost-sync \
    --auto-prune-keep-n-blocks 2000
```

### Testnet Configuration
For testnet TOS daemon deployments:

```bash
./tos_daemon \
    --network testnet \
    --rpc-bind-address 0.0.0.0:8080 \
    --p2p-bind-address 0.0.0.0:2125 \
    --allow-boost-sync \
    --auto-prune-keep-n-blocks 2000
```

## Configuration Methods

### 1. Command Line
```bash
./tos_daemon --allow-boost-sync --auto-prune-keep-n-blocks 2000
```

### 2. Systemd Service

**Mainnet Service** `/etc/systemd/system/tos-daemon.service`:
```ini
[Unit]
Description=TOS Mainnet Daemon
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/root/tos
ExecStart=/root/tos/target/release/tos_daemon \
    --rpc-bind-address 0.0.0.0:8080 \
    --p2p-bind-address 0.0.0.0:2125 \
    --allow-boost-sync \
    --auto-prune-keep-n-blocks 2000
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

**Testnet Service** `/etc/systemd/system/tos-daemon.service` (on testnet server):
```ini
[Unit]
Description=TOS Testnet Daemon
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/root/tos
ExecStart=/root/tos/target/release/tos_daemon \
    --network testnet \
    --rpc-bind-address 0.0.0.0:8080 \
    --p2p-bind-address 0.0.0.0:2125 \
    --allow-boost-sync \
    --auto-prune-keep-n-blocks 2000
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

### 3. Using Update Scripts

**For Mainnet:**
```bash
./daemon/update_systemd_with_sync_options.sh
```

**For Testnet:**
```bash
./daemon/update_systemd_with_sync_options.sh
```

**Setup Scripts:**
```bash
# Complete daemon setup
./daemon/setup_daemon.sh
```

## Performance Comparison

| Configuration | Sync Time | Storage Usage | Memory Usage |
|---------------|-----------|---------------|--------------|
| Default | 2-4 hours | 5-10 GB | 1-2 GB |
| Boost Sync Only | 30-60 min | 5-10 GB | 1-2 GB |
| Boost + Pruning | 30-60 min | 50-100 MB | 500 MB - 1 GB |

## Important Notes

- **First Run**: Initial sync with boost sync may take 30-60 minutes depending on network conditions
- **Block Retention**: 2000 blocks provides sufficient data for most operations while keeping storage minimal
- **Network Requirements**: Boost sync may use more bandwidth during initial synchronization
- **Compatibility**: These options are compatible with all TOS network modes (Mainnet, Testnet, Devnet)

## Network-Specific Configuration

### Testnet Features
- **Seed Node**: 157.7.65.157:2125 (primary testnet bootstrap node)
- **Address Prefix**: `tst` (vs mainnet `tos`)
- **Lower Difficulty**: 100 H/s minimum (vs mainnet 20 KH/s)
- **Genesis Block**: Independent testnet genesis with correct dev address
- **Purpose**: Stable testing environment with reduced mining requirements

### Network Comparison
| Feature | Mainnet | Testnet | Devnet |
|---------|---------|---------|---------|
| Address Prefix | `tos` | `tst` | `tos` |
| Min Hashrate | 20 KH/s | 100 H/s | 100 H/s |
| Seed Nodes | 7 nodes | 1 node | None |
| Genesis | Fixed | Fixed | Generated |
| Purpose | Production | Testing | Development |

## Troubleshooting

### Sync Issues
If sync appears slow or stuck:
1. Check network connectivity
2. Verify firewall allows P2P port (2125)
3. Monitor logs: `journalctl -u tos-daemon -f`

### Storage Issues
If storage usage is higher than expected:
1. Verify pruning is enabled in configuration
2. Restart daemon to trigger pruning
3. Check logs for pruning activity

### Memory Issues
If daemon uses excessive memory:
1. Consider reducing `--auto-prune-keep-n-blocks` value
2. Monitor system resources during sync
3. Ensure adequate swap space on low-memory systems