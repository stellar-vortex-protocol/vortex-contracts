extern crate std;

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _};

fn setup() -> (Env, DirectoryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, Directory);
    let client = DirectoryClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn set_get_list_remove() {
    let (env, c, _) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    c.set_address(&symbol_short!("settle"), &a);
    c.set_address(&symbol_short!("proof"), &b);
    assert_eq!(c.get_address(&symbol_short!("settle")), a);
    assert_eq!(c.list_names().len(), 2);
    // update does not duplicate the name
    let d = Address::generate(&env);
    c.set_address(&symbol_short!("settle"), &d);
    assert_eq!(c.get_address(&symbol_short!("settle")), d);
    assert_eq!(c.list_names().len(), 2);
    c.remove_address(&symbol_short!("settle"));
    assert_eq!(c.find_address(&symbol_short!("settle")), None);
    assert_eq!(c.list_names().len(), 1);
}

#[test]
fn get_missing_fails() {
    let (_, c, _) = setup();
    assert!(c.find_address(&symbol_short!("nope")).is_none());
    assert!(c.try_get_address(&symbol_short!("nope")).is_err());
    assert!(c.try_remove_address(&symbol_short!("nope")).is_err());
}

#[test]
fn double_initialize_fails() {
    let (env, c, _) = setup();
    assert!(c.try_initialize(&Address::generate(&env)).is_err());
}

#[test]
fn set_requires_admin_auth() {
    let env = Env::default();
    let id = env.register_contract(None, Directory);
    let c = DirectoryClient::new(&env, &id);
    env.mock_all_auths();
    c.initialize(&Address::generate(&env));
    env.set_auths(&[]);
    assert!(c
        .try_set_address(&symbol_short!("a"), &Address::generate(&env))
        .is_err());
}

#[test]
fn two_step_admin_rotation() {
    let (env, c, _) = setup();
    assert!(c.try_accept_admin().is_err());
    let new_admin = Address::generate(&env);
    c.propose_admin(&new_admin);
    c.accept_admin();
    assert_eq!(c.admin(), new_admin);
    assert!(c.try_accept_admin().is_err());
}

#[test]
fn duplicate_address_under_different_name_rejected() {
    let (env, c, _) = setup();
    let a = Address::generate(&env);
    c.set_address(&symbol_short!("settle"), &a);
    let res = c.try_set_address(&symbol_short!("proof"), &a);
    assert_eq!(res, Err(Ok(Error::AddressAlreadyRegistered)));
    // First record untouched; second name never registered.
    assert_eq!(c.get_address(&symbol_short!("settle")), a);
    assert_eq!(c.find_address(&symbol_short!("proof")), None);
    assert_eq!(c.list_names().len(), 1);
    // Re-setting the same name to the same address is still allowed.
    c.set_address(&symbol_short!("settle"), &a);
}
