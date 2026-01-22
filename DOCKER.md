# TOS Network - Docker Guide

This guide explains how to build and run TOS Network components using Docker.

## Quick Start

### 1. Build Docker Image

```bash
# Build daemon (default)
./docker-build.sh

# Build specific component
./docker-build.sh --app tos_miner --tag v1.0.0

# Build all components
./docker-build.sh --all --tag latest
```

### 2. Run with Docker Compose

```bash
# Start daemon only
docker-compose up tos-daemon

# Start daemon with miner
docker-compose --profile miner up

# Start all services
docker-compose --profile miner --profile wallet up
```

## Available Components

- **tos_daemon** - Blockchain node daemon
- **tos_miner** - Mining program
- **tos_wallet** - Wallet program
- **tos_genesis** - Genesis block generator

## Docker Build Script

The `docker-build.sh` script provides easy building of Docker images.

### Usage

```bash
./docker-build.sh [OPTIONS]
```

### Options

- `-a, --app APP` - Application to build (default: tos_daemon)
- `-t, --tag TAG` - Docker image tag (default: latest)
- `-r, --registry REGISTRY` - Registry prefix for pushing
- `-p, --push` - Push image to registry after build
- `--no-cache` - Build without using cache
- `--all` - Build all applications
- `-h, --help` - Show help message

### Examples

```bash
# Build daemon with default settings
./docker-build.sh

# Build miner with custom tag
./docker-build.sh --app tos_miner --tag v1.0.0

# Build and push to registry
./docker-build.sh --app tos_daemon --registry ghcr.io/tos-network --tag v1.0.0 --push

# Build all components
./docker-build.sh --all --tag v1.0.0
```

## Docker Compose

### Services

#### TOS Daemon
```yaml
tos-daemon:
  ports:
    - "2125:2125"  # P2P port
    - "8080:8080"  # RPC / HTTP API port
  volumes:
    - tos-data:/var/run/tos/data
    - tos-config:/var/run/tos/config
```

#### TOS Miner
```yaml
tos-miner:
  environment:
    - TOS_DAEMON_URL=http://tos-daemon:8080
    - TOS_MINER_ADDRESS=${TOS_MINER_ADDRESS}
    - TOS_MINER_THREADS=${TOS_MINER_THREADS:-4}
```

### Environment Variables

Copy `.env.example` to `.env` and configure:

```bash
cp .env.example .env
```

Key variables:
- `TOS_NETWORK` - Network type (mainnet/testnet)
- `TOS_MINER_ADDRESS` - Your mining wallet address
- `RUST_LOG` - Logging level (debug/info/warn/error)
- `TOS_RPC_BIND_ADDRESS` - RPC bind address (default `0.0.0.0:8080`)
- `TOS_RPC_STOP_TIMEOUT_SECS` - RPC stop timeout in seconds (default `5`)
- `TOS_P2P_BIND_ADDRESS` - P2P bind address (default `0.0.0.0:2125`)
- `TOS_PRIORITY_NODES` - Comma-separated seed/priority nodes
- `TOS_EXCLUSIVE_NODES` - Comma-separated exclusive nodes (connect only to these)

### A2A Escrow Validation Flags

When running the daemon, you can pass these CLI flags to control on-chain escrow validation
for A2A settlement requests:

```
--a2a-escrow-validate-states true
--a2a-escrow-allowed-states created,funded,pending-release,challenged
--a2a-escrow-validate-timeout true
--a2a-escrow-validate-amounts false
```

### Testnet Nodes (2026-01-08)

From `memo/08-Network/TestNet/TESTNET_NODES.md`:

| Name | IP | Role |
|------|----|------|
| tos-testnode-1 | 157.7.65.157 | Seed + Miner |
| tos-testnode-2 | 157.7.78.65 | Validator + Miner |
| tos-testnode-3 | 157.7.192.216 | Validator + Miner |

Example `.env` for a validator node:
```bash
TOS_NETWORK=testnet
TOS_RPC_BIND_ADDRESS=0.0.0.0:8080
TOS_P2P_BIND_ADDRESS=0.0.0.0:2125
TOS_EXCLUSIVE_NODES=157.7.65.157:2125
TOS_PRIORITY_NODES=
```

Example `.env` for a seed node:
```bash
TOS_NETWORK=testnet
TOS_RPC_BIND_ADDRESS=0.0.0.0:8080
TOS_P2P_BIND_ADDRESS=0.0.0.0:2125
TOS_PRIORITY_NODES=
TOS_EXCLUSIVE_NODES=
```

### Profiles

Use profiles to run specific combinations:

```bash
# Only daemon
docker-compose up tos-daemon

# Daemon + miner
docker-compose --profile miner up

# Everything
docker-compose --profile miner --profile wallet up
```

## Manual Docker Commands

### Build Image

```bash
docker build \
  --build-arg app=tos_daemon \
  --build-arg commit_hash=$(git rev-parse --short HEAD) \
  -t tos-network-daemon:latest \
  .
```

### Run Container

```bash
# Run daemon
docker run -d \
  --name tos-daemon \
  -p 2125:2125 \
  -p 2126:2126 \
  -p 8080:8080 \
  -v tos-data:/var/run/tos/data \
  tos-network-daemon:latest

# Run miner
docker run -d \
  --name tos-miner \
  --link tos-daemon \
  -e TOS_DAEMON_URL=http://tos-daemon:8080 \
  -e TOS_MINER_ADDRESS=your_address \
  tos-network-miner:latest
```

## Multi-Architecture Builds

Build for multiple architectures:

```bash
# Setup buildx
docker buildx create --name tos-builder --use

# Build multi-arch
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --build-arg app=tos_daemon \
  -t tos-network-daemon:latest \
  --push \
  .
```

## Development

### Development Dockerfile

For development with hot reloading:

```dockerfile
FROM rust:1.86-slim-bookworm

WORKDIR /app
COPY . .

RUN cargo install cargo-watch
CMD ["cargo", "watch", "-x", "run --bin tos_daemon"]
```

### Debug Build

```bash
docker build \
  --build-arg app=tos_daemon \
  --target builder \
  -t tos-network-debug:latest \
  .

docker run -it tos-network-debug:latest bash
```

## Health Checks

The daemon includes a health check endpoint:

```bash
# Check health
curl http://localhost:8080/health

# Docker health check
docker exec tos-daemon curl -f http://localhost:8080/health
```

## Logging

View logs:

```bash
# Docker Compose
docker-compose logs -f tos-daemon

# Docker
docker logs -f tos-daemon
```

## Troubleshooting

### Common Issues

1. **Docker not running**
   ```bash
   sudo systemctl start docker  # Linux
   # Or start Docker Desktop on macOS/Windows
   ```

2. **Permission denied**
   ```bash
   sudo chmod +x docker-build.sh
   ```

3. **Build failures**
   ```bash
   # Clear cache and rebuild
   ./docker-build.sh --no-cache
   ```

4. **Port conflicts**
   ```bash
   # Check what's using the port
   netstat -tulpn | grep :2125

   # Stop conflicting services
   docker-compose down
   ```

### Resource Requirements

Minimum requirements:
- RAM: 2GB
- Storage: 10GB
- CPU: 2 cores

Recommended:
- RAM: 8GB
- Storage: 100GB SSD
- CPU: 4+ cores

## Security

### Production Deployment

1. **Use specific tags** instead of `latest`
2. **Run as non-root** user
3. **Limit container resources**
4. **Use secrets management** for sensitive data
5. **Keep images updated**

### Example Production Compose

```yaml
version: '3.8'
services:
  tos-daemon:
    image: ghcr.io/tos-network/tos-network-daemon:v1.0.0
    restart: unless-stopped
    user: "1000:1000"
    read_only: true
    tmpfs:
      - /tmp
    cap_drop:
      - ALL
    cap_add:
      - NET_BIND_SERVICE
    deploy:
      resources:
        limits:
          memory: 4G
          cpus: '2'
```

## License

This project is licensed under the BSD 3-Clause License. See the [LICENSE](LICENSE) file for details.

---

**TOS Network** - Next Generation Blockchain Network 🚀
