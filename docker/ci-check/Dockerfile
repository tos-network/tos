# TOS CI Check Docker Image
# Used to run CI checks locally before pushing to GitHub
#
# Build: docker build -t tos-ci-check docker/ci-check/
# Run:   docker run --rm -v $(pwd):/workspace tos-ci-check

FROM rust:1.91-bookworm

# Install system dependencies
RUN apt-get update && apt-get install -y \
    clang \
    cmake \
    libssl-dev \
    pkg-config \
    git \
    && rm -rf /var/lib/apt/lists/*

# Install Rust components
RUN rustup component add rustfmt clippy

# Set working directory
WORKDIR /workspace

# Copy the check script
COPY ci-check.sh /usr/local/bin/ci-check
RUN chmod +x /usr/local/bin/ci-check

# Default command
ENTRYPOINT ["/usr/local/bin/ci-check"]
