# Bootstrap Node Deployment Guide

This guide explains how to deploy and configure bootstrap nodes for the TOS network.

## Overview

Bootstrap nodes serve as initial connection points for new TOS network participants. They help with peer discovery and network connectivity.

## Prerequisites

- TOS daemon compiled with bootstrap node support
- Server with public IP address
- Open network ports (default: 2125)
- Basic understanding of TOS network configuration

## Deployment Options

### Option 1: Deploy Your Own Bootstrap Node

#### 1. Server Setup

```bash
# Minimum requirements
# - 2 CPU cores
# - 4GB RAM
# - 50GB storage
# - 10Mbps network connection
# - Public static IP address

# Update system
sudo apt update && sudo apt upgrade -y

# Install dependencies
sudo apt install -y curl wget git build-essential
```

#### 2. Build TOS Daemon

```bash
# Clone the repository
git clone https://github.com/tos-network/tos.git
cd tos

# Build the daemon
cargo build --release --bin tos_daemon

# Copy binary to system path
sudo cp target/release/tos_daemon /usr/local/bin/
```

#### 3. Configure Bootstrap Node

Create configuration file `/etc/tos/daemon.toml`:

```toml
[p2p]
# P2P listening address - bind to all interfaces for public access
bind_address = "0.0.0.0:2125"

# Maximum number of peer connections allowed
max_peers = 100

# Maximum number of outgoing peer connections
max_outgoing_peers = 50

# Enable P2P server (false disables all P2P communication)
disable = false

# Priority nodes to connect when P2P starts (connected only once)
priority_nodes = []

# Exclusive nodes with maintained connections (replaces seed nodes)
exclusive_nodes = []

# Additional bootstrap nodes for initial network discovery
bootstrap_nodes = [
    "51.210.117.23:2125",        # France seed
    "198.71.55.87:2125",         # US seed
    "162.19.249.100:2125"        # Germany seed
]

# Allow fast sync mode (sync bootstrapped chain without verifying history)
# Use with extreme caution and trusted nodes only
allow_fast_sync = false

# Allow parallel block requests instead of sequential sync
allow_boost_sync = false

# Forward blocks from priority nodes before self-verification
allow_priority_blocks = false

# Configure maximum chain response size for sync operations
max_chain_response_size = 4096

# Prevent IP address sharing with other peers and through API
disable_ip_sharing = false

# Limit concurrent tasks for accepting incoming connections
concurrency_task_count_limit = 4

# Duration to temporarily ban peers after reaching fail limit
temp_ban_duration = "15m"

# Number of failures before temporarily banning a peer
fail_count_limit = 50

# Disable re-execution of orphaned blocks during chain sync
disable_reexecute_blocks_on_sync = false

# Log level for block propagation (trace, debug, info, warn, error)
block_propagation_log_level = "debug"

# Disable requesting propagated transactions from peers
disable_fetching_txs_propagated = false

# Handle peer packets in dedicated task for better performance
handle_peer_packets_in_dedicated_task = false

# P2P stream processing concurrency
stream_concurrency = 8

# Optional proxy configuration
# proxy_address = "socks5://127.0.0.1:9050"
# proxy_username = "user"
# proxy_password = "pass"

# Optional node identifier tag
# tag = "bootstrap-node-1"

[rpc]
# Disable RPC server (also disables GetWork server)
disable = false

# RPC server listening address
bind_address = "127.0.0.1:8080"

# Number of worker threads for HTTP server
threads = 8

# Concurrency for RPC event notifications
notify_events_concurrency = 8

[rpc.getwork]
# Disable GetWork server for miners
disable = false

# Rate limit for GetWork jobs in milliseconds (0 = no limit)
rate_limit_ms = 500

# Concurrency for job notifications to miners
notify_job_concurrency = 8

[rpc.prometheus]
# Enable Prometheus metrics endpoint
enable = false

# URL path for metrics export
route = "/metrics"

# Storage configuration
[storage]
# Database backend to use (rocksdb or sled)
use_db_backend = "rocksdb"

# Directory path for blockchain data storage
# dir_path = "/var/lib/tos/"

# Enable automatic pruning keeping N blocks before top
# auto_prune_keep_n_blocks = 1000

# Enable database integrity check on startup
check_db_integrity = false

# Enable recovery mode (skips integrity checks and pre-computations)
recovery_mode = false

# Flush storage to disk every N blocks
# flush_db_every_n_blocks = 100

# Disable ZKP (Zero-Knowledge Proof) cache
disable_zkp_cache = false

[storage.rocksdb]
# Number of background threads for RocksDB operations
parallelism = 8

# Maximum concurrent background jobs (compactions and flushes)
max_background_jobs = 8

# Maximum subcompaction jobs running simultaneously
max_subcompaction_jobs = 8

# Low priority background thread pool size
low_priority_background_threads = 8

# Maximum number of open files (-1 = unlimited)
max_open_files = 1024

# Maximum number of log files to keep
keep_max_log_files = 4

# Compression mode (none, snappy, lz4, zstd)
compression_mode = "lz4"

# Block cache mode (none, lru, clock)
cache_mode = "lru"

# Cache size in bytes
cache_size = 67108864  # 64 MB

# Write buffer size for memtables
write_buffer_size = 67108864  # 64 MB

# Share write buffer across column families
write_buffer_shared = false

[storage.sled]
# LRU cache size (0 = disabled)
cache_size = 1024

# Internal database cache size in bytes
internal_cache_size = 67108864  # 64 MB

# Internal database mode (fast, small, low_space)
internal_db_mode = "low_space"

# Performance and security settings
[daemon]
# Skip PoW verification (WARNING: dangerous for production)
skip_pow_verification = false

# Skip transaction verification in block templates
skip_block_template_txs_verification = false

# Number of threads for transaction verification
txs_verification_threads_count = 8

# Custom genesis block in hexadecimal format for dev mode
# genesis_block_hex = "..."

# Block hash checkpoints (no rewind below these points)
checkpoints = []

# Simulator mode configuration (skip PoW, generate blocks automatically)
# [simulator]
# enable = true
# block_time_ms = 12000
```

#### 4. Firewall Configuration

```bash
# Allow P2P port
sudo ufw allow 2125/tcp

# Optional: Allow RPC port for monitoring (restrict to trusted IPs)
sudo ufw allow from YOUR_MONITORING_IP to any port 8080

# Enable firewall
sudo ufw enable
```

#### 5. Create Systemd Service

Create `/etc/systemd/system/tos-bootstrap.service`:

```ini
[Unit]
Description=TOS Bootstrap Node
After=network.target
Wants=network.target

[Service]
Type=simple
User=tos
Group=tos
WorkingDirectory=/var/lib/tos
ExecStart=/usr/local/bin/tos_daemon --config /etc/tos/daemon.toml --network mainnet
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

# Security settings
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/tos

[Install]
WantedBy=multi-user.target
```

#### 6. User and Directory Setup

```bash
# Create tos user
sudo useradd --system --home-dir /var/lib/tos --create-home tos

# Set permissions
sudo chown -R tos:tos /var/lib/tos
sudo mkdir -p /etc/tos
sudo chown -R tos:tos /etc/tos
```

#### 7. Start and Enable Service

```bash
# Reload systemd
sudo systemctl daemon-reload

# Start the service
sudo systemctl start tos-bootstrap

# Enable on boot
sudo systemctl enable tos-bootstrap

# Check status
sudo systemctl status tos-bootstrap
```

### Option 2: Use Existing Bootstrap Nodes

#### Connect to Community Bootstrap Nodes

```bash
# Start daemon with community bootstrap nodes
./tos_daemon \
  --network mainnet \
  --bootstrap-nodes \
    community1.tos-network.com:2125,\
    community2.tos-network.com:2125,\
    community3.tos-network.com:2125
```

#### Connect to Regional Bootstrap Nodes

```bash
# Asia-Pacific region
./tos_daemon --bootstrap-nodes \
  singapore.tos-nodes.com:2125,\
  tokyo.tos-nodes.com:2125

# Europe region
./tos_daemon --bootstrap-nodes \
  london.tos-nodes.com:2125,\
  frankfurt.tos-nodes.com:2125

# Americas region
./tos_daemon --bootstrap-nodes \
  newyork.tos-nodes.com:2125,\
  toronto.tos-nodes.com:2125
```

## Bootstrap Node Configuration Examples

### High Availability Setup

```bash
# Primary bootstrap node with failover
./tos_daemon \
  --priority-nodes primary-pool.com:2125 \
  --bootstrap-nodes \
    backup1.tos-network.com:2125,\
    backup2.tos-network.com:2125,\
    backup3.tos-network.com:2125 \
  --max-peers 75 \
  --p2p-max-outgoing-peers 25
```

### Mining Pool Configuration

```bash
# Mining pool with dedicated bootstrap infrastructure
./tos_daemon \
  --priority-nodes \
    pool-primary.mining-corp.com:2125,\
    pool-backup.mining-corp.com:2125 \
  --bootstrap-nodes \
    bootstrap-us.mining-corp.com:2125,\
    bootstrap-eu.mining-corp.com:2125,\
    bootstrap-asia.mining-corp.com:2125 \
  --network mainnet
```

### Development/Testing Setup

```bash
# Testnet with custom bootstrap nodes
./tos_daemon \
  --network testnet \
  --bootstrap-nodes \
    testnet-bootstrap1.dev.com:2125,\
    testnet-bootstrap2.dev.com:2125 \
  --max-peers 20
```

## Monitoring and Maintenance

### Health Check Script

Create `bootstrap_health_check.sh`:

```bash
#!/bin/bash

# Check if daemon is running
if ! pgrep -f "tos_daemon" > /dev/null; then
    echo "ERROR: TOS daemon not running"
    exit 1
fi

# Check if P2P port is listening
if ! netstat -ln | grep ":2125 " > /dev/null; then
    echo "ERROR: P2P port 2125 not listening"
    exit 1
fi

# Check peer count (requires RPC enabled)
if command -v curl > /dev/null; then
    PEER_COUNT=$(curl -s localhost:8080/get_info | jq -r '.peers_count' 2>/dev/null)
    if [ "$PEER_COUNT" -lt 5 ]; then
        echo "WARNING: Low peer count: $PEER_COUNT"
    else
        echo "OK: $PEER_COUNT peers connected"
    fi
fi

echo "Bootstrap node health check passed"
```

### Log Monitoring

```bash
# Monitor logs
sudo journalctl -u tos-bootstrap -f

# Check for errors
sudo journalctl -u tos-bootstrap --since "1 hour ago" | grep -i error

# Monitor peer connections
sudo journalctl -u tos-bootstrap --since "1 hour ago" | grep -i "peer\|connect"
```

## Security Best Practices

### 1. Network Security

```bash
# Use fail2ban to prevent brute force attacks
sudo apt install fail2ban

# Configure rate limiting for P2P connections
# Add to /etc/iptables/rules.v4:
# -A INPUT -p tcp --dport 2125 -m limit --limit 25/minute --limit-burst 100 -j ACCEPT
```

### 2. System Hardening

```bash
# Disable root SSH access
sudo sed -i 's/PermitRootLogin yes/PermitRootLogin no/' /etc/ssh/sshd_config

# Enable automatic security updates
sudo apt install unattended-upgrades
sudo dpkg-reconfigure -plow unattended-upgrades
```

### 3. Backup Strategy

```bash
# Backup blockchain data
sudo rsync -av /var/lib/tos/blockchain/ /backup/tos-$(date +%Y%m%d)/

# Backup configuration
sudo cp /etc/tos/daemon.toml /backup/config-$(date +%Y%m%d).toml
```

## Troubleshooting

### Common Issues

1. **Port binding failure**
   ```bash
   # Check if port is already in use
   sudo netstat -tulpn | grep :2125

   # Kill existing process if needed
   sudo pkill -f tos_daemon
   ```

2. **Peer connection issues**
   ```bash
   # Check firewall rules
   sudo ufw status

   # Test connectivity
   telnet your-bootstrap-node.com 2125
   ```

3. **High resource usage**
   ```bash
   # Monitor resource usage
   htop

   # Reduce max peers if needed
   # Edit /etc/tos/daemon.toml and restart service
   ```

### Performance Tuning

```bash
# Increase file descriptor limits
echo "tos soft nofile 65536" | sudo tee -a /etc/security/limits.conf
echo "tos hard nofile 65536" | sudo tee -a /etc/security/limits.conf

# Optimize network parameters
echo "net.core.rmem_max = 16777216" | sudo tee -a /etc/sysctl.conf
echo "net.core.wmem_max = 16777216" | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

## Cost Estimation

### Cloud Provider Costs (Monthly)

| Provider | Instance Type | CPU | RAM | Storage | Bandwidth | Estimated Cost |
|----------|---------------|-----|-----|---------|-----------|----------------|
| AWS | t3.medium | 2 | 4GB | 50GB SSD | 1TB | $35-45 |
| Google Cloud | e2-medium | 2 | 4GB | 50GB SSD | 1TB | $30-40 |
| DigitalOcean | Basic Droplet | 2 | 4GB | 50GB SSD | 4TB | $24 |
| Vultr | Regular Performance | 2 | 4GB | 50GB SSD | 3TB | $20 |
| Hetzner | CX21 | 2 | 4GB | 40GB SSD | 20TB | $7 |

### Recommended Providers for Bootstrap Nodes

1. **Hetzner** - Best value for European locations
2. **Vultr** - Good global coverage and performance
3. **DigitalOcean** - Reliable with good documentation
4. **AWS/GCP** - Enterprise-grade reliability (higher cost)

## Getting Listed as Community Bootstrap Node

To get your bootstrap node listed in the official TOS documentation:

1. **Stability**: Run continuously for 30+ days
2. **Performance**: Maintain >95% uptime
3. **Monitoring**: Provide public status page
4. **Contact**: Submit PR to TOS documentation with node details

## Support and Community

- **GitHub Issues**: https://github.com/tos-network/tos/issues
- **Documentation**: https://docs.tos-network.com
- **Community Discord**: [TOS Network Discord]
- **Bootstrap Node Registry**: [Community maintained list]

---

For additional support or questions about bootstrap node deployment, please open an issue on the TOS GitHub repository.