// #576 — `expire_settlement` publishes a dedicated `settlement_expired` event so
// indexers can see expiry in the event stream without polling state. The payload
// is the `Settlement` in its final `Expired` state (see docs/event-schema.md) and
// is pinned by an XDR snapshot, following the pattern in
// contracts/invoice/tests/event_snapshots.rs.

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    xdr::ToXdr,
    Address, Env, Symbol, TryFromVal, Val,
};
use std::path::Path;
use treasury::{Settlement, SettlementStatus, TreasuryContract, TreasuryContractClient};

extern crate std;

/// Mirrors `SETTLEMENT_TTL` (7 days) in `contracts/treasury/src/settlements.rs`.
const SETTLEMENT_TTL: u64 = 7 * 24 * 60 * 60;
const PROPOSED_AT: u64 = 1_000;

fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address, u64) {
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = PROPOSED_AT);
    let admin = Address::generate(env);
    let merchant = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    (client, admin, sid)
}

fn expire(env: &Env, client: &TreasuryContractClient<'_>, admin: &Address, sid: u64) {
    env.ledger()
        .with_mut(|l| l.timestamp = PROPOSED_AT + SETTLEMENT_TTL + 1);
    client.expire_settlement(admin, &sid);
}

fn to_hex(env: &Env, payload: Val) -> std::string::String {
    use std::fmt::Write;
    let mut out = std::string::String::new();
    for byte in payload.to_xdr(env).iter() {
        write!(out, "{byte:02x}").unwrap();
    }
    out
}

fn snapshot_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
}

fn assert_snapshot(event_name: &str, hex: &str) {
    let path = snapshot_dir().join(std::format!("{event_name}.snap"));
    if path.exists() {
        let expected =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        assert_eq!(
            expected.trim(),
            hex,
            "XDR snapshot mismatch for {event_name}"
        );
    } else {
        std::fs::create_dir_all(snapshot_dir()).ok();
        std::fs::write(&path, hex).unwrap_or_else(|e| panic!("write {path:?}: {e}"));
        panic!("snapshot created at {path:?}; re-run to verify");
    }
}

#[test]
fn expire_settlement_emits_settlement_expired_event() {
    let env = Env::default();
    let (client, admin, sid) = setup(&env);

    expire(&env, &client, &admin, sid);

    let events = env.events().all();
    let (_, topics, data) = events.last().unwrap();
    assert_eq!(
        Symbol::try_from_val(&env, &topics.get_unchecked(0)).unwrap(),
        Symbol::new(&env, "settlement_expired")
    );
    assert_eq!(
        u64::try_from_val(&env, &topics.get_unchecked(1)).unwrap(),
        sid
    );

    let payload = Settlement::try_from_val(&env, &data).unwrap();
    assert_eq!(payload.id, sid);
    assert_eq!(payload.status, SettlementStatus::Expired);
    assert_eq!(payload, client.get_settlement(&sid));
}

#[test]
fn settlement_expired_event_payload_snapshot() {
    let env = Env::default();
    let (client, admin, sid) = setup(&env);

    expire(&env, &client, &admin, sid);

    let events = env.events().all();
    let (_, _, data) = events.last().unwrap();
    assert_snapshot("settlement_expired", &to_hex(&env, data));
}

#[test]
fn expire_settlement_before_ttl_emits_no_expired_event() {
    let env = Env::default();
    let (client, admin, sid) = setup(&env);

    assert!(client.try_expire_settlement(&admin, &sid).is_err());

    let expired = Symbol::new(&env, "settlement_expired");
    let emitted_expired = env.events().all().iter().any(|(_, topics, _)| {
        Symbol::try_from_val(&env, &topics.get_unchecked(0)).ok() == Some(expired.clone())
    });
    assert!(!emitted_expired);
}
