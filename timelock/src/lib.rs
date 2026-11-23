#![no_std]

//! Generic `TimelockController` for the protocol.
//!
//! Modelled on OpenZeppelin's `TimelockController`, adapted to Soroban auth.
//! The timelock owns protocol admin roles: it schedules arbitrary cross-contract
//! calls, hash-commits each operation, and replays the exact committed call via
//! `env.invoke_contract` once the delay has elapsed.
//!
//! Roles:
//! * `Proposer`  - may schedule and cancel operations.
//! * `Executor`  - may execute ready operations.
//! * `Canceller` - emergency role that may cancel any pending operation.
//! * `Admin`     - may grant/revoke roles and change the minimum delay. The
//!                 timelock is its own admin, so role management and delay
//!                 changes must themselves go through the timelock.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    IntoVal, Symbol, Val, Vec,
};

/// Operation lifecycle states, mirroring OpenZeppelin's `TimelockController`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationState {
    Unset,
    Waiting,
    Ready,
    Done,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TimelockError {
    /// Caller is missing the required role.
    Unauthorized = 1,
    /// The requested delay is below the current minimum delay.
    InsufficientDelay = 2,
    /// The operation id has already been scheduled.
    AlreadyScheduled = 3,
    /// The operation is not in the `Waiting` state.
    NotWaiting = 4,
    /// The operation is not yet ready (delay has not elapsed).
    NotReady = 5,
    /// The operation has already been executed.
    AlreadyDone = 6,
    /// The operation is unknown.
    UnknownOperation = 7,
    /// The operation's predecessor has not been executed yet.
    UnmetPredecessor = 8,
    /// The operation's predecessor is not a valid operation id.
    InvalidPredecessor = 9,
    /// The operation has expired and can no longer be executed.
    Expired = 10,
    /// The operation is still within its grace period and cannot be expired.
    NotExpired = 11,
    /// The operation has no predecessor but one was supplied, or vice versa.
    InvalidPredecessorState = 12,
}

/// A single call inside a batch.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Call {
    pub target: Address,
    pub function: Symbol,
    pub args: Vec<Val>,
}

/// A scheduled operation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Operation {
    pub id: BytesN<32>,
    pub predecessor: BytesN<32>,
    pub salt: BytesN<32>,
    pub calls: Vec<Call>,
    pub ready_at: u64,
    pub expires_at: u64,
    pub executed: bool,
}

const PROPOSER: Symbol = symbol_short!("Proposer");
const EXECUTOR: Symbol = symbol_short!("Executor");
const CANCELLER: Symbol = symbol_short!("Canceller");
const ADMIN: Symbol = symbol_short!("Admin");

const MIN_DELAY: Symbol = symbol_short!("MinDelay");
const GRACE_PERIOD: Symbol = symbol_short!("Grace");

const OP_KEY: Symbol = symbol_short!("Op");
const ROLE_KEY: Symbol = symbol_short!("Role");

/// Default grace period after `ready_at` during which an operation may execute.
const DEFAULT_GRACE_PERIOD: u64 = 14 * 24 * 60 * 60;

/// Number of ledgers below which a persistent entry's TTL is bumped on write.
const PERSISTENT_TTL_THRESHOLD: u32 = 100_000;
/// Number of ledgers a persistent entry's TTL is extended to on write.
const PERSISTENT_TTL_EXTEND_TO: u32 = 500_000;

#[contract]
pub struct TimelockController;

#[contractimpl]
impl TimelockController {
    /// Initialise the timelock.
    ///
    /// `admin` is granted the `Admin` role and is expected to be the timelock
    /// itself (or a bootstrap account that immediately hands over). `proposers`,
    /// `executors`, and `cancellers` are granted their respective roles.
    pub fn __constructor(
        env: Env,
        admin: Address,
        proposers: Vec<Address>,
        executors: Vec<Address>,
        cancellers: Vec<Address>,
        min_delay: u64,
    ) {
        env.storage().instance().set(&MIN_DELAY, &min_delay);
        env.storage()
            .instance()
            .set(&GRACE_PERIOD, &DEFAULT_GRACE_PERIOD);

        Self::grant_role_internal(&env, &ADMIN, &admin);
        for p in proposers.iter() {
            Self::grant_role_internal(&env, &PROPOSER, &p);
        }
        for e in executors.iter() {
            Self::grant_role_internal(&env, &EXECUTOR, &e);
        }
        for c in cancellers.iter() {
            Self::grant_role_internal(&env, &CANCELLER, &c);
        }
    }

    // ---------------------------------------------------------------------
    // TTL helpers
    // ---------------------------------------------------------------------

    /// Bump the TTL of a persistent storage key so it does not get archived.
    fn bump_persistent_ttl<K: soroban_sdk::IntoVal<Env, Val>>(env: &Env, key: &K) {
        env.storage().persistent().extend_ttl(
            key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    // ---------------------------------------------------------------------
    // Role management (must be authorised by the timelock itself)
    // ---------------------------------------------------------------------

    pub fn grant_role(env: Env, role: Symbol, account: Address) {
        Self::require_admin(&env);
        Self::grant_role_internal(&env, &role, &account);
    }

    pub fn revoke_role(env: Env, role: Symbol, account: Address) {
        Self::require_admin(&env);
        let key = (ROLE_KEY, role, account);
        env.storage().persistent().set(&key, &false);
        Self::bump_persistent_ttl(&env, &key);
    }

    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        env.storage()
            .persistent()
            .get(&(ROLE_KEY, role, account))
            .unwrap_or(false)
    }

    // ---------------------------------------------------------------------
    // Delay management (only the timelock itself may change the delay)
    // ---------------------------------------------------------------------

    pub fn get_min_delay(env: Env) -> u64 {
        env.storage().instance().get(&MIN_DELAY).unwrap_or(0)
    }

    pub fn update_delay(env: Env, new_delay: u64) {
        Self::require_admin(&env);
        env.storage().instance().set(&MIN_DELAY, &new_delay);
    }

    pub fn get_grace_period(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&GRACE_PERIOD)
            .unwrap_or(DEFAULT_GRACE_PERIOD)
    }

    // ---------------------------------------------------------------------
    // Scheduling
    // ---------------------------------------------------------------------

    /// Schedule a single call.
    pub fn schedule(
        env: Env,
        target: Address,
        function: Symbol,
        args: Vec<Val>,
        predecessor: BytesN<32>,
        salt: BytesN<32>,
        delay: u64,
    ) -> BytesN<32> {
        let mut calls = Vec::new(&env);
        calls.push_back(Call {
            target,
            function,
            args,
        });
        Self::schedule_batch(env, calls, predecessor, salt, delay)
    }

    /// Schedule a batch of calls that execute atomically.
    pub fn schedule_batch(
        env: Env,
        calls: Vec<Call>,
        predecessor: BytesN<32>,
        salt: BytesN<32>,
        delay: u64,
    ) -> BytesN<32> {
        Self::require_role(&env, &PROPOSER);

        let id = Self::hash_operation(env.clone(), calls.clone(), predecessor.clone(), salt.clone());

        let min_delay = Self::get_min_delay(env.clone());
        if delay < min_delay {
            Self::reject(&env, "schedule_rejected", &id, TimelockError::InsufficientDelay);
        }

        if env.storage().persistent().has(&(OP_KEY, id.clone())) {
            Self::reject(&env, "schedule_rejected", &id, TimelockError::AlreadyScheduled);
        }

        // A predecessor, when supplied, must itself be a known operation.
        if predecessor != Self::zero_hash(&env)
            && !env
                .storage()
                .persistent()
                .has(&(OP_KEY, predecessor.clone()))
        {
            Self::reject(&env, "schedule_rejected", &id, TimelockError::InvalidPredecessor);
        }

        let now = env.ledger().timestamp();
        let operation = Operation {
            id: id.clone(),
            predecessor,
            salt,
            calls,
            ready_at: now + delay,
            expires_at: now + delay + Self::get_grace_period(env.clone()),
            executed: false,
        };

        let op_key = (OP_KEY, id.clone());
        env.storage().persistent().set(&op_key, &operation);
        Self::bump_persistent_ttl(&env, &op_key);

        env.events()
            .publish((symbol_short!("scheduled"), id.clone()), id.clone());

        id
    }

    // ---------------------------------------------------------------------
    // Cancellation
    // ---------------------------------------------------------------------

    pub fn cancel(env: Env, id: BytesN<32>) {
        Self::require_role(&env, &CANCELLER);
        let mut operation: Operation = env
            .storage()
            .persistent()
            .get(&(OP_KEY, id.clone()))
            .unwrap_or_else(|| env.panic_with_error(TimelockError::UnknownOperation));

        if operation.executed {
            env.panic_with_error(TimelockError::AlreadyDone);
        }

        operation.executed = true;
        env.storage()
            .persistent()
            .set(&(OP_KEY, id.clone()), &operation);

        env.events()
            .publish((symbol_short!("cancelled"), id), ());
    }

    // ---------------------------------------------------------------------
    // Execution
    // ---------------------------------------------------------------------

    pub fn execute(env: Env, id: BytesN<32>) {
        Self::require_role(&env, &EXECUTOR);

        let mut operation: Operation = env
            .storage()
            .persistent()
            .get(&(OP_KEY, id.clone()))
            .unwrap_or_else(|| Self::reject(&env, "execute_rejected", &id, TimelockError::UnknownOperation));

        if operation.executed {
            Self::reject(&env, "execute_rejected", &id, TimelockError::AlreadyDone);
        }

        let now = env.ledger().timestamp();
        if now < operation.ready_at {
            Self::reject(&env, "execute_rejected", &id, TimelockError::NotReady);
        }
        if now > operation.expires_at {
            Self::reject(&env, "execute_rejected", &id, TimelockError::Expired);
        }

        // Verify the stored operation still matches its committed hash.
        let recomputed = Self::hash_operation(
            env.clone(),
            operation.calls.clone(),
            operation.predecessor.clone(),
            operation.salt.clone(),
        );
        if recomputed != id {
            Self::reject(&env, "execute_rejected", &id, TimelockError::UnknownOperation);
        }

        // Predecessor must be executed before this operation can run.
        if operation.predecessor != Self::zero_hash(&env) {
            let pred: Operation = env
                .storage()
                .persistent()
                .get(&(OP_KEY, operation.predecessor.clone()))
                .unwrap_or_else(|| Self::reject(&env, "execute_rejected", &id, TimelockError::InvalidPredecessor));
            if !pred.executed {
                Self::reject(&env, "execute_rejected", &id, TimelockError::UnmetPredecessor);
            }
        }

        operation.executed = true;
        env.storage()
            .persistent()
            .set(&(OP_KEY, id.clone()), &operation);

        for call in operation.calls.iter() {
            env.invoke_contract::<Val>(&call.target, &call.function, call.args.clone());
        }

        env.events()
            .publish((symbol_short!("executed"), id), ());
    }

    // ---------------------------------------------------------------------
    // Views
    // ---------------------------------------------------------------------

    pub fn get_operation_state(env: Env, id: BytesN<32>) -> OperationState {
        let operation: Option<Operation> = env.storage().persistent().get(&(OP_KEY, id));
        match operation {
            None => OperationState::Unset,
            Some(op) => {
                if op.executed {
                    OperationState::Done
                } else if env.ledger().timestamp() < op.ready_at {
                    OperationState::Waiting
                } else if env.ledger().timestamp() > op.expires_at {
                    OperationState::Done
                } else {
                    OperationState::Ready
                }
            }
        }
    }

    pub fn is_operation_done(env: Env, id: BytesN<32>) -> bool {
        let operation: Option<Operation> = env.storage().persistent().get(&(OP_KEY, id));
        operation.map(|op| op.executed).unwrap_or(false)
    }

    // ---------------------------------------------------------------------
    // Public hash helper
    // ---------------------------------------------------------------------

    /// Compute the operation id for a batch of calls.
    pub fn hash_operation(
        env: Env,
        calls: Vec<Call>,
        predecessor: BytesN<32>,
        salt: BytesN<32>,
    ) -> BytesN<32> {
        Self::hash_operation_internal(&env, &calls, &predecessor, &salt)
    }

    // ---------------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------------

    fn hash_operation_internal(
        env: &Env,
        calls: &Vec<Call>,
        predecessor: &BytesN<32>,
        salt: &BytesN<32>,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);

        for call in calls.iter() {
            let target_bytes = call.target.clone().to_xdr(env);
            preimage.append(&target_bytes);

            let function_bytes = call.function.clone().to_xdr(env);
            preimage.append(&function_bytes);

            for arg in call.args.iter() {
                let arg_bytes = arg.to_xdr(env);
                preimage.append(&arg_bytes);
            }
        }

        preimage.append(&Bytes::from_slice(env, &predecessor.to_array()));
        preimage.append(&Bytes::from_slice(env, &salt.to_array()));

        env.crypto().sha256(&preimage).into()
    }

    /// Emit a rejection event for `id` and abort with `err`.
    ///
    /// Topic is `(<entrypoint>_rejected, id)`; data is the numeric
    /// `TimelockError` code. Soroban discards contract events of a failed
    /// invocation, so this surfaces in diagnostic events of the failed
    /// transaction rather than in the ledger event stream.
    fn reject(env: &Env, topic: &str, id: &BytesN<32>, err: TimelockError) -> ! {
        env.events()
            .publish((Symbol::new(env, topic), id.clone()), err as u32);
        env.panic_with_error(err)
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn grant_role_internal(env: &Env, role: &Symbol, account: &Address) {
        env.storage()
            .persistent()
            .set(&(ROLE_KEY, role.clone(), account.clone()), &true);
    }

    fn require_role(env: &Env, role: &Symbol) {
        let caller = env.current_contract_address();
        let has: bool = env
            .storage()
            .persistent()
            .get(&(ROLE_KEY, role.clone(), caller))
            .unwrap_or(false);
        if !has {
            env.panic_with_error(TimelockError::Unauthorized);
        }
    }

    fn require_admin(env: &Env) {
        Self::require_role(env, &ADMIN);
    }
}
