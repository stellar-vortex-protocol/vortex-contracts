# Vortex Contracts — justfile
#
# Alternative to the Makefile using `just` (https://just.systems/).
# All recipes mirror the Makefile exactly; use whichever tool you prefer.
#
# Usage:
#   just          — list available recipes
#   just all      — fmt + lint + test + build
#   just fmt      — format source
#   just lint     — clippy -D warnings (identical to CI)
#   just test     — run tests
#   just build    — build release wasm
#   just deploy-testnet STELLAR_SOURCE=<key>  — deploy to testnet

workspace := "intent_settlement"
wasm_target := "wasm32-unknown-unknown"
wasm_out := workspace / "target" / wasm_target / "release/vortex_intent_settlement.wasm"

# Show available recipes (default)
default:
    @just --list

# Run fmt, lint, test, and build (full pre-push check)
all: fmt lint test build

# Format all source files with rustfmt
fmt:
    cd {{workspace}} && cargo fmt --all

# Check formatting without modifying files (used by CI)
fmt-check:
    cd {{workspace}} && cargo fmt --all -- --check

# Run clippy with -D warnings (identical to the CI command)
lint:
    cd {{workspace}} && cargo clippy --all-targets -- -D warnings

# Run the full test suite
test:
    cd {{workspace}} && cargo test

# Build the release wasm binary
build:
    cd {{workspace}} && cargo build --target {{wasm_target}} --release

# Run cargo-audit against the dependency tree
audit:
    cd {{workspace}} && cargo audit

# Remove build artefacts
clean:
    cd {{workspace}} && cargo clean

# Deploy the wasm to Stellar testnet
# Usage: just deploy-testnet STELLAR_SOURCE=<secret-key-or-name>
deploy-testnet STELLAR_SOURCE network="testnet": build
    stellar contract deploy \
        --wasm {{wasm_out}} \
        --source {{STELLAR_SOURCE}} \
        --network {{network}}
