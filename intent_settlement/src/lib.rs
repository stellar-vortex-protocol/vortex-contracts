#![no_std]

//! Vortex Protocol — Cross-Chain Intent Settlement
//!
//! Users submit swap intents (e.g. "swap 1 ETH on Ethereum for ~3500 USDC on Stellar").
//! Solvers compete to fill these intents off-chain, then settle on-chain via this contract.
//! Settlement is guaranteed by a solver bond; failing to fill within the deadline slashes the bond.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, xdr::ToXdr,
    Address, Bytes, BytesN, Env, String, Symbol,
};

#[cfg(test)]
mod test;

// ─── Constants ────────────────────────────────────────────────────────────────

const INTENT_EXPIRY: u64 = 1800; // 30 minutes
const FILL_WINDOW: u64 = 300; // 5 minutes to fill after intent accepted
const MIN_BOND: i128 = 50 * 10_000_000; // 50 USDC minimum solver bond
const PROTOCOL_FEE_BPS: i128 = 5; // 0.05%

// Soroban archives ledger entries that go too long without being touched.
// Persistent Intent/Solver records get their TTL bumped on every write so
// they don't need to be manually restored before later calls can read them.
const DAY_IN_LEDGERS: u32 = 17280; // ~5s per ledger
const PERSISTENT_TTL_THRESHOLD: u32 = DAY_IN_LEDGERS * 14;
const PERSISTENT_TTL_EXTEND_TO: u32 = DAY_IN_LEDGERS * 30;

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    FeeRecipient,
    BondToken,          // USDC address for bonds
    Intent(BytesN<32>), // intent_id -> IntentRecord
    Solver(Address),    // address -> SolverRecord
    TotalIntents,
    TotalVolume,
}

// ─── Data Structs ─────────────────────────────────────────────────────────────

/// A user's cross-chain swap intent
#[contracttype]
#[derive(Clone)]
pub struct IntentRecord {
    pub intent_id: BytesN<32>,
    pub user: Address,

    /// Source chain details (off-chain reference)
    pub src_chain: String, // "ethereum" | "base" | "polygon" etc.
    pub src_token: String, // token address on source chain
    pub src_amount: i128,  // amount in source token's smallest unit

    /// Destination (always Stellar)
    pub dst_token: Address, // SAC/SEP-41 token on Stellar
    pub min_dst_amount: i128, // minimum acceptable output

    pub solver: Option<Address>, // assigned solver
    pub state: IntentState,

    pub created_at: u64,
    pub deadline: u64,
    pub filled_at: Option<u64>,
    pub fill_amount: Option<i128>, // actual amount received
}

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum IntentState {
    Open,      // awaiting solver
    Accepted,  // solver claimed it
    Filled,    // user received output
    Cancelled, // user cancelled before fill
    Expired,   // deadline passed, no fill
    Slashed,   // solver failed to fill after accepting
}

/// A registered solver (market maker)
#[contracttype]
#[derive(Clone)]
pub struct SolverRecord {
    pub address: Address,
    pub bond_amount: i128, // USDC locked as collateral
    pub fills_completed: u32,
    pub fills_failed: u32,
    pub total_volume: i128,
    pub is_active: bool,
    pub registered_at: u64,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    Unauthorized = 2,
    IntentNotFound = 3,
    IntentNotOpen = 4,
    IntentExpired = 5,
    IntentNotAccepted = 6,
    SolverNotRegistered = 7,
    SolverBondTooLow = 8,
    InsufficientOutput = 9,
    FillWindowExpired = 10,
    CannotCancelAccepted = 11,
    SolverInactive = 12,
    ZeroAmount = 13,
    InvalidDeadline = 14,
    IntentAlreadyFilled = 15,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct IntentSettlement;

#[contractimpl]
impl IntentSettlement {
    // ── Initialization ────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address, fee_recipient: Address, bond_token: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &fee_recipient);
        env.storage()
            .instance()
            .set(&DataKey::BondToken, &bond_token);
        env.storage().instance().set(&DataKey::TotalIntents, &0u64);
        env.storage().instance().set(&DataKey::TotalVolume, &0i128);
    }

    // ── Solver Management ─────────────────────────────────────────────────────

    /// Solvers register by depositing a USDC bond
    pub fn register_solver(env: Env, solver: Address, bond_amount: i128) {
        solver.require_auth();

        if bond_amount < MIN_BOND {
            panic_with_error!(&env, Error::SolverBondTooLow);
        }

        let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
        let client = token::Client::new(&env, &bond_token);
        client.transfer(&solver, &env.current_contract_address(), &bond_amount);

        let existing: Option<SolverRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()));

        let record = match existing {
            Some(mut s) => {
                s.bond_amount += bond_amount;
                s.is_active = true;
                s
            }
            None => SolverRecord {
                address: solver.clone(),
                bond_amount,
                fills_completed: 0,
                fills_failed: 0,
                total_volume: 0,
                is_active: true,
                registered_at: env.ledger().timestamp(),
            },
        };

        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &record);
        Self::bump_solver_ttl(&env, &solver);

        env.events().publish(
            (Symbol::new(&env, "solver_registered"), solver),
            bond_amount,
        );
    }

    pub fn deregister_solver(env: Env, solver: Address) {
        solver.require_auth();

        let record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SolverNotRegistered));

        // Return bond
        if record.bond_amount > 0 {
            let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
            let client = token::Client::new(&env, &bond_token);
            client.transfer(
                &env.current_contract_address(),
                &solver,
                &record.bond_amount,
            );
        }

        env.storage()
            .persistent()
            .remove(&DataKey::Solver(solver.clone()));

        env.events().publish(
            (Symbol::new(&env, "solver_deregistered"), solver),
            record.bond_amount,
        );
    }

    // ── Intent Lifecycle ──────────────────────────────────────────────────────

    /// User submits a swap intent. No funds are locked on Stellar at this point —
    /// the user initiates the source-chain tx separately.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_intent(
        env: Env,
        user: Address,
        src_chain: String,
        src_token: String,
        src_amount: i128,
        dst_token: Address,
        min_dst_amount: i128,
        deadline: Option<u64>,
    ) -> BytesN<32> {
        user.require_auth();

        if src_amount <= 0 || min_dst_amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }

        let now = env.ledger().timestamp();
        let expiry = deadline.unwrap_or(now + INTENT_EXPIRY);

        if expiry <= now {
            panic_with_error!(&env, Error::InvalidDeadline);
        }

        // Deterministic intent_id = hash(user, src_chain, src_token, src_amount, now)
        let intent_id = Self::compute_intent_id(&env, &user, &src_chain, src_amount, now);

        let intent = IntentRecord {
            intent_id: intent_id.clone(),
            user: user.clone(),
            src_chain,
            src_token,
            src_amount,
            dst_token,
            min_dst_amount,
            solver: None,
            state: IntentState::Open,
            created_at: now,
            deadline: expiry,
            filled_at: None,
            fill_amount: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        let total: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TotalIntents)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalIntents, &(total + 1));

        env.events().publish(
            (Symbol::new(&env, "intent_submitted"), user),
            (intent_id.clone(), min_dst_amount, expiry),
        );

        intent_id
    }

    /// Solver claims an intent (exclusive fill right for FILL_WINDOW seconds)
    pub fn accept_intent(env: Env, solver: Address, intent_id: BytesN<32>) {
        solver.require_auth();

        let solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SolverNotRegistered));

        if !solver_record.is_active {
            panic_with_error!(&env, Error::SolverInactive);
        }

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        let now = env.ledger().timestamp();
        if now >= intent.deadline {
            intent.state = IntentState::Expired;
            env.storage()
                .persistent()
                .set(&DataKey::Intent(intent_id.clone()), &intent);
            Self::bump_intent_ttl(&env, &intent_id);
            panic_with_error!(&env, Error::IntentExpired);
        }

        if intent.state != IntentState::Open {
            panic_with_error!(&env, Error::IntentNotOpen);
        }

        intent.solver = Some(solver.clone());
        intent.state = IntentState::Accepted;
        // Extend deadline to fill window from now
        intent.deadline = now + FILL_WINDOW;

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "intent_accepted"), solver),
            (intent_id, intent.deadline),
        );
    }

    /// Solver fills the intent by sending dst_token to the user
    /// The solver provides cross-chain proof (stored off-chain; on-chain we trust solver's bond)
    pub fn fill_intent(env: Env, solver: Address, intent_id: BytesN<32>, fill_amount: i128) {
        solver.require_auth();

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        let now = env.ledger().timestamp();
        if now >= intent.deadline {
            panic_with_error!(&env, Error::FillWindowExpired);
        }

        match &intent.state {
            IntentState::Accepted => {}
            IntentState::Filled => panic_with_error!(&env, Error::IntentAlreadyFilled),
            _ => panic_with_error!(&env, Error::IntentNotAccepted),
        }

        if intent.solver.as_ref() != Some(&solver) {
            panic_with_error!(&env, Error::Unauthorized);
        }

        if fill_amount < intent.min_dst_amount {
            panic_with_error!(&env, Error::InsufficientOutput);
        }

        // Solver delivers the full requested output to the user.
        let dst_client = token::Client::new(&env, &intent.dst_token);
        dst_client.transfer(&solver, &intent.user, &fill_amount);

        // Solver also pays the protocol fee (priced into their quote). Taking the
        // fee from the solver — rather than clawing it back from the user — keeps
        // the user's received amount at or above `min_dst_amount`, and keeps every
        // token transfer authorized by the solver who signed this call.
        let fee = fill_amount * PROTOCOL_FEE_BPS / 10_000;
        if fee > 0 {
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .unwrap();
            dst_client.transfer(&solver, &fee_recipient, &fee);
        }

        intent.state = IntentState::Filled;
        intent.filled_at = Some(now);
        intent.fill_amount = Some(fill_amount);

        // Update solver stats
        let mut solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver.clone()))
            .unwrap();
        solver_record.fills_completed += 1;
        solver_record.total_volume += fill_amount;
        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver.clone()), &solver_record);
        Self::bump_solver_ttl(&env, &solver);

        // Update protocol stats
        let total_vol: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalVolume)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalVolume, &(total_vol + fill_amount));

        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "intent_filled"), solver),
            (intent_id, fill_amount, fee),
        );
    }

    /// User can cancel an Open intent (not yet accepted)
    pub fn cancel_intent(env: Env, user: Address, intent_id: BytesN<32>) {
        user.require_auth();

        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        if intent.user != user {
            panic_with_error!(&env, Error::Unauthorized);
        }

        if intent.state == IntentState::Accepted {
            panic_with_error!(&env, Error::CannotCancelAccepted);
        }

        if intent.state != IntentState::Open {
            panic_with_error!(&env, Error::IntentNotOpen);
        }

        intent.state = IntentState::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events()
            .publish((Symbol::new(&env, "intent_cancelled"), user), intent_id);
    }

    /// Permissionless: slash a solver that accepted but didn't fill within FILL_WINDOW
    pub fn slash_solver(env: Env, intent_id: BytesN<32>) {
        let mut intent: IntentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::IntentNotFound));

        let now = env.ledger().timestamp();

        if intent.state != IntentState::Accepted {
            panic_with_error!(&env, Error::IntentNotAccepted);
        }

        if now < intent.deadline {
            panic_with_error!(&env, Error::FillWindowExpired); // not expired yet
        }

        let solver_addr = intent.solver.clone().unwrap();
        let mut solver_record: SolverRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Solver(solver_addr.clone()))
            .unwrap();

        // Slash 10% of bond
        let slash_amount = solver_record.bond_amount / 10;
        solver_record.bond_amount -= slash_amount;
        solver_record.fills_failed += 1;

        // Re-open the intent
        intent.state = IntentState::Open;
        intent.solver = None;
        intent.deadline = now + INTENT_EXPIRY;

        // Send slash to fee recipient
        if slash_amount > 0 {
            let bond_token: Address = env.storage().instance().get(&DataKey::BondToken).unwrap();
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .unwrap();
            let client = token::Client::new(&env, &bond_token);
            client.transfer(
                &env.current_contract_address(),
                &fee_recipient,
                &slash_amount,
            );
        }

        env.storage()
            .persistent()
            .set(&DataKey::Solver(solver_addr.clone()), &solver_record);
        Self::bump_solver_ttl(&env, &solver_addr);
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent_id.clone()), &intent);
        Self::bump_intent_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "solver_slashed"), solver_addr),
            (intent_id, slash_amount),
        );
    }

    // ── Views ─────────────────────────────────────────────────────────────────

    pub fn get_intent(env: Env, intent_id: BytesN<32>) -> Option<IntentRecord> {
        env.storage().persistent().get(&DataKey::Intent(intent_id))
    }

    pub fn get_solver(env: Env, solver: Address) -> Option<SolverRecord> {
        env.storage().persistent().get(&DataKey::Solver(solver))
    }

    pub fn get_stats(env: Env) -> (u64, i128) {
        let intents: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TotalIntents)
            .unwrap_or(0);
        let volume: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalVolume)
            .unwrap_or(0);
        (intents, volume)
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn bump_intent_ttl(env: &Env, intent_id: &BytesN<32>) {
        env.storage().persistent().extend_ttl(
            &DataKey::Intent(intent_id.clone()),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    fn bump_solver_ttl(env: &Env, solver: &Address) {
        env.storage().persistent().extend_ttl(
            &DataKey::Solver(solver.clone()),
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    fn compute_intent_id(
        env: &Env,
        user: &Address,
        src_chain: &String,
        amount: i128,
        timestamp: u64,
    ) -> BytesN<32> {
        // Build a collision-resistant preimage from the full intent context, then
        // hash to a 32-byte id. Including the user and source chain ensures two
        // otherwise-identical intents from different users or chains never collide.
        let mut preimage = Bytes::new(env);
        preimage.append(&user.clone().to_xdr(env));
        preimage.append(&src_chain.clone().to_xdr(env));
        preimage.extend_from_array(&amount.to_be_bytes());
        preimage.extend_from_array(&timestamp.to_be_bytes());
        env.crypto().sha256(&preimage).into()
    }
}
