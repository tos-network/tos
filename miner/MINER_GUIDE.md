# TOS Testnet Miner Guide

## Quick Start

### Automated Setup (Recommended)
```bash
# 1. Setup miner service
./miner/setup_miner.sh

# 2. Start mining
sudo systemctl start tos-miner

# 3. Monitor mining
sudo journalctl -u tos-miner -f
```

### Manual Mining (Testing)
```bash
# Run miner in foreground for testing
./target/release/tos_miner \
    --miner-address tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u \
    --daemon-address 127.0.0.1:8080 \
    --num-threads 1 \
    --disable-interactive-mode
```

## Mining Configuration

| Parameter | Value | Description |
|-----------|-------|-------------|
| Miner Address | `tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u` | Testnet wallet address to receive rewards |
| Daemon Address | `127.0.0.1:8080` | Local daemon RPC endpoint |
| Threads | `1` | Single thread mining |
| Network | Testnet | Low difficulty mining |
| Interactive Mode | Disabled | Non-interactive operation (no CLI commands) |

## Management Commands

### Using Control Script
```bash
# Start mining
./miner/miner_control.sh start

# Stop mining
./miner/miner_control.sh stop

# Check status
./miner/miner_control.sh status

# View real-time logs
./miner/miner_control.sh logs

# Restart mining
./miner/miner_control.sh restart

# Manual testing
./miner/miner_control.sh manual
```

### Direct SystemD Commands
```bash
# Start/stop/restart
sudo systemctl start tos-miner
sudo systemctl stop tos-miner
sudo systemctl restart tos-miner

# Status and logs
sudo systemctl status tos-miner
sudo journalctl -u tos-miner -f

# Enable/disable auto-start
sudo systemctl enable tos-miner
sudo systemctl disable tos-miner
```

## Resource Usage

- **Memory**: Limited to 1GB max (512MB soft limit)
- **CPU**: Single thread mining (configurable)
- **Storage**: Logs only, minimal storage impact
- **Network**: WebSocket connection to local daemon

## Prerequisites

1. **Daemon Running**:
   ```bash
   sudo systemctl start tos-daemon
   sudo systemctl status tos-daemon
   ```

2. **Daemon Connectivity**:
   ```bash
   curl -X POST -H "Content-Type: application/json" \
   -d '{"jsonrpc":"2.0","method":"get_info","id":1}' \
   http://127.0.0.1:8080/json_rpc
   ```

3. **Built Miner Binary**:
   ```bash
   cargo build --release --bin tos_miner
   ls -la target/release/tos_miner
   ```

## Troubleshooting

### Miner Won't Start
```bash
# Check daemon connectivity
curl -s http://127.0.0.1:8080/json_rpc -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"get_info","id":1}'

# Check daemon status
sudo systemctl status tos-daemon

# Check miner service logs
sudo journalctl -u tos-miner -n 50
```

### Connection Issues
```bash
# Verify daemon is listening
sudo netstat -tlnp | grep 8080

# Check firewall
sudo ufw status

# Test local connectivity
telnet 127.0.0.1 8080
```

### Performance Issues
```bash
# Monitor resource usage
sudo systemctl status tos-miner
htop

# Adjust thread count (edit service file)
sudo systemctl edit tos-miner

# Check system load
uptime
```

## Service Files

- **Service Config**: `/etc/systemd/system/tos-miner.service`
- **Setup Script**: `./miner/setup_miner.sh`
- **Control Script**: `./miner/miner_control.sh`
- **Binary Location**: `./target/release/tos_miner`

## Mining Rewards

- **Network**: Testnet (testing purposes)
- **Address**: `tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u`
- **Difficulty**: Low (100 H/s minimum)
- **Block Time**: ~60 seconds target

## Security Notes

- Mining runs as root user (required for system service)
- Local daemon connection only (127.0.0.1)
- Memory limits enforced to prevent system impact
- Auto-restart enabled for reliability