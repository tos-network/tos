# TOS Daemon

The TOS daemon is a blockchain node implementation that participates in the TOS network. It provides P2P networking, block validation, transaction processing, and RPC services.

## Node Types and Network Configuration

The TOS daemon supports three types of network nodes for different connection strategies:

### 1. Seed Nodes
- **Definition**: Hard-coded network bootstrap nodes built into the daemon
- **Purpose**: Provide initial network discovery for new nodes
- **Configuration**: Cannot be modified by users (compiled into the binary)
- **Trust Level**: Standard peer permissions
- **Usage**: Automatically used when no other nodes are available

### 2. Bootstrap Nodes
- **Definition**: User-configurable nodes for initial network connection
- **Purpose**: Supplement seed nodes with custom network entry points
- **Configuration**: `--bootstrap-nodes` command line parameter
- **Trust Level**: Standard peer permissions
- **Usage**: Connected after priority nodes during startup

### 3. Priority Nodes
- **Definition**: Fully trusted nodes with special privileges
- **Purpose**: Enable trusted operations like chain reorganization
- **Configuration**: `--priority-nodes` command line parameter
- **Trust Level**: High trust with special permissions
- **Usage**: Connected first with elevated privileges

## Connection Priority

Nodes are connected in the following order:
1. **Priority Nodes** → Connected immediately with special privileges
2. **Bootstrap Nodes** → Connected after priority nodes with standard permissions
3. **Seed Nodes** → Used automatically when other options are unavailable

## Usage Examples

### Basic Usage
```bash
# Start daemon with default seed nodes
./tos_daemon

# Start with custom bootstrap nodes
./tos_daemon --bootstrap-nodes 192.168.1.100:2125

# Multiple bootstrap nodes
./tos_daemon --bootstrap-nodes 192.168.1.100:2125,node2.example.com:2125
```

### Advanced Configuration
```bash
# Combine priority and bootstrap nodes
./tos_daemon \
  --priority-nodes trusted-pool.com:2125 \
  --bootstrap-nodes bootstrap1.com:2125,bootstrap2.com:2125

# Network-specific configuration
./tos_daemon \
  --network testnet \
  --bootstrap-nodes testnet-node1.com:2125,testnet-node2.com:2125

# With additional P2P settings
./tos_daemon \
  --bootstrap-nodes my-node.com:2125 \
  --p2p-bind-address 0.0.0.0:2125 \
  --max-peers 50 \
  --p2p-max-outgoing-peers 10
```

## Node Configuration Comparison

| Feature | Seed Nodes | Bootstrap Nodes | Priority Nodes |
|---------|------------|----------------|----------------|
| **Configuration** | Hard-coded | CLI/Config file | CLI/Config file |
| **Connection Priority** | Low | Medium | High |
| **Trust Level** | Standard | Standard | High Trust |
| **Special Privileges** | ❌ | ❌ | ✅ |
| **Chain Reorg Rights** | ❌ | ❌ | ✅ |
| **User Configurable** | ❌ | ✅ | ✅ |
| **DNS Resolution** | ❌ | ✅ | ✅ |

## Use Cases

### Bootstrap Nodes
- **Enterprise Deployments**: Connect to company-owned infrastructure nodes
- **Geographic Optimization**: Use nodes closer to your location for better latency
- **Testing Environments**: Connect to specific test network nodes
- **Custom Networks**: Bootstrap private or consortium networks

### Priority Nodes
- **Mining Pools**: Trust specific pool coordination nodes
- **Critical Operations**: Nodes that can perform emergency chain operations
- **High Availability**: Mission-critical nodes with special operational privileges

## Network Formats

All node addresses support:
- **IP Addresses**: `192.168.1.100:2125`
- **Domain Names**: `node.example.com:2125` (with automatic DNS resolution)
- **Multiple Nodes**: Comma-separated list `node1.com:2125,node2.com:2125`

## Built-in Seed Nodes

### Mainnet Seed Nodes
- `51.210.117.23:2125` (France)
- `198.71.55.87:2125` (US)
- `162.19.249.100:2125` (Germany)
- `139.99.89.27:2125` (Singapore)
- `51.68.142.141:2125` (Poland)
- `51.195.220.137:2125` (Great Britain)
- `66.70.179.137:2125` (Canada)

### Testnet Seed Nodes
- `74.208.251.149:2125` (US)

## Command Line Help

For a complete list of available options:
```bash
./tos_daemon --help
```

## Configuration Files

The daemon also supports configuration via TOML files. All CLI parameters can be specified in configuration files for persistent settings.

Example configuration:
```toml
[p2p]
bootstrap_nodes = ["node1.example.com:2125", "node2.example.com:2125"]
priority_nodes = ["trusted-node.com:2125"]
max_peers = 50
max_outgoing_peers = 10
```