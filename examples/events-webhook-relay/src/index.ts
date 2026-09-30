/**
 * events-webhook-relay.ts
 *
 * Example event relay service for Vortex Protocol.
 * Subscribes to contract events and forwards them to a configurable webhook URL.
 *
 * Issue #139
 */

import express, { Express, Request, Response } from "express";
import axios from "axios";
import pino from "pino";
import { Server } from "soroban-client";

// ──────────────────────────────────────────────────────────────────────────

/**
 * Configuration from environment variables
 */
interface Config {
  // Soroban RPC endpoint
  rpcUrl: string;

  // Contract ID to watch for events
  contractId: string;

  // Webhook URL to POST events to
  webhookUrl: string;

  // Retry policy: max attempts for failed webhook deliveries
  maxRetries: number;

  // Retry backoff: ms to wait before retrying
  retryBackoffMs: number;

  // Express server port
  port: number;

  // Logging level
  logLevel: "debug" | "info" | "warn" | "error";

  // Event cursor (ledger sequence) to start from
  // If not set, starts from the next new event
  startingCursor?: string;
}

/**
 * Load configuration from environment variables with defaults
 */
function loadConfig(): Config {
  return {
    rpcUrl: process.env.SOROBAN_RPC_URL || "http://localhost:8001",
    contractId: process.env.CONTRACT_ID || "",
    webhookUrl: process.env.WEBHOOK_URL || "",
    maxRetries: parseInt(process.env.MAX_RETRIES || "3"),
    retryBackoffMs: parseInt(process.env.RETRY_BACKOFF_MS || "1000"),
    port: parseInt(process.env.PORT || "3000"),
    logLevel: (process.env.LOG_LEVEL as Config["logLevel"]) || "info",
    startingCursor: process.env.STARTING_CURSOR,
  };
}

// ──────────────────────────────────────────────────────────────────────────

/**
 * Payload sent to the webhook for each event
 */
interface EventPayload {
  id: string; // Unique event ID
  contractId: string;
  ledgerSequence: number;
  timestamp: number;
  type: string;
  data: Record<string, unknown>;
  relayedAt: string; // ISO 8601 timestamp
}

/**
 * Event relay service
 */
class EventRelay {
  private config: Config;
  private logger: ReturnType<typeof pino>;
  private server: Server;
  private app: Express;
  private currentCursor: string | undefined;
  private isRunning: boolean = false;

  constructor(config: Config) {
    this.config = config;
    this.logger = pino({
      level: config.logLevel,
      transport:
        process.env.NODE_ENV !== "production"
          ? {
              target: "pino-pretty",
              options: { colorize: true },
            }
          : undefined,
    });

    this.server = new Server(config.rpcUrl);
    this.app = express();
    this.currentCursor = config.startingCursor;

    this.validateConfig();
    this.setupExpress();
  }

  /**
   * Validate required configuration
   */
  private validateConfig(): void {
    if (!this.config.contractId) {
      throw new Error("CONTRACT_ID environment variable is required");
    }
    if (!this.config.webhookUrl) {
      throw new Error("WEBHOOK_URL environment variable is required");
    }
    try {
      new URL(this.config.webhookUrl);
    } catch {
      throw new Error(`Invalid WEBHOOK_URL: ${this.config.webhookUrl}`);
    }
  }

  /**
   * Setup Express routes
   */
  private setupExpress(): void {
    // Health check endpoint
    this.app.get("/health", (req: Request, res: Response) => {
      res.json({
        status: "ok",
        running: this.isRunning,
        currentCursor: this.currentCursor,
        contractId: this.config.contractId,
      });
    });

    // Stats endpoint
    this.app.get("/stats", (req: Request, res: Response) => {
      res.json({
        contractId: this.config.contractId,
        webhookUrl: this.config.webhookUrl,
        currentCursor: this.currentCursor,
        isRunning: this.isRunning,
      });
    });

    // Graceful shutdown
    this.app.post("/shutdown", (req: Request, res: Response) => {
      this.logger.info("Shutdown requested");
      res.json({ message: "Shutting down..." });
      this.stop();
    });
  }

  /**
   * Fetch and process new events from the contract
   */
  private async pollEvents(): Promise<void> {
    try {
      this.logger.debug({ cursor: this.currentCursor }, "Polling for events");

      // Fetch events from Soroban RPC
      // Note: This is a conceptual example. Actual event fetching depends on
      // Soroban RPC's event query API (when available) or Horizon's event stream.
      // For now, we demonstrate the relay structure.

      const events = await this.fetchContractEvents();

      if (events.length > 0) {
        this.logger.info({ count: events.length }, "Fetched events");

        for (const event of events) {
          await this.relayEvent(event);
          this.currentCursor = event.id;
        }
      }
    } catch (error) {
      this.logger.error(
        { error, contractId: this.config.contractId },
        "Error polling events"
      );
      // Continue retrying on error; don't crash the service
    }
  }

  /**
   * Fetch contract events from Soroban RPC
   * Placeholder: actual implementation depends on RPC event API
   */
  private async fetchContractEvents(): Promise<EventPayload[]> {
    // This is a conceptual implementation.
    // In production, you would use the actual Soroban RPC event query API
    // (currently under development at time of writing).
    //
    // Example structure (when RPC supports events):
    //   POST /soroban/rpc with method: "getEvents"
    //   params: { contractId, startLedger, cursor, limit }

    try {
      const response = await axios.post(this.config.rpcUrl, {
        jsonrpc: "2.0",
        id: Date.now(),
        method: "getEvents",
        params: {
          contractIds: [this.config.contractId],
          startLedger: this.currentCursor ? parseInt(this.currentCursor) : 0,
          limit: 100,
        },
      });

      return response.data.result?.events || [];
    } catch (error) {
      // getEvents not yet available; return empty for now
      this.logger.debug(
        "Event polling not yet available on this RPC; placeholder mode"
      );
      return [];
    }
  }

  /**
   * Send event to the webhook URL with retries
   */
  private async relayEvent(
    event: EventPayload,
    attempt: number = 1
  ): Promise<void> {
    try {
      this.logger.debug(
        { eventId: event.id, attempt },
        "Relaying event to webhook"
      );

      const response = await axios.post(this.config.webhookUrl, event, {
        timeout: 10_000, // 10 second timeout
        headers: {
          "Content-Type": "application/json",
          "User-Agent": "vortex-events-webhook-relay/0.1.0",
          "X-Vortex-Event-ID": event.id,
          "X-Vortex-Contract-ID": event.contractId,
        },
      });

      if (response.status >= 200 && response.status < 300) {
        this.logger.info(
          { eventId: event.id, status: response.status },
          "Event relayed successfully"
        );
      } else {
        throw new Error(`Webhook returned status ${response.status}`);
      }
    } catch (error) {
      if (attempt < this.config.maxRetries) {
        const backoff = this.config.retryBackoffMs * Math.pow(2, attempt - 1);
        this.logger.warn(
          { eventId: event.id, attempt, nextRetryMs: backoff, error },
          "Webhook delivery failed; retrying"
        );

        await new Promise((resolve) => setTimeout(resolve, backoff));
        await this.relayEvent(event, attempt + 1);
      } else {
        this.logger.error(
          { eventId: event.id, attempts: attempt, error },
          "Webhook delivery failed after max retries; dropping event"
        );
        // In production, you might log this to a dead-letter queue
      }
    }
  }

  /**
   * Start the relay service
   */
  async start(): Promise<void> {
    if (this.isRunning) {
      this.logger.warn("Service is already running");
      return;
    }

    this.isRunning = true;
    this.logger.info(
      {
        contractId: this.config.contractId,
        webhookUrl: this.config.webhookUrl,
        rpcUrl: this.config.rpcUrl,
      },
      "Starting event relay service"
    );

    // Start Express server
    this.app.listen(this.config.port, () => {
      this.logger.info(
        { port: this.config.port },
        "Health check server listening"
      );
    });

    // Poll for events indefinitely
    // In production, consider using Soroban's WebSocket event stream when available
    const pollInterval = setInterval(() => {
      this.pollEvents().catch((error) => {
        this.logger.error({ error }, "Unexpected error in poll loop");
      });
    }, 5_000); // Poll every 5 seconds

    // Handle graceful shutdown
    process.on("SIGINT", () => {
      this.logger.info("SIGINT received; shutting down gracefully");
      clearInterval(pollInterval);
      this.isRunning = false;
      process.exit(0);
    });

    process.on("SIGTERM", () => {
      this.logger.info("SIGTERM received; shutting down gracefully");
      clearInterval(pollInterval);
      this.isRunning = false;
      process.exit(0);
    });
  }

  /**
   * Stop the relay service
   */
  private stop(): void {
    this.isRunning = false;
    this.logger.info("Event relay service stopped");
  }
}

// ──────────────────────────────────────────────────────────────────────────

/**
 * Main entry point
 */
async function main(): Promise<void> {
  try {
    const config = loadConfig();
    const relay = new EventRelay(config);
    await relay.start();
  } catch (error) {
    console.error("Fatal error:", error);
    process.exit(1);
  }
}

main();
