# Vortex Contracts — justfile
#
# Alternative to the Makefile using `just` (https://just.systems/).
# `just all` delegates to `make all` so both tools run the same workspace-wide
# checks; the other recipes operate on intent_settlement (and proof_registry).
#
# Usage:
#   just          — list available recipes
#   just all      — runs `make all` (fmt + lint + test + build across all crates)
#   just fmt      — format source
#   just lint     — clippy -D warnings (identical to CI)
#   just test     — run tests
#   just build    — build release wasm
#   just deploy-testnet STELLAR_SOURCE=<key>  — deploy to testnet
#
# Proof Registry recipes (issue #213):
#   just proof-registry-fmt      — format proof_registry source
#   just proof-registry-lint     — clippy proof_registry
#   just proof-registry-test     — test proof_registry
#   just proof-registry-build    — build proof_registry wasm
#   just proof-registry-all      — full check for proof_registry

workspace := "intent_settlement"
proof_registry := "proof_registry"
wasm_target := "wasm32-unknown-unknown"
wasm_out := workspace / "target" / wasm_target / "release/vortex_intent_settlement.wasm"
pr_wasm_out := proof_registry / "target" / wasm_target / "release/vortex_proof_registry.wasm"

# Show available recipes (default)
default:
    @just --list

# Run the full pre-push check across all crates (delegates to `make all`)
all:
    make all

# Full check for proof_registry
proof-registry-all: proof-registry-fmt proof-registry-lint proof-registry-test proof-registry-build

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

# Regenerate README.md's Error Reference table from the Error enum (issue #531)
error-reference:
    python3 scripts/generate-error-reference.py --write

# Verify README.md's Error Reference table matches the Error enum (issue #531)
error-reference-check:
    python3 scripts/generate-error-reference.py --check

# Remove build artefacts
clean:
    cd {{workspace}} && cargo clean

# Proof Registry recipes (issue #213)
proof-registry-fmt:
    cd {{proof_registry}} && cargo fmt --all

proof-registry-fmt-check:
    cd {{proof_registry}} && cargo fmt --all -- --check

proof-registry-lint:
    cd {{proof_registry}} && cargo clippy --all-targets -- -D warnings

proof-registry-test:
    cd {{proof_registry}} && cargo test

proof-registry-build:
    cd {{proof_registry}} && cargo build --target {{wasm_target}} --release

proof-registry-audit:
    cd {{proof_registry}} && cargo audit

proof-registry-clean:
    cd {{proof_registry}} && cargo clean

# Deploy the wasm to Stellar testnet
# Usage: just deploy-testnet STELLAR_SOURCE=<secret-key-or-name>
deploy-testnet STELLAR_SOURCE network="testnet": build
    stellar contract deploy \
        --wasm {{wasm_out}} \
        --source {{STELLAR_SOURCE}} \
        --network {{network}}
