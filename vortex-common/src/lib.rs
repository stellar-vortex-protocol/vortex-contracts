#![no_std]

//! Vortex Protocol Shared Utilities
//!
//! Common constants, helpers, and types used across all vortex contracts.
//! Eliminates duplication and ensures consistency (issue #352).

pub use soroban_sdk::{Address, Env};

// ─── Common Error Type ───────────────────────────────────────────────────────
// Issue #505: Named error type instead of `()` so callers can map directly.

/// Shared error type for `vortex-common` helpers.
///
/// Soroban `#[contracterror]` enums must be fieldless `u32` variants so they
/// can cross the contract boundary; context fields cannot be attached to the
/// variants without changing the ABI. Off-chain diagnostic context (the
/// offending admin key and caller) is instead emitted via `log!` by the
/// helpers that return these errors, and shows up in simulation diagnostics.
#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CommonError {
  /// The caller is not the stored admin.
  Unauthorized = 1,
  /// The admin key is absent from storage (contract not initialized).
  NotInitialized = 2,
}

// ─── TTL Constants (Soroban Ledger Entry Lifecycle) ──────────────────────────
// Issue #352: Unified TTL constants prevent divergence between contracts

/// Approximate number of ledgers per day. Soroban has ~5s ledger close time.
pub const DAY_IN_LEDGERS: u32 = 17_280;

/// Threshold before persistent entries are archived by Soroban.
pub const PERSISTENT_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 14;

/// Duration to extend persistent TTL to (30 days).
pub const PERSISTENT_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 30;

/// Threshold for instance storage (contract metadata).
pub const INSTANCE_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 30;

/// Duration to extend instance TTL to (60 days).
pub const INSTANCE_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 60;

// ─── Tier Constants ──────────────────────────────────────────────────────────
// Issue #352: Unified tier constants for reputation system

/// Maximum solver reputation tier (0 = Unranked, 1–5 are ranked tiers).
pub const MAX_TIER: u32 = 5;

/// Slash percentage in basis points per tier: tier N is slashed at (N × TIER_SLASH_BPS).
/// E.g., tier 3 at 500 bps = 5% slash.
pub const TIER_SLASH_BPS: i128 = 100; // 1% per tier

/// Basis point scale for all fractional arithmetic.
pub const BPS: i128 = 10_000;

// ─── Math Helpers ────────────────────────────────────────────────────────────

/// Multiply `amount` by `bps` (basis points) and divide by BPS (10_000).
/// #352: Checked math to prevent overflow on large amounts.
///
/// Rounds **down** (toward zero). Use [`bps_mul_div_ceil`] when the protocol
/// must round in its own favour (e.g. a fee that may never be under-charged).
pub fn bps_mul_div(amount: i128, bps: i128) -> Option<i128> {
  amount
    .checked_mul(bps)?
    .checked_div(BPS)
}

/// Rounding direction for division operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rounding {
  /// Round towards zero (truncate) - default behavior.
  Floor,
  /// Round up (ceiling).
  Ceil,
  /// Round to nearest (half up).
  Nearest,
}

/// Multiply `amount` by `bps` and divide by BPS with explicit rounding direction.
/// Returns None on overflow.
pub fn bps_mul_div_round(amount: i128, bps: i128, rounding: Rounding) -> Option<i128> {
  let prod = amount.checked_mul(bps)?;
  match rounding {
    Rounding::Floor => Some(prod / BPS),
    Rounding::Ceil => {
      let (div, rem) = (prod / BPS, prod % BPS);
      if rem > 0 { div.checked_add(1) } else { Some(div) }
    }
    Rounding::Nearest => {
      let (div, rem) = (prod / BPS, prod % BPS);
      if rem * 2 >= BPS { div.checked_add(1) } else { Some(div) }
    }
  }
}

/// Multiply `amount` by `bps` (basis points) and divide by BPS (10_000),
/// rounding **up** (toward +∞) instead of down.
///
/// This is the ceiling counterpart of [`bps_mul_div`]. It generalises the
/// rounding-direction logic that `intent_settlement::math::compute_fee`
/// relies on (and that the `kani_fee_rounding_direction` harness formally
/// proves for the floor direction) into the shared crate, so any consumer
/// that needs "round in the protocol's favour" basis-point math has a single
/// reviewed helper instead of reimplementing the arithmetic.
///
/// #535: A single floor-rounding helper left no way to request ceiling
/// rounding, forcing every consumer to reimplement the same small,
/// security-sensitive math.
///
/// #352: Checked math to prevent overflow on large amounts.
///
/// ### Rounding
///
/// ```text
/// floor: amount * bps / 10_000        (truncating)
/// ceil:  ceil(amount * bps / 10_000)
/// ```
///
/// The two agree whenever `amount * bps` is an exact multiple of `BPS`, and
/// differ by exactly `1` otherwise (for a positive product). For a negative
/// product the ceiling still rounds toward `+∞` (i.e. toward zero), matching
/// the mathematical definition of `ceil` rather than Rust's truncating `/`.
///
/// Returns `None` if the multiplication overflows `i128`.
///
/// ### Examples
///
/// ```
/// use vortex_common::{bps_mul_div, bps_mul_div_ceil};
/// // 1000 * 5000 / 10_000 = 500 exactly — both directions agree.
/// assert_eq!(bps_mul_div(1000, 5000), Some(500));
/// assert_eq!(bps_mul_div_ceil(1000, 5000), Some(500));
/// // 1 * 1 / 10_000 = 0.0001 — floor drops it, ceil keeps one stroop.
/// assert_eq!(bps_mul_div(1, 1), Some(0));
/// assert_eq!(bps_mul_div_ceil(1, 1), Some(1));
/// ```
pub fn bps_mul_div_ceil(amount: i128, bps: i128) -> Option<i128> {
  let product = amount.checked_mul(bps)?;
  // Rust's `/` and `%` truncate toward zero, so `q` is the floor for a
  // positive product and the ceiling for a negative one.
  let quotient = product / BPS;
  let remainder = product % BPS;
  if remainder == 0 {
    // Exact multiple: floor == ceil, no adjustment needed.
    Some(quotient)
  } else if product > 0 {
    // Positive product with a fractional part: round up by one.
    quotient.checked_add(1)
  } else {
    // Negative product: truncation already produced the ceiling.
    Some(quotient)
  }
}

/// Compute slash amount for a given tier (tier × TIER_SLASH_BPS of the principal).
/// Tier 0 (Unranked) = no slash.
pub fn compute_slash_bps(tier: u32) -> i128 {
  (tier as i128)
    .checked_mul(TIER_SLASH_BPS)
    .unwrap_or(0)
    .min(BPS)
}

// ─── Admin Pattern ───────────────────────────────────────────────────────────
// Issue #352: Two-step admin transfer is consistent across contracts
// Note: the unused `AdminStorage` and `Versioned` traits were removed as dead code (issue #506);
// the #353 migration framework uses `CURRENT_SCHEMA_VERSION` instead of `Versioned`.

/// Helper to assert the caller is admin.
///
/// Returns [`CommonError::NotInitialized`] if no admin is stored under
/// `admin_key`, and [`CommonError::Unauthorized`] if `caller` is not the admin.
///
/// `admin_key` is the instance-storage key under which the admin `Address` is
/// stored. It must be a concrete storage-key type (e.g. a `Symbol` or a
/// contract-defined key enum) that implements `IntoVal<Env, Val>` so it can be
/// passed to `storage().instance().get(...)`.
///
/// #575: The `IntoVal` bound was written as `IntoVal<Env>` with a single
/// generic argument, but the trait takes two (`IntoVal<E, T>`). The crate
/// therefore did not compile, which blocked the unit-test suite — and with it
/// any test of the helpers above — from running at all. The bound is spelled
/// out in full here (`IntoVal<Env, Val>`) so the crate builds; the intended
/// meaning — "any key that can be converted into a storage key value" — is
/// unchanged.
pub fn require_admin(
  env: &Env,
  admin_key: impl soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
  caller: &Address,
) -> Result<(), CommonError> {
  let admin: Address = env
    .storage()
    .instance()
    .get(&admin_key)
    .ok_or_else(|| {
      soroban_sdk::log!(env, "require_admin: admin not initialized");
      CommonError::NotInitialized
    })?;
  if admin != *caller {
    soroban_sdk::log!(env, "require_admin: caller is not admin", caller, admin);
    return Err(CommonError::Unauthorized);
  }
  Ok(())
}

// ─── Versioning for Storage Migrations (Issue #353) ──────────────────────────

/// Schema version for persistent records. Enables lazy migrations on first touch.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_bps_mul_div() {
    // 1000 * 5000 BPS / 10_000 = 500
    assert_eq!(bps_mul_div(1000, 5000), Some(500));
    // Overflow case
    assert_eq!(bps_mul_div(i128::MAX, BPS + 1), None);
  }

  // ── bps_mul_div_ceil (#535) ────────────────────────────────────────────────

  #[test]
  fn test_bps_mul_div_ceil_exact_multiple_agrees_with_floor() {
    // 1000 * 5000 BPS / 10_000 = 500 exactly — no fractional part, so the
    // ceiling and the floor must agree.
    assert_eq!(bps_mul_div_ceil(1000, 5000), Some(500));
    assert_eq!(bps_mul_div_ceil(1000, 5000), bps_mul_div(1000, 5000));
  }

  #[test]
  fn test_bps_mul_div_ceil_rounds_up_where_floor_rounds_down() {
    // 1 * 1 BPS / 10_000 = 0.0001 → floor 0, ceil 1.
    assert_eq!(bps_mul_div(1, 1), Some(0));
    assert_eq!(bps_mul_div_ceil(1, 1), Some(1));
    // 9999 * 9999 BPS / 10_000 = 9998.0001 → floor 9998, ceil 9999.
    assert_eq!(bps_mul_div(9999, 9999), Some(9998));
    assert_eq!(bps_mul_div_ceil(9999, 9999), Some(9999));
  }

  #[test]
  fn test_bps_mul_div_ceil_never_below_floor() {
    for amount in [-1_000_000i128, -7, 0, 1, 7, 999, 1_000_000] {
      for bps in [-10_000i128, -1, 0, 1, 3_333, 10_000] {
        let floor = bps_mul_div(amount, bps).unwrap();
        let ceil = bps_mul_div_ceil(amount, bps).unwrap();
        assert!(ceil >= floor, "ceil {ceil} < floor {floor} for {amount}*{bps}");
        // The two differ by at most one unit.
        assert!(
          floor - ceil <= 1 && ceil - floor <= 1,
          "ceil and floor differ by more than 1 for {amount}*{bps}"
        );
      }
    }
  }

  #[test]
  fn test_bps_mul_div_ceil_is_exact_ceiling() {
    for amount in [0i128, 1, 2, 10, 123, 4_567, 1_000_000] {
      for bps in [1i128, 7, 250, 3_333, 9_999, 10_000] {
        let ceil = bps_mul_div_ceil(amount, bps).unwrap();
        let floor = bps_mul_div(amount, bps).unwrap();
        // ceil * BPS >= amount * bps  (never under-charges)
        assert!(
          ceil.checked_mul(BPS).unwrap() >= amount.checked_mul(bps).unwrap(),
          "ceil must not be below the exact rational value"
        );
        // (ceil - 1) * BPS < amount * bps  (is the minimal such integer)
        if ceil > 0 {
          assert!(
            (ceil - 1).checked_mul(BPS).unwrap() < amount.checked_mul(bps).unwrap(),
            "ceil must be the minimal integer not below the exact value"
          );
        }
        // Exact multiples are not bumped.
        if amount.checked_mul(bps).unwrap() % BPS == 0 {
          assert_eq!(ceil, floor);
        }
      }
    }
  }

  #[test]
  fn test_bps_mul_div_ceil_negative_product_rounds_toward_zero() {
    // ceil(-1 * 1 / 10_000) = ceil(-0.0001) = 0 (mathematical ceiling).
    assert_eq!(bps_mul_div_ceil(-1, 1), Some(0));
    assert_eq!(bps_mul_div(-1, 1), Some(0));
    // ceil(-10_005 / 10_000) = ceil(-1.0005) = -1.
    assert_eq!(bps_mul_div_ceil(-10_005, 1), Some(-1));
    assert_eq!(bps_mul_div(-10_005, 1), Some(-1));
    // Exact negative multiple: no adjustment.
    assert_eq!(bps_mul_div_ceil(-10_000, 1), Some(-1));
  }

  #[test]
  fn test_bps_mul_div_ceil_zero_inputs() {
    assert_eq!(bps_mul_div_ceil(0, 5_000), Some(0));
    assert_eq!(bps_mul_div_ceil(1_000, 0), Some(0));
    assert_eq!(bps_mul_div_ceil(0, 0), Some(0));
  }

  #[test]
  fn test_bps_mul_div_ceil_overflow_returns_none() {
    // Same overflow guard as the floor variant.
    assert_eq!(bps_mul_div_ceil(i128::MAX, BPS + 1), None);
    assert_eq!(bps_mul_div_ceil(i128::MAX, i128::MAX), None);
  }

  #[test]
  fn test_bps_mul_div_round() {
    // Exact multiple: all directions agree.
    assert_eq!(bps_mul_div_round(1000, 5000, Rounding::Floor), Some(500));
    assert_eq!(bps_mul_div_round(1000, 5000, Rounding::Ceil), Some(500));
    assert_eq!(bps_mul_div_round(1000, 5000, Rounding::Nearest), Some(500));
    // 1 * 1 / 10_000 = 0.0001 → floor 0, ceil 1, nearest 0.
    assert_eq!(bps_mul_div_round(1, 1, Rounding::Floor), Some(0));
    assert_eq!(bps_mul_div_round(1, 1, Rounding::Ceil), Some(1));
    assert_eq!(bps_mul_div_round(1, 1, Rounding::Nearest), Some(0));
    // 1 * 5000 / 10_000 = 0.5 → nearest rounds half up to 1.
    assert_eq!(bps_mul_div_round(1, 5000, Rounding::Nearest), Some(1));
    // Overflow case.
    assert_eq!(bps_mul_div_round(i128::MAX, BPS + 1, Rounding::Floor), None);
  }

  #[test]
  fn test_compute_slash_bps() {
    assert_eq!(compute_slash_bps(0), 0); // Unranked, no slash
    assert_eq!(compute_slash_bps(1), 100); // Tier 1 = 1%
    assert_eq!(compute_slash_bps(5), 500); // Tier 5 = 5%
    assert_eq!(compute_slash_bps(101), 10_000); // Capped at 100% (BPS)
  }

  #[test]
  fn test_ttl_constants() {
    assert!(PERSISTENT_TTL_THRESHOLD < PERSISTENT_TTL_EXTEND_TO);
    assert!(INSTANCE_TTL_THRESHOLD < INSTANCE_TTL_EXTEND_TO);
  }

  #[test]
  fn test_common_error_values() {
    assert_eq!(CommonError::Unauthorized as u32, 1);
    assert_eq!(CommonError::NotInitialized as u32, 2);
  }

  #[test]
  fn test_require_admin() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let other = Address::generate(&env);
    let key = soroban_sdk::symbol_short!("admin");

    // Missing admin key -> NotInitialized.
    assert_eq!(
      require_admin(&env, key.clone(), &admin),
      Err(CommonError::NotInitialized)
    );

    env.storage().instance().set(&key, &admin);

    // Matching caller -> Ok.
    assert_eq!(require_admin(&env, key.clone(), &admin), Ok(()));

    // Non-matching caller -> Unauthorized.
    assert_eq!(
      require_admin(&env, key, &other),
      Err(CommonError::Unauthorized)
    );
  }
}
