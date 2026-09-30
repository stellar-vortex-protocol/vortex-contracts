# proof_registry: declare the Axelar gateway and authorized-source keys (#381)

This PR delivers one acceptance-criteria item from #381. #382 and #383 are referenced so that they close with this PR, but nothing from them is implemented here.

## #381 Authenticate `receive_message_axelar` through the Axelar Gateway

**What existed:** `initialize` wrote `ProofKey::AxelarGateway`, and `receive_message_axelar` called `Self::get_authorized_axelar_source(..)`, but neither the key variant nor the helper existed on `main`. They were lost in merge debris, so these were 2 of the crate's compile errors. There was also no way for the admin to configure an Axelar source.

**Done:**
- `ProofKey::AxelarGateway` and `ProofKey::AuthorizedAxelarSource(Symbol)` declared.
- Admin setters `set_authorized_axelar_source(chain_name, source_address)` and `remove_authorized_axelar_source(chain_name)`. Both are admin-auth gated, bump the instance TTL, and emit `axelar_source_authorized` / `axelar_source_removed`.
- Getters `get_authorized_axelar_source(chain_name)` (the missing helper, now a contract entry point) and `get_axelar_gateway()`.
- Test fixture now passes a gateway to `initialize`, which takes 3 args on `main`. The existing tests still called it with 2.
- 9 new tests: gateway recorded at init; source set/get/unset/per-chain/remove; setters rejected without admin auth; `receive_message_axelar` accepts the configured source and rejects a wrong source and an unconfigured or removed chain.

**Not done in this PR:**
- `command_id` + `gateway.validate_message(..)` through a `contractclient` trait (the forged-call exploit is still open).
- Fail-closed behaviour when the gateway is unset, and a timelocked gateway setter.
- SECURITY.md advisory and CHANGELOG entry.

## #382 Replay protection and chain-identity binding on the Axelar path

**Not done in this PR:**
- `SeenAxelar(command_id)` replay key and its TTL.
- Axelar chain name to Wormhole chain id mapping with `EmitterChainMismatch`.
- Replacing the raw `payload.get(i)` calls with the checked `byte()` helper.
- `command_id` in `ProofRecord`, and the shared `store_proof` helper.
- docs/124 update.

## #383 Restore the `proof_registry` pause circuit breaker

**Not done in this PR:**
- `pause` / `unpause` with admin + guardian and per-bridge `ProofKey::Paused(bridge)`.
- Pause checks in both receive paths.
- Exported-spec regression test, pause matrix test, CHANGELOG, and the git-archaeology note. (The pause from 6cd974d was dropped by the merges f88a51b / 0ea03d3 / 6a3814c.)

## Verification

`proof_registry` does **not compile on `main`** (pre-existing merge debris, unrelated to this change). It is missing `PROOF_TTL_*` / `INSTANCE_TTL_*` constants and the `ChainIdOutOfRange` / `ProofStale` variants, `receive_message_axelar` casts `Option<u8>` with `as`, and the `#[cfg(feature = "testutils")]` mock fns break `#[contractimpl]`. Because `intent_settlement` depends on it, that crate fails too.

To test this change I applied a minimal repair of that debris locally (not part of this PR) and ran it with this commit on top:

- `cargo test --lib` in `proof_registry`: 28 passed, 0 failed (19 existing + 9 new).
- `cargo fmt --check` and `cargo clippy --all-targets`: no findings on lines this PR touches. The remaining fmt hunks and 3 clippy warnings are on pre-existing lines.
- Doctests: 6 pre-existing failures from the untagged ```` ``` ```` block in `receive_message_axelar`'s doc comment (not touched).

Closes #381
Closes #382
Closes #383
