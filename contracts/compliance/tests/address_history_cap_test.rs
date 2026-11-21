//! Ring-buffer coverage for `MAX_STATUS_HISTORY` (issue #753).
//!
//! `ComplianceContract` records a per-address status history capped at
//! `MAX_STATUS_HISTORY` entries. Once the cap is reached, the oldest entry is
//! dropped and the newest is appended (a ring buffer), rather than rejecting
//! the new status change. These tests pin that behavior down at the 11th and
//! 20th entries so the semantics are explicit and cannot silently regress.
//!
//! `MAX_STATUS_HISTORY` is re-exported by the `compliance` crate, so this suite
//! asserts against the real value rather than a hand-mirrored copy.

use compliance::{ComplianceContract, ComplianceContractClient, MAX_STATUS_HISTORY};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup() -> (Env, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

/// Drives `count` status changes on `address` by alternating between
/// `block_address` and `allow_address`, stepping the ledger clock between each
/// so any timestamp-based ordering stays strictly increasing.
fn push_status_changes(
    env: &Env,
    admin: &Address,
    client: &ComplianceContractClient<'static>,
    address: &Address,
    count: u32,
) {
    for i in 0..count {
        if i % 2 == 0 {
            client.block_address(admin, address, &None);
        } else {
            client.allow_address(admin, address);
        }
        env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    }
}

#[test]
fn history_accepts_up_to_exactly_max_status_history_entries() {
    let (env, admin, client) = setup();
    let address = Address::generate(&env);

    push_status_changes(&env, &admin, &client, &address, MAX_STATUS_HISTORY);

    let history = client.get_address_history(&address);
    assert_eq!(
        history.len(),
        MAX_STATUS_HISTORY,
        "history must hold exactly MAX_STATUS_HISTORY entries at the cap"
    );
}

#[test]
fn eleventh_status_change_drops_oldest_entry() {
    let (env, admin, client) = setup();
    let address = Address::generate(&env);

    // Fill to the cap, then record the first entry's status so we can confirm
    // it is the one evicted by the 11th change.
    push_status_changes(&env, &admin, &client, &address, MAX_STATUS_HISTORY);
    let at_cap = client.get_address_history(&address);
    let oldest_at_cap = at_cap.get(0).unwrap();

    // The 11th change must be accepted (ring buffer), not rejected.
    client.block_address(&admin, &address, &None);

    let after_eleven = client.get_address_history(&address);
    assert_eq!(
        after_eleven.len(),
        MAX_STATUS_HISTORY,
        "history length must stay capped at MAX_STATUS_HISTORY"
    );
    assert_ne!(
        after_eleven.get(0).unwrap(),
        oldest_at_cap,
        "the oldest entry must be dropped once the cap is exceeded"
    );
}

#[test]
fn twentieth_status_change_keeps_history_capped() {
    let (env, admin, client) = setup();
    let address = Address::generate(&env);

    // Push well past the cap (20 entries) and confirm the ring buffer keeps the
    // history bounded rather than growing or rejecting new entries.
    push_status_changes(&env, &admin, &client, &address, 20);

    let history = client.get_address_history(&address);
    assert_eq!(
        history.len(),
        MAX_STATUS_HISTORY,
        "history must remain capped at MAX_STATUS_HISTORY after 20 changes"
    );
}
