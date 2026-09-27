# Contributing to vortex-contracts

This document covers everything specific to contributing to *this* repository:
toolchain setup, project structure, code conventions, and how to run the test
suite. For general process (issue triage, PR etiquette, code of conduct), see
the org-wide
[CONTRIBUTING.md](https://github.com/vortex-protocol/.github/blob/main/CONTRIBUTING.md).

---

## Table of Contents

1. [Toolchain Setup](#toolchain-setup)
2. [Project Structure](#project-structure)
3. [Build](#build)
4. [Testing](#testing)
5. [Linting and Formatting](#linting-and-formatting)
6. [Dependency Auditing](#dependency-auditing)
7. [Code Conventions](#code-conventions)
8. [Submitting a PR](#submitting-a-pr)

---

## Toolchain Setup

### Rust

Install Rust via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

This project requires **Rust 1.78 or later**. Check your version:

```bash
rustc --version
```

### wasm32 target

Soroban contracts compile to WebAssembly. Add the target:

```bash
rustup target add wasm32-unknown-unknown
```

### Stellar CLI

Install the [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli),
which is required for `stellar contract build` and deployment:

```bash
cargo install --locked stellar-cli --features opt
```

Verify the install:

```bash
stellar --version
```

### cargo-audit (optional but recommended)

The CI dependency-audit job runs `cargo audit`. Install it locally to catch
advisories before pushing:

```bash
cargo install --locked cargo-audit
```

---

## Project Structure

```
vortex-contracts/
├── intent_settlement/       # The deployed Soroban contract
│   ├── Cargo.toml
│   ├── Cargo.lock
│   └── src/
│       ├── lib.rs           # All contract logic, types, and errors
│       └── test.rs          # soroban_sdk testutils test suite
├── docs/                    # Extended documentation
│   ├── solver-integration-guide.md
│   ├── mainnet-deployment-runbook.md
│   ├── ttl-constants-rationale.md
│   └── CONTRIBUTING.md      # This file
├── README.md
├── CHANGELOG.md
└── .github/
    └── workflows/
        └── ci.yml
```

The entire on-chain logic lives in `intent_settlement/src/lib.rs`. There are no
additional modules — keep it that way unless a refactor is explicitly discussed
and agreed on in an issue first.

---

## Build

```bash
cd intent_settlement

stellar contract build
```

The optimized wasm artifact is written to:

```
intent_settlement/target/wasm32-unknown-unknown/release/vortex_intent_settlement.wasm
```

`stellar contract build` runs `cargo build --target wasm32-unknown-unknown
--release` under the hood and applies wasm-opt automatically.

---

## Testing

Tests live in `intent_settlement/src/test.rs` and use `soroban_sdk`'s
`testutils` feature to run a simulated Soroban environment in-process — no
network or deployed contract required.

```bash
cd intent_settlement
cargo test
```

Run a single test by name:

```bash
cargo test test_fill_intent
```

Run with output visible (useful when debugging):

```bash
cargo test -- --nocapture
```

### What the test suite covers

- Full intent lifecycle: `submit → accept → fill`
- `cancel_intent` (open intents only)
- `expire_intent` (permissionless expiry)
- `slash_solver` (missed fill window, bond deduction, re-open)
- Bond deactivation when post-slash bond drops below `MIN_BOND`
- `register_solver` top-up and `withdraw_bond`
- `deregister_solver` with and without active intents
- Admin controls: `set_fee_recipient`, `transfer_admin`
- Pause/unpause and gated functions
- Destination token allowlist enforcement
- Storage TTL management (instance and persistent)
- All relevant error paths

When adding a new entrypoint or changing existing behavior, add or update a test
that exercises the new code path. PRs that change logic without a corresponding
test change will be asked to add coverage.

---

## Linting and Formatting

All of these must pass cleanly before a PR is merged. Run them locally before
pushing:

```bash
cd intent_settlement

# Format (edits in place)
cargo fmt --all

# Lint (must produce zero warnings)
cargo clippy --all-targets -- -D warnings
```

The CI workflow runs both with the same flags; a `clippy` warning that is
suppressed locally with `#[allow(...)]` must include a comment explaining why
the suppression is intentional and safe.

---

## Dependency Auditing

```bash
cd intent_settlement
cargo audit
```

This checks `Cargo.lock` against the [RustSec advisory database](https://rustsec.org/).
Any unresolved `error`-level advisory will fail CI. If you add a dependency,
run `cargo audit` before pushing.

When upgrading a dependency to resolve an advisory, note the advisory ID in
the CHANGELOG entry.

---

## Code Conventions

### `#![no_std]`

The contract uses `#![no_std]` (required for wasm32 targets). Do not add any
crate that pulls in `std`; use `soroban_sdk` types (`String`, `Vec`, `Map`,
`Bytes`, etc.) instead of their `std` equivalents.

### Error variants

All errors are defined in the `Error` enum with explicit `#[repr(u32)]`
discriminants. When adding a new error:

1. Append it at the end of the enum — do not renumber existing variants.
2. Add a comment explaining the condition that triggers it.
3. Update the relevant section of this document or the integration guide if the
   error is user-facing.

### Storage keys

All storage keys are variants of the `DataKey` enum. Do not store anything
directly under a raw string or bytes key.

### TTL bumping

Every function that writes to persistent storage must call the appropriate
`bump_*_ttl` helper (`bump_intent_ttl`, `bump_solver_ttl`). Every public
function must call `bump_instance_ttl`. See
[`docs/ttl-constants-rationale.md`](./ttl-constants-rationale.md) for why.

### Events

Every state transition emits an event. New entrypoints should follow the
existing pattern:

```rust
env.events().publish(
    (Symbol::new(&env, "event_name"), actor_address),
    payload_value,
);
```

Event topic and payload shapes are documented in the
[Solver Integration Guide](./solver-integration-guide.md#event-topics).

### Rustdoc

Public functions must have rustdoc comments explaining their behavior,
preconditions, and authorization requirements. Internal helpers (prefixed with
`fn`, not `pub fn`) should have inline comments for anything non-obvious.

---

## Submitting a PR

1. Fork the repo and create a branch from `main`:
   ```bash
   git checkout -b <type>/<short-description>
   ```
   Use `fix/`, `feat/`, or `docs/` prefixes to match CI branch naming.

2. Make your changes, then run the full check suite locally:
   ```bash
   cd intent_settlement
   cargo fmt --all
   cargo clippy --all-targets -- -D warnings
   cargo test
   stellar contract build
   cargo audit
   ```

3. Commit with a conventional commit message:
   ```
   <type>: <short summary in imperative mood>
   ```
   Examples: `fix: prevent bond withdrawal while intents are active`,
   `feat: add solver reputation score view`, `docs: expand TTL rationale`.

4. Open a PR against `main`. The description must include:
   - A summary of what changed and why.
   - `Closes #<issue-number>` for every issue the PR resolves.
   - Notes on anything that could not be tested (e.g., mainnet-only behavior).

5. All CI jobs must be green before merge:
   - `fmt` — `cargo fmt --check`
   - `clippy` — zero warnings
   - `test` — all tests pass
   - `build` — wasm artifact produced
   - `audit` — no unresolved advisories
# Contributing to Vortex Contracts

Thank you for contributing! This document covers the day-to-day workflow for
**contributors** and includes a dedicated [Maintainer Guide](#maintainer-guide)
section covering CI, branch protection, and required-check management.

---

## Contributor Quick-start

### Prerequisites

- Rust 1.78+ with the `wasm32-unknown-unknown` target
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli)
- [GNU Make](https://www.gnu.org/software/make/) (optional but recommended)
- [`just`](https://just.systems/) (optional alternative to Make)

### Local development commands

A `Makefile` (and equivalent `justfile`) is provided at the repo root so you
never need to copy-paste raw multi-flag commands from the README.

```bash
# inside the repo root
make fmt        # cargo fmt --all
make lint       # cargo clippy --all-targets -- -D warnings   ← same as CI
make test       # cargo test
make build      # cargo build --target wasm32-unknown-unknown --release
make all        # fmt + lint + test + build (full pre-push check)
make deploy-testnet   # stellar contract deploy … --network testnet
```

See [`Makefile`](./Makefile) and [`justfile`](./justfile) for the full list of
targets, or run `make help` / `just --list`.

### Pre-push checklist

Before opening a PR, run `make all` (or its `just` equivalent) and confirm:

1. `cargo fmt --all -- --check` exits 0
2. `cargo clippy --all-targets -- -D warnings` exits 0 (no warnings allowed)
3. `cargo test` passes
4. The wasm binary builds cleanly

---

## Maintainer Guide

This section is for maintainers with write access to the repository. It
documents which CI jobs are **required** for merging and how to keep that
configuration correct as the workflow grows.

### Current CI jobs and required-check status

| Job name (as reported by GitHub) | Workflow file | Required to merge? |
|---|---|---|
| `Contract (stable)` | `ci.yml` / `contract` matrix leg | ✅ Yes |
| `Contract (1.78)` | `ci.yml` / `contract` matrix leg | ✅ Yes |
| `Dependency audit` | `ci.yml` / `audit` | ✅ Yes |

> **Note:** Matrix jobs are reported to GitHub as `<job.name> (<matrix value>)`.
> The exact strings you must enter in the branch-protection UI are
> `Contract (stable)` and `Contract (1.78)`.

Each `contract` matrix leg runs:

1. `cargo fmt --all -- --check` (stable leg only)
2. `cargo clippy --all-targets -- -D warnings` — **identical** to the command
   documented in README.md; `-D warnings` is enforced on every leg
3. `cargo test`
4. `cargo build --target wasm32-unknown-unknown --release`

### Verifying clippy `-D warnings` is enforced

1. Open `.github/workflows/ci.yml`.
2. Find the `Clippy` step inside the `contract` job.
3. Confirm the `run:` value is exactly:
   ```
   cargo clippy --all-targets -- -D warnings
   ```
   Any relaxation (e.g. dropping `-D warnings`, adding `--allow …`) must be
   reviewed and approved by a second maintainer before merging.

### How to confirm a required-check is actually enforced (GitHub UI)

1. Go to **Settings → Branches** on the GitHub repository page.
2. Click **Edit** next to the `main` branch rule (or create one if absent).
3. Enable **"Require status checks to pass before merging"**.
4. Enable **"Require branches to be up to date before merging"**.
5. In the search box, type the exact job names from the table above and select
   each one. If a job name doesn't appear in autocomplete, trigger a CI run on
   any open PR first — GitHub only indexes checks it has seen recently.
6. Save the rule.
7. To verify enforcement: open a test PR that intentionally fails one of the
   required checks and confirm the **Merge** button is blocked.

> **Tip — GitHub CLI alternative:**
> ```bash
> gh api repos/{owner}/{repo}/branches/main/protection \
>   --jq '.required_status_checks.contexts'
> ```
> This prints the list of currently required check names without needing the UI.

### How to update required checks when new CI jobs are added

When a new job is added to a workflow file (e.g. `wasm-size`, `coverage`):

1. Add a row to the table above with the correct job name, workflow, and
   proposed required status.
2. Add the new check name to branch protection (see steps above) **in the same
   PR** that introduces the workflow job — never after.
3. If the job is advisory-only (informational, not blocking), mark it as
   `❌ No (advisory)` in the table and add a comment in the workflow step
   explaining why it is advisory.

#### Proposed future required checks

The following jobs are under discussion or in the roadmap. Update this table
once they are merged:

| Job name | Workflow | Notes |
|---|---|---|
| `WASM size gate` | `ci.yml` (planned) | Blocks merges that grow the wasm by > N KB |
| `Coverage` | `coverage.yml` (planned) | Advisory until a baseline is established |

### GITHUB_TOKEN permission model

All jobs in `ci.yml` run under a **`permissions: contents: read`** top-level
policy. This is the minimum required: every job only needs to check out source
code.

**Why this matters:** Without an explicit `permissions:` block, GitHub applies
its default token scopes, which include `contents: write` for `push` events on
the same repository. A compromised third-party action (e.g. `Swatinem/rust-cache`,
`dtolnay/rust-toolchain`) or a malicious PR-triggered step would then have
write access to the repository — more privilege than any CI job here actually
needs.

**Per-job audit (checked 2026-08-31):**

| Job | What it does | Minimum scope |
|---|---|---|
| `fmt` | Checkout + `cargo fmt --check` | `contents: read` |
| `contract` | Checkout + clippy + test + (optional) fmt | `contents: read` |
| `wasm-size` | Checkout + build + write to `$GITHUB_STEP_SUMMARY` (local runner file, not a GitHub API call) | `contents: read` |
| `proptest` | Checkout + `cargo test` | `contents: read` |
| `audit` | Checkout + `cargo audit` (queries RustSec DB over HTTPS, not the GitHub API) | `contents: read` |
| `mutants` | Checkout + `cargo mutants` (mutates source in a runner-local temp copy) | `contents: read` |

**Adding a job that needs elevated scope:**

If a future job needs to post PR comments, push a commit, create a release, or
call any other GitHub API, add a **job-level** `permissions:` override with
only the additional scope that specific job requires. Do not widen the
workflow-level block:

```yaml
jobs:
  my-new-job:
    permissions:
      contents: read        # still needed for checkout
      pull-requests: write  # needed to post a PR comment — only grant here
```

See the [GitHub docs on workflow permissions](https://docs.github.com/en/actions/security-guides/automatic-token-authentication#permissions-for-the-github_token)
for the full list of available scopes.

### MSRV policy

The declared MSRV is **Rust 1.78** (see README.md). CI enforces this via a
matrix leg that runs on toolchain `1.78` alongside `stable`.

- If a dependency update or language feature requires bumping the MSRV, update
  both `ci.yml` (the matrix value) and README.md in the same PR, and note it
  in `CHANGELOG.md`.
- The MSRV leg intentionally skips `rustfmt` to avoid false failures from
  format-output changes across Rust versions; linting and testing still run on
  both legs.

---

## Code style

- Format: `cargo fmt --all`
- Lints: `cargo clippy --all-targets -- -D warnings` (zero warnings policy)
- Commit messages: conventional-commits style
  (`feat:`, `fix:`, `chore:`, `docs:`, `test:`, `refactor:`)

## Pull request checklist

- [ ] Branch is up to date with `main`
- [ ] All required CI checks pass
- [ ] PR description includes `Closes #<issue-number>`
- [ ] New public items have doc-comments
- [ ] `CHANGELOG.md` updated under `[Unreleased]`

## License

By contributing you agree your work will be licensed under the
[MIT License](./LICENSE).
