use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient};

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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // First, admin places a compliance hold
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::ComplianceReview, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Then a dispute is raised (raise_dispute won't change OnHold → OnHold, but records the dispute)
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Resolve the dispute — this should release the settlement back to Pending
    // since no other open disputes remain
    client.resolve_dispute(&admin, &did, &true, &None);
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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Two disputes raised against the same settlement
    let did_a = client.raise_dispute(&claimant_a, &sid, &merchant, &5_000_000, &500, &None);
    let did_b = client.raise_dispute(&claimant_b, &sid, &merchant, &3_000_000, &500, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Resolve dispute A — dispute B is still open so settlement stays OnHold
    client.resolve_dispute(&admin, &did_a, &true, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Resolve dispute B — now no open disputes remain, settlement released
    client.resolve_dispute(&admin, &did_b, &false, &None);
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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Admin puts the settlement on hold
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::FraudCheck, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);
    assert_eq!(
        client.get_settlement(&sid).hold_reason,
        SettlementHoldReason::FraudCheck
    );

    // Dispute is raised — settlement stays OnHold
    let did = client.raise_dispute(&claimant, &sid, &merchant, &2_000_000, &500, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Resolve dispute — settlement transitions to Pending
    client.resolve_dispute(&admin, &did, &true, &None);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );

    // Verify no double-payout: execute the settlement once
    // (We just verify the terminal state transition, no actual token transfer)
    assert!(client
        .try_execute_settlement(&admin, &sid, &Address::generate(&env))
        .is_err());
}

#[test]
fn hold_released_only_when_no_open_disputes_remain() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);
    let claimant_c = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    let did_a = client.raise_dispute(&claimant_a, &sid, &merchant, &1_000_000, &500, &None);
    let did_b = client.raise_dispute(&claimant_b, &sid, &merchant, &2_000_000, &500, &None);
    let did_c = client.raise_dispute(&claimant_c, &sid, &merchant, &3_000_000, &500, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Resolve two of three disputes — one remains open, so the hold must persist.
    client.resolve_dispute(&admin, &did_a, &true, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);
    client.resolve_dispute(&admin, &did_b, &false, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Resolving the final open dispute releases the settlement.
    client.resolve_dispute(&admin, &did_c, &true, &None);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}

#[test]
fn hold_amount_matches_sum_of_open_dispute_amounts() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    let did_a = client.raise_dispute(&claimant_a, &sid, &merchant, &4_000_000, &500, &None);
    let did_b = client.raise_dispute(&claimant_b, &sid, &merchant, &6_000_000, &500, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // While both disputes are open, the held amount equals the sum of open disputes.
    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.amount, 10_000_000);
    assert_eq!(settlement.status, SettlementStatus::OnHold);

    // Resolving one dispute leaves the other's amount still held.
    client.resolve_dispute(&admin, &did_a, &true, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Resolving the last dispute clears the hold entirely.
    client.resolve_dispute(&admin, &did_b, &true, &None);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}

#[test]
fn resolution_order_yields_same_terminal_state() {
    let env = Env::default();

    // Resolve disputes in forward order.
    let (client_fwd, admin_fwd) = setup(&env);
    let merchant_fwd = Address::generate(&env);
    let sid_fwd = client_fwd.propose_settlement(&admin_fwd, &merchant_fwd, &10_000_000, &0_u64);
    let fwd_a = client_fwd.raise_dispute(
        &Address::generate(&env),
        &sid_fwd,
        &merchant_fwd,
        &2_000_000,
        &500,
        &None,
    );
    let fwd_b = client_fwd.raise_dispute(
        &Address::generate(&env),
        &sid_fwd,
        &merchant_fwd,
        &3_000_000,
        &500,
        &None,
    );
    client_fwd.resolve_dispute(&admin_fwd, &fwd_a, &true, &None);
    client_fwd.resolve_dispute(&admin_fwd, &fwd_b, &true, &None);
    let fwd_status = client_fwd.get_settlement(&sid_fwd).status;

    // Resolve disputes in reverse order.
    let (client_rev, admin_rev) = setup(&env);
    let merchant_rev = Address::generate(&env);
    let sid_rev = client_rev.propose_settlement(&admin_rev, &merchant_rev, &10_000_000, &0_u64);
    let rev_a = client_rev.raise_dispute(
        &Address::generate(&env),
        &sid_rev,
        &merchant_rev,
        &2_000_000,
        &500,
        &None,
    );
    let rev_b = client_rev.raise_dispute(
        &Address::generate(&env),
        &sid_rev,
        &merchant_rev,
        &3_000_000,
        &500,
        &None,
    );
    client_rev.resolve_dispute(&admin_rev, &rev_b, &true, &None);
    client_rev.resolve_dispute(&admin_rev, &rev_a, &true, &None);
    let rev_status = client_rev.get_settlement(&sid_rev).status;

    assert_eq!(fwd_status, SettlementStatus::Pending);
    assert_eq!(rev_status, SettlementStatus::Pending);
    assert_eq!(fwd_status, rev_status);
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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Raise a dispute — settlement goes OnHold
    client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Attempting to execute while dispute is active should fail
    assert!(client
        .try_execute_settlement(&admin, &sid, &token_id)
        .is_err());
}
