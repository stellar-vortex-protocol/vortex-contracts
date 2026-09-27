#![cfg(test)]

//! Tests for the `ProofRegistry` contract.
//!
//! The Wormhole Core contract is the **only** mocked boundary: [`MockWormholeCore`]
//! stands in for Guardian-quorum signature verification.  Everything downstream
//! of it — emitter authorization, payload decoding, chain cross-checks and
//! replay protection — is the real `proof_registry` logic under test.
//!
//! Fixtures for issue #189:
//! * `receive_message_stores_verified_proof` — a valid VAA.
//! * `receive_message_rejects_unauthorized_emitter` — VAA from an emitter not
//!   on the allowlist.
//! * `receive_message_rejects_tampered_payload` — VAA whose payload was
//!   altered after signing (Core verification traps).
//! * `receive_message_rejects_replayed_sequence` — a VAA whose
//!   `(emitter_chain, sequence)` was already processed.

use crate::{Error, ProofRecord, ProofRegistry, ProofRegistryClient, VaaEnvelope};
use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, Address, Bytes, BytesN, Env, String,
};

// ─── Mock Wormhole Core (the one mocked boundary) ────────────────────────────

/// Test double for the Wormhole Core contract.
///
/// It parses the real Wormhole VAA wire layout and, in place of a secp256k1
/// Guardian-quorum check, requires that signature 0's `r` field equals
/// `sha256(body)`.  This gives a faithful "the signature commits to the whole
/// body" property — any post-signing tamper of the envelope or payload makes
/// verification trap — without needing a Guardian set in-process.
///
/// VAA layout it understands:
/// ```text
///  [0]        version (== 1)
///  [1..5]     guardian_set_index  (u32 BE, ignored)
///  [5]        signature_count n   (>= 1 required = quorum stand-in)
///  [6 .. 6+66n] signatures; sig 0 = [idx u8][r 32][s 32][v u8]
///  --- body (offset B = 6 + 66n) ---
///  [B+0..B+4]    timestamp u32 BE (ignored)
///  [B+4..B+8]    nonce u32 BE (ignored)
///  [B+8..B+10]   emitter_chain u16 BE
///  [B+10..B+42]  emitter_address 32 bytes
///  [B+42..B+50]  sequence u64 BE
///  [B+50]        consistency_level u8 (ignored)
///  [B+51..]      payload
/// ```
#[contract]
pub struct MockWormholeCore;

#[contractimpl]
impl MockWormholeCore {
    pub fn parse_and_verify_vaa(env: Env, vaa: Bytes) -> VaaEnvelope {
        assert!(vaa.len() >= 6, "VAA header truncated");
        assert_eq!(vaa.get(0).unwrap(), 1, "unsupported VAA version");

        let n = vaa.get(5).unwrap() as u32;
        assert!(n >= 1, "no Guardian signatures / quorum not met");

        let body_start = 6 + 66 * n;
        assert!(vaa.len() as u32 > body_start, "VAA body missing");
        let body = vaa.slice(body_start..vaa.len());

        // Guardian-signature stand-in: sig 0's r == sha256(body).
        let expected: BytesN<32> = env.crypto().sha256(&body).into();
        let r: BytesN<32> = vaa
            .slice(7..39)
            .try_into()
            .expect("signature 0 r-field must be 32 bytes");
        assert_eq!(r, expected, "invalid Guardian signature (body tampered)");

        let emitter_chain =
            (((body.get(8).unwrap() as u32) << 8) | (body.get(9).unwrap() as u32)) as u32;
        let emitter_address: BytesN<32> = body
            .slice(10..42)
            .try_into()
            .expect("emitter_address must be 32 bytes");
        let mut seq = [0u8; 8];
        let mut i = 0u32;
        while i < 8 {
            seq[i as usize] = body.get(42 + i).unwrap();
            i += 1;
        }
        let sequence = u64::from_be_bytes(seq);
        let payload = body.slice(51..body.len());

        VaaEnvelope {
            emitter_chain,
            emitter_address,
            sequence,
            payload,
        }
    }
}

// ─── Fixture ─────────────────────────────────────────────────────────────────

struct Ctx {
    env: Env,
    admin: Address,
    wormhole_core: Address,
    contract_id: Address,
}

impl Ctx {
    fn client(&self) -> ProofRegistryClient<'_> {
        ProofRegistryClient::new(&self.env, &self.contract_id)
    }
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let wormhole_core = env.register_contract(None, MockWormholeCore);
    let contract_id = env.register_contract(None, ProofRegistry);

    let ctx = Ctx {
        env,
        admin,
        wormhole_core,
        contract_id,
    };
    ctx.client().initialize(&ctx.admin, &ctx.wormhole_core);
    ctx
}

fn make_intent_id(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    bytes[31] = seed;
    BytesN::from_array(env, &bytes)
}

fn emitter_addr(env: &Env, tag: u8) -> BytesN<32> {
    BytesN::from_array(env, &[tag; 32])
}

/// Build the fixed 102-byte Vortex deposit payload.
fn make_payload(intent_id: &BytesN<32>, src_chain_id: u16, src_amount: i128) -> [u8; 102] {
    let mut p = [0u8; 102];
    p[0..32].copy_from_slice(&intent_id.to_array());
    // [32..52] src_user — left zeroed
    p[52] = (src_chain_id >> 8) as u8;
    p[53] = (src_chain_id & 0xff) as u8;
    // [54..86] src_token — left zeroed
    p[86..102].copy_from_slice(&src_amount.to_be_bytes());
    p
}

/// Assemble a full VAA the [`MockWormholeCore`] will accept.
fn build_vaa(
    env: &Env,
    emitter_chain: u16,
    emitter_address: &BytesN<32>,
    sequence: u64,
    payload: &[u8],
) -> Bytes {
    let mut body = Bytes::new(env);
    body.extend_from_array(&0u32.to_be_bytes()); // timestamp
    body.extend_from_array(&0u32.to_be_bytes()); // nonce
    body.extend_from_array(&emitter_chain.to_be_bytes());
    body.extend_from_array(&emitter_address.to_array());
    body.extend_from_array(&sequence.to_be_bytes());
    body.push_back(1u8); // consistency_level
    body.append(&Bytes::from_slice(env, payload));

    let hash: BytesN<32> = env.crypto().sha256(&body).into();

    let mut vaa = Bytes::new(env);
    vaa.push_back(1u8); // version
    vaa.extend_from_array(&0u32.to_be_bytes()); // guardian_set_index
    vaa.push_back(1u8); // signature_count
    vaa.push_back(0u8); // sig 0: guardian_index
    vaa.extend_from_array(&hash.to_array()); // sig 0: r == sha256(body)
    vaa.extend_from_array(&[0u8; 32]); // sig 0: s
    vaa.push_back(0u8); // sig 0: v
    vaa.append(&body);
    vaa
}

// ─── Initialization ─────────────────────────────────────────────────────────

#[test]
fn initialize_succeeds_once() {
    let ctx = setup();
    let res = ctx.client().try_initialize(&ctx.admin, &ctx.wormhole_core);
    assert_eq!(res, Err(Ok(Error::AlreadyInitialized.into())));
}

#[test]
fn initialize_records_wormhole_core() {
    let ctx = setup();
    assert_eq!(ctx.client().get_wormhole_core(), Some(ctx.wormhole_core.clone()));
}

// ─── Authorized emitter management ──────────────────────────────────────────

#[test]
fn set_and_get_authorized_emitter() {
    let ctx = setup();
    let emitter = emitter_addr(&ctx.env, 0xde);
    ctx.client().set_authorized_emitter(&2, &emitter);
    assert_eq!(ctx.client().get_authorized_emitter(&2), Some(emitter));
}

#[test]
fn get_authorized_emitter_returns_none_if_unset() {
    let ctx = setup();
    assert_eq!(ctx.client().get_authorized_emitter(&2), None);
}

// #262 — the real Wormhole chain-ID space is 16 bits; chain_id must be
// rejected once it exceeds u16::MAX.
#[test]
fn set_authorized_emitter_accepts_u16_max_boundary() {
    let ctx = setup();
    let c = ctx.client();
    let emitter: BytesN<32> = BytesN::from_array(&ctx.env, &[0xde; 32]);

    c.set_authorized_emitter(&(u16::MAX as u32), &emitter);
    assert_eq!(c.get_authorized_emitter(&(u16::MAX as u32)), Some(emitter));
}

#[test]
fn set_authorized_emitter_rejects_above_u16_max() {
    let ctx = setup();
    let c = ctx.client();
    let emitter: BytesN<32> = BytesN::from_array(&ctx.env, &[0xde; 32]);

    let res = c.try_set_authorized_emitter(&(u16::MAX as u32 + 1), &emitter);
    assert_eq!(res, Err(Ok(Error::ChainIdOutOfRange.into())));
}

#[test]
fn remove_authorized_emitter_clears_entry() {
    let ctx = setup();
    let emitter = emitter_addr(&ctx.env, 0xab);
    ctx.client().set_authorized_emitter(&2, &emitter);
    assert!(ctx.client().get_authorized_emitter(&2).is_some());
    ctx.client().remove_authorized_emitter(&2);
    assert_eq!(ctx.client().get_authorized_emitter(&2), None);
}

// ─── receive_message: valid VAA ────────────────────────────────────────────

#[test]
fn receive_message_stores_verified_proof() {
    let ctx = setup();
    let c = ctx.client();

    let emitter = emitter_addr(&ctx.env, 0x11);
    c.set_authorized_emitter(&2, &emitter);

    let intent_id = make_intent_id(&ctx.env, 1);
    let payload = make_payload(&intent_id, 2, 1_000_000_000);
    let vaa = build_vaa(&ctx.env, 2, &emitter, 4242, &payload);

    c.receive_message(&vaa);

    assert!(c.has_proof(&intent_id));
    let record = c.get_proof(&intent_id).expect("proof should exist");
    assert_eq!(record.intent_id, intent_id);
    assert_eq!(record.src_chain_id, 2);
    assert_eq!(record.src_amount, 1_000_000_000);
    // vaa_sequence comes from the real VAA header, not a placeholder.
    assert_eq!(record.vaa_sequence, 4242);
}

#[test]
fn receive_message_decodes_large_src_amount() {
    let ctx = setup();
    let c = ctx.client();
    let emitter = emitter_addr(&ctx.env, 0x11);
    c.set_authorized_emitter(&2, &emitter);

    let intent_id = make_intent_id(&ctx.env, 3);
    let eth_amount: i128 = 1_000_000_000_000_000_000; // 1 ETH in wei
    let payload = make_payload(&intent_id, 2, eth_amount);
    let vaa = build_vaa(&ctx.env, 2, &emitter, 1, &payload);

    c.receive_message(&vaa);
    assert_eq!(c.get_proof(&intent_id).unwrap().src_amount, eth_amount);
}

// ─── receive_message: unauthorized emitter ─────────────────────────────────

#[test]
fn receive_message_rejects_unauthorized_emitter() {
    let ctx = setup();
    let c = ctx.client();

    let authorized = emitter_addr(&ctx.env, 0xaa);
    let attacker = emitter_addr(&ctx.env, 0xbb);
    c.set_authorized_emitter(&2, &authorized);

    let intent_id = make_intent_id(&ctx.env, 4);
    let payload = make_payload(&intent_id, 2, 500);
    let vaa = build_vaa(&ctx.env, 2, &attacker, 1, &payload);

    let res = c.try_receive_message(&vaa);
    assert_eq!(res, Err(Ok(Error::EmitterNotAuthorized.into())));
    assert!(!c.has_proof(&intent_id));
}

#[test]
fn receive_message_rejects_when_no_emitter_configured_for_chain() {
    let ctx = setup();
    let c = ctx.client();

    // chain 2 configured, VAA claims chain 30 — nothing authorized there.
    c.set_authorized_emitter(&2, &emitter_addr(&ctx.env, 0xaa));

    let intent_id = make_intent_id(&ctx.env, 5);
    let payload = make_payload(&intent_id, 30, 500);
    let vaa = build_vaa(&ctx.env, 30, &emitter_addr(&ctx.env, 0xaa), 1, &payload);

    let res = c.try_receive_message(&vaa);
    assert_eq!(res, Err(Ok(Error::EmitterNotAuthorized.into())));
}

// ─── receive_message: tampered payload ─────────────────────────────────────

#[test]
fn receive_message_rejects_tampered_payload() {
    let ctx = setup();
    let c = ctx.client();
    let emitter = emitter_addr(&ctx.env, 0x11);
    c.set_authorized_emitter(&2, &emitter);

    let intent_id = make_intent_id(&ctx.env, 6);
    let payload = make_payload(&intent_id, 2, 1_000);
    let vaa = build_vaa(&ctx.env, 2, &emitter, 1, &payload);

    // Flip one byte inside the signed body (the src_amount region of the
    // payload) without re-signing. Core verification must trap.
    let mut raw = [0u8; 512];
    let len = vaa.len();
    vaa.copy_into_slice(&mut raw[..len as usize]);
    let tamper_idx = (len - 1) as usize; // last payload byte = low byte of src_amount
    raw[tamper_idx] ^= 0xff;
    let tampered = Bytes::from_slice(&ctx.env, &raw[..len as usize]);

    let res = c.try_receive_message(&tampered);
    assert!(res.is_err(), "tampered VAA must be rejected at the Core boundary");
    assert!(!c.has_proof(&intent_id));
}

// ─── receive_message: replay protection ────────────────────────────────────

#[test]
fn receive_message_rejects_replayed_sequence() {
    let ctx = setup();
    let c = ctx.client();
    let emitter = emitter_addr(&ctx.env, 0x11);
    c.set_authorized_emitter(&2, &emitter);

    let intent_a = make_intent_id(&ctx.env, 7);
    let payload_a = make_payload(&intent_a, 2, 111);
    c.receive_message(&build_vaa(&ctx.env, 2, &emitter, 900, &payload_a));

    // A different intent_id, but the same (emitter_chain, sequence) pair.
    let intent_b = make_intent_id(&ctx.env, 8);
    let payload_b = make_payload(&intent_b, 2, 222);
    let res = c.try_receive_message(&build_vaa(&ctx.env, 2, &emitter, 900, &payload_b));

    assert_eq!(res, Err(Ok(Error::VaaAlreadyProcessed.into())));
    assert!(!c.has_proof(&intent_b));
}

#[test]
fn receive_message_rejects_duplicate_intent_id() {
    let ctx = setup();
    let c = ctx.client();
    let emitter = emitter_addr(&ctx.env, 0x11);
    c.set_authorized_emitter(&2, &emitter);

    let intent_id = make_intent_id(&ctx.env, 9);
    let payload = make_payload(&intent_id, 2, 333);
    c.receive_message(&build_vaa(&ctx.env, 2, &emitter, 10, &payload));

    // Same intent_id, fresh sequence — caught by the per-intent_id guard.
    let res = c.try_receive_message(&build_vaa(&ctx.env, 2, &emitter, 11, &payload));
    assert_eq!(res, Err(Ok(Error::ProofAlreadyExists.into())));
}

// ─── receive_message: malformed / inconsistent ────────────────────────────

#[test]
fn receive_message_rejects_chain_mismatch() {
    let ctx = setup();
    let c = ctx.client();
    let emitter = emitter_addr(&ctx.env, 0x11);
    c.set_authorized_emitter(&2, &emitter);

    let intent_id = make_intent_id(&ctx.env, 10);
    // Envelope says chain 2 (authorized) but the payload claims chain 30.
    let payload = make_payload(&intent_id, 30, 1);
    let vaa = build_vaa(&ctx.env, 2, &emitter, 1, &payload);

    let res = c.try_receive_message(&vaa);
    assert_eq!(res, Err(Ok(Error::EmitterChainMismatch.into())));
}

#[test]
fn receive_message_rejects_wrong_payload_length() {
    let ctx = setup();
    let c = ctx.client();
    let emitter = emitter_addr(&ctx.env, 0x11);
    c.set_authorized_emitter(&2, &emitter);

    // 80-byte payload instead of 102.
    let vaa = build_vaa(&ctx.env, 2, &emitter, 1, &[0u8; 80]);
    let res = c.try_receive_message(&vaa);
    assert_eq!(res, Err(Ok(Error::InvalidPayload.into())));
}

#[test]
fn receive_message_traps_on_truncated_vaa() {
    let ctx = setup();
    let short = Bytes::from_slice(&ctx.env, &[0u8; 4]);
    let res = ctx.client().try_receive_message(&short);
    assert!(res.is_err());
}

// ─── has_proof / get_proof ────────────────────────────────────────────────

#[test]
fn has_proof_returns_false_for_unknown_intent() {
    let ctx = setup();
    assert!(!ctx.client().has_proof(&make_intent_id(&ctx.env, 99)));
}

#[test]
fn get_proof_returns_none_for_unknown_intent() {
    let ctx = setup();
    assert!(ctx.client().get_proof(&make_intent_id(&ctx.env, 99)).is_none());
}

// ─── mock_set_proof back-door (testutils only) ────────────────────────────

#[cfg(feature = "testutils")]
#[test]
fn mock_set_proof_injects_controllable_record() {
    let ctx = setup();
    let c = ctx.client();
    let intent_id = make_intent_id(&ctx.env, 20);

    let record = ProofRecord {
        intent_id: intent_id.clone(),
        src_user: String::from_str(&ctx.env, "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
        src_chain_id: 2,
        src_token: String::from_str(&ctx.env, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        src_amount: 5_000 * 1_000_000,
        vaa_sequence: 42,
        received_at: ctx.env.ledger().timestamp(),
    };
    c.mock_set_proof(&record);

    let stored = c.get_proof(&intent_id).unwrap();
    assert_eq!(stored.src_amount, 5_000 * 1_000_000);
    assert_eq!(stored.vaa_sequence, 42);
}

#[cfg(feature = "testutils")]
#[test]
fn mock_set_proof_rejects_duplicate() {
    let ctx = setup();
    let c = ctx.client();
    let intent_id = make_intent_id(&ctx.env, 21);
    let record = ProofRecord {
        intent_id: intent_id.clone(),
        src_user: String::from_str(&ctx.env, "0xabc"),
        src_chain_id: 30,
        src_token: String::from_str(&ctx.env, "0xdef"),
        src_amount: 100,
        vaa_sequence: 1,
        received_at: 0,
    };
    c.mock_set_proof(&record.clone());
    assert_eq!(
        c.try_mock_set_proof(&record),
        Err(Ok(Error::ProofAlreadyExists.into()))
    );
}

#[cfg(feature = "testutils")]
#[test]
fn mock_remove_proof_clears_stored_record() {
    let ctx = setup();
    let c = ctx.client();
    let intent_id = make_intent_id(&ctx.env, 22);
    let record = ProofRecord {
        intent_id: intent_id.clone(),
        src_user: String::from_str(&ctx.env, "0x1234"),
        src_chain_id: 5,
        src_token: String::from_str(&ctx.env, "0x5678"),
        src_amount: 999,
        vaa_sequence: 7,
        received_at: 0,
    };
    c.mock_set_proof(&record);
    assert!(c.has_proof(&intent_id));
    c.mock_remove_proof(&intent_id);
    assert!(!c.has_proof(&intent_id));
}
