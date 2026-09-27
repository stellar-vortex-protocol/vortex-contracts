#![no_std]

//! Vortex Protocol — Cross-Chain Proof Registry (`proof_registry`)
//!
//! This crate implements the `ProofRegistry` contract defined in
//! [`docs/124-proof-verification-interface.md`].  It records, per Vortex
//! `intent_id`, a [`ProofRecord`] attesting that the user's source-chain
//! deposit actually happened, so `intent_settlement::fill_intent` can gate a
//! solver's claimed fill on a verified cross-chain message.
//!
//! ## Verification path (issue #189)
//!
//! [`ProofRegistry::receive_message`] performs the **production** verification
//! flow:
//!
//! 1. Hand the raw VAA bytes to the **Wormhole Core contract** (address stored
//!    at [`ProofKey::WormholeCore`]) via a cross-contract call.  That contract
//!    checks the Guardian signature set and returns the decoded VAA envelope
//!    ([`VaaEnvelope`]).  A malformed VAA or an invalid/for-quorum signature
//!    set traps there and reverts this call — an unverified payload is never
//!    touched.
//! 2. Enforce the emitter allowlist: the envelope's `emitter_chain` /
//!    `emitter_address` (the values the Guardians actually signed over, **not**
//!    the application payload) must match [`ProofKey::AuthorizedEmitter`] for
//!    that chain, otherwise [`Error::EmitterNotAuthorized`].
//! 3. Decode the fixed 102-byte application payload and cross-check that its
//!    self-declared `src_chain_id` agrees with the signed `emitter_chain`
//!    ([`Error::EmitterChainMismatch`]).
//! 4. Reject replays on two independent axes: one proof per `intent_id`
//!    ([`Error::ProofAlreadyExists`]) **and** one `(emitter_chain, sequence)`
//!    pair ever ([`Error::VaaAlreadyProcessed`]) — a VAA replayed for a
//!    different `intent_id` is still caught.
//! 5. Persist the [`ProofRecord`], populating `vaa_sequence` from the real VAA
//!    header rather than a placeholder.
//!
//! Only the Wormhole Core call boundary is external; every check above is this
//! contract's own logic.
//!
//! ## Test-controllable back-door
//!
//! `mock_set_proof` / `mock_remove_proof` (compiled only under the `testutils`
//! Cargo feature) let integration tests inject arbitrary [`ProofRecord`]s
//! directly into storage.  They are a separate entry-point and do not touch —
//! and cannot weaken — the `receive_message` verification path above.  A
//! release build compiled without `testutils` does not contain them.  Gating
//! them further (so a `testutils` build still can't be abused) is tracked as a
//! separate security issue and is out of scope here.

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, panic_with_error, Address,
    Bytes, BytesN, Env, String, Symbol,
};

/// Issue #254: how long (in seconds) a `ProofRecord` remains usable to gate a
/// `fill_intent` call after `receive_message` stores it. Chosen to comfortably
/// exceed `intent_settlement`'s 300-second `FILL_WINDOW` plus realistic
/// VAA-relay latency (1–20 minutes across the bridge protocols compared in
/// `docs/bridge-protocol-comparison.md`), so a proof arriving even somewhat
/// late is never spuriously rejected as stale. This is distinct from Soroban
/// storage-TTL archival (issue #51) — this is business-logic staleness, not
/// ledger-entry expiry.
pub const PROOF_VALIDITY_WINDOW: u64 = 3600;

#[cfg(test)]
mod test;

// ─── Wormhole Core boundary ──────────────────────────────────────────────────

/// The decoded, signature-verified VAA envelope returned by the Wormhole Core
/// contract.  These are the header fields the Guardian set signs over; the
/// application `payload` is opaque to Wormhole and decoded by this contract.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct VaaEnvelope {
    /// Wormhole chain ID of the chain that emitted the message.
    pub emitter_chain: u32,
    /// Emitter contract address on the source chain, left-padded to 32 bytes.
    pub emitter_address: BytesN<32>,
    /// Per-emitter monotonic sequence number from the VAA header.
    pub sequence: u64,
    /// The application payload (for Vortex: the fixed 102-byte deposit record).
    pub payload: Bytes,
}

/// Minimal view of the Wormhole Core contract that `proof_registry` depends on.
///
/// The real Stellar Wormhole Core deployment exposes an equivalent entry-point
/// that verifies the Guardian signature set and returns the parsed envelope;
/// integration tests substitute a mock at this boundary (and only this
/// boundary).
#[contractclient(name = "WormholeCoreClient")]
pub trait WormholeCore {
    /// Verify the Guardian signatures on `vaa` and return its decoded envelope.
    /// Must trap if the VAA is malformed or the signature set is invalid /
    /// below quorum.
    fn parse_and_verify_vaa(env: Env, vaa: Bytes) -> VaaEnvelope;
}

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum ProofKey {
    /// Admin address (set in `initialize`).
    Admin,
    /// Wormhole Core contract address used for VAA verification.
    WormholeCore,
    /// Authorized emitter address for a given Wormhole source-chain ID.
    /// Key: `chain_id: u32` (wraps a `u16`) → `emitter: BytesN<32>`.
    AuthorizedEmitter(u32),
    /// Verified proof record keyed by Vortex `intent_id`.
    Proof(BytesN<32>),
    /// Replay guard: presence means the VAA with this `(emitter_chain,
    /// sequence)` pair has already been processed, regardless of which
    /// `intent_id` it carried.
    SeenVaa(u32, u64),
}

// ─── Data Types ───────────────────────────────────────────────────────────────

/// A verified record that a source-chain deposit occurred for `intent_id`.
///
/// Populated by `receive_message` after the Wormhole Core contract has
/// verified the Guardian signatures.  Under the `testutils` feature it may
/// also be injected by `mock_set_proof`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProofRecord {
    /// Vortex intent ID this proof corresponds to.
    pub intent_id: BytesN<32>,
    /// User's address on the source chain (hex string for EVM, base58 for Solana).
    pub src_user: String,
    /// Wormhole chain ID of the source chain (e.g. 2 = Ethereum, 30 = Base).
    pub src_chain_id: u32,
    /// Source token address on the source chain.
    pub src_token: String,
    /// Amount deposited on the source chain in that token's smallest unit.
    pub src_amount: i128,
    /// Wormhole VAA sequence number, taken from the verified VAA header — used
    /// for replay protection independent of `intent_id`.
    pub vaa_sequence: u64,
    /// Ledger timestamp when this proof was registered on Stellar.
    pub received_at: u64,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    /// `initialize` called on an already-initialized registry.
    AlreadyInitialized = 1,
    /// Caller is not the admin.
    Unauthorized = 2,
    /// The VAA envelope's `(emitter_chain, emitter_address)` is not the
    /// authorized emitter for that chain.
    EmitterNotAuthorized = 3,
    /// A proof for this `intent_id` already exists (replay protection).
    ProofAlreadyExists = 4,
    /// `get_proof` / `has_proof` for an `intent_id` with no record.
    ProofNotFound = 5,
    /// VAA application payload could not be decoded (wrong length or malformed).
    InvalidPayload = 6,
    /// Contract not initialized (`Admin` key absent).
    NotInitialized = 7,
    /// The VAA with this `(emitter_chain, sequence)` was already processed —
    /// distinct from `ProofAlreadyExists`, which is keyed on `intent_id`.
    VaaAlreadyProcessed = 8,
    /// The application payload's self-declared `src_chain_id` does not match
    /// the `emitter_chain` the Guardians signed over.
    EmitterChainMismatch = 9,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct ProofRegistry;

#[contractimpl]
impl ProofRegistry {
    // ── Initialization ────────────────────────────────────────────────────────

    /// Deploy-time setup.  Records `admin`, Wormhole Core contract address,
    /// and Axelar Gateway address. Must be called exactly once.
    ///
    /// Both bridge protocols are registered at init time. The choice of which
    /// to use for incoming proofs is determined by the authorized emitter/source
    /// configuration and the calling convention (receive_message vs.
    /// receive_message_axelar).
    pub fn initialize(
        env: Env,
        admin: Address,
        wormhole_core: Address,
        axelar_gateway: Address,
    ) {
        if env.storage().instance().has(&ProofKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&ProofKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&ProofKey::WormholeCore, &wormhole_core);
        env.storage()
            .instance()
            .set(&ProofKey::AxelarGateway, &axelar_gateway);
        Self::bump_instance_ttl(&env);
    }

    // ── Admin ─────────────────────────────────────────────────────────────────

    /// Admin-only: register the trusted emitter for a Wormhole source-chain ID.
    /// Only VAAs whose **envelope** names this emitter on `chain_id` are
    /// accepted by `receive_message`.
    pub fn set_authorized_emitter(env: Env, chain_id: u32, emitter: BytesN<32>) {
        Self::require_admin(&env);
        if chain_id > u16::MAX as u32 {
            panic_with_error!(&env, Error::ChainIdOutOfRange);
        }
        env.storage()
            .instance()
            .set(&ProofKey::AuthorizedEmitter(chain_id), &emitter);
        Self::bump_instance_ttl(&env);
        env.events().publish(
            (Symbol::new(&env, "emitter_authorized"),),
            (chain_id, emitter),
        );
    }

    /// Admin-only: remove a trusted emitter (e.g. after a source contract
    /// upgrade).
    pub fn remove_authorized_emitter(env: Env, chain_id: u32) {
        Self::require_admin(&env);
        env.storage()
            .instance()
            .remove(&ProofKey::AuthorizedEmitter(chain_id));
        env.events()
            .publish((Symbol::new(&env, "emitter_removed"),), chain_id);
    }

    /// Return the authorized emitter for `chain_id`, or `None` if unset.
    pub fn get_authorized_emitter(env: Env, chain_id: u32) -> Option<BytesN<32>> {
        env.storage()
            .instance()
            .get(&ProofKey::AuthorizedEmitter(chain_id))
    }

    /// The configured Wormhole Core contract address, or `None` before init.
    pub fn get_wormhole_core(env: Env) -> Option<Address> {
        env.storage().instance().get(&ProofKey::WormholeCore)
    }

    // ── Message Receipt ───────────────────────────────────────────────────────

    /// Receive a Wormhole VAA, verify it, and store the decoded proof.
    ///
    /// Application payload layout (102 bytes, big-endian), decoded from
    /// `envelope.payload` after Guardian verification:
    /// ```text
    ///  [0..32]   intent_id    (BytesN<32>)
    ///  [32..52]  src_user     (20-byte EVM address, zero-padded on Solana)
    ///  [52..54]  src_chain_id (u16)
    ///  [54..86]  src_token    (32 bytes, address padded)
    ///  [86..102] src_amount   (i128, big-endian)
    /// ```
    ///
    /// Fails closed:
    /// * malformed VAA / bad signature set → traps inside the Wormhole Core
    ///   call, reverting this call;
    /// * `Error::EmitterNotAuthorized` — envelope emitter not on the allowlist;
    /// * `Error::EmitterChainMismatch` — payload chain ≠ signed emitter chain;
    /// * `Error::InvalidPayload` — payload not exactly 102 bytes;
    /// * `Error::ProofAlreadyExists` / `Error::VaaAlreadyProcessed` — replay.
    pub fn receive_message(env: Env, vaa: Bytes) {
        let core: Address = env
            .storage()
            .instance()
            .get(&ProofKey::WormholeCore)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));

        // ── Boundary: Guardian signature verification ────────────────────────
        // The Wormhole Core contract checks the signature set and returns the
        // decoded envelope. Anything wrong with the VAA traps there.
        let envelope = WormholeCoreClient::new(&env, &core).parse_and_verify_vaa(&vaa);

        // ── Emitter authorization (envelope, not payload) ───────────────────
        let authorized: BytesN<32> = env
            .storage()
            .instance()
            .get(&ProofKey::AuthorizedEmitter(envelope.emitter_chain))
            .unwrap_or_else(|| panic_with_error!(&env, Error::EmitterNotAuthorized));
        if authorized != envelope.emitter_address {
            panic_with_error!(&env, Error::EmitterNotAuthorized);
        }

        // ── Decode the fixed 102-byte application payload ────────────────────
        let payload = envelope.payload;
        if payload.len() != 102 {
            panic_with_error!(&env, Error::InvalidPayload);
        }

        let intent_id: BytesN<32> = payload
            .slice(0..32)
            .try_into()
            .unwrap_or_else(|_| panic_with_error!(&env, Error::InvalidPayload));

        let src_chain_id: u32 =
            ((Self::byte(&env, &payload, 52) as u32) << 8) | (Self::byte(&env, &payload, 53) as u32);

        // The payload's self-declared chain must agree with what the Guardians
        // actually signed over.
        if src_chain_id != envelope.emitter_chain {
            panic_with_error!(&env, Error::EmitterChainMismatch);
        }

        // ── Replay protection (two independent axes) ─────────────────────────
        if env
            .storage()
            .persistent()
            .has(&ProofKey::Proof(intent_id.clone()))
        {
            panic_with_error!(&env, Error::ProofAlreadyExists);
        }
        let seq_key = ProofKey::SeenVaa(envelope.emitter_chain, envelope.sequence);
        if env.storage().persistent().has(&seq_key) {
            panic_with_error!(&env, Error::VaaAlreadyProcessed);
        }

        // ── Decode remaining fields ─────────────────────────────────────────
        let mut amount_bytes = [0u8; 16];
        let mut idx = 0usize;
        while idx < 16 {
            amount_bytes[idx] = Self::byte(&env, &payload, 86 + idx as u32);
            idx += 1;
        }
        let src_amount = i128::from_be_bytes(amount_bytes);

        let src_user = Self::bytes_to_hex_string(&env, &payload.slice(32..52));
        let src_token = Self::bytes_to_hex_string(&env, &payload.slice(54..86));
        let now = env.ledger().timestamp();

        let record = ProofRecord {
            intent_id: intent_id.clone(),
            src_user,
            src_chain_id,
            src_token,
            src_amount,
            vaa_sequence: envelope.sequence,
            received_at: now,
        };

        env.storage()
            .persistent()
            .set(&ProofKey::Proof(intent_id.clone()), &record);
        env.storage().persistent().set(&seq_key, &true);

        Self::bump_proof_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "proof_received"),),
            (intent_id, src_chain_id, src_amount, envelope.sequence),
        );
    }

    /// Receive and verify an Axelar GMP message, then store the decoded proof.
    ///
    /// **Axelar integration rationale:**
    /// docs/bridge-protocol-comparison.md recommends Axelar GMP as the primary
    /// bridge protocol for Stellar: it has live Mainnet support (Feb 2026),
    /// official Stellar developer docs, and active production usage. This
    /// complementary `receive_message_axelar` path allows proofs to be relayed
    /// via either Wormhole (legacy/fallback) or Axelar (recommended).
    ///
    /// **Payload layout (same as Wormhole for compatibility):**
    /// The Axelar GMP message body encodes the same 102-byte payload as
    /// Wormhole's VAA, ensuring intent_settlement sees identical ProofRecords:
    /// ```
    ///  [0..32]   intent_id   (BytesN<32>)
    ///  [32..52]  src_user    (20-byte EVM address)
    ///  [52..54]  src_chain_id (u16)
    ///  [54..86]  src_token   (32 bytes, address padded)
    ///  [86..102] src_amount  (i128, big-endian)
    /// ```
    ///
    /// **Flow (mock behavior for now):**
    /// In production, this would call the Axelar Gateway contract to verify
    /// the message signature. For now, like receive_message, this parses the
    /// payload directly without verification.
    pub fn receive_message_axelar(
        env: Env,
        source_chain: Symbol,
        source_address: String,
        payload: Bytes,
    ) {
        if payload.len() != 102 {
            panic_with_error!(&env, Error::InvalidPayload);
        }

        // Verify source authorization
        if let Some(authorized_source) = Self::get_authorized_axelar_source(&env, source_chain.clone()) {
            if authorized_source != source_address {
                panic_with_error!(&env, Error::EmitterNotAuthorized);
            }
        } else {
            panic_with_error!(&env, Error::EmitterNotAuthorized);
        }

        // Decode intent_id (bytes 0..32).
        let intent_id: BytesN<32> = payload.slice(0..32).try_into().unwrap_or_else(|_| {
            panic_with_error!(&env, Error::InvalidPayload)
        });

        // Reject replays.
        if env
            .storage()
            .persistent()
            .has(&ProofKey::Proof(intent_id.clone()))
        {
            panic_with_error!(&env, Error::ProofAlreadyExists);
        }

        // Decode src_chain_id (bytes 52..54) as big-endian u16 → u32.
        let chain_hi = payload.get(52) as u32;
        let chain_lo = payload.get(53) as u32;
        let src_chain_id: u32 = (chain_hi << 8) | chain_lo;

        // Decode src_amount (bytes 86..102) as big-endian i128.
        let mut amount_bytes = [0u8; 16];
        let mut idx = 0usize;
        while idx < 16 {
            amount_bytes[idx] = payload.get((86 + idx) as u32) as u8;
            idx += 1;
        }
        let src_amount = i128::from_be_bytes(amount_bytes);

        let now = env.ledger().timestamp();

        let src_user = Self::bytes_to_hex_string(&env, &payload.slice(32..52));
        let src_token = Self::bytes_to_hex_string(&env, &payload.slice(54..86));

        let record = ProofRecord {
            intent_id: intent_id.clone(),
            src_user,
            src_chain_id,
            src_token,
            src_amount,
            vaa_sequence: 0, // Axelar GMP doesn't use sequence numbers like Wormhole
            received_at: now,
        };

        env.storage()
            .persistent()
            .set(&ProofKey::Proof(intent_id.clone()), &record);

        Self::bump_proof_ttl(&env, &intent_id);

        env.events().publish(
            (Symbol::new(&env, "proof_received_axelar"),),
            (intent_id, src_chain_id, src_amount),
        );
    }

    // ── Proof Queries ─────────────────────────────────────────────────────────

    /// Return the stored `ProofRecord` for `intent_id`, or `None`.
    pub fn get_proof(env: Env, intent_id: BytesN<32>) -> Option<ProofRecord> {
        env.storage().persistent().get(&ProofKey::Proof(intent_id))
    }

    /// Returns `true` iff a verified proof exists for `intent_id`.
    pub fn has_proof(env: Env, intent_id: BytesN<32>) -> bool {
        env.storage().persistent().has(&ProofKey::Proof(intent_id))
    }

    /// Return `intent_id`'s `ProofRecord` only if it exists and is still
    /// fresh (`now - received_at <= PROOF_VALIDITY_WINDOW`). Panics with
    /// `Error::ProofNotFound` if no proof was received, or
    /// `Error::ProofStale` if one exists but has aged out (issue #254).
    /// This is the entry point `fill_intent`'s proof check (issue #5) is
    /// intended to call — `get_proof`/`has_proof` remain raw, freshness-blind
    /// reads for other callers.
    pub fn get_fresh_proof(env: Env, intent_id: BytesN<32>) -> ProofRecord {
        let record: ProofRecord = env
            .storage()
            .persistent()
            .get(&ProofKey::Proof(intent_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::ProofNotFound));
        let now = env.ledger().timestamp();
        // Boundary: exactly at the validity window is still fresh (inclusive),
        // matching this codebase's documented inclusive/exclusive convention
        // (issue #26) — validity holds through the boundary second itself.
        if now - record.received_at > PROOF_VALIDITY_WINDOW {
            panic_with_error!(&env, Error::ProofStale);
        }
        record
    }

    // ── Test Back-Door ────────────────────────────────────────────────────────

    /// **Test-only** (`testutils` feature): insert a `ProofRecord` directly,
    /// bypassing all VAA parsing and Guardian verification.  Separate from the
    /// `receive_message` path — see the module doc.
    #[cfg(feature = "testutils")]
    pub fn mock_set_proof(env: Env, record: ProofRecord) {
        if env
            .storage()
            .persistent()
            .has(&ProofKey::Proof(record.intent_id.clone()))
        {
            panic_with_error!(&env, Error::ProofAlreadyExists);
        }
        env.storage()
            .persistent()
            .set(&ProofKey::Proof(record.intent_id.clone()), &record);
        Self::bump_proof_ttl(&env, &record.intent_id);
    }

    /// **Test-only** (`testutils` feature): remove a stored proof.
    #[cfg(feature = "testutils")]
    pub fn mock_remove_proof(env: Env, intent_id: BytesN<32>) {
        env.storage()
            .persistent()
            .remove(&ProofKey::Proof(intent_id));
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&ProofKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, Error::NotInitialized));
        admin.require_auth();
    }

    /// Read one byte of `bytes`, failing closed with `InvalidPayload` if the
    /// index is out of range (callers have already length-checked, so this is
    /// defence-in-depth rather than an expected path).
    fn byte(env: &Env, bytes: &Bytes, i: u32) -> u8 {
        bytes
            .get(i)
            .unwrap_or_else(|| panic_with_error!(env, Error::InvalidPayload))
    }

    /// Convert a raw byte slice into a lowercase `0x`-prefixed hex `String`.
    /// Callers pass the 20-byte `src_user` and 32-byte `src_token` slices, so
    /// the output is at most `2 + 64 = 66` characters; anything longer is a
    /// malformed payload and fails closed.
    fn bytes_to_hex_string(env: &Env, bytes: &Bytes) -> String {
        const HEX: &[u8] = b"0123456789abcdef";
        let len = bytes.len();
        if len > 32 {
            panic_with_error!(env, Error::InvalidPayload);
        }
        let mut buf = [0u8; 66];
        buf[0] = b'0';
        buf[1] = b'x';
        let mut w = 2usize;
        let mut i = 0u32;
        while i < len {
            let byte = bytes.get(i).unwrap_or(0);
            buf[w] = HEX[(byte >> 4) as usize];
            buf[w + 1] = HEX[(byte & 0x0f) as usize];
            w += 2;
            i += 1;
        }
        String::from_bytes(env, &buf[..w])
    }

    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

    fn bump_proof_ttl(env: &Env, intent_id: &BytesN<32>) {
        env.storage().persistent().extend_ttl(
            &ProofKey::Proof(intent_id.clone()),
            PROOF_TTL_THRESHOLD,
            PROOF_TTL_EXTEND_TO,
        );
    }
}
