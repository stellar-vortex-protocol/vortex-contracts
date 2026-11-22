#![no_std]

//! Vortex Protocol — on-chain address book (issues #399 / #554).
//!
//! Canonical, admin-updatable registry mapping protocol component names
//! (e.g. `intent_settlement`) to contract addresses. See
//! `docs/directory-design.md`.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, Symbol,
    Vec,
};

#[cfg(test)]
mod test;

const TTL_THRESHOLD: u32 = 17_280 * 30;
const TTL_EXTEND_TO: u32 = 17_280 * 90;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    PendingAdmin,
    /// Ordered list of registered names.
    Names,
    Entry(Symbol),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotFound = 3,
    NoPendingAdmin = 4,
    /// The address is already registered under a different name.
    AddressAlreadyRegistered = 5,
}

#[contract]
pub struct Directory;

#[contractimpl]
impl Directory {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Self::bump_instance(&env);
    }

    /// Register or update the address stored under `name`. Admin-only.
    /// Traps with `AddressAlreadyRegistered` if `addr` is already stored
    /// under a different name (one-to-one mapping).
    pub fn set_address(env: Env, name: Symbol, addr: Address) {
        Self::require_admin(&env);
        Self::bump_instance(&env);
        let key = DataKey::Entry(name.clone());
        for other in Self::names(&env).iter() {
            if other != name {
                let existing: Option<Address> =
                    env.storage().persistent().get(&DataKey::Entry(other));
                if existing.as_ref() == Some(&addr) {
                    panic_with_error!(&env, Error::AddressAlreadyRegistered);
                }
            }
        }
        if !env.storage().persistent().has(&key) {
            let mut names = Self::names(&env);
            names.push_back(name.clone());
            env.storage().instance().set(&DataKey::Names, &names);
        }
        env.storage().persistent().set(&key, &addr);
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
        env.events().publish((Symbol::new(&env, "addr_set"), name), addr);
    }

    /// Remove the entry stored under `name`. Admin-only.
    pub fn remove_address(env: Env, name: Symbol) {
        Self::require_admin(&env);
        Self::bump_instance(&env);
        let key = DataKey::Entry(name.clone());
        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, Error::NotFound);
        }
        env.storage().persistent().remove(&key);
        let names = Self::names(&env);
        let mut kept = Vec::new(&env);
        for n in names.iter() {
            if n != name {
                kept.push_back(n);
            }
        }
        env.storage().instance().set(&DataKey::Names, &kept);
        env.events().publish((Symbol::new(&env, "addr_rm"),), name);
    }

    /// Look up `name`; traps with `NotFound` if absent.
    pub fn get_address(env: Env, name: Symbol) -> Address {
        Self::find_address(env.clone(), name)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotFound))
    }

    /// Look up `name`, returning `None` if absent.
    pub fn find_address(env: Env, name: Symbol) -> Option<Address> {
        let key = DataKey::Entry(name);
        let v: Option<Address> = env.storage().persistent().get(&key);
        if v.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
        }
        v
    }

    /// All registered names, in registration order.
    pub fn list_names(env: Env) -> Vec<Symbol> {
        Self::names(&env)
    }

    pub fn admin(env: Env) -> Address {
        Self::load_admin(&env)
    }

    /// Step 1 of two-step admin rotation. Admin-only.
    pub fn propose_admin(env: Env, new_admin: Address) {
        Self::require_admin(&env);
        Self::bump_instance(&env);
        env.storage().instance().set(&DataKey::PendingAdmin, &new_admin);
    }

    /// Step 2: the proposed admin accepts.
    pub fn accept_admin(env: Env) {
        Self::bump_instance(&env);
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NoPendingAdmin));
        pending.require_auth();
        env.storage().instance().set(&DataKey::Admin, &pending);
        env.storage().instance().remove(&DataKey::PendingAdmin);
    }

    fn names(env: &Env) -> Vec<Symbol> {
        env.storage()
            .instance()
            .get(&DataKey::Names)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn load_admin(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized))
    }

    fn require_admin(env: &Env) {
        Self::load_admin(env).require_auth();
    }

    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
    }
}
