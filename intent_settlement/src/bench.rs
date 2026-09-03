#![cfg(test)]

//! Resource-cost harness for `intent_settlement` (issue #195).
//!
//! Runs each state-changing entrypoint once, from an isolated fixture, under
//! `soroban_sdk`'s test-mode [`Budget`] and records:
//!
//!   * `cpu` — CPU instructions consumed (`Budget::cpu_instruction_cost`)
//!   * `mem` — memory bytes consumed (`Budget::memory_bytes_cost`)
//!
//! A second table reports the serialised XDR size of the two persistent
//! records (`IntentRecord`, `SolverRecord`) read back from storage — the
//! per-write ledger footprint.
//!
//! ## Methodology & caveats
//!
//! * The SDK runs the contract **natively as Rust**, not as Wasm. Per the
//!   SDK's own docs the CPU / memory figures are approximate and generally an
//!   *underestimate* of on-chain cost; treat them as a consistent relative
//!   ranking between entrypoints, not a fee quote.
//! * Fine-grained ledger read/write **entry counts** are not exposed by the
//!   `soroban-sdk` 21 testutils `Budget`; obtaining them needs the on-chain
//!   simulator (`stellar contract invoke --cost`) or `soroban-sdk >= 22`'s
//!   `Env::cost_estimate`. The record-size table below covers the write-bytes
//!   dimension that matters for #196.
//! * Token transfers in `fill_intent` / `register_solver` / `slash_solver`
//!   invoke the Stellar Asset Contract; that cost is included in the row.
//! * Fixtures are built identically, so runs are deterministic:
//!   `resource_cost_is_reproducible` asserts identical numbers across runs.
//!
//! Regenerate the published tables with:
//! ```text
//! cargo test --features testutils bench::resource_cost_report -- --nocapture
//! ```

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token,
    xdr::ToXdr,
    Address, BytesN, Env, String,
};
use std::{format, string::String as StdString, vec::Vec as StdVec};

use crate::{DataKey, IntentRecord, IntentSettlement, IntentSettlementClient, SolverRecord};

const BOND: i128 = 1_000 * 10_000_000;
const SRC_AMT: i128 = 500_000_000;
const MIN_DST: i128 = 100 * 10_000_000;
const FULL_FILL: i128 = 105 * 10_000_000;
const PARTIAL_FILL: i128 = 40 * 10_000_000;
const EVM_TOKEN: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Measurement {
    cpu: u64,
    mem: u64,
}

/// Reset the budget, run `f`, and snapshot CPU + memory consumed.
fn measure<T>(env: &Env, f: impl FnOnce() -> T) -> (T, Measurement) {
    env.budget().reset_default();
    let out = f();
    let b = env.budget();
    let m = Measurement {
        cpu: b.cpu_instruction_cost(),
        mem: b.memory_bytes_cost(),
    };
    (out, m)
}

struct Fixture {
    env: Env,
    contract: Address,
    admin: Address,
    fee_recipient: Address,
    user: Address,
    solver: Address,
    dst_token: Address,
    bond_token: Address,
}

impl Fixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let fee_recipient = Address::generate(&env);
        let user = Address::generate(&env);
        let solver = Address::generate(&env);
        let bond_token = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let dst_token = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let contract = env.register_contract(None, IntentSettlement);
        let f = Fixture {
            env,
            contract,
            admin,
            fee_recipient,
            user,
            solver,
            dst_token,
            bond_token,
        };
        f.client()
            .initialize(&f.admin, &f.fee_recipient, &f.bond_token);
        f
    }

    fn client(&self) -> IntentSettlementClient<'_> {
        IntentSettlementClient::new(&self.env, &self.contract)
    }

    fn bond_admin(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.bond_token)
    }

    fn dst_admin(&self) -> token::StellarAssetClient<'_> {
        token::StellarAssetClient::new(&self.env, &self.dst_token)
    }

    fn s(&self, v: &str) -> String {
        String::from_str(&self.env, v)
    }

    fn register_solver(&self) {
        self.bond_admin().mint(&self.solver, &(BOND * 4));
        self.client().register_solver(&self.solver, &BOND);
    }

    fn submit(&self, salt: u64) -> BytesN<32> {
        self.pass(salt);
        self.client().submit_intent(
            &self.user,
            &self.s("ethereum"),
            &self.s(EVM_TOKEN),
            &SRC_AMT,
            &self.dst_token,
            &MIN_DST,
            &None,
        )
    }

    fn pass(&self, secs: u64) {
        self.env.ledger().with_mut(|li| li.timestamp += secs);
    }
}

type Row = (StdString, Measurement);

fn push(rows: &mut StdVec<Row>, label: &str, m: Measurement) {
    rows.push((StdString::from(label), m));
}

/// Exercise every state-changing entrypoint once.
fn collect_rows() -> StdVec<Row> {
    let mut rows: StdVec<Row> = StdVec::new();

    {
        let f = Fixture::new();
        f.bond_admin().mint(&f.solver, &(BOND * 4));
        let (_, m) = measure(&f.env, || f.client().register_solver(&f.solver, &BOND));
        push(&mut rows, "register_solver (first)", m);
        let (_, m) = measure(&f.env, || f.client().register_solver(&f.solver, &BOND));
        push(&mut rows, "register_solver (top-up)", m);
        let (_, m) = measure(&f.env, || f.client().withdraw_bond(&f.solver, &BOND));
        push(&mut rows, "withdraw_bond", m);
        let (_, m) = measure(&f.env, || f.client().deregister_solver(&f.solver));
        push(&mut rows, "deregister_solver", m);
    }

    {
        let f = Fixture::new();
        let (_, m) = measure(&f.env, || f.submit(1));
        push(&mut rows, "submit_intent", m);
    }

    {
        let f = Fixture::new();
        f.register_solver();
        let id = f.submit(1);
        let (_, m) = measure(&f.env, || f.client().accept_intent(&f.solver, &id));
        push(&mut rows, "accept_intent", m);
    }

    {
        let f = Fixture::new();
        f.register_solver();
        f.dst_admin().mint(&f.solver, &(FULL_FILL * 2));
        let id = f.submit(1);
        f.client().accept_intent(&f.solver, &id);
        let (_, m) = measure(&f.env, || {
            f.client().fill_intent(&f.solver, &id, &FULL_FILL)
        });
        push(&mut rows, "fill_intent (full fill)", m);
    }

    {
        let f = Fixture::new();
        f.register_solver();
        f.dst_admin().mint(&f.solver, &(FULL_FILL * 2));
        let id = f.submit(1);
        f.client().accept_intent(&f.solver, &id);
        let (_, m) = measure(&f.env, || {
            f.client().fill_intent(&f.solver, &id, &PARTIAL_FILL)
        });
        push(&mut rows, "fill_intent (partial fill)", m);
    }

    {
        let f = Fixture::new();
        let id = f.submit(1);
        let (_, m) = measure(&f.env, || f.client().cancel_intent(&f.user, &id));
        push(&mut rows, "cancel_intent", m);
    }

    {
        let f = Fixture::new();
        let id = f.submit(1);
        f.pass(crate::INTENT_EXPIRY + 1);
        let (_, m) = measure(&f.env, || f.client().expire_intent(&id));
        push(&mut rows, "expire_intent", m);
    }

    {
        let f = Fixture::new();
        f.register_solver();
        let id = f.submit(1);
        f.client().accept_intent(&f.solver, &id);
        f.pass(crate::FILL_WINDOW + 1);
        let (_, m) = measure(&f.env, || f.client().slash_solver(&id));
        push(&mut rows, "slash_solver", m);
    }

    {
        let f = Fixture::new();
        f.register_solver();
        let id = f.submit(1);
        f.client().accept_intent(&f.solver, &id);
        let (_, m) = measure(&f.env, || f.client().request_extension(&f.solver, &id));
        push(&mut rows, "request_extension", m);
    }

    rows
}

/// Batch throughput: N sequential `submit_intent` / `accept_intent` calls,
/// which is exactly what `batch_submit_intent` / `batch_accept_intent` do
/// under the hood (plus a one-off size check). Reported total and per-item.
fn collect_batch_rows() -> StdVec<(StdString, Measurement, u64)> {
    let mut rows = StdVec::new();

    for &n in &[1u64, 5, 10] {
        let f = Fixture::new();
        let (_, m) = measure(&f.env, || {
            for i in 0..n {
                f.submit(1 + i);
            }
        });
        rows.push((format!("submit_intent x{n}"), m, n));

        let f = Fixture::new();
        f.register_solver();
        let mut ids: StdVec<BytesN<32>> = StdVec::new();
        for i in 0..n {
            ids.push(f.submit(1 + i));
        }
        let (_, m) = measure(&f.env, || {
            for id in &ids {
                f.client().accept_intent(&f.solver, id);
            }
        });
        rows.push((format!("accept_intent x{n}"), m, n));
    }

    rows
}

/// Serialised XDR size of the two persistent records rewritten on the hot
/// paths, read back from storage after `accept_intent`.
fn record_sizes() -> (u32, u32) {
    let f = Fixture::new();
    f.register_solver();
    let id = f.submit(1);
    f.client().accept_intent(&f.solver, &id);

    let env = &f.env;
    env.as_contract(&f.contract, || {
        let p = env.storage().persistent();
        let intent: IntentRecord = p.get(&DataKey::Intent(id.clone())).unwrap();
        let solver: SolverRecord = p.get(&DataKey::Solver(f.solver.clone())).unwrap();
        (intent.to_xdr(env).len(), solver.to_xdr(env).len())
    })
}

fn fmt_table(rows: &[Row]) -> StdString {
    let mut out = StdString::new();
    out.push_str("| Entrypoint | CPU insns | Mem bytes |\n|---|--:|--:|\n");
    for (label, m) in rows {
        out.push_str(&format!("| `{}` | {} | {} |\n", label, m.cpu, m.mem));
    }
    out
}

fn fmt_batch_table(rows: &[(StdString, Measurement, u64)]) -> StdString {
    let mut out = StdString::new();
    out.push_str(
        "| Sequence | CPU insns | CPU / item | Mem bytes | Mem / item |\n|---|--:|--:|--:|--:|\n",
    );
    for (label, m, n) in rows {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            label,
            m.cpu,
            m.cpu / n,
            m.mem,
            m.mem / n
        ));
    }
    out
}

/// Prints the resource-cost tables for `docs/149-resource-cost-per-entrypoint.md`.
#[test]
fn resource_cost_report() {
    let rows = collect_rows();
    let batch = collect_batch_rows();
    let (intent_bytes, solver_bytes) = record_sizes();

    std::println!("\n=== intent_settlement resource cost (testutils budget) ===\n");
    std::println!("{}", fmt_table(&rows));
    std::println!("{}", fmt_batch_table(&batch));
    std::println!("IntentRecord serialised: {intent_bytes} bytes");
    std::println!("SolverRecord serialised: {solver_bytes} bytes\n");
}

/// Smoke test: identical fixtures ⇒ identical measurements, so the published
/// numbers are reproducible run to run.
#[test]
fn resource_cost_is_reproducible() {
    let run = || {
        let f = Fixture::new();
        f.register_solver();
        f.dst_admin().mint(&f.solver, &(FULL_FILL * 2));
        let id = f.submit(1);
        f.client().accept_intent(&f.solver, &id);
        measure(&f.env, || {
            f.client().fill_intent(&f.solver, &id, &FULL_FILL)
        })
        .1
    };
    let a = run();
    let b = run();
    assert_eq!(
        a, b,
        "resource measurement not reproducible: {a:?} vs {b:?}"
    );
    assert!(a.cpu > 0, "cpu should be metered: {a:?}");
    assert!(a.mem > 0, "mem should be metered: {a:?}");
}
