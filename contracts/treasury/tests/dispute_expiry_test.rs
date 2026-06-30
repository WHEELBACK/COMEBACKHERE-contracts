use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use treasury::{DisputeStatus, SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
fn expire_dispute_transitions_to_expired() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500);

    env.ledger().with_mut(|l| l.timestamp = 600);
    client.expire_dispute(&admin, &did);

    let dispute = client.get_dispute(&did);
    assert_eq!(dispute.status, DisputeStatus::Expired);
}

#[test]
fn expire_dispute_releases_settlement_hold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500);

    let s = client.get_settlement(&sid);
    assert_eq!(s.status, SettlementStatus::OnHold);

    env.ledger().with_mut(|l| l.timestamp = 600);
    client.expire_dispute(&admin, &did);

    let s = client.get_settlement(&sid);
    assert_eq!(s.status, SettlementStatus::Pending);
    assert_eq!(s.hold_reason, SettlementHoldReason::None);
}

#[test]
fn expire_dispute_at_exact_deadline_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500);

    // The guard is `timestamp < expires_at`, so timestamp == expires_at must succeed.
    env.ledger().with_mut(|l| l.timestamp = 500);
    client.expire_dispute(&admin, &did);

    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Expired);
}

#[test]
fn raise_dispute_stores_expires_at_field() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &86_400);

    let dispute = client.get_dispute(&did);
    assert_eq!(dispute.dispute_expires_at, 86_400);
    // The DisputeNotExpired guard fires when timestamp < dispute_expires_at.
    // With default timestamp=0 and expires_at=86_400 that guard would trigger;
    // the stored field is the sole precondition checked.
    assert_eq!(dispute.status, DisputeStatus::Raised);
}

#[test]
fn expire_dispute_without_linked_settlement_still_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    // Raise a dispute against a non-existent settlement (no OnHold settlement to release).
    let did = client.raise_dispute(&claimant, &9999, &merchant, &5_000_000, &500);

    env.ledger().with_mut(|l| l.timestamp = 600);
    client.expire_dispute(&admin, &did);

    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Expired);
}
