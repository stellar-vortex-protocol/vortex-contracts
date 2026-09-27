#!/usr/bin/env bash
# verify-build.sh — reproducible WASM build verification for vortex-contracts
#
# Usage:
#   ./verify-build.sh                        # build + print hash
#   ./verify-build.sh <CONTRACT_ID>          # build + compare against on-chain hash
#
# Dependencies:
#   - Rust toolchain (see RUST_CHANNEL below)
#   - wasm32-unknown-unknown target
#   - stellar CLI (only required when CONTRACT_ID is supplied)
#   - sha256sum (coreutils) or shasum (macOS)
#
# Closes #103

set -euo pipefail

# ---------------------------------------------------------------------------
# Toolchain pin — must match the version used to produce the deployed binary.
# Update this whenever the contract is rebuilt for a new deployment.
# ---------------------------------------------------------------------------
RUST_CHANNEL="1.78.0"
WASM_TARGET="wasm32-unknown-unknown"
WASM_OUT="intent_settlement/target/${WASM_TARGET}/release/vortex_intent_settlement.wasm"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info()  { echo "[verify-build] $*"; }
die()   { echo "[verify-build] ERROR: $*" >&2; exit 1; }

sha256() {
  if command -v sha256sum &>/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum &>/dev/null; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "Neither sha256sum nor shasum found. Install coreutils."
  fi
}

# ---------------------------------------------------------------------------
# 1. Ensure the correct Rust toolchain is active
# ---------------------------------------------------------------------------
if ! command -v rustup &>/dev/null; then
  die "rustup not found. Install from https://rustup.rs then re-run."
fi

ACTIVE=$(rustup show active-toolchain 2>/dev/null | awk '{print $1}' || true)
if [[ "${ACTIVE}" != "${RUST_CHANNEL}"* ]]; then
  info "Installing/selecting Rust ${RUST_CHANNEL} ..."
  rustup toolchain install "${RUST_CHANNEL}" --target "${WASM_TARGET}" --no-self-update
fi

info "Using toolchain: $(rustup run "${RUST_CHANNEL}" rustc --version)"

# ---------------------------------------------------------------------------
# 2. Add the WASM target if not already present
# ---------------------------------------------------------------------------
if ! rustup target list --installed --toolchain "${RUST_CHANNEL}" | grep -q "${WASM_TARGET}"; then
  info "Adding ${WASM_TARGET} to toolchain ${RUST_CHANNEL} ..."
  rustup target add --toolchain "${RUST_CHANNEL}" "${WASM_TARGET}"
fi

# ---------------------------------------------------------------------------
# 3. Clean prior build artifacts to guarantee a fresh, reproducible output
# ---------------------------------------------------------------------------
info "Cleaning previous build artifacts ..."
(cd intent_settlement && rustup run "${RUST_CHANNEL}" cargo clean)

# ---------------------------------------------------------------------------
# 4. Build with the exact same flags used in CI / production
#
#    Key reproducibility settings (already in Cargo.toml [profile.release]):
#      opt-level = "z"       — deterministic size optimisation
#      codegen-units = 1     — single CGU → no non-deterministic parallelism
#      debug = 0             — no debug info embedded
#      strip = "symbols"     — strip symbol table
#      panic = "abort"       — no unwind tables
#      overflow-checks = true
#      debug-assertions = false
#
#    RUSTFLAGS is kept empty here; any extra flags would break reproducibility.
# ---------------------------------------------------------------------------
info "Building vortex_intent_settlement.wasm ..."
(
  cd intent_settlement
  RUSTFLAGS="" rustup run "${RUST_CHANNEL}" \
    cargo build \
      --target "${WASM_TARGET}" \
      --release \
      --locked           # ensures Cargo.lock is respected exactly
)

# ---------------------------------------------------------------------------
# 5. Compute and display the local hash
# ---------------------------------------------------------------------------
if [[ ! -f "${WASM_OUT}" ]]; then
  die "Expected wasm output not found at ${WASM_OUT}"
fi

LOCAL_HASH=$(sha256 "${WASM_OUT}")
info "Local SHA-256:  ${LOCAL_HASH}"
info "Wasm size:      $(du -h "${WASM_OUT}" | cut -f1)"

# ---------------------------------------------------------------------------
# 6. (Optional) Compare against the on-chain hash
#
#    stellar contract fetch --id <CONTRACT_ID> --network testnet
#    downloads the installed wasm and prints its sha256 in the response.
#
#    Alternatively, retrieve the hash directly via:
#      stellar contract info --id <CONTRACT_ID> --network testnet
# ---------------------------------------------------------------------------
CONTRACT_ID="${1:-}"
if [[ -n "${CONTRACT_ID}" ]]; then
  if ! command -v stellar &>/dev/null; then
    die "stellar CLI not found. Install from https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli"
  fi

  info "Fetching on-chain wasm for contract ${CONTRACT_ID} ..."
  TMP_WASM=$(mktemp /tmp/onchain_XXXXXX.wasm)
  trap 'rm -f "${TMP_WASM}"' EXIT

  stellar contract fetch \
    --id "${CONTRACT_ID}" \
    --network testnet \
    --out "${TMP_WASM}"

  ONCHAIN_HASH=$(sha256 "${TMP_WASM}")
  info "On-chain SHA-256: ${ONCHAIN_HASH}"

  if [[ "${LOCAL_HASH}" == "${ONCHAIN_HASH}" ]]; then
    info "✅  Hashes MATCH — local build is reproducible against the deployed contract."
    exit 0
  else
    die "❌  Hashes DO NOT MATCH.
  local:    ${LOCAL_HASH}
  on-chain: ${ONCHAIN_HASH}
Check that RUST_CHANNEL (${RUST_CHANNEL}) matches the toolchain used for deployment."
  fi
fi

info "Done. Pass the deployed CONTRACT_ID as the first argument to compare against on-chain."
