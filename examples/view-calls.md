# Read-only view call collection

`view-calls.postman_collection.json` contains one example Stellar RPC
`simulateTransaction` request for every read-only view in
`intent_settlement/src/lib.rs`:

- `get_admin`
- `get_arbiter`
- `get_best_bid`
- `get_bond_token`
- `get_bond_token_min`
- `get_config`
- `get_effective_intent_state`
- `get_fee_recipient`
- `get_intent`
- `get_max_active_intents_per_solver`
- `get_min_bond_multiplier`
- `get_pauser`
- `get_pending_admin`
- `get_pending_fee_recipient`
- `get_pending_upgrade`
- `get_proof_registry`
- `get_protocol_health`
- `get_reputation_score`
- `get_solver`
- `get_solver_bond`
- `get_solver_bonds`
- `get_solver_count`
- `get_solver_intents`
- `get_solver_routes`
- `get_stats`
- `get_token_stats`
- `is_allowed_bond_token`
- `is_bid_window_enabled`
- `is_dst_allowlist_enabled`
- `is_dst_token_allowed`
- `is_paused`
- `is_solver_eligible`
- `is_src_chain_allowed`
- `is_src_chain_allowlist_enabled`
- `list_allowed_dst_tokens`
- `list_intents_by_user`
- `list_solvers`

## Import

1. Import `examples/view-calls.postman_collection.json` into Postman,
   Insomnia, or any tool that accepts Postman v2.1 collections.
2. Set collection variables:
   - `rpc_url` — for example `https://soroban-testnet.stellar.org`
   - `contract_id` — deployed `intent_settlement` contract ID
   - `source_account` — public key used to build unsigned simulation
     transactions
3. Replace the request-specific `*_tx_xdr` variable with a transaction XDR for
   that view call.

## Generating transaction XDR

Build each XDR with the Stellar CLI for the target function and arguments, then
copy the generated transaction XDR into the matching collection variable.

Example:

```bash
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source "$SOURCE_ACCOUNT" \
  --network testnet -- \
  is_solver_eligible \
  --solver "$SOLVER_ADDRESS"
```

The collection request then submits that XDR to Stellar RPC's
`simulateTransaction` method, which is the read-only path used for contract view
calls.
