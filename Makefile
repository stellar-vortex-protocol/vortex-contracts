# Vortex Contracts — Makefile
#
# Wraps the commands documented in README.md so contributors never need to
# copy-paste raw multi-flag invocations.  All targets run inside the
# intent_settlement workspace directory.
#
# Usage:
#   make          — print help
#   make all      — fmt + lint + test + build (full pre-push check)
#   make fmt      — format all source files
#   make lint     — clippy with -D warnings (identical to CI)
#   make test     — run unit / integration tests
#   make build    — compile the release wasm binary
#   make deploy-testnet — deploy to Stellar testnet (requires STELLAR_SOURCE)

.DEFAULT_GOAL := help

# ── Configuration ─────────────────────────────────────────────────────────────

WORKSPACE       := intent_settlement
WASM_TARGET     := wasm32-unknown-unknown
WASM_OUT        := $(WORKSPACE)/target/$(WASM_TARGET)/release/vortex_intent_settlement.wasm
STELLAR_NETWORK ?= testnet
# Set STELLAR_SOURCE to your secret key or key-name before running deploy-testnet
STELLAR_SOURCE  ?=

# ── Compound targets ──────────────────────────────────────────────────────────

.PHONY: all
all: fmt lint test build  ## Run fmt, lint, test, and build (full pre-push check)

# ── Individual targets ────────────────────────────────────────────────────────

.PHONY: fmt
fmt:  ## Format all source files with rustfmt
	cd $(WORKSPACE) && cargo fmt --all

.PHONY: fmt-check
fmt-check:  ## Check formatting without modifying files (used by CI)
	cd $(WORKSPACE) && cargo fmt --all -- --check

.PHONY: lint
lint:  ## Run clippy with -D warnings (identical to the CI command)
	cd $(WORKSPACE) && cargo clippy --all-targets -- -D warnings

.PHONY: test
test:  ## Run the full test suite
	cd $(WORKSPACE) && cargo test

.PHONY: build
build:  ## Build the release wasm binary
	cd $(WORKSPACE) && cargo build --target $(WASM_TARGET) --release

.PHONY: audit
audit:  ## Run cargo-audit against the dependency tree
	cd $(WORKSPACE) && cargo audit

.PHONY: clean
clean:  ## Remove build artefacts
	cd $(WORKSPACE) && cargo clean

# ── Deploy ────────────────────────────────────────────────────────────────────

.PHONY: deploy-testnet
deploy-testnet: build  ## Deploy the wasm to Stellar testnet (set STELLAR_SOURCE first)
ifndef STELLAR_SOURCE
	$(error STELLAR_SOURCE is not set. Export your Stellar secret key or key-name: export STELLAR_SOURCE=<SECRET_KEY>)
endif
	stellar contract deploy \
		--wasm $(WASM_OUT) \
		--source $(STELLAR_SOURCE) \
		--network $(STELLAR_NETWORK)

# ── Help ──────────────────────────────────────────────────────────────────────

.PHONY: help
help:  ## Show this help message
	@echo "Usage: make <target>"
	@echo ""
	@echo "Targets:"
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*##"}; {printf "  %-20s %s\n", $$1, $$2}'
