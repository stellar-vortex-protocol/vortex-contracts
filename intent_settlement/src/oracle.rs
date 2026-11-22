//! Oracle adapter module for Vortex Protocol (#368).
//!
//! Provides an `OracleAdapter` client interface compatible with SEP-40
//! (Reflector protocol). The settlement contract queries asset prices
//! through this adapter for exposure checks and min-bond adjustments.
//!
//! ## Staleness guard
//! Prices older than `MAX_PRICE_AGE_SECS` are rejected (fail-closed).
//!
//! ## Deviation guard
//! If two prices disagree by more than `MAX_PRICE_DEVIATION_BPS` the
//! data is treated as unreliable (fail-closed).
//!
//! ## Fail-closed behaviour
//! On stale / zero / missing price data:
//! - `accept_intent` exposure check → uses conservative 1.0× weight
//!   (the check still runs, so an over-exposed solver is still blocked)
//! - `slash_solver` → proceeds regardless (slashing must never be blocked)

use soroban_sdk::{contractclient, Address, Env};

// ─── SEP-40 compatible oracle client ─────────────────────────────────────────

/// SEP-40 `lastprice` result.
#[soroban_sdk::contracttype]
#[derive(Clone, Debug)]
pub struct PriceData {
    /// Price in oracle-defined units, normalised by `decimals`.
    pub price: i128,
    /// Decimal precision of `price`.
    pub decimals: u32,
    /// Unix timestamp of the observation (seconds).
    pub timestamp: u64,
}

/// Minimal SEP-40 oracle interface.
/// The settlement contract expects a deployed contract that implements at
/// least `lastprice(asset: Address) -> Option<PriceData>`.
#[contractclient(name = "OracleAdapterClient")]
pub trait OracleAdapterTrait {
    /// Return the latest price for `asset`, or `None` if unavailable.
    fn lastprice(env: Env, asset: Address) -> Option<PriceData>;
}

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum age of a valid price in seconds (5 minutes).
pub const MAX_PRICE_AGE_SECS: u64 = 300;

/// Maximum allowed deviation between two oracle sources, in basis points (2%).
pub const MAX_PRICE_DEVIATION_BPS: i128 = 200;

/// Value of `from` expressed in units of `to`, in basis points (10_000 = 1.0×),
/// derived from oracle prices. Fail-closed: returns the conservative 1.0×
/// weight when `from == to`, no oracle is configured, or either price is
/// stale / zero / missing / would overflow. Never traps.
pub fn weight_bps(env: &Env, oracle_addr: Option<&Address>, from: &Address, to: &Address) -> i128 {
    const NEUTRAL: i128 = 10_000;
    let Some(oracle_addr) = oracle_addr else {
        return NEUTRAL;
    };
    if from == to {
        return NEUTRAL;
    }
    let (Ok((pf, df)), Ok((pt, dt))) = (
        fetch_price(env, oracle_addr, from),
        fetch_price(env, oracle_addr, to),
    ) else {
        return NEUTRAL;
    };
    // (pf / 10^df) / (pt / 10^dt) * 10_000
    let w = 10i128
        .checked_pow(dt)
        .and_then(|a| pf.checked_mul(a))
        .and_then(|n| n.checked_mul(NEUTRAL))
        .and_then(|n| {
            10i128
                .checked_pow(df)
                .and_then(|b| pt.checked_mul(b))
                .and_then(|d| n.checked_div(d))
        });
    match w {
        Some(w) if w > 0 => w,
        _ => NEUTRAL,
    }
}

// ─── Result type ─────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum OracleError {
    /// Oracle returned `None` — no price available.
    NotAvailable,
    /// Price is older than `MAX_PRICE_AGE_SECS`.
    Stale,
    /// Price is zero or negative — unusable.
    ZeroOrNegative,
}

// ─── Public helpers ───────────────────────────────────────────────────────────

/// Fetch the price of `asset` from the oracle at `oracle_addr`.
///
/// Returns `Ok((price, decimals))` on success.
/// Returns `Err(OracleError)` if the price is stale, zero, or unavailable.
pub fn fetch_price(
    env: &Env,
    oracle_addr: &Address,
    asset: &Address,
) -> Result<(i128, u32), OracleError> {
    let client = OracleAdapterClient::new(env, oracle_addr);
    // `try_` so a trapping / non-conforming oracle is a fault, not a revert:
    // slashing must never be blocked by the oracle.
    match client.try_lastprice(asset).ok().and_then(|r| r.ok()).flatten() {
        None => Err(OracleError::NotAvailable),
        Some(pd) => {
            let now = env.ledger().timestamp();
            if now > pd.timestamp && now - pd.timestamp > MAX_PRICE_AGE_SECS {
                return Err(OracleError::Stale);
            }
            if pd.price <= 0 {
                return Err(OracleError::ZeroOrNegative);
            }
            Ok((pd.price, pd.decimals))
        }
    }
}

/// Check that two prices (from different sources) agree within
/// `MAX_PRICE_DEVIATION_BPS`. Returns `true` if they agree, `false`
/// if the deviation exceeds the threshold or either price is zero.
#[allow(dead_code)] // reserved for dual-source oracle setups
pub fn prices_agree(price_a: i128, dec_a: u32, price_b: i128, dec_b: u32) -> bool {
    if price_a <= 0 || price_b <= 0 {
        return false;
    }
    // Normalise to the higher precision.
    let (a, b) = if dec_a >= dec_b {
        let scale = 10i128.pow(dec_a - dec_b);
        (price_a, price_b.saturating_mul(scale))
    } else {
        let scale = 10i128.pow(dec_b - dec_a);
        (price_a.saturating_mul(scale), price_b)
    };
    let diff = (a - b).abs();
    let denom = a.max(b);
    // diff / denom <= MAX_PRICE_DEVIATION_BPS / 10_000
    diff.saturating_mul(10_000) <= MAX_PRICE_DEVIATION_BPS.saturating_mul(denom)
}
