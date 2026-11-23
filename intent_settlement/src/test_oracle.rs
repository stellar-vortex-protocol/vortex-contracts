//! Issue #549: fail-closed behaviour of the oracle adapter and its wiring.
extern crate std;

use super::oracle::{fetch_price, weight_bps, OracleError, PriceData};
use soroban_sdk::{
    contract, contractimpl, contracttype, testutils::{Address as _, Ledger}, Address, Env,
};

#[contracttype]
enum K {
    Price(Address),
}

#[contract]
struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn set(env: Env, asset: Address, price: i128, decimals: u32, timestamp: u64) {
        env.storage()
            .instance()
            .set(&K::Price(asset), &PriceData { price, decimals, timestamp });
    }
    pub fn lastprice(env: Env, asset: Address) -> Option<PriceData> {
        env.storage().instance().get(&K::Price(asset))
    }
}

fn setup() -> (Env, Address, MockOracleClient<'static>, Address, Address) {
    let env = Env::default();
    env.ledger().set_timestamp(10_000);
    let id = env.register_contract(None, MockOracle);
    let c = MockOracleClient::new(&env, &id);
    let (a, b) = (Address::generate(&env), Address::generate(&env));
    (env, id, c, a, b)
}

#[test]
fn fetch_price_fail_closed_paths() {
    let (env, id, c, a, _) = setup();
    assert_eq!(fetch_price(&env, &id, &a), Err(OracleError::NotAvailable));
    c.set(&a, &100, &7, &(10_000 - 301));
    assert_eq!(fetch_price(&env, &id, &a), Err(OracleError::Stale));
    c.set(&a, &0, &7, &10_000);
    assert_eq!(fetch_price(&env, &id, &a), Err(OracleError::ZeroOrNegative));
    c.set(&a, &100, &7, &(10_000 - 300));
    assert_eq!(fetch_price(&env, &id, &a), Ok((100, 7)));
}

#[test]
fn weight_is_oracle_cross_rate_or_neutral() {
    let (env, id, c, a, b) = setup();
    // no oracle configured / same token => 1.0x
    assert_eq!(weight_bps(&env, None, &a, &b), 10_000);
    assert_eq!(weight_bps(&env, Some(&id), &a, &a), 10_000);
    // missing price => 1.0x
    assert_eq!(weight_bps(&env, Some(&id), &a, &b), 10_000);
    // a = 2.0, b = 1.0 (different decimals) => 2.0x
    c.set(&a, &20_000_000, &7, &10_000);
    c.set(&b, &100, &2, &10_000);
    assert_eq!(weight_bps(&env, Some(&id), &a, &b), 20_000);
    // stale `to` price => 1.0x
    c.set(&b, &100, &2, &1);
    assert_eq!(weight_bps(&env, Some(&id), &a, &b), 10_000);
    // zero price => 1.0x
    c.set(&b, &0, &2, &10_000);
    assert_eq!(weight_bps(&env, Some(&id), &a, &b), 10_000);
}

#[test]
fn trapping_oracle_is_a_fault_not_a_revert() {
    let (env, _, _, a, b) = setup();
    // An address with no contract behind it: lastprice call fails.
    let dead = Address::generate(&env);
    assert_eq!(fetch_price(&env, &dead, &a), Err(OracleError::NotAvailable));
    assert_eq!(weight_bps(&env, Some(&dead), &a, &b), 10_000);
}
