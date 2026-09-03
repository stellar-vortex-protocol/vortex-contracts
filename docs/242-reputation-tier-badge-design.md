# Solver Reputation Tier Badge — Design Note

> **Status:** Draft prototype.
> **Closes:** #242
> **Resolves:** `docs/solver-registry-design.md` §10, open question 3 ("NFT-style
> on-chain tier badge — defer to v2?").

---

## 1. Decision

Build the badge now, as a minimal standalone prototype (`reputation_badge`
crate), rather than deferring it. It is small enough that prototyping it
does not block or complicate `solver_registry` (issue #1), and gives the
community a concrete artifact instead of an open question.

## 2. Transferable vs. soulbound

**Soulbound (non-transferable).** A tier badge represents a *fact about a
specific solver's current standing* (its bond size and reputation score at
`solver_registry`), not an asset with independent value. A transferable
badge would let a low-tier solver buy its way into a higher perceived tier
without the bond and fill history the tier is meant to certify, which
defeats its purpose as a trust signal. The prototype therefore exposes no
`transfer` entry point at all — non-transferability is enforced by the
interface, not by a runtime check.

## 3. SEP-41-shaped token vs. bespoke minimal contract

Evaluated reusing Soroban's standard token interface (SEP-41), non-transferable
by convention (an "always reverts" `transfer`):

- **Pro:** familiar interface for wallets/explorers that already render SEP-41
  balances.
- **Con:** SEP-41 models a *fungible balance per holder*. A solver's tier is
  a single enum value (`Bronze`/`Silver`/`Gold`/`Platinum`), not a quantity —
  modeling it as a balance would need a separate token contract instance per
  tier plus balance-of-1 semantics, adding real complexity for no behavior
  the badge needs.

**Decision:** a bespoke minimal contract storing `Address -> Tier` directly.
It is simpler, and its full public interface (`mint_badge`, `burn_badge`,
`get_badge`) already says exactly what it does — a SEP-41 wrapper would only
be worth it once a wallet/explorer integration is actually built, which is
out of scope here (§11 of `docs/solver-registry-design.md` excludes UI work).

## 4. Mint / burn trigger

Automatic, not manual: `mint_badge` and `burn_badge` are meant to be called
by `solver_registry`'s tier-computation logic whenever a solver's tier
changes (bond top-up/withdrawal or reputation-score movement crossing a
tier boundary from `docs/solver-registry-design.md` §3), not by the solver
itself. Until `solver_registry` (issue #1) exists to call them, both
entry points are gated behind `require_admin` as a placeholder authority —
swapping that gate for "caller is the `solver_registry` contract address"
is the integration point once issue #1 lands.

A tier *change* (not just a drop to `Unranked`) calls `mint_badge` again
with the new tier, overwriting the stored value in place — there is
intentionally no dangling old-tier badge left in storage for a UI to
mistakenly read.  A drop below the `Bronze` threshold (`Unranked`) calls
`burn_badge`, which removes the record entirely; `get_badge` then returns
`None`, so a badge's mere presence is itself proof of `Bronze`+ standing.

## 5. Out of scope (per issue #242)

- Any frontend/UI display of the badge.
- Marketplace or transfer functionality (excluded by design, §2 above).
- Wiring to `solver_registry` (issue #1 does not exist yet) — the admin gate
  above is the seam where that wiring lands.
