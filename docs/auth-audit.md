# `require_auth()` Call Site Audit

Closes the "Authorization hardening" item in `docs/pre-deploy-security-checklist.md`
(#45, tracked here as #263). Every `require_auth()` call site in
`intent_settlement/src/lib.rs` was reviewed for whether upgrading to
`require_auth_for_args` would meaningfully reduce delegated-execution risk —
i.e. the risk that a third-party invoker contract calling on a signer's behalf
could redirect their signature toward unintended arguments.

## Upgraded

| Function | Old | New scope | Rationale |
|---|---|---|---|
| `submit_intent` | `user.require_auth()` | `(user, dst_token, min_dst_amount)` | If a composable invoker ever submits on a user's behalf, this prevents it from redirecting the user's signed submission to a different destination token or minimum output. |
| `accept_intent` | `solver.require_auth()` | `(intent_id,)` | Prevents a delegating invoker contract from having a solver accept a different intent than the one the solver actually signed for. |
| `fill_intent` | `solver.require_auth()` | `(solver, intent_id, fill_amount)` | Highest-value call site — the auth gates an outgoing token transfer. Prevents a delegating invoker from filling a different intent, or a different amount, than the solver signed for. |

`accept_intent`'s batch wrapper (`accept_intent_batch`) delegates to
`accept_intent` per element and needed no separate change.

## Kept as `require_auth()`

| Function | Signer | Rationale |
|---|---|---|
| `initialize` | `admin` | One-time setup; the signer *is* the value being recorded as admin — no sub-scope to narrow. |
| `propose_fee_recipient` | stored `admin` | Single global admin capability; no meaningful sub-scope within "being admin". |
| `accept_fee_recipient` | `new_fee_recipient` | Recipient proves ownership of their own address; the timelock and pending-proposal match (`pending != new_fee_recipient` check) already constrain which proposal can be accepted. |
| `propose_admin_transfer` | stored `admin` | Same as `propose_fee_recipient`. |
| `accept_admin_transfer` | `new_admin` | Same as `accept_fee_recipient`. |
| `register_solver` | `solver` | Solver consents to locking their own bond funds; simple self-action with no delegated-execution surface. |
| `deregister_solver` | `solver` | Solver-only self-action. |
| `withdraw_bond` | `solver` | Solver-only self-action on their own bond. |
| `cancel_intent` | `user` | Simple "cancel my own intent" self-action; an explicit `intent.user != user` ownership check runs immediately after, providing defence-in-depth. |
| `request_extension` | `solver` | Grants at most one grace-period extension per intent; no funds move and no cross-intent redirection is possible (the intent is loaded and ownership-checked before use). |
| `require_admin` (helper; gates `unpause`, `set_pauser`, dst-token allowlist admin functions) | `admin` | Single admin address with uniform authority across these functions — no per-argument capability to scope. |
| `require_admin_or_pauser` (helper; gates `pause`) | `admin` or `pauser` | Same reasoning as `require_admin`; the admin/pauser check already precedes the auth call. |

## Integration impact

`require_auth_for_args` changes the exact signed-payload shape a client must
build. `submit_intent`, `accept_intent`, and `fill_intent` are called
respectively by user-facing clients and solver bots — see
`docs/solver-integration-guide.md` for the updated payload shapes solver bot
authors must sign.
