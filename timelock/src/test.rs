#![cfg(test)]

extern crate std;

use super::*;
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, testutils::Address as _, Address, Env, Map,
    String, Symbol, Val, Vec, BytesN,
};

// ---------------------------------------------------------------------------
// A minimal target contract used to verify that the timelock replays the
// exact committed call via `env.invoke_contract`.
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallRecord {
    pub caller: Address,
    pub value: u32,
    pub label: String,
}

#[contract]
pub struct TargetContract;

#[contractimpl]
impl TargetContract {
    pub fn set_value(env: Env, value: u32) {
        env.storage().instance().set(&symbol_short!("value"), &value);
    }

    pub fn get_value(env: Env) -> u32 {
        env.storage().instance().get(&symbol_short!("value")).unwrap_or(0)
    }

    pub fn record(env: Env, caller: Address, value: u32, label: String) {
        caller.require_auth();
        let record = CallRecord { caller, value, label };
        env.storage().instance().set(&symbol_short!("record"), &record);
    }

    pub fn get_record(env: Env) -> CallRecord {
        env.storage().instance().get(&symbol_short!("record")).unwrap()
    }

    pub fn sum_vec(env: Env, values: Vec<u32>) -> u32 {
        let mut total: u32 = 0;
        for v in values.iter() {
            total += v;
        }
        env.storage().instance().set(&symbol_short!("sum"), &total);
        total
    }

    pub fn get_sum(env: Env) -> u32 {
        env.storage().instance().get(&symbol_short!("sum")).unwrap_or(0)
    }

    pub fn set_map(env: Env, entries: Map<Symbol, u32>) {
        env.storage().instance().set(&symbol_short!("map"), &entries);
    }

    pub fn get_map(env: Env) -> Map<Symbol, u32> {
        env.storage().instance().get(&symbol_short!("map")).unwrap()
    }

    pub fn fail(_env: Env) {
        panic!("target failure");
    }
}

// ---------------------------------------------------------------------------
// Test harness helpers.
// ---------------------------------------------------------------------------

struct Harness<'a> {
    env: Env,
    client: TimelockControllerClient<'a>,
    timelock: Address,
    target: Address,
    proposer: Address,
    executor: Address,
    canceller: Address,
    stranger: Address,
}

fn setup<'a>() -> Harness<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let timelock = env.register_contract(None, TimelockController);
    let target = env.register_contract(None, TargetContract);
    let client = TimelockControllerClient::new(&env, &timelock);

    let proposer = Address::generate(&env);
    let executor = Address::generate(&env);
    let canceller = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.initialize(&proposer, &executor, &canceller, &MIN_DELAY);

    Harness { env, client, timelock, target, proposer, executor, canceller, stranger }
}

fn salt(env: &Env, tag: &str) -> soroban_sdk::Bytes {
    soroban_sdk::Bytes::from_slice(env, tag.as_bytes())
}

// ---------------------------------------------------------------------------
// Initialization & role management.
// ---------------------------------------------------------------------------

#[test]
fn initialize_sets_roles_and_min_delay() {
    let h = setup();
    assert_eq!(h.client.get_min_delay(), MIN_DELAY);
    assert!(h.client.has_role(&PROPOSER_ROLE, &h.proposer));
    assert!(h.client.has_role(&EXECUTOR_ROLE, &h.executor));
    assert!(h.client.has_role(&CANCELLER_ROLE, &h.canceller));
    assert!(h.client.has_role(&ADMIN_ROLE, &h.timelock));
    assert!(!h.client.has_role(&PROPOSER_ROLE, &h.stranger));
}

#[test]
#[should_panic]
fn initialize_twice_panics() {
    let h = setup();
    h.client.initialize(&h.proposer, &h.executor, &h.canceller, &MIN_DELAY);
}

#[test]
fn role_management_through_timelock() {
    let h = setup();
    let new_proposer = Address::generate(&h.env);

    // Only the timelock (admin) can grant roles; the call is scheduled and
    // executed by the timelock itself.
    let args = (PROPOSER_ROLE, new_proposer.clone()).into_val(&h.env);
    let id = h.client.schedule(
        &h.timelock,
        &Symbol::new(&h.env, "grant_role"),
        &args,
        &salt(&h.env, "grant"),
        &MIN_DELAY,
    );
    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&id);

    assert!(h.client.has_role(&PROPOSER_ROLE, &new_proposer));
}

#[test]
#[should_panic]
fn stranger_cannot_grant_role_directly() {
    let h = setup();
    let new_proposer = Address::generate(&h.env);
    h.client.grant_role(&h.stranger, &PROPOSER_ROLE, &new_proposer);
}

// ---------------------------------------------------------------------------
// Scheduling, execution, cancellation.
// ---------------------------------------------------------------------------

#[test]
fn schedule_and_execute_single_call() {
    let h = setup();
    let args = (42u32,).into_val(&h.env);
    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "single"),
        &MIN_DELAY,
    );

    assert_eq!(h.client.get_operation_state(&id), OperationState::Pending);

    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&id);

    let target = TargetContractClient::new(&h.env, &h.target);
    assert_eq!(target.get_value(), 42);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Done);
}

#[test]
#[should_panic]
fn execute_before_delay_panics() {
    let h = setup();
    let args = (1u32,).into_val(&h.env);
    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "early"),
        &MIN_DELAY,
    );
    h.client.execute(&id);
}

#[test]
#[should_panic]
fn schedule_below_min_delay_panics() {
    let h = setup();
    let args = (1u32,).into_val(&h.env);
    h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "short"),
        &(MIN_DELAY - 1),
    );
}

#[test]
fn cancel_prevents_execution() {
    let h = setup();
    let args = (7u32,).into_val(&h.env);
    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "cancel"),
        &MIN_DELAY,
    );

    h.client.cancel(&id);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Cancelled);

    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.client.execute(&id)));
    assert!(res.is_err());
}

#[test]
#[should_panic]
fn stranger_cannot_cancel() {
    let h = setup();
    let args = (7u32,).into_val(&h.env);
    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "stranger-cancel"),
        &MIN_DELAY,
    );
    h.client.cancel_as(&h.stranger, &id);
}

#[test]
fn emergency_canceller_can_cancel() {
    let h = setup();
    let args = (7u32,).into_val(&h.env);
    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "emergency"),
        &MIN_DELAY,
    );
    h.client.cancel_as(&h.canceller, &id);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Cancelled);
}

#[test]
fn operation_id_is_hash_committed() {
    let h = setup();
    let args = (5u32,).into_val(&h.env);
    let s = salt(&h.env, "hash");
    let id_a = h.client.hash_operation(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &s,
    );
    let id_b = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &s,
        &MIN_DELAY,
    );
    assert_eq!(id_a, id_b);
}

#[test]
#[should_panic]
fn cannot_schedule_same_operation_twice() {
    let h = setup();
    let args = (5u32,).into_val(&h.env);
    let s = salt(&h.env, "dup");
    h.client.schedule(&h.target, &Symbol::new(&h.env, "set_value"), &args, &s, &MIN_DELAY);
    h.client.schedule(&h.target, &Symbol::new(&h.env, "set_value"), &args, &s, &MIN_DELAY);
}

#[test]
#[should_panic]
fn execute_unknown_operation_panics() {
    let h = setup();
    let args = (5u32,).into_val(&h.env);
    let id = h.client.hash_operation(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "unknown"),
    );
    h.client.execute(&id);
}

// ---------------------------------------------------------------------------
// Batch scheduling.
// ---------------------------------------------------------------------------

#[test]
fn batch_schedule_and_execute() {
    let h = setup();
    let mut targets = Vec::new(&h.env);
    let mut fns = Vec::new(&h.env);
    let mut args = Vec::new(&h.env);

    targets.push_back(h.target.clone());
    fns.push_back(Symbol::new(&h.env, "set_value"));
    args.push_back((11u32,).into_val(&h.env));

    targets.push_back(h.target.clone());
    fns.push_back(Symbol::new(&h.env, "sum_vec"));
    let mut values = Vec::new(&h.env);
    values.push_back(1u32);
    values.push_back(2u32);
    values.push_back(3u32);
    args.push_back((values,).into_val(&h.env));

    let id = h.client.schedule_batch(&targets, &fns, &args, &salt(&h.env, "batch"), &MIN_DELAY);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Pending);

    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&id);

    let target = TargetContractClient::new(&h.env, &h.target);
    assert_eq!(target.get_value(), 11);
    assert_eq!(target.get_sum(), 6);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Done);
}

#[test]
#[should_panic]
fn batch_length_mismatch_panics() {
    let h = setup();
    let mut targets = Vec::new(&h.env);
    let mut fns = Vec::new(&h.env);
    let args = Vec::new(&h.env);
    targets.push_back(h.target.clone());
    fns.push_back(Symbol::new(&h.env, "set_value"));
    h.client.schedule_batch(&targets, &fns, &args, &salt(&h.env, "mismatch"), &MIN_DELAY);
}

// ---------------------------------------------------------------------------
// Complex argument types (Vec / Map) and auth propagation.
// ---------------------------------------------------------------------------

#[test]
fn executes_call_with_vec_args() {
    let h = setup();
    let mut values = Vec::new(&h.env);
    values.push_back(10u32);
    values.push_back(20u32);
    let args = (values,).into_val(&h.env);

    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "sum_vec"),
        &args,
        &salt(&h.env, "vec"),
        &MIN_DELAY,
    );
    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&id);

    let target = TargetContractClient::new(&h.env, &h.target);
    assert_eq!(target.get_sum(), 30);
}

#[test]
fn executes_call_with_map_args() {
    let h = setup();
    let mut entries = Map::new(&h.env);
    entries.set(Symbol::new(&h.env, "a"), 1u32);
    entries.set(Symbol::new(&h.env, "b"), 2u32);
    let args = (entries.clone(),).into_val(&h.env);

    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_map"),
        &args,
        &salt(&h.env, "map"),
        &MIN_DELAY,
    );
    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&id);

    let target = TargetContractClient::new(&h.env, &h.target);
    assert_eq!(target.get_map(), entries);
}

#[test]
fn timelock_authorizes_call_as_itself() {
    let h = setup();
    let label = String::from_str(&h.env, "from-timelock");
    let args = (h.timelock.clone(), 99u32, label.clone()).into_val(&h.env);

    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "record"),
        &args,
        &salt(&h.env, "auth"),
        &MIN_DELAY,
    );
    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&id);

    let target = TargetContractClient::new(&h.env, &h.target);
    let record = target.get_record();
    assert_eq!(record.caller, h.timelock);
    assert_eq!(record.value, 99);
    assert_eq!(record.label, label);
}

// ---------------------------------------------------------------------------
// Predecessor dependencies.
// ---------------------------------------------------------------------------

#[test]
fn predecessor_must_execute_first() {
    let h = setup();
    let args_a = (1u32,).into_val(&h.env);
    let id_a = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args_a,
        &salt(&h.env, "pred-a"),
        &MIN_DELAY,
    );

    let args_b = (2u32,).into_val(&h.env);
    let id_b = h.client.schedule_with_predecessor(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args_b,
        &salt(&h.env, "pred-b"),
        &MIN_DELAY,
        &id_a,
    );

    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);

    // Executing the dependent operation before its predecessor must fail.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.client.execute(&id_b)));
    assert!(res.is_err());

    h.client.execute(&id_a);
    h.client.execute(&id_b);

    let target = TargetContractClient::new(&h.env, &h.target);
    assert_eq!(target.get_value(), 2);
}

// ---------------------------------------------------------------------------
// Delay management.
// ---------------------------------------------------------------------------

#[test]
fn only_timelock_can_update_delay() {
    let h = setup();
    let new_delay = MIN_DELAY + 100;
    let args = (new_delay,).into_val(&h.env);

    let id = h.client.schedule(
        &h.timelock,
        &Symbol::new(&h.env, "update_delay"),
        &args,
        &salt(&h.env, "delay"),
        &MIN_DELAY,
    );
    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&id);

    assert_eq!(h.client.get_min_delay(), new_delay);
}

#[test]
#[should_panic]
fn stranger_cannot_update_delay() {
    let h = setup();
    h.client.update_delay(&h.stranger, &(MIN_DELAY + 1));
}

#[test]
#[should_panic]
fn cannot_lower_delay_below_floor() {
    let h = setup();
    h.client.update_delay(&h.timelock, &(MIN_DELAY - 1));
}

// ---------------------------------------------------------------------------
// Expired operations.
// ---------------------------------------------------------------------------

#[test]
fn expired_operation_cannot_execute() {
    let h = setup();
    let args = (3u32,).into_val(&h.env);
    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "expire"),
        &MIN_DELAY,
    );

    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY + OPERATION_TTL + 1);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Expired);

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.client.execute(&id)));
    assert!(res.is_err());
}

#[test]
fn expired_operation_can_be_rescheduled() {
    let h = setup();
    let args = (3u32,).into_val(&h.env);
    let s = salt(&h.env, "resched");
    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &s,
        &MIN_DELAY,
    );

    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY + OPERATION_TTL + 1);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Expired);

    // Re-scheduling the same operation after expiry is allowed.
    let id2 = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &s,
        &MIN_DELAY,
    );
    assert_eq!(id, id2);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Pending);
}

// ---------------------------------------------------------------------------
// Full admin-transfer flow for settlement.
// ---------------------------------------------------------------------------

#[test]
fn full_admin_transfer_flow() {
    let h = setup();

    // 1. Schedule the transfer of the admin role to the timelock on the
    //    settlement contract (represented here by the target).
    let args = (h.timelock.clone(),).into_val(&h.env);
    let id = h.client.schedule(
        &h.target,
        &Symbol::new(&h.env, "set_value"),
        &args,
        &salt(&h.env, "admin-transfer"),
        &MIN_DELAY,
    );
    assert_eq!(h.client.get_operation_state(&id), OperationState::Pending);

    // 2. Wait out the delay and execute.
    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&id);
    assert_eq!(h.client.get_operation_state(&id), OperationState::Done);

    // 3. The timelock now owns the admin role and can manage roles itself.
    assert!(h.client.has_role(&ADMIN_ROLE, &h.timelock));

    let new_executor = Address::generate(&h.env);
    let grant_args = (EXECUTOR_ROLE, new_executor.clone()).into_val(&h.env);
    let grant_id = h.client.schedule(
        &h.timelock,
        &Symbol::new(&h.env, "grant_role"),
        &grant_args,
        &salt(&h.env, "grant-executor"),
        &MIN_DELAY,
    );
    h.env.ledger().with_mut(|l| l.timestamp += MIN_DELAY);
    h.client.execute(&grant_id);

    assert!(h.client.has_role(&EXECUTOR_ROLE, &new_executor));
}

// ---------------------------------------------------------------------------
// Empty target function name.
// ---------------------------------------------------------------------------

#[test]
fn schedule_rejects_empty_function_name_and_stores_nothing() {
    let h = setup();
    let args: Vec<Val> = Vec::new(&h.env);
    let pred = BytesN::from_array(&h.env, &[0u8; 32]);
    let s = BytesN::from_array(&h.env, &[7u8; 32]);

    let mut calls = Vec::new(&h.env);
    calls.push_back(Call {
        target: h.target.clone(),
        function: Symbol::new(&h.env, ""),
        args: args.clone(),
    });
    let id = h.client.hash_operation(&calls, &pred, &s);

    let res = h.client.try_schedule_batch(&calls, &pred, &s, &MIN_DELAY);
    assert_eq!(res, Err(Ok(TimelockError::EmptyFunctionName)));
    assert_eq!(h.client.get_operation_state(&id), OperationState::Unset);

    // A non-empty name still schedules.
    let mut ok_calls = Vec::new(&h.env);
    ok_calls.push_back(Call {
        target: h.target.clone(),
        function: Symbol::new(&h.env, "set_value"),
        args,
    });
    let ok_id = h.client.schedule_batch(&ok_calls, &pred, &s, &MIN_DELAY);
    assert_eq!(h.client.get_operation_state(&ok_id), OperationState::Waiting);
}
