use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementHoldReason, TreasuryContract, TreasuryContractClient, TreasuryError};

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
fn hold_settlement_returns_already_on_hold_when_called_twice() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    assert_eq!(
        client.try_hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &None),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_hold_settlement(&admin, &sid, &SettlementHoldReason::FraudCheck, &None),
        Err(Ok(TreasuryError::AlreadyOnHold))
    );
}

#[test]
fn hold_settlement_still_returns_already_executed_for_other_non_pending_statuses() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    client.cancel_settlement(&admin, &sid);

    assert_eq!(
        client.try_hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &None),
        Err(Ok(TreasuryError::AlreadyExecuted))
    );
}

#[test]
fn get_hold_reason_returns_reason_while_on_hold() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Initially hold_reason is None
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::None);

    // Hold with a reason
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::ComplianceReview, &None);

    // Reason is retrievable while on hold
    assert_eq!(
        client.get_hold_reason(&sid),
        SettlementHoldReason::ComplianceReview
    );
}

#[test]
fn get_hold_reason_resets_to_none_after_release() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Hold with a reason
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::FraudCheck, &None);
    assert_eq!(
        client.get_hold_reason(&sid),
        SettlementHoldReason::FraudCheck
    );

    // Release the hold
    client.release_hold(&admin, &sid);

    // Reason is reset to None
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::None);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn get_hold_reason_panics_for_nonexistent_settlement() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    // Non-existent settlement ID
    client.get_hold_reason(&999);
}

#[test]
fn hold_settlement_preserves_non_negative_amount_invariant() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let amount = 10_000_000_i128;
    let sid = client.propose_settlement(&admin, &merchant, &amount, &0_u64);

    // Amount is preserved before the hold.
    assert_eq!(client.get_settlement(&sid).amount, amount);

    client.hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &None);

    // Holding must not mutate the settlement amount.
    let held = client.get_settlement(&sid);
    assert_eq!(held.amount, amount);
    assert!(held.amount >= 0, "amount invariant violated after hold");

    client.release_hold(&admin, &sid);

    // Releasing must not mutate the settlement amount either.
    let released = client.get_settlement(&sid);
    assert_eq!(released.amount, amount);
    assert!(
        released.amount >= 0,
        "amount invariant violated after release"
    );
}

#[test]
fn hold_settlement_rejects_invalid_status_transition() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    client.cancel_settlement(&admin, &sid);

    // A cancelled settlement cannot transition into the held state.
    assert_eq!(
        client.try_hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &None),
        Err(Ok(TreasuryError::AlreadyExecuted))
    );
}

#[test]
fn release_hold_rejects_invalid_status_transition() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Releasing a settlement that is not on hold is an invalid transition.
    assert_eq!(
        client.try_release_hold(&admin, &sid),
        Err(Ok(TreasuryError::NotOnHold))
    );
}

#[test]
fn hold_state_transitions_keep_settlement_status_consistent() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Pending -> Held
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::ComplianceReview, &None);
    assert_eq!(
        client.get_hold_reason(&sid),
        SettlementHoldReason::ComplianceReview
    );

    // Held -> Pending
    client.release_hold(&admin, &sid);
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::None);

    // The settlement remains a valid, non-negative amount throughout.
    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.amount, 10_000_000);
    assert!(settlement.amount >= 0);
}
