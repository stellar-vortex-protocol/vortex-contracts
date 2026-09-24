# Vortex Ops Monitoring & Alerting Tool

Real-time monitoring and alerting service for the Vortex intent-settlement contract based on [`docs/110-monitoring-alerting-spec.md`](../docs/110-monitoring-alerting-spec.md).

## Overview

This tool runs two complementary monitoring modes:

1. **Event-stream listener** — Subscribes to contract events in real-time and triggers P1/P2 alerts immediately.
2. **Periodic poller** — Polls `get_protocol_health` on a schedule to catch any signals missed by the event stream (e.g., intents sitting past deadline with no expiry event).

## Installation

```bash
# Ensure Node.js 16+ is installed
node --version

# No dependencies; uses Node.js built-ins and fetch API
```

## Configuration

Set environment variables before running:

```bash
# Soroban RPC endpoint
export SOROBAN_RPC_URL="https://soroban-mainnet.stellar.org"

# Contract address to monitor
export CONTRACT_ID="C..."

# Stellar network (mainnet | testnet)
export NETWORK="mainnet"

# Polling interval in seconds (default: 60)
export POLL_INTERVAL=60

# Webhook URL for alerts (optional)
export ALERT_WEBHOOK_URL="https://example.com/alerts"

# Pre-announced maintenance windows (optional ISO 8601 timestamps)
export MAINTENANCE_WINDOW_START="2026-10-01T00:00:00Z"
export MAINTENANCE_WINDOW_END="2026-10-01T02:00:00Z"

# Thresholds for alerts (all optional, use defaults if not set)
export SLASH_RATE_THRESHOLD=0.20        # 20%
export SLASH_COUNT_THRESHOLD=5          # 5 slashes
export SLASH_WINDOW_MINUTES=10
export BOND_DROP_THRESHOLD=0.25         # 25%
export MASS_SOLVER_EXIT_THRESHOLD=3     # 3 deregistrations in 1h
```

## Usage

```bash
# Run both listener and poller
node monitoring/vortex-monitor.js

# The script logs all activity to stdout and posts alerts to the configured webhook
```

## Signals Monitored

### P1 — Page Immediately

- **Unexpected pause/unpause** — `paused` or `unpaused` event outside maintenance window
- **Admin transfer** — `admin_transferred` event
- **Fee recipient change** — `fee_recipient_proposed` or `fee_recipient_updated` event
- **Token rescue** — `tokens_rescued` event (any occurrence)

### P2 — Escalate

- **Unusual slash rate** — >20% of intents slashed or >5 slashes in 10 minutes
- **Bond utilization drop** — >25% in 1 hour
- **Mass solver exit** — >3 deregistrations in 1 hour
- **Paused longer than expected** — Pause persists past maintenance window end
- **Expiry-rate spike** — Unusual increase in `intent_expired` events
- **Config churn** — Allowlist or config changes outside maintenance window

### P3 — Informational

- **Fill-rate stagnation** — No volume increase over expected window
- **Extension-granting frequency** — Same intent granted multiple extensions
- **Repeated slash-cooldown lockouts** — Solvers repeatedly hitting cooldown

## Alert Delivery

Alerts are posted to `ALERT_WEBHOOK_URL` (if configured) as JSON:

```json
{
  "timestamp": "2026-10-01T12:34:56.789Z",
  "severity": "P1",
  "signal": "admin_transfer",
  "message": "Admin address changed to C...",
  "details": {
    "newAdmin": "C...",
    "txHash": "...",
    "source": "event"
  }
}
```

## Testing

Generate test events:

```bash
# Deploy a contract to testnet and trigger events manually, then monitor:
export SOROBAN_RPC_URL="https://soroban-testnet.stellar.org"
export CONTRACT_ID="<testnet contract address>"
export NETWORK="testnet"
node monitoring/vortex-monitor.js
```

## Architecture

```
vortex-monitor.js
├── Event Listener
│   ├── Subscribes to contract events
│   ├── Decodes event topics and payloads
│   └── Triggers real-time P1/P2 alerts
├── Periodic Poller
│   ├── Polls get_protocol_health on schedule
│   ├── Reconciles state from views
│   └── Detects missed events
└── Alert Manager
    ├── Filters false positives (maintenance windows)
    ├── Formats alert messages
    └── Sends to webhook / logs
```

## Maintenance

**Daily:** Review P3 (informational) logs for patterns.

**Weekly:** Confirm webhook delivery is working by intentionally pausing the contract on testnet.

**Monthly:** Tune alert thresholds based on observed baseline metrics.

## See Also

- [`docs/110-monitoring-alerting-spec.md`](../docs/110-monitoring-alerting-spec.md) — Full signal catalog and severity definitions
- [`docs/event-schema.md`](../docs/event-schema.md) — Event structure reference
- [`indexer/reference-indexer.js`](../indexer/reference-indexer.js) — Event decoding patterns

## Future Work

- Dashboard UI (see [`docs/110-monitoring-alerting-spec.md`](../docs/110-monitoring-alerting-spec.md) §4)
- PagerDuty/Slack integration templates
- Historical alert archive
- Automated response actions (e.g., auto-pause on P1 detection)
