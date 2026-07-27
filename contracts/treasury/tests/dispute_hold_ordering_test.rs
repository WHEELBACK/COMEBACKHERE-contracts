use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{
    SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient,
};

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
fn dispute_resolved_while_hold_active_releases_to_pending() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    // First, admin places a compliance hold
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::ComplianceReview);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::OnHold
    );

    // Then a dispute is raised (raise_dispute won't change OnHold → OnHold, but records the dispute)
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::OnHold
    );

    // Resolve the dispute — this should release the settlement back to Pending
    // since no other open disputes remain
    client.resolve_dispute(&admin, &did, &true);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}

#[test]
fn second_dispute_keeps_hold_after_first_resolved() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    // Two disputes raised against the same settlement
    let did_a = client.raise_dispute(&claimant_a, &sid, &merchant, &5_000_000, &500);
    let did_b = client.raise_dispute(&claimant_b, &sid, &merchant, &3_000_000, &500);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::OnHold
    );

    // Resolve dispute A — dispute B is still open so settlement stays OnHold
    client.resolve_dispute(&admin, &did_a, &true);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::OnHold
    );

    // Resolve dispute B — now no open disputes remain, settlement released
    client.resolve_dispute(&admin, &did_b, &false);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}

#[test]
fn dispute_raised_after_hold_settlement_produces_consistent_state() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    // Admin puts the settlement on hold
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::FraudCheck);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::OnHold
    );
    assert_eq!(
        client.get_settlement(&sid).hold_reason,
        SettlementHoldReason::FraudCheck
    );

    // Dispute is raised — settlement stays OnHold
    let did = client.raise_dispute(&claimant, &sid, &merchant, &2_000_000, &500);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::OnHold
    );

    // Resolve dispute — settlement transitions to Pending
    client.resolve_dispute(&admin, &did, &true);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );

    // Verify no double-payout: execute the settlement once
    // (We just verify the terminal state transition, no actual token transfer)
    assert!(client.try_execute_settlement(&admin, &sid, &Address::generate(&env)).is_err());
}

#[test]
fn execute_settlement_rejected_while_dispute_active() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let token_id = Address::generate(&env);

    // Add token to allowlist to avoid TokenNotAllowed error
    client.add_allowed_token(&admin, &token_id);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    // Raise a dispute — settlement goes OnHold
    client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::OnHold
    );

    // Attempting to execute while dispute is active should fail
    assert!(client
        .try_execute_settlement(&admin, &sid, &token_id)
        .is_err());
}
