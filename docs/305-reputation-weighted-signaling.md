# Off-Chain Reputation-Weighted Signaling Tool

**Issue:** [#305](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/305)  
**Status:** Proposed  
**Last updated:** 2026-09-24

---

## 1. Overview

This document specifies a lightweight, off-chain "temperature check" signaling mechanism for the Vortex Protocol community. The tool allows solvers, users, and other stakeholders to register a signed preference (for/against/abstain) on a governance proposal **before** the admin commits to an on-chain `propose_*` transaction.

**Key properties:**
- **Verifiable:** Signed Stellar messages tie preferences to real addresses.
- **Weighted:** Each address's vote is weighted using the governance-weight formula from issue #41 (reputation score × bond amount).
- **Non-binding:** Results are published as community sentiment signals, not automatic enforcement.
- **Optional:** Proposal approval and execution remain admin decisions; signaling is an optional input.

**What this solves:**
- Currently, GitHub discussion comments are unweighted, unverifiable, and unlinked to actual on-chain participation.
- This tool gives users a concrete way to register preference proportional to their stake/reputation before an admin acts.
- It enables issue #112's RFC process to be informed by quantified community sentiment, not just discourse.

---

## 2. Design Principles

1. **Simplicity:** The message format and signing/verification are minimal, so a solver bot or user can participate with standard Stellar SDK tools.

2. **Backwards-compatible with issue #41:** Reuse issue #41's governance-weight formula exactly; do not invent a new weighting scheme.

3. **Latest-signal-wins:** If the same address signals multiple times on the same proposal, only the most recent signal counts, preventing vote spam.

4. **Public aggregation:** Results are published in a human-readable format (JSON, markdown, etc.) that anyone can independently verify.

5. **No on-chain touch:** The tool is purely off-chain; it generates no on-chain transactions and imposes no on-chain gas costs.

---

## 3. Governance-Weight Formula (from Issue #41)

Before this tool can launch, issue #41 must define and publish the governance-weight formula. This tool will reuse that formula exactly.

**Expected formula shape:**
```
governance_weight(address) = reputation_score(address) × bond_amount(address)
```

Where:
- `reputation_score` comes from `intent_settlement`'s `compute_reputation_score()` (`intent_settlement/src/lib.rs`).
- `bond_amount` is the solver's current bonded USDC from `SolverRecord.bond`.

**Snapshot timing:** Weights are computed at a fixed "snapshot block" (e.g., the block where the proposal was first announced), so latecomers cannot sway results by bonding just before the signal window closes.

---

## 4. Message Format

### 4.1 Signed Message Structure

A signaler sends a JSON structure with:

```json
{
  "proposal_id": "proposal-2026-10-15-treasury-audit",
  "proposal_title": "Fund Security Audit Q4 2026",
  "voter_address": "GXXXXXX...",
  "signal": "for",
  "timestamp": 1698067200,
  "message": "Audit is critical for mainnet launch. Supports immediate funding."
}
```

**Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `proposal_id` | string | A stable, human-readable identifier for the proposal (e.g., the GitHub issue URL or a short code). Allows deduplication. |
| `proposal_title` | string | Human-readable title of the proposal being signaled on; for clarity in logs and reports. |
| `voter_address` | string | The Stellar address (starting with `G`) of the signer. Must be on-chain (a registered solver or user who submitted an intent). |
| `signal` | enum | One of `"for"`, `"against"`, `"abstain"`. Ranking choice is possible as a future extension (see §11). |
| `timestamp` | integer | Unix timestamp (seconds since epoch) when the message was signed; prevents replay attacks and helps date the signal. |
| `message` | string | Optional: a brief written rationale (e.g., "Audit reduces mainnet risk"). Max 500 characters. Includes in reports for transparency. |

### 4.2 Signing Process (Stellar Native)

1. **Canonicalize the message:** Serialize the JSON in canonical form (sorted keys, no extra whitespace):
   ```
   {"message":"...","proposal_id":"...","proposal_title":"...","signal":"...","timestamp":..., "voter_address":"..."}
   ```

2. **Sign with the voter's Stellar private key:**
   ```bash
   # Using the Stellar CLI or SDK, sign the canonical message
   stellar signer sign --secret-key <SOLVER_SECRET_KEY> <canonical_message>
   # Output: a base64-encoded signature
   ```

3. **Publish the message + signature:**
   - Submit to the signaling aggregator service (see §5) along with the signature.
   - Or: Post to a GitHub issue/discussion with the signature as a comment (for lightweight, decentralized collection).

### 4.3 Example Signed Message (CLI)

```bash
#!/bin/bash
# Solver script to signal on a proposal

PROPOSAL_ID="proposal-2026-10-15-treasury-audit"
PROPOSAL_TITLE="Fund Security Audit Q4 2026"
VOTER_ADDRESS="GXXXXXX..."
SIGNAL="for"
TIMESTAMP=$(date +%s)
MESSAGE="Audit is critical for mainnet launch."

# Canonical JSON (sorted keys, no whitespace)
CANONICAL='{"message":"Audit is critical for mainnet launch.","proposal_id":"proposal-2026-10-15-treasury-audit","proposal_title":"Fund Security Audit Q4 2026","signal":"for","timestamp":'$TIMESTAMP',"voter_address":"GXXXXXX..."}'

# Sign using Stellar CLI (requires secret key)
SIGNATURE=$(stellar signer sign --secret-key $SOLVER_SECRET_KEY "$CANONICAL" | grep "Signature:" | cut -d' ' -f2)

# Output the signed message
echo "{"
echo '  "payload": '$CANONICAL','
echo '  "signature": "'$SIGNATURE'"'
echo "}"
```

---

## 5. Aggregator Service (Reference Implementation)

A simple, off-chain service aggregates signed signals from multiple signers and publishes results. This can be run by a community member or automated.

### 5.1 Input: Signal Collection

**Collection method 1 (GitHub):**
- Signers post their signed message + signature as a comment on a designated GitHub discussion.
- The aggregator periodically crawls the discussion and collects new signals.

**Collection method 2 (HTTP endpoint):**
- A simple HTTP API accepts POST requests with signed messages:
  ```bash
  curl -X POST https://signals.vortex.example/aggregate \
    -H "Content-Type: application/json" \
    -d '{
      "payload": {...},
      "signature": "..."
    }'
  ```

**Collection method 3 (Decentralized):**
- No centralized collection; signers self-publish, and the aggregator reads from a public source (e.g., GitHub, IPFS, or a public bucket).

### 5.2 Verification Logic

For each signal:

1. **Parse the payload JSON.**
2. **Extract `voter_address` and `signature`.**
3. **Reconstruct the canonical JSON from the payload.**
4. **Verify the signature using the Stellar SDK:**
   ```python
   from stellar_sdk import verify_tx_envelope_signature
   
   def verify_signal(payload: dict, signature: str, voter_address: str) -> bool:
       """Verify a signal's signature against the voter address."""
       canonical = json.dumps(payload, sort_keys=True, separators=(',', ':'))
       
       # Stellar signature verification (pseudocode)
       public_key = voter_address  # Stellar addresses are derived from public keys
       try:
           return verify_stellar_signature(canonical, signature, public_key)
       except:
           return False
   ```

5. **Reject if verification fails.** Log the failure and skip this signal.

6. **Check for duplicates:** If `voter_address` and `proposal_id` have already been seen, keep only the most recent signal (highest timestamp). Discard older ones.

### 5.3 Weight Calculation

Once all signals are verified:

1. **Query the on-chain data** at the snapshot block (e.g., block height X, decided at proposal start):
   - For each `voter_address` that signed, fetch `compute_reputation_score(address)` from `intent_settlement`.
   - Fetch the solver's `bond` from `SolverRecord`.
   - Compute `weight = reputation_score × bond` (or the exact formula from issue #41).

2. **Aggregate by signal:**
   ```
   for_weight = sum(weight for each signer with signal="for")
   against_weight = sum(weight for each signer with signal="against")
   abstain_weight = sum(weight for each signer with signal="abstain")
   total_weight = for_weight + against_weight + abstain_weight
   ```

3. **Compute percentages:**
   ```
   for_pct = for_weight / total_weight * 100
   against_pct = against_weight / total_weight * 100
   abstain_pct = abstain_weight / total_weight * 100
   ```

### 5.4 Output: Aggregated Results

Publish a report (as JSON or markdown) with:

```json
{
  "proposal_id": "proposal-2026-10-15-treasury-audit",
  "proposal_title": "Fund Security Audit Q4 2026",
  "snapshot_block": 123456,
  "signal_window_start": "2026-10-15T00:00:00Z",
  "signal_window_end": "2026-10-22T00:00:00Z",
  "total_signers": 42,
  "total_weight": 5000000,
  "results": {
    "for": {
      "count": 32,
      "weight": 3500000,
      "percentage": 70.0
    },
    "against": {
      "count": 8,
      "weight": 1200000,
      "percentage": 24.0
    },
    "abstain": {
      "count": 2,
      "weight": 300000,
      "percentage": 6.0
    }
  },
  "top_signers": [
    {
      "address": "GXXXXX...",
      "signal": "for",
      "weight": 500000,
      "reputation_score": 1000,
      "bond": 500
    },
    ...
  ],
  "generated_at": "2026-10-22T12:00:00Z"
}
```

---

## 6. Integration with Governance Process (Issue #112)

Signaling results feed into the RFC discussion as follows:

1. **Proposal discussion begins** (issue #112 RFC phase, ~14 days).
2. **Signal window opens** (concurrent with or after discussion end).
3. **Aggregator publishes results** (48 hours after signal window closes).
4. **Results are posted to the GitHub RFC discussion:** A comment with the aggregated report, so the RFC discussion includes the sentiment summary.
5. **Admin makes approval decision** informed by sentiment + discussion + other factors.

**Example RFC post:**
```markdown
## Signal Results

A reputation-weighted temperature check was conducted from Oct 15–22, 2026.

**Results:**
- **For:** 70% (3.5M weight, 32 signers)
- **Against:** 24% (1.2M weight, 8 signers)
- **Abstain:** 6% (300K weight, 2 signers)

[Full report](./signals/2026-10-15-treasury-audit.json)

Interpretation: Strong community support (70%). The multisig admin will proceed with approval.
```

---

## 7. Reference Implementation (Python)

A minimal reference aggregator implementation is provided in `tools/signal_aggregator.py`:

```python
#!/usr/bin/env python3
"""
Off-chain reputation-weighted signaling aggregator.

Verifies signed Stellar messages, computes governance-weight aggregates,
and publishes results.
"""

import json
import sys
from datetime import datetime
from typing import Dict, List

from stellar_sdk import verify_tx_envelope_signature, PublicKey


class SignalAggregator:
    def __init__(self, intent_settlement_contract_id: str, rpc_url: str):
        self.contract_id = intent_settlement_contract_id
        self.rpc_url = rpc_url
        self.signals: Dict[str, dict] = {}  # {proposal_id: {address: signal}}
    
    def add_signal(self, payload: dict, signature: str) -> bool:
        """Verify and add a signal. Return True if valid."""
        canonical = json.dumps(payload, sort_keys=True, separators=(',', ':'))
        voter_address = payload.get("voter_address")
        proposal_id = payload.get("proposal_id")
        
        # Verify signature
        try:
            pk = PublicKey(voter_address)
            # Use Stellar SDK to verify (pseudocode)
            if not verify_stellar_signature(canonical, signature, voter_address):
                print(f"Signature verification failed for {voter_address}")
                return False
        except Exception as e:
            print(f"Error verifying signal from {voter_address}: {e}")
            return False
        
        # Dedup: keep latest
        if proposal_id not in self.signals:
            self.signals[proposal_id] = {}
        
        old_signal = self.signals[proposal_id].get(voter_address)
        if old_signal and old_signal["timestamp"] > payload["timestamp"]:
            print(f"Keeping older signal for {voter_address} (newer one ignored)")
            return False
        
        self.signals[proposal_id][voter_address] = {
            "signal": payload["signal"],
            "timestamp": payload["timestamp"],
            "message": payload.get("message", ""),
        }
        return True
    
    def aggregate_results(self, proposal_id: str, snapshot_block: int) -> dict:
        """Aggregate signals and compute weighted results."""
        signals = self.signals.get(proposal_id, {})
        
        # Fetch on-chain weights for each signer
        weights = {}
        for address in signals.keys():
            try:
                # Query on-chain at snapshot_block
                rep_score = self._get_reputation_score(address, snapshot_block)
                bond = self._get_solver_bond(address, snapshot_block)
                weights[address] = rep_score * bond
            except Exception as e:
                print(f"Error fetching weight for {address}: {e}")
                continue
        
        # Aggregate by signal
        for_weight = sum(w for addr, w in weights.items() if signals[addr]["signal"] == "for")
        against_weight = sum(w for addr, w in weights.items() if signals[addr]["signal"] == "against")
        abstain_weight = sum(w for addr, w in weights.items() if signals[addr]["signal"] == "abstain")
        
        total_weight = for_weight + against_weight + abstain_weight
        
        return {
            "proposal_id": proposal_id,
            "snapshot_block": snapshot_block,
            "total_signers": len(signals),
            "total_weight": total_weight,
            "results": {
                "for": {
                    "count": sum(1 for s in signals.values() if s["signal"] == "for"),
                    "weight": for_weight,
                    "percentage": (for_weight / total_weight * 100) if total_weight > 0 else 0,
                },
                "against": {
                    "count": sum(1 for s in signals.values() if s["signal"] == "against"),
                    "weight": against_weight,
                    "percentage": (against_weight / total_weight * 100) if total_weight > 0 else 0,
                },
                "abstain": {
                    "count": sum(1 for s in signals.values() if s["signal"] == "abstain"),
                    "weight": abstain_weight,
                    "percentage": (abstain_weight / total_weight * 100) if total_weight > 0 else 0,
                },
            },
            "generated_at": datetime.utcnow().isoformat() + "Z",
        }
    
    def _get_reputation_score(self, address: str, block: int) -> int:
        """Fetch reputation score from on-chain at snapshot block (pseudocode)."""
        # Query intent_settlement contract at block
        pass
    
    def _get_solver_bond(self, address: str, block: int) -> int:
        """Fetch solver bond from on-chain at snapshot block (pseudocode)."""
        # Query intent_settlement contract at block
        pass
```

**Usage:**
```bash
# Collect signals from GitHub discussion
python3 tools/signal_aggregator.py \
  --contract CXXXXXX \
  --proposal-id proposal-2026-10-15-treasury-audit \
  --snapshot-block 123456 \
  --github-discussion-url https://github.com/stellar-vortex-protocol/vortex-contracts/discussions/302

# Output: results.json with aggregated signals
```

---

## 8. Limitations & Future Extensions

### 8.1 Current Scope (v1)

- Binary signals only: for, against, abstain.
- Snapshot-at-proposal-start (no in-flight reputation changes during signal window).
- Off-chain verification; no on-chain enforcement.

### 8.2 Future Extensions

- **Ranked-choice voting:** Allow signers to rank proposals (1st choice, 2nd, etc.).
- **Delegated voting:** A solver can delegate their governance weight to another address.
- **Time-weighted voting:** Weight based on how long an address has been active (recency discount).
- **On-chain vote locking:** The signal itself is recorded on-chain as a non-binding statement (costs gas, but immutable).

---

## 9. Dependency on Issue #41

**Critical:** This tool reuses issue #41's governance-weight formula. Before this issue ships:

1. **Issue #41 must publish:**
   - The exact formula (reputation × bond, or alternative).
   - Snapshot timing (when weights are measured).
   - Any special cases (e.g., non-solver addresses, tie-breaking rules).

2. **This tool will implement:**
   - The published formula, verbatim (no re-derivation).
   - The snapshot block at proposal announcement time.

If issue #41 changes its formula later, this tool's aggregation logic updates in sync.

---

## 10. Testing

Minimum test coverage:

1. **Signature verification:**
   - ✅ Accept a validly-signed message from a real Stellar keypair.
   - ✅ Reject a forged signature.
   - ✅ Reject a signature from an address not on-chain.

2. **De-duplication:**
   - ✅ When the same address signals twice on the same proposal, keep only the latest signal (highest timestamp).
   - ✅ Reject a duplicate with an older timestamp.

3. **Weight aggregation:**
   - ✅ Correctly sum weights for each signal type (for, against, abstain).
   - ✅ Correctly compute percentages.
   - ✅ Handle zero total weight (division by zero).

4. **End-to-end:**
   - ✅ Given 10 test signals (mix of for/against/abstain), verify the aggregated report matches manual calculation.

---

## 11. Example: Full Signal Flow

**Scenario:** A proposal to fund a security audit is posted to GitHub (issue #302 as an RFC).

**Step 1: Proposal announcement**
- RFC discussion opens on Oct 15, 2026, 00:00 UTC.
- Snapshot block is set to the current mainnet block: 123456.
- Signal window opens immediately and closes Oct 22, 2026, 18:00 UTC.

**Step 2: Solvers signal**
- Solver A (bond: 500 USDC, reputation score: 1000) signs "for" the proposal.
  ```
  {"proposal_id": "...", ..., "signal": "for", "timestamp": 1698067200}
  ```
- Solver B (bond: 200 USDC, reputation score: 500) signs "against".
- 40 other participants signal (mix of for/against/abstain).

**Step 3: Aggregation**
```bash
python3 tools/signal_aggregator.py \
  --contract C... \
  --proposal proposal-2026-10-15-treasury-audit \
  --snapshot 123456 \
  --signals-file signals.jsonl  # Line-delimited JSON
```

**Step 4: Results published**
```json
{
  "proposal_id": "proposal-2026-10-15-treasury-audit",
  "total_signers": 42,
  "total_weight": 5000000,
  "results": {
    "for": {"count": 32, "weight": 3500000, "percentage": 70.0},
    "against": {"count": 8, "weight": 1200000, "percentage": 24.0},
    "abstain": {"count": 2, "weight": 300000, "percentage": 6.0}
  }
}
```

**Step 5: RFC discussion updated**
The results are posted to the GitHub discussion. The multisig admin reviews and approves the proposal based on discussion + sentiment.

---

## 12. References

- **Issue #41:** Governance-weight formula (dependency).
- **Issue #112:** RFC governance process (integration point).
- **Issue #305:** This issue.
- **docs/treasury-spending-governance.md:** Spending proposals use signaling as an optional input.
- `tools/signal_aggregator.py` — Reference implementation.
