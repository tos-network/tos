# TOS Network - Build Guide

This document describes how to build TOS Network binaries.

## Binaries

The TOS Network project contains the following binary programs:

- **tos_daemon** - Blockchain node daemon
- **tos_miner** - Mining program
- **tos_wallet** - Wallet program
- **tos_genesis** - Genesis block generator
- **tos_ai_miner** - AI mining program

## Build Scripts

### 1. Local Build (Recommended for Development)

```bash
./build_local.sh
```

This script will:
- Automatically detect the current platform
- Build all binary programs
- Copy build results to `build/local/` directory
- Generate checksum files

**Supported Platforms:**
- Linux (x86_64)
- macOS (Intel x86_64 and Apple Silicon arm64)
- Windows (x86_64)

### 2. Cross-Platform Build (For Releases)

```bash
./build_all.sh
```

This script will:
- Cross-compile for multiple target platforms
- Generate archives and checksums
- Create release-ready build artifacts

**Target Platforms:**
- `aarch64-unknown-linux-gnu` (Linux ARM64)
- `armv7-unknown-linux-gnueabihf` (Linux ARMv7)
- `x86_64-unknown-linux-musl` (Linux x86_64 static linking)
- `x86_64-unknown-linux-gnu` (Linux x86_64)
- `x86_64-pc-windows-gnu` (Windows x86_64)
- `x86_64-apple-darwin` (macOS Intel)
- `aarch64-apple-darwin` (macOS Apple Silicon)

## Prerequisites

### Local Build
Only requires Rust toolchain installation:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Cross-Platform Build
Additionally requires cross installation:
```bash
cargo install cross
```

Also needs Docker runtime environment:
- Linux: `sudo systemctl start docker`
- macOS: Start Docker Desktop
- Windows: Start Docker Desktop

## Manual Build

### Development Mode
```bash
cargo build
```

### Release Mode
```bash
cargo build --release
```

### Specific Target Platform
```bash
cargo build --target x86_64-unknown-linux-gnu --release
```

## Build Output

### Local Build
- Binary files: `build/local/`
- Checksums: `build/local/checksums.txt`

### Cross-Platform Build
- Platform-specific directories: `build/{target}/`
- Archives: `build/tos-network-{target}.tar.gz` or `.zip`
- Release checksums: `build/release-checksums.txt`

## Troubleshooting

### Common Issues

1. **Docker Not Running**
   ```
   Error: Docker is not running
   ```
   Solution: Start Docker Desktop or docker daemon

2. **Target Platform Not Supported**
   ```
   Error: target not found
   ```
   Solution: Use `rustup target add <target>` to add the target platform

3. **Cross-Compilation Failure**
   - Ensure Docker has sufficient storage space
   - Check network connectivity for downloading build images
   - Try cleaning and rebuilding: `cross clean`

### Build Time Optimization

1. **Use Local Cache**
   ```bash
   export CARGO_TARGET_DIR=/tmp/cargo-target
   ```

2. **Parallel Build**
   ```bash
   export CARGO_BUILD_JOBS=8
   ```

3. **Incremental Build**
   Avoid using `cross clean` unless necessary

## Release Process

1. Update version numbers in `Cargo.toml`
2. Run full tests: `cargo test`
3. Execute cross-platform build: `./build_all.sh`
4. Verify build artifacts
5. Release to GitHub Releases

## License

This project is licensed under the BSD 3-Clause License. See the [LICENSE](LICENSE) file for details.

---

**TOS Network** - Next Generation Blockchain Network 🚀