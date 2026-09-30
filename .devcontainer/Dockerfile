FROM mcr.microsoft.com/devcontainers/rust:1.78

# Install system dependencies required by Stellar CLI and other tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    make \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install wasm32-unknown-unknown target
RUN rustup target add wasm32-unknown-unknown

# Install Stellar CLI with optimization features
# The --features opt flag enables additional tooling optimizations
RUN cargo install --locked stellar-cli --features opt

# Install cargo-audit for dependency vulnerability scanning
RUN cargo install --locked cargo-audit

# Install just for task running (alternative to make)
RUN cargo install --locked just

# Create setup script that runs on container creation
RUN echo '#!/bin/bash\n\
set -e\n\
echo "Vortex Contracts devcontainer setup..."\n\
\n\
# Verify toolchain versions\n\
echo "Rust version: $(rustc --version)"\n\
echo "Stellar CLI version: $(stellar --version 2>&1 || echo \"stellar command ready\")"\n\
\n\
# Pre-fetch dependencies to speed up first build\n\
echo "Pre-fetching cargo dependencies..."\n\
cd /workspaces/vortex-contracts && cargo fetch --locked 2>/dev/null || true\n\
\n\
echo "✓ Devcontainer setup complete"\n\
echo ""\n\
echo "Run \"make all\" or \"just all\" to build and test both contracts"\n\
' > /tmp/devcontainer-setup.sh && chmod +x /tmp/devcontainer-setup.sh

# Set working directory
WORKDIR /workspaces/vortex-contracts
