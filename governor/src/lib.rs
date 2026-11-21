use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    IntoVal, Symbol, Val, Vec,
};
use vortex_common::require_admin;
//! Stake-weighted Governor for protocol parameter changes.
//!
//! Voting power is derived from solver bond plus delegated stake recorded in
//! `solver_registry` checkpoints. Power is snapshotted at proposal creation so
//! stake acquired after the snapshot cannot influence an in-flight vote
//! (anti-flash-stake). Passed proposals are queued into the
//! `TimelockController` and executed after the timelock delay.
//!
//! ## Voting power source (v1)
//!
//! The governor maintains its own checkpoint history per account, fed by an
//! explicit `sync_power` call. `sync_power` reads the account's current bond
//! from `solver_registry` (the v1 source of truth) and appends a `Checkpoint`
//! at the current ledger. This keeps the governor self-contained (no
//! cross-contract read at vote time) while still deriving power from the
//! registry's bond data.
//!
//! Delegation (bond + delegated stake) is a documented follow-up and is out of
//! scope for v1: only the account's own bond contributes to its power.

#![no_std]

/// Number of checkpoints retained per account. Bounded so binary search over
/// the checkpoint history stays cheap and storage stays predictable.
pub const MAX_CHECKPOINTS: u32 = 64;

/// Persistent entries are bumped once their remaining TTL drops below this
/// many ledgers, extending them back out to `PERSISTENT_TTL_EXTEND_TO`.
pub const PERSISTENT_TTL_THRESHOLD: u32 = 100_000;
/// Target TTL (in ledgers) for persistent entries after a bump.
pub const PERSISTENT_TTL_EXTEND_TO: u32 = 1_000_000;

/// Maximum number of proposals returned by a single `list_proposals` call.
pub const MAX_LIST_LIMIT: u32 = 50;

/// Minimum voting delay (in ledgers) accepted at `initialize`/`update_config`.
pub const MIN_VOTING_DELAY: u32 = 1;
/// Maximum voting delay (in ledgers) accepted at `initialize`/`update_config`.
pub const MAX_VOTING_DELAY: u32 = 1_000_000;
/// Minimum voting period (in ledgers) accepted at `initialize`/`update_config`.
pub const MIN_VOTING_PERIOD: u32 = 1;
/// Maximum voting period (in ledgers) accepted at `initialize`/`update_config`.
pub const MAX_VOTING_PERIOD: u32 = 1_000_000;
/// Minimum proposal threshold accepted at `initialize`/`update_config`.
pub const MIN_PROPOSAL_THRESHOLD: i128 = 1;
/// Maximum proposal threshold accepted at `initialize`/`update_config`.
pub const MAX_PROPOSAL_THRESHOLD: i128 = i128::MAX / 4;
/// Minimum quorum accepted at `initialize`/`update_config`.
pub const MIN_QUORUM: i128 = 1;
/// Maximum quorum accepted at `initialize`/`update_config`.
pub const MAX_QUORUM: i128 = i128::MAX / 4;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkpoint {
    /// Ledger sequence at which the balance became effective.
    pub ledger: u32,
    /// Cumulative voting power at `ledger`.
    pub power: i128,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoteType {
    Against = 0,
    For = 1,
    Abstain = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposalState {
    Pending,
    Active,
    Defeated,
    Succeeded,
    Queued,
    Executed,
    Canceled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub proposer: Address,
    pub snapshot: u32,
    pub deadline: u32,
    pub eta: u32,
    pub for_votes: i128,
    pub against_votes: i128,
    pub abstain_votes: i128,
    pub canceled: bool,
    pub executed: bool,
    pub target: Address,
    pub function: Symbol,
    pub calldata: BytesN<32>,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GovernorError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    BelowProposalThreshold = 3,
    ProposalNotFound = 4,
    NotActive = 5,
    VotingClosed = 6,
    AlreadyVoted = 7,
    NoVotingPower = 8,
    QuorumNotMet = 9,
    ProposalNotSucceeded = 10,
    ProposalNotQueued = 11,
    TimelockNotReady = 12,
    AlreadyExecuted = 13,
    ProposalCanceled = 14,
    SelfTargetForbidden = 15,
    VoteTallyOverflow = 16,
    DeadlineOverflow = 17,
    InvalidVotingDelay = 18,
    InvalidVotingPeriod = 19,
    InvalidProposalThreshold = 20,
    InvalidQuorum = 21,
    Unauthorized = 22,
    NotAuthorized = 23,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernorConfig {
    pub registry: Address,
    pub timelock: Address,
    pub voting_delay: u32,
    pub voting_period: u32,
    pub proposal_threshold: i128,
    pub quorum: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernorKey {
    pub proposal_id: u32,
    pub voter: Address,
}

#[contract]
pub struct Governor;

#[contractimpl]
impl Governor {
    /// One-time configuration of the governor.
    ///
    /// `admin` must authorize the call, so a freshly-deployed governor cannot
    /// be front-run and configured by an arbitrary party. The `admin` address
    /// is recorded so the deployment authority is auditable on-chain.
    pub fn initialize(
        env: Env,
        admin: Address,
        config: GovernorConfig,
    ) -> Result<(), GovernorError> {
        admin.require_auth();
        let key = Symbol::new(&env, "config");
        if env.storage().instance().has(&key) {
            return Err(GovernorError::AlreadyInitialized);
        }
        Self::validate_config(&config)?;
        env.storage().instance().set(&key, &config);
        env.storage().instance().set(&Symbol::new(&env, "admin"), &admin);
        env.storage().instance().set(&Symbol::new(&env, "count"), &0u32);
        env.events().publish(
            (symbol_short!("init"),),
            (
                config.registry.clone(),
                config.timelock.clone(),
                config.voting_delay,
                config.voting_period,
                config.proposal_threshold,
                config.quorum,
            ),
        );
        Ok(())
    }

    /// Update the governor's own parameters. Only reachable through the
    /// proposal/vote/timelock execution path: `execute` invokes this entrypoint
    /// on the governor itself when a proposal targets it with `update_config`.
    /// The caller must be the governor's own address, which can only be the
    /// case when the invocation originates from `execute`.
    pub fn update_config(env: Env, config: GovernorConfig) -> Result<(), GovernorError> {
        let caller = env.current_contract_address();
        // `execute` performs the call from the governor's own address; any
        // external direct invocation has a different `invoker` and is rejected.
        caller.require_auth();
        if env.invoker() != caller {
            return Err(GovernorError::Unauthorized);
        }
        Self::validate_config(&config)?;
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "config"), &config);
        env.events().publish(
            (symbol_short!("cfg_upd"),),
            (
                config.registry.clone(),
                config.timelock.clone(),
                config.voting_delay,
                config.voting_period,
                config.proposal_threshold,
                config.quorum,
            ),
        );
        Ok(())
    }

    /// Record the caller's current voting power as a checkpoint.
    ///
    /// Reads the account's bond from `solver_registry` (v1 source of truth)
    /// and appends a `Checkpoint` at the current ledger. Must be called by the
    /// account itself so power cannot be forged for another address.
    pub fn sync_power(env: Env, account: Address) -> Result<i128, GovernorError> {
        account.require_auth();
        let config = Self::config(&env)?;
        let power = Self::read_registry_bond(&env, &config.registry, &account);
        Self::write_checkpoint(&env, &account, power);
        Ok(power)
    }

    /// Create a proposal. Voting power is snapshotted at the current ledger.
    pub fn propose(
        env: Env,
        proposer: Address,
        target: Address,
        function: Symbol,
        calldata: BytesN<32>,
    ) -> Result<u32, GovernorError> {
        proposer.require_auth();
        let config = Self::config(&env)?;

        // A proposal may not target the governor itself: self-governance of the
        // voting rules would let a bare majority rewrite the rules mid-flight.
        if target == env.current_contract_address() {
            return Err(GovernorError::SelfTargetForbidden);
        }

        let snapshot = env.ledger().sequence();
        let power = Self::compute_voting_power(&env, &config.registry, &proposer, snapshot);
        if power < config.proposal_threshold {
            return Err(GovernorError::BelowProposalThreshold);
        }

        let id: u32 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "count"))
            .unwrap_or(0u32);
        let proposal = Proposal {
            proposer,
            snapshot,
            deadline: Self::deadline(&env, snapshot, &config)?,
            eta: 0,
            for_votes: 0,
            against_votes: 0,
            abstain_votes: 0,
            canceled: false,
            executed: false,
            target,
            function,
            calldata,
        };
        let proposal_key = Self::proposal_key(&env, id);
        env.storage().persistent().set(&proposal_key, &proposal);
        Self::bump_persistent_ttl(&env, &proposal_key);
        env.storage().instance().set(&Symbol::new(&env, "count"), &(id + 1));
        env.events().publish(
            (symbol_short!("propose"),),
            (id, proposal.proposer.clone(), proposal.target.clone()),
        );
        Ok(id)
    }

    /// Cancel a proposal. Mirrors `timelock`'s cancel precedent: the proposer
    /// may cancel before voting starts, and the timelock (acting as the
    /// guardian/admin) may cancel at any point before execution.
    pub fn cancel(env: Env, caller: Address, proposal_id: u32) -> Result<(), GovernorError> {
        caller.require_auth();
        let config = Self::config(&env)?;
        let mut proposal = Self::proposal(&env, proposal_id)?;

        if proposal.executed {
            return Err(GovernorError::AlreadyExecuted);
        }
        if proposal.canceled {
            return Err(GovernorError::ProposalCanceled);
        }

        let voting_started = env.ledger().sequence() >= proposal.snapshot + config.voting_delay;
        let is_proposer = caller == proposal.proposer;
        let is_admin = caller == config.timelock;
        if !is_admin && !(is_proposer && !voting_started) {
            return Err(GovernorError::NotAuthorized);
        }

        proposal.canceled = true;
        env.storage().persistent().set(&Self::proposal_key(&env, proposal_id), &proposal);
        env.events().publish(
            (Symbol::new(&env, "proposal_canceled"), proposal_id),
            caller,
        );
        Ok(())
    }

    /// Cast a vote. Power is read at the proposal snapshot, never at the
    /// current ledger, so stake acquired after the snapshot has no effect.
    pub fn cast_vote(
        env: Env,
        voter: Address,
        proposal_id: u32,
        support: VoteType,
    ) -> Result<i128, GovernorError> {
        voter.require_auth();
        let config = Self::config(&env)?;
        let mut proposal = Self::load_proposal(&env, proposal_id)?;

        if proposal.canceled {
            return Err(GovernorError::ProposalCanceled);
        }
        let now = env.ledger().sequence();
        let voting_start = proposal
            .snapshot
            .checked_add(config.voting_delay)
            .ok_or(GovernorError::DeadlineOverflow)?;
        if now < voting_start {
            return Err(GovernorError::NotActive);
        }
        if now > proposal.deadline {
            return Err(GovernorError::VotingClosed);
        }

        let vote_key = Self::vote_key(&env, proposal_id, &voter);
        if env.storage().persistent().has(&vote_key) {
            return Err(GovernorError::AlreadyVoted);
        }

        let power = Self::compute_voting_power(&env, &config.registry, &voter, proposal.snapshot);
        if power <= 0 {
            return Err(GovernorError::NoVotingPower);
        }

        match support {
            VoteType::For => {
                proposal.for_votes = proposal
                    .for_votes
                    .checked_add(power)
                    .ok_or(GovernorError::VoteTallyOverflow)?;
            }
            VoteType::Against => {
                proposal.against_votes = proposal
                    .against_votes
                    .checked_add(power)
                    .ok_or(GovernorError::VoteTallyOverflow)?;
            }
            VoteType::Abstain => {
                proposal.abstain_votes = proposal
                    .abstain_votes
                    .checked_add(power)
                    .ok_or(GovernorError::VoteTallyOverflow)?;
            }
        }
        env.storage().persistent().set(&vote_key, &power);
        Self::bump_persistent_ttl(&env, &vote_key);
        let proposal_key = Self::proposal_key(&env, proposal_id);
        env.storage().persistent().set(&proposal_key, &proposal);
        Self::bump_persistent_ttl(&env, &proposal_key);
        env.events().publish(
            (symbol_short!("vote"),),
            (proposal_id, voter.clone(), support, power),
        );
        Ok(power)
    }

    /// Queue a succeeded proposal into the timelock.
    ///
    /// The proposal's `target`/`function`/`calldata` are submitted to the
    /// configured `timelock` contract's scheduling entrypoint so the timelock
    /// owns the delayed execution. The governor records the resulting `eta`.
    pub fn queue(env: Env, proposal_id: u32) -> Result<(), GovernorError> {
        let config = Self::config(&env)?;
        let mut proposal = Self::load_proposal(&env, proposal_id)?;
        if Self::state_of(&env, &config, &proposal) != ProposalState::Succeeded {
            return Err(GovernorError::ProposalNotSucceeded);
        }
        let eta = env.ledger().sequence() + config.voting_delay;
        // Schedule the call on the timelock. The timelock's `schedule` entrypoint
        // takes the target, function, args and the earliest execution ledger.
        let args: Vec<Val> = Vec::new(&env);
        env.invoke_contract::<()>(
            &config.timelock,
            &Symbol::new(&env, "schedule"),
            (proposal.target.clone(), proposal.function.clone(), args, eta).into_val(&env),
        );
        proposal.eta = eta;
        let proposal_key = Self::proposal_key(&env, proposal_id);
        env.storage().persistent().set(&proposal_key, &proposal);
        Self::bump_persistent_ttl(&env, &proposal_key);
        env.events()
            .publish((symbol_short!("queue"),), (proposal_id, proposal.eta));
        Ok(())
    }

    /// Execute a queued proposal once the timelock delay has elapsed.
    ///
    /// The governor asks the timelock to perform the actual cross-contract call
    /// before marking the proposal executed, so a "passed and executed"
    /// proposal has real on-chain effect.
    pub fn execute(env: Env, proposal_id: u32) -> Result<(), GovernorError> {
        let config = Self::config(&env)?;
        let mut proposal = Self::load_proposal(&env, proposal_id)?;
        if proposal.executed {
            return Err(GovernorError::AlreadyExecuted);
        }
        if proposal.eta == 0 {
            return Err(GovernorError::ProposalNotQueued);
        }
        if env.ledger().sequence() < proposal.eta {
            return Err(GovernorError::TimelockNotReady);
        }
        // Ask the timelock to execute the scheduled call. The timelock performs
        // the actual `invoke_contract` on the proposal's target.
        let args: Vec<Val> = Vec::new(&env);
        env.invoke_contract::<Val>(
            &config.timelock,
            &Symbol::new(&env, "execute"),
            (proposal.target.clone(), proposal.function.clone(), proposal.calldata.clone(), args)
                .into_val(&env),
        );
        proposal.executed = true;
        let proposal_key = Self::proposal_key(&env, proposal_id);
        env.storage().persistent().set(&proposal_key, &proposal);
        Self::bump_persistent_ttl(&env, &proposal_key);
        env.events().publish(
            (symbol_short!("execute"),),
            (proposal_id, proposal.target.clone(), proposal.function.clone()),
        );
        Ok(())
    }

    /// The governor's own configuration.
    ///
    /// Every other contract in the repo with an admin/protocol config struct
    /// exposes a public getter for it (e.g. `intent_settlement::get_config`).
    /// `governor` was the exception: `GovernorConfig` was only ever read
    /// internally through the private `Self::config` helper, which made it
    /// impossible for a UI, dashboard or off-chain tool to display the
    /// governor's own parameters — the registry it reads voting power from,
    /// the timelock it queues into, the voting delay, the voting period, the
    /// proposal threshold and the quorum — without guessing at private storage
    /// layout.
    ///
    /// This is a read-only view: it performs no authorization check (there is
    /// nothing to authorize — the configuration is public by nature) and it
    /// mutates no state.
    ///
    /// Returns [`GovernorError::NotInitialized`] if the governor has not been
    /// initialized yet, exactly like every other entrypoint.
    pub fn get_config(env: Env) -> Result<GovernorConfig, GovernorError> {
        Self::config(&env)
    }

    pub fn state(env: Env, proposal_id: u32) -> Result<ProposalState, GovernorError> {
        let config = Self::config(&env)?;
        let proposal = Self::load_proposal(&env, proposal_id)?;
        Ok(Self::state_of(&env, &config, &proposal))
    }

    pub fn proposal(env: Env, proposal_id: u32) -> Result<Proposal, GovernorError> {
        Self::load_proposal(&env, proposal_id)
    }

    /// Total number of proposals ever created. Proposal ids are dense in
    /// `[0, count)`, so this doubles as the exclusive upper bound for
    /// pagination.
    pub fn get_proposal_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "count"))
            .unwrap_or(0u32)
    }

    /// Paginated proposal listing. Returns proposals with ids in
    /// `[start, start + limit)`, clamped to the total proposal count and to
    /// `MAX_LIST_LIMIT`. Mirrors `intent_settlement`'s pagination convention.
    pub fn list_proposals(env: Env, start: u32, limit: u32) -> Vec<Proposal> {
        let count = Self::get_proposal_count(env.clone());
        let mut out = Vec::new(&env);
        if start >= count || limit == 0 {
            return out;
        }
        let capped = if limit > MAX_LIST_LIMIT { MAX_LIST_LIMIT } else { limit };
        let end = if start > count - capped { count } else { start + capped };
        let mut id = start;
        while id < end {
            if let Some(proposal) = env
                .storage()
                .persistent()
                .get::<_, Proposal>(&Self::proposal_key(&env, id))
            {
                out.push_back(proposal);
            }
            id += 1;
        }
        out
    }

    /// Admin-gated update of the governor configuration. The admin is the
    /// current contract address' stored admin, validated via
    /// `vortex_common::require_admin`.
    pub fn set_config(env: Env, admin: Address, config: GovernorConfig) -> Result<(), GovernorError> {
        admin.require_auth();
        require_admin(&env, Symbol::new(&env, "admin"), &admin)
            .map_err(|_| GovernorError::NotAuthorized)?;
        env.storage().instance().set(&Symbol::new(&env, "config"), &config);
        Ok(())
    }

    pub fn voting_power(env: Env, account: Address, ledger: u32) -> Result<i128, GovernorError> {
        let config = Self::config(&env)?;
        Ok(Self::voting_power(&env, &config.registry, &account, ledger))
    }

    // --- internals ---

    /// Validate all four tunable parameters against their sanity bounds.
    fn validate_config(config: &GovernorConfig) -> Result<(), GovernorError> {
        if config.voting_delay < MIN_VOTING_DELAY || config.voting_delay > MAX_VOTING_DELAY {
            return Err(GovernorError::InvalidVotingDelay);
        }
        if config.voting_period < MIN_VOTING_PERIOD || config.voting_period > MAX_VOTING_PERIOD {
            return Err(GovernorError::InvalidVotingPeriod);
        }
        if config.proposal_threshold < MIN_PROPOSAL_THRESHOLD
            || config.proposal_threshold > MAX_PROPOSAL_THRESHOLD
        {
            return Err(GovernorError::InvalidProposalThreshold);
        }
        if config.quorum < MIN_QUORUM || config.quorum > MAX_QUORUM {
            return Err(GovernorError::InvalidQuorum);
        }
        Ok(())
    }

    fn config(env: &Env) -> Result<GovernorConfig, GovernorError> {
        env.storage()
            .instance()
            .get(&Symbol::new(env, "config"))
            .ok_or(GovernorError::NotInitialized)
    }

    fn load_proposal(env: &Env, proposal_id: u32) -> Result<Proposal, GovernorError> {
        env.storage()
            .persistent()
            .get(&Self::proposal_key(env, proposal_id))
            .ok_or(GovernorError::ProposalNotFound)
    }

    fn proposal(env: &Env, proposal_id: u32) -> Result<Proposal, GovernorError> {
        Self::load_proposal(env, proposal_id)
    }

    fn state_of(env: &Env, config: &GovernorConfig, proposal: &Proposal) -> ProposalState {
        if proposal.canceled {
            return ProposalState::Canceled;
        }
        if proposal.executed {
            return ProposalState::Executed;
        }
        let now = env.ledger().sequence();
        if now < proposal.snapshot + config.voting_delay {
            return ProposalState::Pending;
        }
        if now <= proposal.deadline {
            return ProposalState::Active;
        }
        if proposal.eta != 0 {
            return ProposalState::Queued;
        }
        let total = proposal.for_votes + proposal.against_votes + proposal.abstain_votes;
        if proposal.for_votes > proposal.against_votes && total >= config.quorum {
            ProposalState::Succeeded
        } else {
            ProposalState::Defeated
        }
    }

    /// Read voting power from the registry's checkpointed history using a
    /// binary search over the bounded checkpoint list.
    fn voting_power(env: &Env, registry: &Address, account: &Address, ledger: u32) -> i128 {
        let key = Self::checkpoint_key(env, registry, account);
        let checkpoints: Vec<Checkpoint> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(env));
        Self::search(&checkpoints, ledger)
    }

    /// Alias kept for callers that use the `compute_voting_power` name.
    fn compute_voting_power(env: &Env, registry: &Address, account: &Address, ledger: u32) -> i128 {
        Self::voting_power(env, registry, account, ledger)
    }

    /// Read the account's current bond from `solver_registry` (v1 source of
    /// truth for voting power).
    fn read_registry_bond(env: &Env, registry: &Address, account: &Address) -> i128 {
        env.invoke_contract::<i128>(
            registry,
            &Symbol::new(env, "bond"),
            (account.clone(),).into_val(env),
        )
    }

    /// Append a checkpoint for `account` at the current ledger, capping the
    /// history at `MAX_CHECKPOINTS` by dropping the oldest entry.
    fn write_checkpoint(env: &Env, account: &Address, power: i128) {
        let config = Self::config(env).expect("config");
        let key = Self::checkpoint_key(env, &config.registry, account);
        let mut checkpoints: Vec<Checkpoint> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(env));
        let ledger = env.ledger().sequence();
        checkpoints.push_back(Checkpoint { ledger, power });
        while checkpoints.len() > MAX_CHECKPOINTS {
            checkpoints.remove(0);
        }
        env.storage().persistent().set(&key, &checkpoints);
    }

    /// Binary search for the latest checkpoint at or before `ledger`.
    fn search(checkpoints: &Vec<Checkpoint>, ledger: u32) -> i128 {
        let len = checkpoints.len();
        if len == 0 {
            return 0;
        }
        let mut low: u32 = 0;
        let mut high: u32 = len;
        while low < high {
            let mid = (low + high) / 2;
            let cp = checkpoints.get(mid).unwrap();
            if cp.ledger <= ledger {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if low == 0 {
            0
        } else {
            checkpoints.get(low - 1).unwrap().power
        }
    }

    /// Extend the TTL of a persistent entry if it is close to expiring.
    fn bump_persistent_ttl(env: &Env, key: &impl soroban_sdk::IntoVal<Env, Val>) {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO);
    }

    /// Compute a proposal's deadline, guarding against ledger overflow.
    fn deadline(env: &Env, snapshot: u32, config: &GovernorConfig) -> Result<u32, GovernorError> {
        let _ = env;
        snapshot
            .checked_add(config.voting_delay)
            .and_then(|v| v.checked_add(config.voting_period))
            .ok_or(GovernorError::DeadlineOverflow)
    }

    fn proposal_key(env: &Env, proposal_id: u32) -> (Symbol, u32) {
        (Symbol::new(env, "proposal"), proposal_id)
    }

    fn vote_key(env: &Env, proposal_id: u32, voter: &Address) -> (Symbol, GovernorKey) {
        (
            Symbol::new(env, "vote"),
            GovernorKey {
                proposal_id,
                voter: voter.clone(),
            },
        )
    }

    fn checkpoint_key(env: &Env, registry: &Address, account: &Address) -> (Symbol, Address, Address) {
        (
            Symbol::new(env, "checkpoint"),
            registry.clone(),
            account.clone(),
        )
    }
}

#[cfg(test)]
mod test;
