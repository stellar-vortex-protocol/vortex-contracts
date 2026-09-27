/**
 * reference-indexer.js
 *
 * Issue #107 — Reference indexer for `intent_settlement`.
 *
 * Reconstructs the full on-chain state of the Vortex intent-settlement
 * contract from its emitted events alone, with zero contract reads.
 *
 * Note (#198): the contract now also exposes an on-chain `list_solvers(start,
 * limit)` view that paginates the registered-solver set. An indexer can use it
 * as a cheaper bootstrap / periodic reconciliation source for `this.solvers`
 * (one bounded contract read per page) instead of, or alongside, replaying
 * every `solver_registered` / `solver_deregistered` event from genesis. This
 * file stays pure event-replay by design; `list_solvers` is the escape hatch
 * when you don't have the full event history.
 *
 * This script is intentionally dependency-light (Node.js built-ins only for
 * the state machine; one optional RPC helper) so it can be dropped into any
 * JS/TS project and adapted. The state machine is the valuable artifact —
 * swap out `fetchEvents` for your preferred Soroban RPC client as needed.
 *
 * Usage:
 *   node reference-indexer.js
 *
 * The script prints a JSON snapshot of every indexed intent and solver to
 * stdout when it finishes replaying. Replace `fetchEvents` at the bottom
 * with a real Soroban RPC call to use against a live network.
 *
 * Event schema reference: docs/event-schema.md
 */

"use strict";

// ---------------------------------------------------------------------------
// State containers
// ---------------------------------------------------------------------------

/**
 * @typedef {Object} IntentState
 * @property {string}      intentId       - Hex-encoded BytesN<32>
 * @property {string}      user           - Stellar address of intent owner
 * @property {string}      srcChain       - e.g. "ethereum"
 * @property {string}      srcToken       - token address on source chain
 * @property {bigint}      srcAmount      - amount in source token units (i128)
 * @property {string}      dstToken       - Stellar SAC/SEP-41 address
 * @property {bigint}      minDstAmount   - minimum acceptable output (i128)
 * @property {bigint}      expiry         - Unix timestamp
 * @property {string|null} solver         - currently assigned solver address, or null
 * @property {string}      state          - Open | Accepted | PartiallyFilled | Filled | Cancelled | Expired | Slashed
 * @property {bigint}      totalFilled    - cumulative dst units delivered so far
 * @property {bigint}      fillDeadline   - fill-window deadline when Accepted
 * @property {number}      ledger         - ledger sequence of last state change
 */

/**
 * @typedef {Object} SolverState
 * @property {string}  address         - Stellar address
 * @property {bigint}  bondAmount      - current bond in smallest units
 * @property {number}  fillsCompleted
 * @property {number}  fillsFailed
 * @property {bigint}  totalVolume
 * @property {boolean} isActive
 * @property {number}  activeIntents
 */

/**
 * @typedef {Object} ProtocolStats
 * @property {bigint}  totalIntents    - cumulative intents ever submitted
 * @property {bigint}  totalVolume     - cumulative dst volume ever filled
 * @property {bigint}  openIntents     - currently Open intents
 * @property {bigint}  totalSolvers    - currently registered solver count
 * @property {string}  admin           - current admin address
 * @property {string}  feeRecipient    - current fee recipient address
 */

class VortexIndexer {
  constructor() {
    /** @type {Map<string, IntentState>} */
    this.intents = new Map();

    /** @type {Map<string, SolverState>} */
    this.solvers = new Map();

    /** @type {ProtocolStats} */
    this.stats = {
      totalIntents: 0n,
      totalVolume: 0n,
      openIntents: 0n,
      totalSolvers: 0n,
      admin: "",
      feeRecipient: "",
    };

    // Track events processed for debugging
    this.processedCount = 0;
    this.errors = [];
  }

  // -------------------------------------------------------------------------
  // Main entry point
  // -------------------------------------------------------------------------

  /**
   * Replay an ordered array of Soroban events and rebuild state.
   *
   * Each event object must have the shape returned by the Soroban RPC
   * `getEvents` method:
   *   { ledger: number, topics: any[], value: any }
   *
   * Topics are decoded values (Symbol strings, Addresses, etc.).
   * Value is the decoded payload (scalar or tuple/array).
   *
   * @param {Array<{ledger: number, topics: string[], value: any}>} events
   */
  replay(events) {
    for (const event of events) {
      try {
        this._dispatch(event);
        this.processedCount++;
      } catch (err) {
        this.errors.push({ event, error: err.message });
      }
    }
  }

  // -------------------------------------------------------------------------
  // Dispatcher
  // -------------------------------------------------------------------------

  /**
   * Route an event to its handler based on topics[0] (the event name Symbol).
   */
  _dispatch(event) {
    const { ledger, topics, value } = event;
    const name = topics[0];

    switch (name) {
      // ── Admin ─────────────────────────────────────────────────────────────
      case "admin_transferred":
        this._onAdminTransferred(ledger, value);
        break;
      case "fee_recipient_proposed":
        // Proposed but not yet active — no state change needed.
        break;
      case "fee_recipient_updated":
        this._onFeeRecipientUpdated(ledger, value);
        break;
      case "config_updated":
        // Protocol params change; no per-intent state to update.
        break;
      case "paused":
        // Emitted when contract is paused; not needed to reconstruct intent/solver state.
        break;
      case "unpaused":
        // Emitted when contract is unpaused; not needed to reconstruct intent/solver state.
        break;
      case "tokens_rescued":
        // Admin recovery; no intent/solver state change.
        break;

      // ── Allowlist ─────────────────────────────────────────────────────────
      case "dst_token_allowed":
      case "dst_token_disallowed":
      case "src_chain_allowed":
      case "src_chain_disallowed":
      case "dst_allowlist_enabled":
      case "src_chain_allowlist_enabled":
      case "bond_multiplier_set":
        // Config changes; no per-intent/solver state.
        break;

      // ── Solver management ─────────────────────────────────────────────────
      case "solver_registered":
        this._onSolverRegistered(ledger, topics[1], value);
        break;
      case "solver_deregistered":
        this._onSolverDeregistered(ledger, topics[1], value);
        break;
      case "bond_withdrawn":
        this._onBondWithdrawn(ledger, topics[1], value);
        break;
      case "solver_slashed":
        this._onSolverSlashed(ledger, topics[1], value);
        break;

      // ── Intent lifecycle ──────────────────────────────────────────────────
      case "intent_submitted":
        this._onIntentSubmitted(ledger, topics[1], value);
        break;
      case "intent_accepted":
        this._onIntentAccepted(ledger, topics[1], value);
        break;
      case "intent_filled":
        this._onIntentFilled(ledger, topics[1], value);
        break;
      case "intent_cancelled":
        this._onIntentCancelled(ledger, topics[1], value);
        break;
      case "intent_expired":
        this._onIntentExpired(ledger, value);
        break;
      case "extension_granted":
        this._onExtensionGranted(ledger, topics[1], value);
        break;

      default:
        // Unknown event — log and skip.
        console.warn(`[indexer] unknown event: ${name}`);
        break;
    }
  }

  // -------------------------------------------------------------------------
  // Admin handlers
  // -------------------------------------------------------------------------

  _onAdminTransferred(_ledger, newAdmin) {
    this.stats.admin = newAdmin;
  }

  _onFeeRecipientUpdated(_ledger, newRecipient) {
    this.stats.feeRecipient = newRecipient;
  }

  // -------------------------------------------------------------------------
  // Solver management handlers
  // -------------------------------------------------------------------------

  /**
   * solver_registered
   * topics: ("solver_registered", solver: Address)
   * data:   bond_amount: i128  (incremental deposit, not cumulative)
   */
  _onSolverRegistered(ledger, solver, bondAmount) {
    bondAmount = BigInt(bondAmount);

    if (this.solvers.has(solver)) {
      // Top-up: accumulate on existing record.
      const rec = this.solvers.get(solver);
      rec.bondAmount += bondAmount;
      rec.isActive = true;
    } else {
      // First registration.
      this.solvers.set(solver, {
        address: solver,
        bondAmount,
        fillsCompleted: 0,
        fillsFailed: 0,
        totalVolume: 0n,
        isActive: true,
        activeIntents: 0,
      });
      this.stats.totalSolvers += 1n;
    }
  }

  /**
   * solver_deregistered
   * topics: ("solver_deregistered", solver: Address)
   * data:   bond_refunded: i128
   *
   * The contract's `list_solvers` view (#198) is kept in sync with exactly
   * these two events, so a snapshot from `list_solvers` and a full replay of
   * `solver_registered` / `solver_deregistered` converge on the same set.
   */
  _onSolverDeregistered(ledger, solver, _bondRefunded) {
    this.solvers.delete(solver);
    this.stats.totalSolvers = this.stats.totalSolvers > 0n
      ? this.stats.totalSolvers - 1n
      : 0n;
  }

  /**
   * bond_withdrawn  (updated in issue #108)
   * topics: ("bond_withdrawn", solver: Address)
   * data:   (amount: i128, remaining: i128)
   *
   * `remaining` is authoritative — use it directly rather than subtracting
   * `amount` from the locally-tracked balance to avoid any accumulated drift.
   */
  _onBondWithdrawn(ledger, solver, value) {
    const [_amount, remaining] = value;
    const rec = this.solvers.get(solver);
    if (rec) {
      rec.bondAmount = BigInt(remaining);
    }
  }

  /**
   * solver_slashed
   * topics: ("solver_slashed", solver: Address)
   * data:   (intent_id: BytesN<32>, slash_amount: i128)
   */
  _onSolverSlashed(ledger, solver, value) {
    const [intentId, slashAmount] = value;

    // Update solver bond.
    const rec = this.solvers.get(solver);
    if (rec) {
      rec.bondAmount -= BigInt(slashAmount);
      rec.fillsFailed += 1;
      rec.activeIntents = Math.max(0, rec.activeIntents - 1);
      // A solver whose bond drops below MIN_BOND is flagged inactive on-chain;
      // we mirror that conservatively — the indexer doesn't know MIN_BOND here
      // but we preserve the is_active flag from the next solver_registered event.
    }

    // Re-open the intent.
    const intent = this.intents.get(intentId);
    if (intent) {
      const wasOpen = intent.state === "Open" || intent.state === "PartiallyFilled";
      intent.state = intent.totalFilled > 0n ? "PartiallyFilled" : "Open";
      intent.solver = null;
      intent.ledger = ledger;

      // Update open_intents counter only if transitioning back to Open from Accepted.
      if (!wasOpen) {
        this.stats.openIntents += 1n;
      }
    }
  }

  // -------------------------------------------------------------------------
  // Intent lifecycle handlers
  // -------------------------------------------------------------------------

  /**
   * intent_submitted
   * topics: ("intent_submitted", user: Address)
   * data:   (intent_id: BytesN<32>, min_dst_amount: i128, expiry: u64)
   *
   * Note: src_chain, src_token, src_amount, dst_token are NOT in the event.
   * They are only in the IntentRecord on-chain. An indexer that needs them
   * must either read the contract or subscribe to transaction metadata.
   * The state machine below tracks only what events expose.
   */
  _onIntentSubmitted(ledger, user, value) {
    const [intentId, minDstAmount, expiry] = value;

    this.intents.set(intentId, {
      intentId,
      user,
      srcChain: null,   // not in event — requires contract read or tx data
      srcToken: null,
      srcAmount: null,
      dstToken: null,
      minDstAmount: BigInt(minDstAmount),
      expiry: BigInt(expiry),
      solver: null,
      state: "Open",
      totalFilled: 0n,
      fillDeadline: 0n,
      ledger,
    });

    this.stats.totalIntents += 1n;
    this.stats.openIntents += 1n;
  }

  /**
   * intent_accepted
   * topics: ("intent_accepted", solver: Address)
   * data:   (intent_id: BytesN<32>, fill_deadline: u64)
   */
  _onIntentAccepted(ledger, solver, value) {
    const [intentId, fillDeadline] = value;

    const intent = this.intents.get(intentId);
    if (!intent) return;

    const wasOpen = intent.state === "Open" || intent.state === "PartiallyFilled";
    intent.solver = solver;
    intent.state = "Accepted";
    intent.fillDeadline = BigInt(fillDeadline);
    intent.ledger = ledger;

    if (wasOpen) {
      this.stats.openIntents = this.stats.openIntents > 0n
        ? this.stats.openIntents - 1n
        : 0n;
    }

    // Update solver's active intents counter.
    const rec = this.solvers.get(solver);
    if (rec) rec.activeIntents += 1;
  }

  /**
   * intent_filled
   * topics: ("intent_filled", solver: Address)
   * data:   (intent_id: BytesN<32>, fill_amount: i128, fee: i128)
   *
   * Emitted on EVERY fill (partial or final). The intent transitions to
   * Filled when totalFilled >= minDstAmount. Otherwise it re-opens as
   * PartiallyFilled and another solver can accept it.
   */
  _onIntentFilled(ledger, solver, value) {
    const [intentId, fillAmount, fee] = value;
    const amount = BigInt(fillAmount);

    const intent = this.intents.get(intentId);
    if (!intent) return;

    intent.totalFilled += amount;
    intent.ledger = ledger;

    // Update solver stats.
    const solverRec = this.solvers.get(solver);
    if (solverRec) {
      solverRec.totalVolume += amount;
      solverRec.activeIntents = Math.max(0, solverRec.activeIntents - 1);
    }

    this.stats.totalVolume += amount;

    if (intent.totalFilled >= intent.minDstAmount) {
      // Fully satisfied.
      intent.state = "Filled";
      intent.solver = null;
      if (solverRec) solverRec.fillsCompleted += 1;
    } else {
      // Partial: re-open for the next solver.
      intent.state = "PartiallyFilled";
      intent.solver = null;
      // PartiallyFilled counts as "open" for dashboard purposes.
      this.stats.openIntents += 1n;
    }
  }

  /**
   * intent_cancelled
   * topics: ("intent_cancelled", user: Address)
   * data:   intent_id: BytesN<32>
   */
  _onIntentCancelled(ledger, _user, intentId) {
    const intent = this.intents.get(intentId);
    if (!intent) return;

    const wasOpen = intent.state === "Open" || intent.state === "PartiallyFilled";
    intent.state = "Cancelled";
    intent.ledger = ledger;

    if (wasOpen) {
      this.stats.openIntents = this.stats.openIntents > 0n
        ? this.stats.openIntents - 1n
        : 0n;
    }
  }

  /**
   * intent_expired
   * topics: ("intent_expired",)
   * data:   intent_id: BytesN<32>
   */
  _onIntentExpired(ledger, intentId) {
    const intent = this.intents.get(intentId);
    if (!intent) return;

    const wasOpen = intent.state === "Open" || intent.state === "PartiallyFilled";
    intent.state = "Expired";
    intent.ledger = ledger;

    if (wasOpen) {
      this.stats.openIntents = this.stats.openIntents > 0n
        ? this.stats.openIntents - 1n
        : 0n;
    }
  }

  /**
   * extension_granted
   * topics: ("extension_granted", solver: Address)
   * data:   (intent_id: BytesN<32>, new_deadline: u64)
   */
  _onExtensionGranted(ledger, _solver, value) {
    const [intentId, newDeadline] = value;
    const intent = this.intents.get(intentId);
    if (!intent) return;

    intent.fillDeadline = BigInt(newDeadline);
    intent.ledger = ledger;
  }

  // -------------------------------------------------------------------------
  // Query helpers
  // -------------------------------------------------------------------------

  /** Return all intents in the given state(s). */
  getIntentsByState(...states) {
    const result = [];
    for (const intent of this.intents.values()) {
      if (states.includes(intent.state)) result.push(intent);
    }
    return result;
  }

  /** Return the solver record, or undefined. */
  getSolver(address) {
    return this.solvers.get(address);
  }

  /**
   * Return a stats snapshot compatible with `get_stats` (including the
   * new open_intents field from issue #109).
   */
  getStats() {
    return { ...this.stats };
  }

  /** Dump full state as a plain object (useful for snapshots / debugging). */
  snapshot() {
    return {
      stats: {
        ...this.stats,
        totalIntents: this.stats.totalIntents.toString(),
        totalVolume: this.stats.totalVolume.toString(),
        openIntents: this.stats.openIntents.toString(),
        totalSolvers: this.stats.totalSolvers.toString(),
      },
      intents: [...this.intents.values()].map((i) => ({
        ...i,
        minDstAmount: i.minDstAmount?.toString(),
        expiry: i.expiry?.toString(),
        totalFilled: i.totalFilled?.toString(),
        fillDeadline: i.fillDeadline?.toString(),
      })),
      solvers: [...this.solvers.values()].map((s) => ({
        ...s,
        bondAmount: s.bondAmount.toString(),
        totalVolume: s.totalVolume.toString(),
      })),
      processedCount: this.processedCount,
      errors: this.errors,
    };
  }
}

// ---------------------------------------------------------------------------
// RPC stub — replace with a real Soroban RPC client for production use
// ---------------------------------------------------------------------------

/**
 * Fetch contract events from Soroban RPC.
 *
 * Replace this function's body with a real HTTP call, e.g.:
 *
 *   const res = await fetch(`${RPC_URL}/getEvents`, {
 *     method: "POST",
 *     headers: { "Content-Type": "application/json" },
 *     body: JSON.stringify({
 *       jsonrpc: "2.0", id: 1, method: "getEvents",
 *       params: {
 *         startLedger: startLedger,
 *         filters: [{ type: "contract", contractIds: [CONTRACT_ID] }],
 *         pagination: { limit: 200 },
 *       },
 *     }),
 *   });
 *   const json = await res.json();
 *   return json.result.events.map(decodeEvent);
 *
 * The `decodeEvent` helper must decode XDR topics/values into JS primitives
 * matching the shapes described in docs/event-schema.md.
 *
 * @param {string}  contractId   - Stellar contract address (StrKey C...)
 * @param {number}  startLedger  - inclusive start ledger for the query
 * @returns {Promise<Array>}     - decoded events ordered by ledger
 */
async function fetchEvents(contractId, startLedger) {
  // ── Stub: return a synthetic event sequence for demonstration ──────────────
  //
  // This sequence exercises every state transition in the intent lifecycle and
  // both bond management paths (registered → withdrawn, registered → slashed →
  // deregistered). The indexer should reconstruct the exact state shown in the
  // assertions at the bottom of this file.
  return [
    // Admin bootstrap
    { ledger: 100, topics: ["admin_transferred"], value: "GADMIN111" },
    { ledger: 100, topics: ["fee_recipient_updated"], value: "GFEE111" },

    // Solver A registers with 500_000_000 (50 USDC)
    { ledger: 101, topics: ["solver_registered", "GSOLVER_A"], value: 500_000_000n },

    // Solver B registers
    { ledger: 102, topics: ["solver_registered", "GSOLVER_B"], value: 600_000_000n },

    // User submits intent I1
    {
      ledger: 110,
      topics: ["intent_submitted", "GUSER1"],
      value: ["intent_id_hex_I1", 3_500_000_000n, 1_700_000_000n],
    },

    // Solver A accepts I1; fill deadline = now+300
    {
      ledger: 111,
      topics: ["intent_accepted", "GSOLVER_A"],
      value: ["intent_id_hex_I1", 1_700_000_300n],
    },

    // Solver A fills I1 fully
    {
      ledger: 112,
      topics: ["intent_filled", "GSOLVER_A"],
      value: ["intent_id_hex_I1", 3_500_000_000n, 1_750_000n],
    },

    // Solver A withdraws part of their bond (#108 payload: [amount, remaining])
    {
      ledger: 120,
      topics: ["bond_withdrawn", "GSOLVER_A"],
      value: [100_000_000n, 400_000_000n],
    },

    // User submits intent I2
    {
      ledger: 130,
      topics: ["intent_submitted", "GUSER2"],
      value: ["intent_id_hex_I2", 2_000_000_000n, 1_700_001_000n],
    },

    // Solver B accepts I2
    {
      ledger: 131,
      topics: ["intent_accepted", "GSOLVER_B"],
      value: ["intent_id_hex_I2", 1_700_001_300n],
    },

    // Solver B is slashed (missed fill window)
    {
      ledger: 200,
      topics: ["solver_slashed", "GSOLVER_B"],
      value: ["intent_id_hex_I2", 60_000_000n],
    },

    // User cancels re-opened I2
    {
      ledger: 201,
      topics: ["intent_cancelled", "GUSER2"],
      value: "intent_id_hex_I2",
    },

    // User submits intent I3, nobody accepts → expires
    {
      ledger: 210,
      topics: ["intent_submitted", "GUSER1"],
      value: ["intent_id_hex_I3", 1_000_000_000n, 1_700_002_000n],
    },
    {
      ledger: 300,
      topics: ["intent_expired"],
      value: "intent_id_hex_I3",
    },

    // Solver B deregisters (refund 540_000_000 after slash)
    {
      ledger: 310,
      topics: ["solver_deregistered", "GSOLVER_B"],
      value: 540_000_000n,
    },
  ];
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const CONTRACT_ID = process.env.CONTRACT_ID || "CEXAMPLECONTRACT";
  const START_LEDGER = parseInt(process.env.START_LEDGER || "0", 10);

  console.log(`[indexer] replaying events for contract ${CONTRACT_ID} from ledger ${START_LEDGER}`);

  const events = await fetchEvents(CONTRACT_ID, START_LEDGER);
  const indexer = new VortexIndexer();
  indexer.replay(events);

  const snap = indexer.snapshot();
  console.log(JSON.stringify(snap, null, 2));

  // ── Assertions: verify the reconstructed state matches expectations ────────
  const stats = indexer.getStats();
  console.log("\n[indexer] --- assertions ---");

  // 3 intents submitted
  assert(stats.totalIntents === 3n, `totalIntents: expected 3, got ${stats.totalIntents}`);

  // I1 filled, I2 cancelled, I3 expired → 0 open
  assert(stats.openIntents === 0n, `openIntents: expected 0, got ${stats.openIntents}`);

  // I1 filled with 3_500_000_000
  assert(stats.totalVolume === 3_500_000_000n, `totalVolume: expected 3_500_000_000, got ${stats.totalVolume}`);

  // Solver B deregistered → 1 solver remaining
  assert(stats.totalSolvers === 1n, `totalSolvers: expected 1, got ${stats.totalSolvers}`);

  // Solver A bond after withdrawal: 400_000_000 (from #108 remaining field)
  const solverA = indexer.getSolver("GSOLVER_A");
  assert(solverA && solverA.bondAmount === 400_000_000n,
    `solverA.bondAmount: expected 400_000_000, got ${solverA?.bondAmount}`);

  // I1 is Filled
  const i1 = indexer.intents.get("intent_id_hex_I1");
  assert(i1?.state === "Filled", `I1 state: expected Filled, got ${i1?.state}`);

  // I2 is Cancelled
  const i2 = indexer.intents.get("intent_id_hex_I2");
  assert(i2?.state === "Cancelled", `I2 state: expected Cancelled, got ${i2?.state}`);

  // I3 is Expired
  const i3 = indexer.intents.get("intent_id_hex_I3");
  assert(i3?.state === "Expired", `I3 state: expected Expired, got ${i3?.state}`);

  console.log("[indexer] all assertions passed ✓");
}

function assert(condition, message) {
  if (!condition) throw new Error(`Assertion failed: ${message}`);
  console.log(`  ✓ ${message.split(":")[0]}`);
}

main().catch((err) => {
  console.error("[indexer] fatal:", err);
  process.exit(1);
});

module.exports = { VortexIndexer };
