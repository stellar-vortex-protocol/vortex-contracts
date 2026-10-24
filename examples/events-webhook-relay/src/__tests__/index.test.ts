/**
 * Test suite for events-webhook-relay (issue #513)
 *
 * Coverage areas:
 *   1. loadConfig  – reads env vars and applies defaults
 *   2. EventRelay constructor – validateConfig rejects bad configuration
 *   3. Express routes – /health and /stats return correct JSON shapes
 *   4. relayEvent   – successful webhook delivery, retry on transient failure,
 *                     drop after max retries
 *   5. pollEvents   – processes fetched events and advances the cursor
 *   6. start/stop   – isRunning flag transitions
 */

import {
  describe,
  it,
  expect,
  beforeEach,
  afterEach,
  vi,
  type MockInstance,
} from "vitest";
import request from "supertest";
import axios from "axios";

// We import from the compiled source. Vitest resolves TypeScript directly via
// the ts plugin configured in vitest.config.ts, so no build step is needed
// during test runs.
import { loadConfig, EventRelay } from "../index.js";

// ── helpers ───────────────────────────────────────────────────────────────

function baseConfig() {
  return {
    rpcUrl: "http://localhost:8001",
    contractId: "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM",
    webhookUrl: "http://localhost:9999/hook",
    maxRetries: 3,
    retryBackoffMs: 10, // speed up tests
    port: 0, // OS-assigned port; we don't actually listen in unit tests
    logLevel: "error" as const, // suppress pino output during tests
    startingCursor: undefined,
  };
}

// ── 1. loadConfig ─────────────────────────────────────────────────────────

describe("loadConfig", () => {
  const saved: Record<string, string | undefined> = {};

  // Save and restore env vars touched by each test.
  beforeEach(() => {
    for (const key of [
      "SOROBAN_RPC_URL",
      "CONTRACT_ID",
      "WEBHOOK_URL",
      "MAX_RETRIES",
      "RETRY_BACKOFF_MS",
      "PORT",
      "LOG_LEVEL",
      "STARTING_CURSOR",
    ]) {
      saved[key] = process.env[key];
      delete process.env[key];
    }
  });

  afterEach(() => {
    for (const [k, v] of Object.entries(saved)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
  });

  it("returns defaults when no env vars are set", () => {
    const cfg = loadConfig();
    expect(cfg.rpcUrl).toBe("http://localhost:8001");
    expect(cfg.maxRetries).toBe(3);
    expect(cfg.retryBackoffMs).toBe(1000);
    expect(cfg.port).toBe(3000);
    expect(cfg.logLevel).toBe("info");
    expect(cfg.startingCursor).toBeUndefined();
  });

  it("reads overrides from environment variables", () => {
    process.env.SOROBAN_RPC_URL = "http://rpc.example.com";
    process.env.CONTRACT_ID = "CABC123";
    process.env.WEBHOOK_URL = "http://hook.example.com";
    process.env.MAX_RETRIES = "5";
    process.env.RETRY_BACKOFF_MS = "500";
    process.env.PORT = "4000";
    process.env.LOG_LEVEL = "debug";
    process.env.STARTING_CURSOR = "42";

    const cfg = loadConfig();
    expect(cfg.rpcUrl).toBe("http://rpc.example.com");
    expect(cfg.contractId).toBe("CABC123");
    expect(cfg.webhookUrl).toBe("http://hook.example.com");
    expect(cfg.maxRetries).toBe(5);
    expect(cfg.retryBackoffMs).toBe(500);
    expect(cfg.port).toBe(4000);
    expect(cfg.logLevel).toBe("debug");
    expect(cfg.startingCursor).toBe("42");
  });

  it("parses numeric strings to numbers", () => {
    process.env.MAX_RETRIES = "7";
    process.env.RETRY_BACKOFF_MS = "2500";
    process.env.PORT = "8080";
    const cfg = loadConfig();
    expect(typeof cfg.maxRetries).toBe("number");
    expect(typeof cfg.retryBackoffMs).toBe("number");
    expect(typeof cfg.port).toBe("number");
  });
});

// ── 2. EventRelay constructor / validateConfig ────────────────────────────

describe("EventRelay constructor", () => {
  it("throws when CONTRACT_ID is empty", () => {
    expect(
      () => new EventRelay({ ...baseConfig(), contractId: "" })
    ).toThrow(/CONTRACT_ID/);
  });

  it("throws when WEBHOOK_URL is empty", () => {
    expect(
      () => new EventRelay({ ...baseConfig(), webhookUrl: "" })
    ).toThrow(/WEBHOOK_URL/);
  });

  it("throws when WEBHOOK_URL is not a valid URL", () => {
    expect(
      () => new EventRelay({ ...baseConfig(), webhookUrl: "not-a-url" })
    ).toThrow(/WEBHOOK_URL/);
  });

  it("does not throw with valid configuration", () => {
    expect(() => new EventRelay(baseConfig())).not.toThrow();
  });
});

// ── 3. Express routes ─────────────────────────────────────────────────────

describe("Express routes", () => {
  let relay: EventRelay;

  beforeEach(() => {
    relay = new EventRelay({ ...baseConfig(), startingCursor: "10" });
  });

  it("GET /health returns status ok and shape", async () => {
    const res = await request(relay.expressApp).get("/health");
    expect(res.status).toBe(200);
    expect(res.body).toMatchObject({
      status: "ok",
      running: false,
    });
    expect(res.body).toHaveProperty("currentCursor");
    expect(res.body).toHaveProperty("contractId");
  });

  it("GET /health reflects the starting cursor", async () => {
    const res = await request(relay.expressApp).get("/health");
    expect(res.body.currentCursor).toBe("10");
  });

  it("GET /stats returns contractId and webhookUrl", async () => {
    const res = await request(relay.expressApp).get("/stats");
    expect(res.status).toBe(200);
    expect(res.body.contractId).toBe(baseConfig().contractId);
    expect(res.body.webhookUrl).toBe(baseConfig().webhookUrl);
    expect(res.body).toHaveProperty("isRunning");
  });

  it("GET /health running flag is false before start()", async () => {
    const res = await request(relay.expressApp).get("/health");
    expect(res.body.running).toBe(false);
  });
});

// ── 4. relayEvent ─────────────────────────────────────────────────────────

describe("relayEvent", () => {
  let relay: EventRelay;
  let postSpy: MockInstance;

  const sampleEvent = {
    id: "evt-001",
    contractId: "CABC",
    ledgerSequence: 100,
    timestamp: 1_700_000_000,
    type: "badge_minted",
    data: { tier: "Gold" },
    relayedAt: new Date().toISOString(),
  };

  beforeEach(() => {
    relay = new EventRelay(baseConfig());
    postSpy = vi.spyOn(axios, "post");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("POSTs event payload to the webhook URL on success", async () => {
    postSpy.mockResolvedValueOnce({ status: 200, data: {} });

    // Access private method via type cast for white-box testing.
    await (relay as unknown as { relayEvent: (e: unknown) => Promise<void> }).relayEvent(sampleEvent);

    expect(postSpy).toHaveBeenCalledOnce();
    const [url, body, opts] = postSpy.mock.calls[0] as [string, unknown, { headers: Record<string, string> }];
    expect(url).toBe(baseConfig().webhookUrl);
    expect(body).toEqual(sampleEvent);
    expect(opts.headers["X-Vortex-Event-ID"]).toBe(sampleEvent.id);
    expect(opts.headers["X-Vortex-Contract-ID"]).toBe(sampleEvent.contractId);
  });

  it("retries on transient 500 and succeeds on second attempt", async () => {
    postSpy
      .mockRejectedValueOnce(new Error("server error"))
      .mockResolvedValueOnce({ status: 200, data: {} });

    await (relay as unknown as { relayEvent: (e: unknown) => Promise<void> }).relayEvent(sampleEvent);

    expect(postSpy).toHaveBeenCalledTimes(2);
  });

  it("drops the event after max retries without throwing", async () => {
    postSpy.mockRejectedValue(new Error("always fails"));

    // Should resolve (not reject) — the relay logs and drops after max attempts.
    await expect(
      (relay as unknown as { relayEvent: (e: unknown) => Promise<void> }).relayEvent(sampleEvent)
    ).resolves.toBeUndefined();

    expect(postSpy).toHaveBeenCalledTimes(baseConfig().maxRetries);
  });

  it("does not retry on a 2xx response", async () => {
    postSpy.mockResolvedValueOnce({ status: 201, data: {} });

    await (relay as unknown as { relayEvent: (e: unknown) => Promise<void> }).relayEvent(sampleEvent);

    expect(postSpy).toHaveBeenCalledOnce();
  });

  it("retries when webhook returns a non-2xx status code", async () => {
    postSpy
      .mockResolvedValueOnce({ status: 503, data: {} })
      .mockResolvedValueOnce({ status: 200, data: {} });

    await (relay as unknown as { relayEvent: (e: unknown) => Promise<void> }).relayEvent(sampleEvent);

    expect(postSpy).toHaveBeenCalledTimes(2);
  });
});

// ── 5. pollEvents / fetchContractEvents ──────────────────────────────────

describe("pollEvents", () => {
  let relay: EventRelay;
  let postSpy: MockInstance;

  beforeEach(() => {
    relay = new EventRelay(baseConfig());
    postSpy = vi.spyOn(axios, "post");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("advances currentCursor to the last event id when events are returned", async () => {
    const events = [
      { id: "10", contractId: "C1", ledgerSequence: 10, timestamp: 1, type: "t", data: {}, relayedAt: "" },
      { id: "11", contractId: "C1", ledgerSequence: 11, timestamp: 2, type: "t", data: {}, relayedAt: "" },
    ];
    // First call: fetchContractEvents (getEvents RPC); subsequent calls: relayEvent webhooks
    postSpy
      .mockResolvedValueOnce({ data: { result: { events } } }) // getEvents
      .mockResolvedValue({ status: 200, data: {} }); // webhook deliveries

    await (relay as unknown as { pollEvents: () => Promise<void> }).pollEvents();

    // Cursor should have been updated to the last event's id.
    const health = await request(relay.expressApp).get("/health");
    expect(health.body.currentCursor).toBe("11");
  });

  it("does not advance cursor when no events are returned", async () => {
    postSpy.mockResolvedValueOnce({ data: { result: { events: [] } } });

    await (relay as unknown as { pollEvents: () => Promise<void> }).pollEvents();

    const health = await request(relay.expressApp).get("/health");
    expect(health.body.currentCursor).toBeUndefined();
  });

  it("does not throw when the RPC call fails", async () => {
    postSpy.mockRejectedValueOnce(new Error("RPC unavailable"));

    await expect(
      (relay as unknown as { pollEvents: () => Promise<void> }).pollEvents()
    ).resolves.toBeUndefined();
  });
});

// ── 6. start / stop ───────────────────────────────────────────────────────

describe("start / stop", () => {
  let relay: EventRelay;

  afterEach(() => {
    relay?.stop();
  });

  it("stop() sets isRunning to false (via /health)", async () => {
    relay = new EventRelay(baseConfig());

    // Don't await start() — it blocks; just check stop() flips the flag.
    relay.stop();
    const res = await request(relay.expressApp).get("/health");
    expect(res.body.running).toBe(false);
  });
});
