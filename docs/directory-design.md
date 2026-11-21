# On-chain Address Book (`directory`) — Design Note (Issue #399 / #554)

## Problem
Contracts such as `intent_periphery` and `governor` hardcode peer addresses at
`initialize`, so there is no canonical, updatable lookup point for the
protocol's own contract addresses.

## Design
`directory` is a minimal key/value registry: `Symbol` name -> `Address`
(e.g. `intent_settlement`, `proof_registry`, `solver_registry`, `governor`,
`timelock`, `backstop_vault`, `fee_router`, `treasury_streams`,
`reputation_badge`, `intent_periphery`).

- **Writes** (`set_address`, `remove_address`) are admin-only. The admin is
  expected to be the `timelock`, so updates are governance-controlled.
- **Reads** (`get_address`, `find_address`, `list_names`) are permissionless.
- **Admin rotation** is two-step: `propose_admin` then `accept_admin`.
- **Storage**: `Admin`/`PendingAdmin`/`Names` in instance storage; each entry in
  persistent storage with the repo-standard TTL bump on write and read.
  A lookup loads the entry once (a single persistent `get`) and bumps its TTL only when found.
- **Events**: `addr_set` (name, address) and `addr_rm` (name).

## Out of scope
Migrating existing contracts to consult the directory; that lands as follow-ups.
