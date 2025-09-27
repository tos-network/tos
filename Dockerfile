# TOS Network - Multi-stage Docker Build
# Supports building any TOS binary (daemon, miner, wallet, etc.)

FROM lukemathwalker/cargo-chef:0.1.71-rust-1.86-slim-bookworm AS chef

ENV BUILD_DIR=/tmp/tos-build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    clang \
    cmake \
    libclang-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p $BUILD_DIR
WORKDIR $BUILD_DIR

# ---

FROM chef AS planner

ARG app=tos_daemon

COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin $app

# ---

FROM chef AS builder

ARG app=tos_daemon
ARG commit_hash
ARG target_arch=x86_64-unknown-linux-gnu

COPY --from=planner /tmp/tos-build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin $app

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY common ./common
COPY daemon ./daemon
COPY miner ./miner
COPY wallet ./wallet
COPY genesis ./genesis
COPY ai_miner ./ai_miner

# Build the specified binary
RUN TOS_COMMIT_HASH=${commit_hash} cargo build --release --bin $app

# ---

FROM gcr.io/distroless/cc-debian12

ARG app=tos_daemon

ENV APP_DIR=/var/run/tos
ENV DATA_DIR=$APP_DIR/data
ENV CONFIG_DIR=$APP_DIR/config
ENV BINARY=$APP_DIR/tos

# Metadata
LABEL org.opencontainers.image.title="TOS Network"
LABEL org.opencontainers.image.description="TOS Network Blockchain Node"
LABEL org.opencontainers.image.authors="TOS Network Team <info@tos.network>"
LABEL org.opencontainers.image.source="https://github.com/tos-network/tos"
LABEL org.opencontainers.image.vendor="TOS Network"
LABEL org.opencontainers.image.licenses="BSD-3-Clause"

# Create directories and copy binary
RUN mkdir -p $DATA_DIR $CONFIG_DIR
COPY --from=builder /tmp/tos-build/target/release/$app $BINARY

# Set proper permissions
RUN chmod +x $BINARY

# Working directory for data
WORKDIR $DATA_DIR

# Expose common ports (can be overridden)
EXPOSE 2125 2126

# Health check for daemon
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

ENTRYPOINT ["/var/run/tos/tos"]
