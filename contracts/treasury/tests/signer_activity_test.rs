/// Tests for `get_signer_last_active` per-signer activity tracking (issue #587).
///
/// `record_approval` now writes `DataKey::SignerLastActive(signer)` with the
/// current ledger timestamp on every approval path:
///   - `propose_settlement` (proposer auto-approves)
///   - `approve_settlement`
///   - `propose_signer_rotation` (proposer auto-approves)
///   - `approve_signer_rotation`
///   - `vote_dispute_resolution`
///
/// `get_signer_last_active` returns `Some(timestamp)` after any approval,
/// and `None` for a signer that has never approved anything.
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};
use treasury::{TreasuryContract, TreasuryContractClient};

fn setup(env: &Env, threshold: u32) -> (TreasuryContractClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &threshold, &Vec::new(env));
    (client, admin)
}

// ── Never-approved signer returns None ───────────────────────────────────

#[test]
fn unknown_signer_returns_none() {
    let env = Env::default();
    let (client, _) = setup(&env, 1);

    let stranger = Address::generate(&env);
    assert_eq!(client.get_signer_last_active(&stranger), None);
}

// ── propose_settlement records timestamp for the proposing signer ─────────

#[test]
fn propose_settlement_records_timestamp() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let ts: u64 = 12_345;
    env.ledger().with_mut(|l| l.timestamp = ts);

    let merchant = Address::generate(&env);
    client.propose_settlement(&admin, &merchant, &1_000_000);

    assert_eq!(client.get_signer_last_active(&admin), Some(ts));
}

// ── approve_settlement updates the approving signer's timestamp ───────────

#[test]
fn approve_settlement_updates_timestamp() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);

    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let merchant = Address::generate(&env);

    // Propose at t=100 (admin gets t=100).
    env.ledger().with_mut(|l| l.timestamp = 100);
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    assert_eq!(client.get_signer_last_active(&admin), Some(100));

    // backup approves at t=200.
    env.ledger().with_mut(|l| l.timestamp = 200);
    client.approve_settlement(&backup, &sid);
    assert_eq!(client.get_signer_last_active(&backup), Some(200));

    // admin's timestamp is still 100 (no new approval from admin).
    assert_eq!(client.get_signer_last_active(&admin), Some(100));
}

// ── Second approval from same signer overwrites the timestamp ────────────

#[test]
fn repeated_approval_updates_to_latest_timestamp() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 500);
    client.propose_settlement(&admin, &merchant_a, &1_000_000);
    assert_eq!(client.get_signer_last_active(&admin), Some(500));

    env.ledger().with_mut(|l| l.timestamp = 800);
    client.propose_settlement(&admin, &merchant_b, &2_000_000);
    assert_eq!(client.get_signer_last_active(&admin), Some(800));
}

// ── propose_signer_rotation records timestamp for the proposer ───────────

#[test]
fn propose_signer_rotation_records_timestamp() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    client.set_signer(&admin, &old_signer, &1);

    env.ledger().with_mut(|l| l.timestamp = 300);
    client.propose_signer_rotation(&old_signer, &old_signer, &new_signer);

    assert_eq!(client.get_signer_last_active(&old_signer), Some(300));
}

// ── vote_dispute_resolution records timestamp for the voting signer ───────

#[test]
fn vote_dispute_resolution_records_timestamp() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &5_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &1_000_000, &9_999_999);

    env.ledger().with_mut(|l| l.timestamp = 700);
    client.vote_dispute_resolution(&admin, &did, &true);

    assert_eq!(client.get_signer_last_active(&admin), Some(700));
}

// ── Multiple signers each get their own independent timestamp ─────────────

#[test]
fn each_signer_has_independent_timestamp() {
    let env = Env::default();
    let (client, admin) = setup(&env, 3);

    let signer_b = Address::generate(&env);
    let signer_c = Address::generate(&env);
    client.set_signer(&admin, &signer_b, &1);
    client.set_signer(&admin, &signer_c, &1);

    let merchant = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);

    env.ledger().with_mut(|l| l.timestamp = 2_000);
    client.approve_settlement(&signer_b, &sid);

    env.ledger().with_mut(|l| l.timestamp = 3_000);
    client.approve_settlement(&signer_c, &sid);

    assert_eq!(client.get_signer_last_active(&admin), Some(1_000));
    assert_eq!(client.get_signer_last_active(&signer_b), Some(2_000));
    assert_eq!(client.get_signer_last_active(&signer_c), Some(3_000));
}
