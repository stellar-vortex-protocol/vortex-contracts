# Admin Action Audit Trail — Approach & Query Patterns

**Issue:** [#123](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/123)  
**Branch:** `feat/protocol-fee-cap`  
**Status:** Design complete

---

## 1. Problem Statement

Every admin action in `intent_settlement` emits a Soroban contract event, but
there is no single documented approach for reconstructing a complete admin
action history.  This makes governance transparency and post-incident review
harder than necessary: a security researcher, DAO voter, or on-call engineer
has to rediscover the query pattern each time.

This document defines:

1. Which events constitute the "admin audit trail".
2. The recommended indexer query pattern to reconstruct that trail.
3. A reference event schema for each admin event.
4. Integration guidance for monitoring and alerting.

---

## 2. Admin Event Inventory

The following events are emitted exclusively by admin-gated functions (those
that call `require_admin` internally).  Non-admin events (solver registration,
intent lifecycle) are excluded.

| Event topic | Emitted by | Data payload | Notes |
|---|---|---|---|
| `fee_recipient_proposed` | `propose_fee_recipient` | `new_fee_recipient: Address` | Two-step handover — proposal only; fee routing unchanged until accepted |
| `fee_recipient_updated` | `accept_fee_recipient` | `new_fee_recipient: Address` | Fee routing is live from this ledger onward |
| `admin_transferred` | `transfer_admin` | `new_admin: Address` | Both old and new admin must sign; effective immediately |
| `config_updated` | `set_config` | `(min_bond: i128, fill_window: u64, intent_expiry: u64, protocol_fee_bps: i128)` | All four params written atomically; cap enforcement: `protocol_fee_bps ≤ MAX_PROTOCOL_FEE_BPS` (1 000 bps / 10%) |
| `dst_token_allowed` | `add_allowed_dst_token` | `token: Address` | SEP-41 probe runs before storage write |
| `dst_token_disallowed` | `remove_allowed_dst_token` | `token: Address` | Immediate: intents targeting this token will fail once enforcement is on |
| `bond_multiplier_set` | `set_min_bond_multiplier` | `(token: Address, multiplier: i128)` | `multiplier` is fixed-point ×10 (10 = 1.0×, 15 = 1.5×) |
| `src_chain_allowed` | `add_allowed_src_chain` | `chain: String` | e.g. `"ethereum"`, `"base"` |
| `src_chain_disallowed` | `remove_allowed_src_chain` | `chain: String` | |
| `paused` | `pause` / `unpause` | `true` (paused) or `false` (unpaused) | Same topic for both; distinguish by payload value |
| `tokens_rescued` | `rescue_tokens` | `(token: Address, amount: i128)` with `to: Address` in the second topic slot | Only non-bond tokens; bond token rescue is blocked |

---

## 3. Recommended Indexer Query Pattern

Soroban events are indexed by Horizon and any Soroban-compatible indexer
(e.g. [Stellar Ecosystem Proposal 38 — event streaming](https://stellar.org/blog)).
The recommended approach is a **dedicated indexer view** that filters on:

1. `contract_id` — the deployed `intent_settlement` contract address.
2. The topic strings from §2 — use an `OR` filter across all admin topic names
   to build a single unified admin event stream.

### 3.1 Horizon REST query

```bash
# Fetch all contract events for the past N ledgers; post-filter by topic.
# Replace CONTRACT_ID and CURSOR with your values.

curl "https://horizon.stellar.org/contracts/CONTRACT_ID/events\
?order=asc\
&cursor=CURSOR\
&limit=200" | jq '
  .._embedded.records[]
  | select(.type == "contract")
  | select(
      .topic[0] == "\"fee_recipient_proposed\""   or
      .topic[0] == "\"fee_recipient_updated\""    or
      .topic[0] == "\"admin_transferred\""        or
      .topic[0] == "\"config_updated\""           or
      .topic[0] == "\"dst_token_allowed\""        or
      .topic[0] == "\"dst_token_disallowed\""     or
      .topic[0] == "\"bond_multiplier_set\""      or
      .topic[0] == "\"src_chain_allowed\""        or
      .topic[0] == "\"src_chain_disallowed\""     or
      .topic[0] == "\"paused\""                   or
      .topic[0] == "\"tokens_rescued\""
    )
  | {ledger: .ledger, tx: .transaction_hash, topic: .topic[0], value: .value}
'
```

### 3.2 Soroban RPC (`getEvents`)

The Soroban RPC method `getEvents` supports topic-based filtering directly,
which is more efficient than filtering after the fact:

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "getEvents",
  "params": {
    "startLedger": 1000000,
    "filters": [
      {
        "type": "contract",
        "contractIds": ["CONTRACT_ID"],
        "topics": [
          // One filter object per admin event topic.
          // Soroban RPC matches events where topic[0] equals the symbol.
          ["AAAADwAAABRmZWVfcmVjaXBpZW50X3Byb3Bvc2Vk"],  // fee_recipient_proposed
          ["AAAADwAAABNmZWVfcmVjaXBpZW50X3VwZGF0ZWQ="],  // fee_recipient_updated
          ["AAAADwAAABFhZG1pbl90cmFuc2ZlcnJlZA=="],       // admin_transferred
          ["AAAADwAAAA5jb25maWdfdXBkYXRlZA=="],            // config_updated
          ["AAAADwAAABFkc3RfdG9rZW5fYWxsb3dlZA=="],        // dst_token_allowed
          ["AAAADwAAABNkc3RfdG9rZW5fZGlzYWxsb3dlZA=="],   // dst_token_disallowed
          ["AAAADwAAABNib25kX211bHRpcGxpZXJfc2V0"],        // bond_multiplier_set
          ["AAAADwAAABFzcmNfY2hhaW5fYWxsb3dlZA=="],        // src_chain_allowed
          ["AAAADwAAABNzcmNfY2hhaW5fZGlzYWxsb3dlZA=="],   // src_chain_disallowed
          ["AAAADwAAAAZwYXVzZWQ="],                         // paused
          ["AAAADwAAAA90b2tlbnNfcmVzY3VlZA=="]             // tokens_rescued
        ]
      }
    ],
    "pagination": { "limit": 200 }
  }
}
```

> **Note on base64 encoding:** Soroban event topic filters use XDR-encoded
> `ScVal` values.  The strings above are base64 XDR for `ScVal::Symbol`.  Use
> the Stellar SDK's `xdr.ScVal.scvSymbol("fee_recipient_proposed")` helper (or
> equivalent in your language) to produce the correct encoding for each topic.

### 3.3 Dedicated indexer view (recommended for production)

For ongoing governance monitoring, run a lightweight indexer process (e.g. a
cron job or a Stellar Ecosystem Proposal 38 streaming subscriber) that:

1. Subscribes to the contract's event stream starting from the deployment ledger.
2. Writes every event matching the topic list above into a relational table:

```sql
CREATE TABLE admin_events (
    id            BIGSERIAL PRIMARY KEY,
    ledger        BIGINT    NOT NULL,
    tx_hash       TEXT      NOT NULL,
    timestamp     TIMESTAMPTZ NOT NULL,
    topic         TEXT      NOT NULL,  -- e.g. 'config_updated'
    payload       JSONB     NOT NULL,  -- decoded event value
    contract_id   TEXT      NOT NULL
);

CREATE INDEX ON admin_events (topic, ledger);
CREATE INDEX ON admin_events (ledger);
```

3. Expose a simple API or dashboard that shows the full chronological admin
   action history, grouped by topic.

This table becomes the single source of truth for governance review, post-
incident analysis, and audits.

---

## 4. Post-Incident Reconstruction Guide

After an incident (e.g. unexpected pause, fee routing change, config drift),
follow this sequence to reconstruct what happened:

1. **Identify the deployment ledger** — note the ledger at which
   `initialize` was first called (check the first `admin_transferred` or any
   admin event in the stream; alternatively, read `DataKey::Admin` from
   contract state via `get_admin`).

2. **Pull all admin events** from deployment ledger to the incident ledger
   using the Horizon or RPC query in §3.

3. **Order chronologically** by ledger number (ascending).  Within a single
   ledger, event order matches the transaction order in the ledger close.

4. **Reconstruct effective state** at any ledger by replaying events in order:
   - `admin_transferred` → new admin address
   - `fee_recipient_proposed` + `fee_recipient_updated` → current fee recipient
   - `config_updated` → effective `ProtocolConfig` (min_bond, fill_window,
     intent_expiry, protocol_fee_bps)
   - `dst_token_allowed` / `dst_token_disallowed` → allowlist membership
   - `src_chain_allowed` / `src_chain_disallowed` → src chain allowlist
   - `paused` (value=true) / `paused` (value=false) → pause state
   - `tokens_rescued` → list of non-bond token extractions with amounts

5. **Cross-check** the reconstructed state against the on-chain state at the
   incident ledger using `get_config`, `get_admin`, `get_fee_recipient`, and
   `is_paused`.  Any divergence indicates a missing event (possible ledger gap
   in the indexer) or a bug in the reconstruction logic.

---

## 5. Monitoring & Alerting Recommendations

The following events should trigger immediate alerts in a production monitoring
system:

| Event | Alert severity | Reason |
|---|---|---|
| `admin_transferred` | 🔴 Critical | Full admin control has changed hands |
| `fee_recipient_updated` | 🔴 Critical | Protocol fee revenue routing changed |
| `fee_recipient_proposed` | 🟡 Warning | Pending fee routing change — watch for accept |
| `paused` (value=true) | 🔴 Critical | Protocol halted — all new intents/fills blocked |
| `tokens_rescued` | 🟡 Warning | Tokens removed from contract; verify not bond leakage |
| `config_updated` with `protocol_fee_bps` change | 🟡 Warning | Fee rate changed; verify within governance-approved range |
| `config_updated` with `min_bond` decrease | 🟡 Warning | Lower collateral requirement may attract undercollateralised solvers |

Events that are expected during routine admin operations (no alert needed unless
outside a maintenance window):

- `dst_token_allowed` / `dst_token_disallowed`
- `src_chain_allowed` / `src_chain_disallowed`
- `bond_multiplier_set`
- `paused` (value=false) — resuming after planned pause

---

## 6. No New On-Chain State Required

This approach deliberately avoids adding any new storage to the contract.
All admin events are already emitted; the only gap was a documented,
consistent query pattern for consuming them.  Advantages:

- Zero gas cost increase for admin operations.
- No storage migration needed for existing deployments.
- Events are already immutable once the ledger closes — the audit trail
  cannot be tampered with after the fact.
- Any off-chain consumer (indexer, dashboard, alerting system) can be
  built or replaced independently of the contract.

---

## 7. Related Documents

- [docs/114-multisig-admin-design.md](./114-multisig-admin-design.md) — how to
  use Stellar native multi-sig for the admin address (no contract changes needed).
- [docs/pre-deploy-security-checklist.md](./pre-deploy-security-checklist.md) —
  pre-launch actions including enabling the dst-token and src-chain allowlists.
- [SECURITY.md](../SECURITY.md) — threat model for `intent_settlement`.
