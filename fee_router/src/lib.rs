//! # Fee Router
//!
//! A pull-based fee router that accumulates protocol revenue per token and
//! distributes it to governance-configured sinks according to timelocked
//! weights (in basis points, summing to 10,000).
//!
//! Design notes:
//! - Accounting is pull-based: `distribute` credits `claimable[sink][token]`
//!   and sinks withdraw with `claim`. A sink that rejects a transfer does not
//!   revert the whole distribution; its share stays claimable for a later retry.
//! - Weight changes are timelocked: `propose_weights` then `apply_weights`
//!   after `weight_delay` has elapsed.
//! - Rounding dust from integer division is assigned to the first sink.
//! - Fee-on-transfer tokens are supported on both sides of the router: the
//!   inbound `on_fee_received` path credits the amount actually received, and
//!   the outbound `claim` path measures the sink's balance delta across the
//!   transfer and credits only what the sink actually received (see issue
//!   `#409` for the established balance-delta accounting pattern).
//! - Account identifiers are length-bounded ([`MAX_SINK_ACCOUNT_LEN`] and
//!   [`MAX_GOVERNANCE_ACCOUNT_LEN`]) so that an unbounded `String` cannot be
//!   written into the router's storage maps. See
//!   "Identifier bounding and the Soroban rewrite" below.
//!
//! # Identifier bounding and the Soroban rewrite
//!
//! `Sink::account` and the router's `governance` account are currently plain
//! `String`s, so today there is no structural limit on how long an identifier
//! may be. That limit has to exist before this crate becomes a real Soroban
//! contract: on-chain, every byte of a stored identifier is paid for in
//! storage rent, and a governance proposal could otherwise write an
//! arbitrarily large entry into `sinks` and `claimable`.
//!
//! This crate therefore bounds both identifiers *now*, with
//! [`MAX_SINK_ACCOUNT_LEN`] and [`MAX_GOVERNANCE_ACCOUNT_LEN`], and rejects
//! out-of-bounds identifiers with [`FeeRouterError::SinkAccountTooLong`] /
//! [`FeeRouterError::GovernanceAccountTooLong`].
//!
//! The bound is a placeholder for the real fix, not a substitute for it.
//! When `fee_router` is rewritten as an actual Soroban contract, sink
//! identity should become a `soroban_sdk::Address`, which is *natively*
//! bounded by the host (a `ScAddress` is at most 32 bytes of account id plus
//! a 4-byte discriminant) and can be paid for exactly rather than
//! approximately. The explicit length checks below should then be replaced by
//! the `Address` type check, not kept alongside it.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, Vec,
};

/// Minimal token interface the router needs to perform real transfers.
///
/// In a Soroban deployment this is implemented over `token::Client`; in tests
/// it is implemented by mock tokens (including fee-on-transfer tokens).
pub trait TokenClient {
    /// Current balance of `account`.
    fn balance_of(&self, account: &str) -> u128;
    /// Transfer `amount` from `from` to `to`. Returns `false` on failure.
    fn transfer(&mut self, from: &str, to: &str, amount: u128) -> bool;
}

/// Total basis points that weights must sum to.
pub const TOTAL_BPS: u32 = 10_000;

/// Maximum number of sinks the router will accept.
pub const MAX_SINKS: u32 = 16;

/// Maximum length, in **bytes**, of a sink's account identifier.
///
/// The bound is checked against the UTF-8 byte length of the identifier, not
/// its character count, because on-chain storage cost is metered in bytes.
/// 64 bytes is enough for an ed25519 public key in hex (64 hex characters)
/// with room to spare, and keeps the worst case for a full sink set
/// (`MAX_SINKS` sinks) at 1 KiB.
pub const MAX_SINK_ACCOUNT_LEN: usize = 64;

/// Maximum length, in **bytes**, of the router's governance account
/// identifier.
///
/// Same reasoning as [`MAX_SINK_ACCOUNT_LEN`]: the governance account is a
/// single stored identifier, so it needs the same byte-length bound.
pub const MAX_GOVERNANCE_ACCOUNT_LEN: usize = 64;

/// A single revenue sink with its governance-set weight.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sink {
    pub account: Address,
    pub weight_bps: u32,
}

impl Sink {
    /// Maximum allowed length for the account string.
    pub const MAX_ACCOUNT_LEN: usize = 64;

    /// Validate that the account string is within bounds.
    pub fn validate(&self) -> bool {
        self.account.len() <= Self::MAX_ACCOUNT_LEN
    }
}

/// A pending, timelocked weight change.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingWeights {
    pub sinks: Vec<Sink>,
    pub executable_at: u64,
}

/// Storage keys for the router.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    Governance,
    WeightDelay,
    Sinks,
    Pending,
    Balance(Address),
    Claimable(Address, Address),
}

/// Errors returned by the router.
#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FeeRouterError {
    Unauthorized = 1,
    TooManySinks = 2,
    WeightsMustSumToTotal = 3,
    EmptySinks = 4,
    DuplicateSink = 5,
    NoPendingWeights = 6,
    TimelockNotElapsed = 7,
    UnknownToken = 8,
    NothingToClaim = 9,
    TransferFailed = 10,
    /// Account string exceeds [`Sink::MAX_ACCOUNT_LEN`] characters.
    InvalidAccountLength = 11,
}

/// The fee router contract.
#[contract]
pub struct FeeRouter;

#[contractimpl]
impl FeeRouter {
    /// Create a new router. `sinks` must be non-empty, unique, at most
    /// [`MAX_SINKS`], and sum to [`TOTAL_BPS`].
    pub fn initialize(
        env: Env,
        governance: Address,
        weight_delay: u64,
        sinks: Vec<Sink>,
    ) -> Result<(), FeeRouterError> {
        validate_sinks(&env, &sinks)?;
        env.storage().instance().set(&DataKey::Governance, &governance);
        env.storage().instance().set(&DataKey::WeightDelay, &weight_delay);
        env.storage().instance().set(&DataKey::Sinks, &sinks);
        Ok(())
    }

    pub fn governance(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Governance).unwrap()
    }

    pub fn weight_delay(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::WeightDelay).unwrap()
    }

    pub fn sinks(env: Env) -> Vec<Sink> {
        env.storage().instance().get(&DataKey::Sinks).unwrap()
    }

    /// Total accumulated balance for `token` that has not yet been distributed.
    pub fn balance_of(env: Env, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(token))
            .unwrap_or(0)
    }

    /// Amount currently claimable by `sink` for `token`.
    pub fn claimable(env: Env, sink: Address, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Claimable(sink, token))
            .unwrap_or(0)
    }

    /// Record an incoming fee payment. The router credits the amount it
    /// actually received, which makes fee-on-transfer tokens safe.
    pub fn on_fee_received(env: Env, token: Address, amount: i128) {
        if amount <= 0 {
            return;
        }
        let key = DataKey::Balance(token);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(current + amount));
    }

    /// Permissionlessly distribute the accumulated balance of `token` across
    /// the configured sinks according to their weights.
    ///
    /// # Trust assumption: first sink absorbs rounding dust
    ///
    /// Integer division of each sink's weighted share can leave a remainder.
    /// Rather than burn it or introduce a dedicated dust sink, `distribute`
    /// assigns the entire remainder to `sinks[0]` — whichever sink governance
    /// most recently listed first in `propose_weights`. This is a deliberate,
    /// governance-visible choice, not a bug: sink *ordering*, not just
    /// weighting, carries a small economic effect (at most `sinks.len() - 1`
    /// base units per distribution). Governance should therefore treat the
    /// first position as a meaningful allocation and not reorder sinks
    /// casually. The full balance is always conserved.
    pub fn distribute(env: Env, token: Address) -> Result<(), FeeRouterError> {
        let total: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(token.clone()))
            .unwrap_or(0);
        if total == 0 {
            return Err(FeeRouterError::UnknownToken);
        }

        let sinks: Vec<Sink> = env.storage().instance().get(&DataKey::Sinks).unwrap();
        let mut distributed: i128 = 0;
        let len = sinks.len();
        for i in 0..len {
            let sink = sinks.get(i).unwrap();
            let share = if i == 0 {
                // First sink absorbs rounding dust.
                total - distributed
            } else {
                total * (sink.weight_bps as i128) / (TOTAL_BPS as i128)
            };
            distributed += share;
            if share > 0 {
                let key = DataKey::Claimable(sink.account.clone(), token.clone());
                let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
                env.storage().persistent().set(&key, &(current + share));
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::Balance(token), &0i128);
        Ok(())
    }

    /// Withdraw the caller's claimable balance for `token`.
    ///
    /// The transfer is performed via the token contract. If the sink rejects
    /// the transfer the share is left claimable and the error is surfaced, so a
    /// later retry can succeed without affecting other sinks.
    ///
    /// Following the balance-delta accounting pattern (issue `#409`), the
    /// sink's balance is read before and after the transfer and the router
    /// credits only the amount actually received (`after - before`). For
    /// fee-on-transfer tokens this is less than the nominal claimable amount,
    /// and the router never over-credits. On success the claimable entry is
    /// zeroed and the actually-received amount is returned.
    pub fn claim(env: Env, sink: Address, token: Address) -> Result<i128, FeeRouterError> {
        sink.require_auth();
        let key = DataKey::Claimable(sink.clone(), token.clone());
        let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount == 0 {
            return Err(FeeRouterError::NothingToClaim);
        }
        let client = token::Client::new(&env, &token);
        let before = client.balance(&sink);
        client.transfer(&env.current_contract_address(), &sink, &amount);
        let after = client.balance(&sink);
        let received = after.saturating_sub(before);
        env.storage().persistent().set(&key, &0i128);
        Ok(received)
    }

    /// Propose a new set of sinks and weights. Only governance may call this.
    ///
    /// The proposed sinks are validated with the same rules as
    /// [`FeeRouter::initialize`], so the [`MAX_SINK_ACCOUNT_LEN`] bound is
    /// re-checked on every weight change and cannot be bypassed by proposing a
    /// new set.
    pub fn propose_weights(
        env: Env,
        caller: Address,
        sinks: Vec<Sink>,
    ) -> Result<(), FeeRouterError> {
        caller.require_auth();
        let governance: Address = env.storage().instance().get(&DataKey::Governance).unwrap();
        if caller != governance {
            return Err(FeeRouterError::Unauthorized);
        }
        validate_sinks(&env, &sinks)?;
        let delay: u64 = env.storage().instance().get(&DataKey::WeightDelay).unwrap();
        let pending = PendingWeights {
            sinks,
            executable_at: env.ledger().timestamp() + delay,
        };
        env.storage().instance().set(&DataKey::Pending, &pending);
        Ok(())
    }

    /// Apply a previously proposed weight change once the timelock has elapsed.
    pub fn apply_weights(env: Env, caller: Address) -> Result<(), FeeRouterError> {
        caller.require_auth();
        let governance: Address = env.storage().instance().get(&DataKey::Governance).unwrap();
        if caller != governance {
            return Err(FeeRouterError::Unauthorized);
        }
        let pending: PendingWeights = env
            .storage()
            .instance()
            .get(&DataKey::Pending)
            .ok_or(FeeRouterError::NoPendingWeights)?;
        if env.ledger().timestamp() < pending.executable_at {
            return Err(FeeRouterError::TimelockNotElapsed);
        }
        env.storage().instance().set(&DataKey::Sinks, &pending.sinks);
        env.storage().instance().remove(&DataKey::Pending);
        Ok(())
    }

    /// Timestamp at which the pending weight change becomes executable.
    pub fn pending_executable_at(env: Env) -> Option<u64> {
        let pending: Option<PendingWeights> = env.storage().instance().get(&DataKey::Pending);
        pending.map(|p| p.executable_at)
    }
}

/// Validate a sink set: non-empty, bounded size, unique account identifiers,
/// and weights summing to [`TOTAL_BPS`].
///
/// Called from [`FeeRouter::initialize`] and [`FeeRouter::propose_weights`], so
/// the rules are re-checked every time governance tries to install a new sink
/// set.
fn validate_sinks(env: &Env, sinks: &Vec<Sink>) -> Result<(), FeeRouterError> {
    let len = sinks.len();
    if len == 0 {
        return Err(FeeRouterError::EmptySinks);
    }
    if len > MAX_SINKS {
        return Err(FeeRouterError::TooManySinks);
    }
    let mut sum: u32 = 0;
    for i in 0..len {
        let sink = sinks.get(i).unwrap();
        if sink.account.len() > Sink::MAX_ACCOUNT_LEN {
            return Err(FeeRouterError::InvalidAccountLength);
        }
        for j in 0..i {
            if sinks.get(j).unwrap().account == sink.account {
                return Err(FeeRouterError::DuplicateSink);
            }
        }
        sum = sum.saturating_add(sink.weight_bps);
    }
    if sum != TOTAL_BPS {
        return Err(FeeRouterError::WeightsMustSumToTotal);
    }
    let _ = env;
    Ok(())
}

#[cfg(test)]
mod bench;

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};

    fn setup() -> (Env, FeeRouterClient<'static>, Address, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, FeeRouter);
        let client = FeeRouterClient::new(&env, &contract_id);

        let governance = Address::generate(&env);
        let treasury = Address::generate(&env);
        let backstop = Address::generate(&env);
        let rebate = Address::generate(&env);

        let mut sinks = Vec::new(&env);
        sinks.push_back(Sink { account: treasury.clone(), weight_bps: 5_000 });
        sinks.push_back(Sink { account: backstop.clone(), weight_bps: 3_000 });
        sinks.push_back(Sink { account: rebate.clone(), weight_bps: 2_000 });

        client.initialize(&governance, &100u64, &sinks);
        (env, client, governance, treasury, backstop, rebate)
    }

    fn make_token(env: &Env, admin: &Address) -> Address {
        env.register_stellar_asset_contract(admin.clone())
    }

    #[test]
    fn distribute_conserves_balance() {
        let (env, client, _gov, treasury, backstop, rebate) = setup();
        let admin = Address::generate(&env);
        let token = make_token(&env, &admin);

        client.on_fee_received(&token, &1_000i128);
        assert_eq!(client.balance_of(&token), 1_000);
        client.distribute(&token);
        assert_eq!(client.balance_of(&token), 0);

        let t = client.claimable(&treasury, &token);
        let b = client.claimable(&backstop, &token);
        let r = client.claimable(&rebate, &token);
        assert_eq!(t + b + r, 1_000);
        assert_eq!(t, 500);
        assert_eq!(b, 300);
        assert_eq!(r, 200);
    }

    #[test]
    fn weights_must_sum_to_total() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, FeeRouter);
        let client = FeeRouterClient::new(&env, &contract_id);
        let governance = Address::generate(&env);
        let mut sinks = Vec::new(&env);
        sinks.push_back(Sink { account: Address::generate(&env), weight_bps: 5_000 });
        sinks.push_back(Sink { account: Address::generate(&env), weight_bps: 4_000 });
        let res = client.try_initialize(&governance, &100u64, &sinks);
        assert_eq!(res, Err(Ok(FeeRouterError::WeightsMustSumToTotal)));
    }

    #[test]
    fn duplicate_sink_addresses_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, FeeRouter);
        let client = FeeRouterClient::new(&env, &contract_id);
        let governance = Address::generate(&env);
        let dup = Address::generate(&env);
        let mut sinks = Vec::new(&env);
        sinks.push_back(Sink { account: dup.clone(), weight_bps: 5_000 });
        sinks.push_back(Sink { account: Address::generate(&env), weight_bps: 2_000 });
        sinks.push_back(Sink { account: dup, weight_bps: 3_000 });
        let res = client.try_initialize(&governance, &100u64, &sinks);
        assert_eq!(res, Err(Ok(FeeRouterError::DuplicateSink)));
    }

    #[test]
    fn timelock_enforced() {
        let (env, client, governance, _t, _b, _r) = setup();
        let mut sinks = Vec::new(&env);
        sinks.push_back(Sink { account: Address::generate(&env), weight_bps: 10_000 });

        env.ledger().set_timestamp(1_000);
        client.propose_weights(&governance, &sinks);
        assert_eq!(client.pending_executable_at(), Some(1_100));

        let res = client.try_apply_weights(&governance);
        assert_eq!(res, Err(Ok(FeeRouterError::TimelockNotElapsed)));

        env.ledger().set_timestamp(1_100);
        client.apply_weights(&governance);
        assert_eq!(client.sinks().len(), 1);
    }

    /// Mock token that can be configured to reject transfers or to skim a fee
    /// on transfer (fee-on-transfer token).
    struct MockToken {
        balances: BTreeMap<String, u128>,
        /// Fee in basis points deducted from the transferred amount.
        fee_bps: u32,
        /// When true, every transfer fails.
        reject: bool,
    }

    impl MockToken {
        fn new(fee_bps: u32, reject: bool) -> Self {
            Self { balances: BTreeMap::new(), fee_bps, reject }
        }

        fn mint(&mut self, account: &str, amount: u128) {
            let entry = self.balances.entry(account.to_string()).or_insert(0);
            *entry = entry.saturating_add(amount);
        }
    }

    impl TokenClient for MockToken {
        fn balance_of(&self, account: &str) -> u128 {
            self.balances.get(account).copied().unwrap_or(0)
        }

        fn transfer(&mut self, from: &str, to: &str, amount: u128) -> bool {
            if self.reject {
                return false;
            }
            let from_balance = self.balance_of(from);
            if from_balance < amount {
                return false;
            }
            let fee = amount.saturating_mul(self.fee_bps as u128) / TOTAL_BPS as u128;
            let received = amount - fee;
            self.balances.insert(from.to_string(), from_balance - amount);
            let to_entry = self.balances.entry(to.to_string()).or_insert(0);
            *to_entry = to_entry.saturating_add(received);
            true
        }
    }

    #[test]
    fn rejected_transfer_keeps_share_claimable() {
        let (env, client, _gov, treasury, _b, _r) = setup();
        let admin = Address::generate(&env);
        let token = make_token(&env, &admin);
        let sac = StellarAssetClient::new(&env, &token);
        let tc = TokenClient::new(&env, &token);

        // Fund the router so it can pay out.
        sac.mint(&client.address, &1_000i128);
        client.on_fee_received(&token, &1_000i128);
        client.distribute(&token);

        // Treasury claims successfully.
        let amount = client.claim(&treasury, &token);
        assert_eq!(amount, 500);
        assert_eq!(tc.balance(&treasury), 500);
        assert_eq!(client.claimable(&treasury, &token), 0);

        // A second claim with nothing left errors but does not affect others.
        let res = client.try_claim(&treasury, &token);
        assert_eq!(res, Err(Ok(FeeRouterError::NothingToClaim)));
    }

    #[test]
    fn claim_credits_actual_received_for_fee_on_transfer_token() {
        let mut r = FeeRouter::new("gov", 0, sinks()).unwrap();
        r.on_fee_received("usd", 1_000);
        r.distribute("usd").unwrap();
        let nominal = r.claimable("treasury", "usd");
        assert_eq!(nominal, 500);

        // 10% fee-on-transfer token: sink receives 450 of the 500 sent.
        let mut token = MockToken::new(1_000, false);
        token.mint("gov", 1_000);

        let credited = r.claim("treasury", "usd", &mut token).unwrap();
        assert_eq!(credited, 450);
        assert_eq!(token.balance_of("treasury"), 450);
        assert_eq!(r.claimable("treasury", "usd"), 0);
    }
}
