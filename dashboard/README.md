# Proof of Reserves Dashboard

This directory contains the Vortex Protocol's public proof-of-reserves dashboard — a transparency tool for verifying that the protocol's on-chain collateral (solver bonds) matches what the security model depends on.

## Quick Start

1. **View the live dashboard**: Open `index.html` in a web browser
   - Or deploy to GitHub Pages, Vercel, or your preferred static host
   - Update `CONFIG.contractId`, `CONFIG.network`, and `CONFIG.rpcEndpoint` to point to your deployment

2. **Deploy to GitHub Pages** (automatic):
   ```bash
   # GitHub Actions will auto-deploy this directory on every push to main
   # Dashboard will be available at: https://stellar-vortex-protocol.github.io/vortex-contracts/dashboard/
   ```

3. **Deploy to Vercel or other host**:
   ```bash
   vercel deploy ./dashboard
   # or copy `dashboard/` to your static host
   ```

## Files

- **index.html**: Single-page dashboard with embedded CSS and JavaScript
  - No backend required; fetches live data from Stellar RPC
  - Responsive design; works on desktop and mobile
  - Refreshes data every 5 minutes

## Configuration

Edit `CONFIG` object in `index.html`:

```javascript
const CONFIG = {
  contractId: 'YOUR_CONTRACT_ID_HERE', // e.g., 'CA...xpz'
  network: 'testnet',                  // or 'mainnet'
  rpcEndpoint: 'https://soroban-testnet.stellar.org', // Stellar RPC
  refreshInterval: 5 * 60 * 1000,      // 5 minutes
};
```

## Data Sources

The dashboard queries:

1. **`get_protocol_health()`** — returns aggregate stats (bonds, solver count, etc.)
   - Fallback: **`list_solvers(start, limit)`** to enumerate and sum manually

2. **Event stream** (via RPC) — for intent-state distribution and volume at risk
   - Events: `intent_submitted`, `fill_intent`, `solver_slashed`

3. **`get_treasury()`** — for treasury balance (once issue #37 deploys)

4. **Module sections** (read-only; shown as `—` until the RPC data layer is wired)
   - **`backstop_vault.get_vault_state()`** — `(total_assets, total_shares, assets_per_share_bps)`
   - **`governor.get_proposal_count()`**, **`governor.list_proposals(start, limit)`**, and **`governor.state(id)`** — proposal totals and open proposals
   - **`treasury_streams.get_stream_count()`** — stream count; aggregate vested totals need the enumeration view (per-stream `balance_of`)
   - **`intent_periphery.get_protocol_health()`** and **`intent_periphery.get_stats()`** — proxied health and stats
   - `fee_router` and `intent_periphery`'s `get_dashboard_snapshot()` have no dashboard section yet

## Building & Development

### Local Testing

```bash
# Open in default browser
open dashboard/index.html

# Or use a local server (Python 3)
python3 -m http.server 8000
# Visit: http://localhost:8000/dashboard/
```

### For Production

1. Update `CONFIG` with real contract ID and RPC endpoint
2. Test against testnet first
3. Deploy static files to GitHub Pages, Vercel, or your CDN
4. Monitor for RPC failures and set up alerting

## Documentation

See [`docs/proof-of-reserves-dashboard.md`](../docs/proof-of-reserves-dashboard.md) for:

- Detailed explanation of what data is displayed and why
- Reconciliation logic against `SECURITY.md`'s Assets-at-Risk
- Implementation options (client-side, server-side, hybrid)
- Testing and validation procedures

## Related Issues

- **Issue #308**: This dashboard (proof-of-reserves)
- **Issue #37**: Protocol treasury (future: display treasury balance)
- **Issue #40**: Governance proposals dashboard (complementary; tracks votes)
- **Issue #46**: `TotalBonded` aggregate counter (preferred data source)
- **Issue #110**: Monitoring and alerting spec (technical foundation)

## License

MIT — same as vortex-contracts repository

## Support & Feedback

- Report bugs or suggest improvements on [GitHub Issue #308](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/308)
- Questions? See [SECURITY.md](../SECURITY.md) for threat model context

