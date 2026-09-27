# Audit Report: Destination Token Allowlist Removal Against In-Flight Intents

**Issues Addressed:** #266  
**Status:** Audit Completed & Verified by Test Coverage  

---

## 1. Executive Summary

This audit investigates the security and operational safety of removing a destination token from the contract's allowlist (`execute_remove_dst_token`) while intents targeting that token are actively in-flight (`Accepted` or `PartiallyFilled` states).

---

## 2. Code Path Trace

### Allowlist Enforcement Point (`submit_intent`)
- In `intent_settlement/src/lib.rs`, `is_dst_token_allowed` is queried exclusively inside `submit_intent`:
  ```rust
  if DstAllowlistEnabled
      && !Self::is_dst_token_allowed(env.clone(), dst_token.clone())
  {
      panic_with_error!(&env, Error::DstTokenNotAllowed);
  }
  ```

### Settlement Path (`fill_intent`)
- During `fill_intent`, the contract checks:
  1. Intent exists and is in `Open` or `Accepted` / `PartiallyFilled` state.
  2. Deadline has not expired.
  3. Solver authentication and solver bond validity.
  4. Token transfer execution of `dst_token` from solver to `recipient`.
- **`fill_intent` never queries `is_dst_token_allowed` or `DataKey::AllowedDstToken`.**

---

## 3. Findings & Safety Verification

1. **Safety for In-Flight Intents:** Because `fill_intent` does not re-verify the token against `is_dst_token_allowed`, removing a token from the allowlist via `execute_remove_dst_token` after an intent has been submitted and accepted does **NOT** block or revert subsequent `fill_intent` execution.
2. **Protection Against New Intent Submissions:** Once `execute_remove_dst_token` completes and removes `DataKey::AllowedDstToken(token)`, any subsequent `submit_intent` call specifying `token` as `dst_token` will immediately revert with `Error::DstTokenNotAllowed`.
3. **Timelock Security:** Removal requires passing through the administrative timelock (`propose_remove_dst_token` followed by `ADMIN_TIMELOCK_DELAY`), allowing solvers ample time to fill or settle active commitments before new submissions cease.

---

## 4. Conclusion & Test Coverage

The behavior is verified to be safe by design. Explicit regression tests (`test_remove_dst_token_in_flight_intent_fill` and `test_remove_dst_token_blocks_new_submissions`) have been added to `intent_settlement/src/test.rs` to permanently prevent regressions.
