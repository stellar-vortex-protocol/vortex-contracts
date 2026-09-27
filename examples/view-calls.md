# Read-only view call collection

`view-calls.postman_collection.json` contains one example Stellar RPC
`simulateTransaction` request for every read-only view in
`intent_settlement/src/lib.rs`:

- `get_protocol_params`
- `get_intent`
- `get_solver`
- `get_reputation_score`
- `is_solver_eligible`
- `get_fee_recipient`
- `get_pending_fee_recipient`
- `get_bond_token`
- `get_admin`
- `get_stats`
- `get_min_bond`
- `list_intents_by_user`
- `get_solver_count`
- `get_protocol_health`

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
