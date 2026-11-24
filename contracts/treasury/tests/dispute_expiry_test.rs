use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use treasury::{
    DisputeStatus, SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient,
};

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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &86_400, &None);

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
    let did = client.raise_dispute(&claimant, &9999, &merchant, &5_000_000, &500, &None);

    env.ledger().with_mut(|l| l.timestamp = 600);
    client.expire_dispute(&admin, &did);

    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Expired);
}

#[test]
fn expire_dispute_only_transitions_raised_to_expired() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    // Precondition: the dispute starts in the Raised state.
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Raised);

    env.ledger().with_mut(|l| l.timestamp = 600);
    client.expire_dispute(&admin, &did);

    // Postcondition: the only legal transition is Raised -> Expired.
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Expired);
}

#[test]
fn expire_dispute_releases_hold_exactly_once_for_last_open_dispute() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did_a = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);
    let did_b = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    // Both disputes hold the settlement.
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    env.ledger().with_mut(|l| l.timestamp = 600);

    // Expiring the first dispute must not release the hold while another is open.
    client.expire_dispute(&admin, &did_a);
    assert_eq!(client.get_dispute(&did_a).status, DisputeStatus::Expired);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Expiring the last open dispute releases the hold exactly once.
    client.expire_dispute(&admin, &did_b);
    assert_eq!(client.get_dispute(&did_b).status, DisputeStatus::Expired);
    let s = client.get_settlement(&sid);
    assert_eq!(s.status, SettlementStatus::Pending);
    assert_eq!(s.hold_reason, SettlementHoldReason::None);
}

#[test]
fn expire_dispute_on_already_expired_returns_already_resolved() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    env.ledger().with_mut(|l| l.timestamp = 600);
    client.expire_dispute(&admin, &did);
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Expired);

    // A second expiry attempt must be rejected with the typed error, not silently
    // re-run the transition (which would double-release the settlement hold).
    let result = client.try_expire_dispute(&admin, &did);
    assert_eq!(
        result,
        Err(Ok(treasury::TreasuryError::DisputeAlreadyResolved))
    );
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Expired);
}
