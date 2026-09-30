# Vortex Protocol Proof-of-Reserves Dashboard

## Overview

The Proof-of-Reserves Dashboard is a public, continuously-updated verification tool that reconciles on-chain observable protocol assets against `SECURITY.md`'s Assets-at-Risk table. It provides the community with an ongoing, verifiable answer to: **"Is the collateral this protocol's security model depends on actually present?"**

This dashboard is a **transparency artifact**, not a guarantee or insurance claim. It displays what the protocol's economic security depends on (solver bond collateral) and tracks whether those assets remain on-chain. It complements issue #40's governance-proposal dashboard (which tracks pending votes) with a complementary focus on fund reconciliation.

---

## Data Sources & Methodology

### Assets Tracked

The dashboard displays the following metrics, each labeled against the corresponding row in `SECURITY.md`'s Assets-at-Risk table:

| Asset | On-Chain Source | Update Frequency | Status |
|-------|---|---|---|
| **Total Bonded Collateral** | `get_protocol_health()` contract call, or enumeration via `list_solvers()` | Every 5 minutes | Live |
| **Total Solver Count** | `get_solver_count()` contract call | Every 5 minutes | Live |
| **Average Bond per Solver** | Total bonded ÷ solver count | Computed every 5 minutes | Live |
| **Min Bond Threshold** | `MIN_BOND` constant (50 USDC) | Static in contract | Fixed |
| **Open Intent Volume at Risk** | Sum of `src_amount` for all intents in `Open` or `Accepted` state; queried via event replay or `list_intents()` (once implemented) | Every 5 minutes | Live |
| **Treasury Balance** | `get_treasury()` contract call (once issue #37 deploys) | Every 5 minutes | Pending issue #37 |
| **Total Protocol Fees Collected** | Cumulative sum from `fee_collected` events | Every 5 minutes | Live |

### Data Refresh Mechanism

The dashboard's data is refreshed via:

1. **On-chain contract queries** (primary):
   - `get_protocol_health()` — returns aggregate protocol stats (bonds, active solvers, etc.) if implemented, or fallback to enumeration
   - `get_solver_count()` — number of registered solvers
   - Event replay from `intent_submitted`, `solver_slashed`, `fill_intent` events to derive intent state distribution and volume at risk

2. **Fallback: Enumeration via `list_solvers()`** (if `get_protocol_health()` not available):
   - Paginate through all registered solvers
   - Sum `bond_amount` for each
   - Compute min/max/avg bond per solver

3. **Update frequency**: Every 5 minutes (or admin-configurable)

### Data Freshness & Caveats

- **Ledger lag**: Numbers reflect contract state as of the last finalized Stellar ledger (typically 2–5 seconds old)
- **Intent state derivation**: Computing open intent volume requires replaying events or querying a full intent list. Until a `get_open_intent_volume()` contract function exists, this is derived via event replay and is subject to indexer latency
- **Stale solvers**: A solver who has partially withdrawn their bond but not fully deregistered is included in "active solvers" but their bond is lower than historical state; the total bonded remains accurate
- **Treasury balance**: Pending issue #37's deployment; will be added once the on-chain treasury contract exists

---

## Reconciliation Logic

The dashboard performs a **continuous reconciliation** against the expected values in `SECURITY.md`:

### Asset Table Reconciliation

```
SECURITY.md States:
  Solver bonds: ≥ 50 USDC per solver

Dashboard Displays:
  ✓ Total bonded collateral
  ✓ Number of solvers
  ✓ Average bond per solver
  ✓ Min/max bond (to detect any solver below MIN_BOND)

Verification:
  average_bond = total_bonded / solver_count
  assertion: average_bond ≥ MIN_BOND (50 USDC)
  
  Also display any outliers:
  - Solvers with bond < MIN_BOND (should be none; they should be deactivated)
  - Solvers with extremely small bonds relative to fills (risk flagging)
```

### Intent Volume at Risk

```
SECURITY.md implies:
  User swap output and open intents are at risk if a solver defaults

Dashboard Displays:
  ✓ Total open intent volume (sum of src_amount for Open+Accepted intents)
  ✓ Total filled volume (historical)
  ✓ Active fill rate (filled intents per day)

Verification:
  Solver bonds are the economic deterrent for open volume at risk.
  Assert: total_bonded > 0 and > threshold as % of open volume
  
  Alert if: (total_bonded / open_volume) < 0.1 (example threshold)
  This would indicate insufficient collateral to back open positions
```

### Treasury Balance (Once #37 Ships)

```
SECURITY.md future state (issue #37):
  Treasury can back solver bonds, fund ecosystem grants, or accrue governance

Dashboard will display:
  ✓ Treasury balance in USDC
  ✓ Allocation of treasury funds (if governance specifies buckets)
  ✓ Historical treasury inflows/outflows
```

---

## Dashboard Presentation

### Public-Facing Display

The dashboard is published at a stable URL (TBD; e.g., `https://vortex-protocol.github.io/proof-of-reserves/` or similar).

It displays:

1. **Key Metrics** (top-level summary cards):
   - Total Bonded Collateral (USDC)
   - Number of Active Solvers
   - Average Bond per Solver (USDC)
   - Total Open Intent Volume (USDC, notional at submission)

2. **Trend Charts**:
   - Bonded collateral over time (past 30 days)
   - Solver count over time
   - Open intent volume over time
   - Protocol fees collected over time

3. **Data Source & Reproducibility** (in footer or "About" section):
   ```
   This dashboard's data comes from querying:
   - Contract: [CONTRACT_ID]
   - Network: [testnet | mainnet | custom]
   - RPC: [RPC_ENDPOINT]
   - Refresh interval: every 5 minutes
   - Last updated: [TIMESTAMP]
   
   To verify these numbers independently, run:
   stellar contract invoke --id <CONTRACT_ID> --source <ANY_KEY> --network <NETWORK> -- \
     get_protocol_health
   ```

4. **Honesty & Caveats** (prominent):
   - "This dashboard verifies on-chain collateral totals; it is **not** insurance or a guarantee"
   - "Solver default risk remains: a bond slash covers only 10% of a missed fill; users bear residual risk"
   - "This dashboard's staleness: data is as recent as the last RPC query (typically 5 min lag)"
   - "Open intent volume is computed; true volume-at-risk may differ if intents are filled between query time and display"

5. **Navigation & Help**:
   - Link to `SECURITY.md` for threat model context
   - Link to `docs/110-monitoring-alerting-spec.md` for technical details
   - Link to source data (GitHub issue #308, if still open for feedback)

---

## Implementation & Hosting

### Architecture

The dashboard is implemented as:

1. **Static HTML/CSS/JS** (for simplicity and low operational overhead):
   - Single `index.html` file with embedded CSS and JavaScript
   - Fetches live data from the Stellar RPC at page load and every 5 minutes
   - No backend required (can be hosted on GitHub Pages, Vercel, or similar)

2. **Data Fetching Script** (optional, for archival/historical data):
   - A Node.js script that runs periodically (e.g., via GitHub Actions)
   - Fetches contract state, stores historical snapshots in JSON
   - Commits snapshots to the repository for audit trail
   - Serves as the source of truth for historical charts

3. **Deployment**:
   - Repository: `vortex-protocol/vortex-contracts` (this repo) or a dedicated dashboard repo
   - Folder: `dashboard/` or `web/proof-of-reserves/`
   - Hosting: GitHub Pages (automatic from `main` branch) or Vercel
   - DNS: Custom domain (e.g., `reserves.vortex-protocol.org`) or GitHub Pages URL

### Update Mechanism

**Option 1: Client-side queries (live, no backend)**
- Page loads and immediately fetches data from Stellar RPC
- Refreshes every 5 minutes by re-querying the contract
- **Pros**: No server to operate, data is always current
- **Cons**: RPC rate-limits, single RPC failure = dashboard fails
- **Recommended for**: Initial launch

**Option 2: Server-side caching + GitHub Actions (robust, auditable)**
- GitHub Actions workflow runs every 5 minutes
- Fetches contract state, appends to `data/history.json`
- Static page reads from `data/history.json`
- **Pros**: Audit trail, no RPC rate-limit risk, reliable
- **Cons**: 5-minute lag is maximum (acceptable for proof-of-reserves use case)
- **Recommended for**: Long-term production

**Option 3: Hybrid (best of both)**
- GitHub Actions updates historical data (committed to repo)
- Page shows "current" data (live query) and "last updated" timestamp
- Falls back to GitHub-cached data if live query fails
- **Pros**: Current + resilient, audit trail maintained
- **Cons**: Slightly more complex

### Example Implementation

See `dashboard/index.html` in this repository for a reference implementation. It includes:

```html
<!DOCTYPE html>
<html>
<head>
  <title>Vortex Protocol — Proof of Reserves</title>
  <style>
    /* Responsive design, light/dark theme support */
  </style>
</head>
<body>
  <h1>Vortex Protocol Proof of Reserves</h1>
  
  <div class="metrics">
    <div class="card">
      <h3>Total Bonded Collateral</h3>
      <p class="value" id="total-bonded">Loading...</p>
      <p class="unit">USDC</p>
    </div>
    <!-- More cards for other metrics -->
  </div>
  
  <div class="charts">
    <div id="trend-chart"><!-- Chart goes here --></div>
  </div>
  
  <footer>
    <p>Data source: <code>[CONTRACT_ID]</code> on <code>[NETWORK]</code></p>
    <p>Last updated: <span id="last-updated">—</span></p>
    <p><a href="./SECURITY.md">Threat Model</a> | <a href="./docs/110-monitoring-alerting-spec.md">Technical Spec</a></p>
  </footer>
  
  <script>
    // Fetch and display data
    async function fetchData() {
      const contractId = '[CONTRACT_ID]';
      const network = '[NETWORK]';
      
      // Query get_protocol_health() or enumerate solvers
      // Update DOM with results
      // Schedule next refresh in 5 minutes
    }
    
    fetchData();
    setInterval(fetchData, 5 * 60 * 1000);
  </script>
</body>
</html>
```

---

## Testing & Validation

### Manual Verification (Testnet)

1. Query the contract directly:
   ```bash
   stellar contract invoke --id <CONTRACT_ID> --source <ANY_KEY> --network testnet -- \
     get_protocol_health
   ```

2. Compare dashboard output to RPC output (should match exactly)

3. Manually adjust a solver's bond (via `withdraw_bond` or `slash_solver`) and verify the dashboard updates within 5 minutes

### Automated Testing (CI)

- Unit tests for data-fetching logic (parsing contract responses, computing aggregates)
- Integration test: spin up a test contract instance, populate with known solver/intent state, query dashboard, assert expected output
- Regression test: verify historical data in `data/history.json` remains consistent after code changes

---

## Maintenance & Monitoring

### Operational Checklist

- [ ] Dashboard data updates automatically every 5 minutes
- [ ] Last-updated timestamp is displayed and accurate
- [ ] No errors in browser console (check weekly)
- [ ] RPC endpoint is responsive and not rate-limited
- [ ] Historical data is committed to repository (audit trail maintained)

### Alert Conditions

Set up monitoring to flag:

- Data fetch failures (RPC not responding, contract not found)
- Anomalous values (total_bonded drops >10% in one update, solver_count is negative, etc.)
- Display staleness (dashboard shows data >15 minutes old)

---

## FAQ

**Q: Is this dashboard a guarantee that my funds are safe?**

A: No. This dashboard verifies that solver bond collateral is on-chain, which is the primary economic deterrent against solver default. But a solver can still accept an intent, miss the fill window, absorb the slash, and leave you with no recourse if the source-chain spread outweighs their loss. Read `SECURITY.md` for the full threat model.

**Q: Why is the open intent volume only updated every 5 minutes?**

A: Computing open intent volume requires either replaying all events (slow) or querying a full intent list. Once the contract provides a `get_open_intent_volume()` function, we'll update to a faster cadence.

**Q: What if a solver's bond is slashed? Will the dashboard show it?**

A: Yes. When `slash_solver` is called, `bond_amount` decreases immediately. The dashboard will show the updated total within 5 minutes.

**Q: Can I fork this dashboard for another Stellar protocol?**

A: Absolutely. The code is designed to be generic; change the contract ID, RPC endpoint, and asset table, and it should work for any Soroban contract.

**Q: Who maintains this dashboard?**

A: The vortex-protocol team initially. The code is open-source; external contributions are welcome. If maintenance becomes a burden, the community can take over or migrate to a community-hosted instance.

---

## Related Issues & References

- **Issue #37**: Treasury contract design (future enhancement: display treasury balance)
- **Issue #40**: Governance proposal dashboard (complementary; tracks pending votes, not reserves)
- **Issue #46**: `TotalBonded` aggregate counter (preferred data source; eliminates enumeration lag)
- **Issue #110**: Monitoring and alerting spec (technical foundation for dashboard data fetching)
- **SECURITY.md**: Assets-at-Risk table (this dashboard's source of truth for what to display)

