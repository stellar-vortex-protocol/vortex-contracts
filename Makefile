# Vortex Contracts — Makefile
#
# Wraps the commands documented in README.md so contributors never need to
# copy-paste raw multi-flag invocations.  All workspace-wide targets (fmt,
# fmt-check, lint, test, build, audit, clean) iterate over every Rust crate
# listed in ALL_CRATES so that a single `make all` covers the whole repo
# (issue #514).
#
# Per-crate targets are also available for focused work:
#   make <crate>-fmt | <crate>-lint | <crate>-test | <crate>-build | <crate>-all
#
# Usage:
#   make              — print help
#   make all          — fmt + lint + test + build across ALL crates
#   make fmt          — format every crate
#   make lint         — clippy -D warnings on every crate
#   make test         — run tests in every crate
#   make build        — build release wasm for every crate
#   make deploy-testnet — deploy intent_settlement to Stellar testnet
#
# Kani / proptest targets (issues #415, #544):
#   make kani         — model-check Kani harnesses (requires kani-verifier)
#   make proptest     — run proptest suites across Kani-covered crates

.PHONY: build-wasms integration-test

build-wasms:
	cargo build --release --target wasm32-unknown-unknown \
		-p registry -p intent_settlement -p proofs -p badges

# ── Configuration ─────────────────────────────────────────────────────────────

# Primary deploy target kept for backward compat (deploy-testnet target).
WORKSPACE       := intent_settlement
WASM_TARGET     := wasm32-unknown-unknown
WASM_OUT        := $(WORKSPACE)/target/$(WASM_TARGET)/release/vortex_intent_settlement.wasm
STELLAR_NETWORK ?= testnet
# Set STELLAR_SOURCE to your secret key or key-name before running deploy-testnet
STELLAR_SOURCE  ?=

# All Rust crates in the workspace (issue #514).
# vortex-common is a library crate with no wasm target; it is included in
# fmt/lint/test but excluded from the build target (no cdylib output).
ALL_CRATES := \
	vortex-common \
	intent_settlement \
	solver_registry \
	proof_registry \
	reputation_badge \
	treasury_streams \
	fee_router \
	arbiter_panel \
	governor \
	backstop_vault \
	intent_periphery \
	timelock

# Crates that produce a wasm binary (cdylib).  vortex-common is excluded.
WASM_CRATES := \
	intent_settlement \
	solver_registry \
	proof_registry \
	reputation_badge \
	treasury_streams \
	fee_router \
	arbiter_panel \
	governor \
	backstop_vault \
	intent_periphery \
	timelock

# Crates that carry Kani harnesses and/or property-based tests (issue #544).
KANI_CRATES     := intent_settlement solver_registry proof_registry

# ── Workspace-wide compound targets ──────────────────────────────────────────

.PHONY: all
all: fmt lint test build  ## Run fmt, lint, test, and build across all crates (full pre-push check)

# ── Workspace-wide individual targets ────────────────────────────────────────

.PHONY: fmt
fmt:  ## Format every crate with rustfmt
	@set -e; for crate in $(ALL_CRATES); do \
		echo "==> cargo fmt in $$crate"; \
		( cd $$crate && cargo fmt --all ); \
	done

.PHONY: fmt-check
fmt-check:  ## Check formatting in every crate without modifying files (used by CI)
	@set -e; for crate in $(ALL_CRATES); do \
		echo "==> cargo fmt --check in $$crate"; \
		( cd $$crate && cargo fmt --all -- --check ); \
	done

.PHONY: lint
lint:  ## Run clippy -D warnings on every crate (identical to the CI command)
	@set -e; for crate in $(ALL_CRATES); do \
		echo "==> cargo clippy in $$crate"; \
		( cd $$crate && cargo clippy --all-targets -- -D warnings ); \
	done

.PHONY: test
test:  ## Run the full test suite in every crate
	@set -e; for crate in $(ALL_CRATES); do \
		echo "==> cargo test in $$crate"; \
		( cd $$crate && cargo test ); \
	done

.PHONY: build
build:  ## Build the release wasm binary for every wasm crate
	@set -e; for crate in $(WASM_CRATES); do \
		echo "==> cargo build --release in $$crate"; \
		( cd $$crate && cargo build --target $(WASM_TARGET) --release ); \
	done

.PHONY: audit
audit:  ## Run cargo-audit against every crate's dependency tree
	@set -e; for crate in $(ALL_CRATES); do \
		echo "==> cargo audit in $$crate"; \
		( cd $$crate && cargo audit ); \
	done

.PHONY: clean
clean:  ## Remove build artefacts from every crate
	@set -e; for crate in $(ALL_CRATES); do \
		echo "==> cargo clean in $$crate"; \
		( cd $$crate && cargo clean ); \
	done

# ── Per-crate convenience targets ─────────────────────────────────────────────
#
# Pattern: make <crate>-fmt | <crate>-fmt-check | <crate>-lint |
#          <crate>-test | <crate>-build | <crate>-audit | <crate>-clean |
#          <crate>-all
#
# Implemented via static rules so `make help` can document them and tab-
# completion works.  Add new crates to ALL_CRATES above; the rules below
# need no changes.

define CRATE_RULES
.PHONY: $(1)-fmt $(1)-fmt-check $(1)-lint $(1)-test $(1)-build $(1)-audit $(1)-clean $(1)-all
$(1)-fmt:        ## Format $(1) source files
	cd $(1) && cargo fmt --all
$(1)-fmt-check:  ## Check $(1) formatting without modifying
	cd $(1) && cargo fmt --all -- --check
$(1)-lint:       ## Run clippy on $(1)
	cd $(1) && cargo clippy --all-targets -- -D warnings
$(1)-test:       ## Run $(1) test suite
	cd $(1) && cargo test
$(1)-build:      ## Build $(1) release wasm
	cd $(1) && cargo build --target $(WASM_TARGET) --release
$(1)-audit:      ## Run cargo-audit on $(1)
	cd $(1) && cargo audit
$(1)-clean:      ## Remove $(1) build artefacts
	cd $(1) && cargo clean
$(1)-all: $(1)-fmt $(1)-lint $(1)-test $(1)-build  ## Full check for $(1)
endef

$(foreach crate,$(ALL_CRATES),$(eval $(call CRATE_RULES,$(crate))))

# ── Generated documentation (issue #531) ─────────────────────────────────────

.PHONY: error-reference
error-reference:  ## Regenerate README.md's Error Reference table from the Error enum
	python3 scripts/generate-error-reference.py --write

.PHONY: error-reference-check
error-reference-check:  ## Verify README.md's Error Reference table matches the Error enum
	python3 scripts/generate-error-reference.py --check

# ── Deploy ────────────────────────────────────────────────────────────────────

.PHONY: deploy-testnet
deploy-testnet: intent_settlement-build  ## Deploy intent_settlement wasm to Stellar testnet (set STELLAR_SOURCE first)
ifndef STELLAR_SOURCE
	$(error STELLAR_SOURCE is not set. Export your Stellar secret key or key-name: export STELLAR_SOURCE=<SECRET_KEY>)
endif
	stellar contract deploy \
		--wasm $(WASM_OUT) \
		--source $(STELLAR_SOURCE) \
		--network $(STELLAR_NETWORK)

# ── Local integration testing ────────────────────────────────────────────────

.PHONY: local-network-start
local-network-start:  ## Start local Soroban test network via docker-compose
	./setup-local-network.sh start

.PHONY: local-network-stop
local-network-stop:  ## Stop local Soroban test network
	./setup-local-network.sh stop

.PHONY: local-network-clean
local-network-clean:  ## Clean and remove local network volumes (destructive)
	./setup-local-network.sh clean

.PHONY: local-network-logs
local-network-logs:  ## Tail local network logs
	./setup-local-network.sh logs

.PHONY: local-network-status
local-network-status:  ## Check local network status
	./setup-local-network.sh status

# ── Integration tests ────────────────────────────────────────────────────────

.PHONY: integration-test
integration-test: build-wasms  ## Run the integration test suite against freshly built wasms
	cargo test -p integration-tests

# ── Kani model checking (issues #415, #544) ─────────────────────────────────

.PHONY: kani
kani:  ## Run Kani proof harnesses for pure arithmetic helpers (requires kani-verifier)
	@command -v cargo-kani >/dev/null 2>&1 || { \
		echo "Kani not found. Install with: cargo install --locked kani-verifier && cargo kani setup"; \
		exit 1; \
	}
	@set -e; \
	for crate in $(KANI_CRATES); do \
		echo "==> cargo kani in $$crate"; \
		case $$crate in \
			intent_settlement) \
				harnesses=" \
					--harness kani_slash_never_negative \
					--harness kani_slash_never_exceeds_bond \
					--harness kani_slash_floor_one \
					--harness kani_tier_fill_window_monotone \
					--harness kani_tier_fill_window_no_overflow \
					--harness kani_fee_never_exceeds_amount \
					--harness kani_fee_rounding_direction \
					--harness kani_dutch_decay_bounds \
					--harness kani_discount_in_range \
					--harness kani_decode_payload_roundtrip \
					--harness kani_slash_bps_monotone \
					--harness kani_decode_i128_be_bounds_safe" ;; \
			solver_registry) \
				harnesses=" \
					--harness kani_slash_bps_never_negative \
					--harness kani_slash_bps_never_exceeds_bond \
					--harness kani_slash_bps_monotone \
					--harness kani_tier_threshold_monotone \
					--harness kani_tier_lookup_in_range \
					--harness kani_bond_never_negative" ;; \
			proof_registry) \
				harnesses=" \
					--harness kani_slash_percentage_never_negative \
					--harness kani_slash_percentage_never_exceeds_bond \
					--harness kani_slash_percentage_monotone \
					--harness kani_bond_never_negative \
					--harness kani_bond_addition_no_overflow" ;; \
			*) \
				harnesses="" ;; \
		esac; \
		( cd $$crate && cargo kani $$harnesses ); \
	done

# ── Property-based tests (issue #544) ───────────────────────────────────────

.PHONY: proptest
proptest:  ## Run property-based tests across all Kani-covered crates
	@set -e; \
	for crate in $(KANI_CRATES); do \
		echo "==> cargo test --features testutils in $$crate"; \
		( cd $$crate && cargo test --features testutils ); \
	done

# ── Help ──────────────────────────────────────────────────────────────────────

.PHONY: help
help:  ## Show this help message
	@echo "Usage: make <target>"
	@echo ""
	@echo "Targets:"
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*##"}; {printf "  %-20s %s\n", $$1, $$2}'
	@echo ""
	@echo "Kani-covered crates (KANI_CRATES): $(KANI_CRATES)"
