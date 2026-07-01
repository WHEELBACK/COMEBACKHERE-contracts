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
