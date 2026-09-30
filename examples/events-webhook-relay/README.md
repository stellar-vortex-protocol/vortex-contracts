# Events-to-Webhook Relay Example

A reference implementation of an event relay service for Vortex Protocol — subscribes to intent settlement contract events and forwards them to a configurable webhook URL.

**Issue #139**

## Overview

Instead of polling the blockchain or replaying events from logs, backend integrators (e.g., `vortex-backend`) can subscribe to a live webhook that pushes event notifications in real-time. This service demonstrates the pattern:

1. **Poll** the Soroban RPC for contract events
2. **Relay** each event to your webhook with retries
3. **Provide** health/status endpoints for monitoring
4. **Handle** graceful shutdown and error recovery

## Use Cases

- **Order management**: Update order status when intents are accepted, filled, slashed, or expired
- **Solver dashboards**: Stream real-time solver activity and reputation updates
- **Analytics**: Collect event streams for off-chain analysis
- **Audit logs**: Archive all state transitions for compliance

## Architecture

```
┌─────────────────────┐
│   Soroban RPC       │
│ (Event Stream)      │
└──────────┬──────────┘
           │ Poll every 5s
           ▼
┌─────────────────────┐
│   Relay Service     │  ← You are here
│  (This Example)     │
└──────────┬──────────┘
           │ POST /webhook
           ▼
┌─────────────────────┐
│   Your Backend      │
│   /vortex/events    │
└─────────────────────┘
```

## Quick Start

### 1. Setup

```bash
cd examples/events-webhook-relay
npm install
```

### 2. Configure

Copy the example environment file and fill in your values:

```bash
cp .env.example .env
```

Edit `.env`:

```bash
# Your contract ID from deployment
CONTRACT_ID=CBQG2KXQOQ5FOA7J4WTCJQXQ2HE6T2HE2HE6T2HE6T2HE6T2HE6T2HE6

# Your backend webhook endpoint
WEBHOOK_URL=https://your-backend.example.com/vortex/events

# Local Soroban RPC (or testnet/mainnet)
SOROBAN_RPC_URL=http://localhost:8001
```

### 3. Build & Run

```bash
npm run build
npm start
```

The relay service will:
- Connect to Soroban RPC
- Poll for events every 5 seconds
- Forward events to your webhook with exponential backoff on failure
- Expose health checks on `http://localhost:3000`

## Webhook Integration

### Payload Format

Each event is sent as a POST request to your webhook:

```json
{
  "id": "unique-event-id",
  "contractId": "CBQG2N...",
  "ledgerSequence": 12345,
  "timestamp": 1234567890,
  "type": "intent_submitted",
  "data": {
    "intent_id": "0x...",
    "user": "GAAA...",
    "src_chain": "ethereum",
    "src_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    "dst_token": "CDAA..."
  },
  "relayedAt": "2025-01-15T10:30:45.123Z"
}
```

### Headers

- `Content-Type: application/json`
- `User-Agent: vortex-events-webhook-relay/0.1.0`
- `X-Vortex-Event-ID: <event-id>`
- `X-Vortex-Contract-ID: <contract-id>`

### Expected Responses

- **2xx**: Success; event marked as delivered
- **4xx**: Client error; event dropped (not retried)
- **5xx**: Server error; retried with exponential backoff

### Webhook Example (Node.js/Express)

```typescript
import express from "express";

const app = express();
app.use(express.json());

app.post("/vortex/events", async (req, res) => {
  const event = req.body;

  console.log("Event received:", {
    id: event.id,
    type: event.type,
    contractId: event.contractId,
  });

  // Process the event
  switch (event.type) {
    case "intent_submitted":
      await handleIntentSubmitted(event);
      break;
    case "intent_accepted":
      await handleIntentAccepted(event);
      break;
    case "intent_filled":
      await handleIntentFilled(event);
      break;
    case "solver_slashed":
      await handleSolverSlashed(event);
      break;
    default:
      console.warn("Unknown event type:", event.type);
  }

  // Return 200 to acknowledge
  res.status(200).json({ received: true });
});

app.listen(3001, () => {
  console.log("Webhook endpoint listening on port 3001");
});
```

## Monitoring

### Health Check

```bash
curl http://localhost:3000/health
```

Response:

```json
{
  "status": "ok",
  "running": true,
  "currentCursor": "12345",
  "contractId": "CBQG2N..."
}
```

### Stats Endpoint

```bash
curl http://localhost:3000/stats
```

Response:

```json
{
  "contractId": "CBQG2N...",
  "webhookUrl": "https://...",
  "currentCursor": "12345",
  "isRunning": true
}
```

### Graceful Shutdown

```bash
curl -X POST http://localhost:3000/shutdown
```

## Retry Logic

Failed webhook deliveries are retried with exponential backoff:

- **Attempt 1**: Immediate
- **Attempt 2**: 1,000 ms delay
- **Attempt 3**: 2,000 ms delay
- **Attempt 4**: 4,000 ms delay

After `MAX_RETRIES` (default: 3), the event is dropped and logged.

Configure retry behavior via environment variables:

```bash
MAX_RETRIES=5              # More aggressive retries
RETRY_BACKOFF_MS=2000      # Longer backoff window
```

## Deployment

### Docker

```dockerfile
FROM node:18-alpine

WORKDIR /app
COPY examples/events-webhook-relay .

RUN npm ci --only=production

EXPOSE 3000

CMD ["npm", "start"]
```

Build and run:

```bash
docker build -f examples/events-webhook-relay/Dockerfile -t vortex-relay .
docker run \
  -e CONTRACT_ID=CBQG2N... \
  -e WEBHOOK_URL=https://... \
  -e SOROBAN_RPC_URL=http://soroban-rpc:8001 \
  -p 3000:3000 \
  vortex-relay
```

### Docker Compose

Add to your `docker-compose.yml`:

```yaml
services:
  vortex-relay:
    build:
      context: .
      dockerfile: examples/events-webhook-relay/Dockerfile
    environment:
      CONTRACT_ID: ${VORTEX_CONTRACT_ID}
      WEBHOOK_URL: http://backend:3001/vortex/events
      SOROBAN_RPC_URL: http://soroban-rpc:8001
      LOG_LEVEL: info
    ports:
      - "3000:3000"
    depends_on:
      - soroban-rpc
      - backend
    restart: unless-stopped
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: vortex-relay
spec:
  replicas: 2
  selector:
    matchLabels:
      app: vortex-relay
  template:
    metadata:
      labels:
        app: vortex-relay
    spec:
      containers:
        - name: relay
          image: vortex-relay:latest
          ports:
            - containerPort: 3000
          env:
            - name: CONTRACT_ID
              valueFrom:
                configMapKeyRef:
                  name: vortex-config
                  key: contract-id
            - name: WEBHOOK_URL
              valueFrom:
                secretKeyRef:
                  name: vortex-secrets
                  key: webhook-url
            - name: SOROBAN_RPC_URL
              value: http://soroban-rpc:8001
          livenessProbe:
            httpGet:
              path: /health
              port: 3000
            initialDelaySeconds: 10
            periodSeconds: 30
          readinessProbe:
            httpGet:
              path: /health
              port: 3000
            initialDelaySeconds: 5
            periodSeconds: 10
```

## Logging

Logs are emitted to stdout (pino format):

```json
{
  "level": 30,
  "time": 1705333445123,
  "pid": 1234,
  "hostname": "relay-service",
  "msg": "Event relayed successfully",
  "eventId": "evt-123",
  "status": 200
}
```

Set `LOG_LEVEL` for different verbosity:

```bash
LOG_LEVEL=debug    # Detailed polling logs
LOG_LEVEL=info     # Normal operation
LOG_LEVEL=warn     # Warnings and errors only
LOG_LEVEL=error    # Errors only
```

In production, pipe logs to your favorite log aggregator (ELK, Splunk, Datadog, etc.):

```bash
npm start | filebeat --input json
```

## Development

### Build

```bash
npm run build
```

### Watch Mode

```bash
npm run dev
```

### Linting

```bash
npm run lint
```

### Format Code

```bash
npm run format
```

## Limitations & Future Work

- **Event polling**: Currently polls Soroban RPC every 5 seconds. When Soroban's event streaming API becomes available, switch to WebSocket subscriptions for lower latency and reduced load.
- **Dead-letter queue**: Failed events after max retries are logged but not persisted. Production systems should implement a DLQ for later replay.
- **Duplicate handling**: Events are assumed to be idempotent. If your webhook is not idempotent, implement event deduplication (via `X-Vortex-Event-ID` header).
- **Batch delivery**: Currently sends one event per request. Add batching for higher throughput.

## Troubleshooting

### "Connection refused" on Soroban RPC

Ensure RPC is running:

```bash
curl http://localhost:8001
```

Or check your `SOROBAN_RPC_URL` in `.env`.

### Webhook not receiving events

- Check logs: `LOG_LEVEL=debug`
- Verify webhook URL is reachable: `curl -X POST $WEBHOOK_URL`
- Ensure CONTRACT_ID is correct in `.env`
- Check firewall/networking rules

### High retry rates

- Increase `MAX_RETRIES` if your webhook is slow
- Increase `RETRY_BACKOFF_MS` for longer backoff window
- Optimize your webhook handler (consider async processing)

## See Also

- [Vortex Protocol](https://github.com/vortex-protocol)
- [Intent Settlement Contract](../../intent_settlement/)
- [Solver Integration Guide](../../docs/solver-integration-guide.md)

## License

MIT
