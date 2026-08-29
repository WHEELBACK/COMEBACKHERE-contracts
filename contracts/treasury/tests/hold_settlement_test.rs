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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    assert_eq!(
        client.try_hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_hold_settlement(&admin, &sid, &SettlementHoldReason::FraudCheck),
        Err(Ok(TreasuryError::AlreadyOnHold))
    );
}

#[test]
fn hold_settlement_still_returns_already_executed_for_other_non_pending_statuses() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.cancel_settlement(&admin, &sid);

    assert_eq!(
        client.try_hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold),
        Err(Ok(TreasuryError::AlreadyExecuted))
    );
}

#[test]
fn get_hold_reason_returns_reason_while_on_hold() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    // Initially hold_reason is None
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::None);

    // Hold with a reason
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::ComplianceReview);

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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    // Hold with a reason
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::FraudCheck);
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
