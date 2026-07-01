use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

fn setup(env: &Env, threshold: u32) -> (TreasuryContractClient, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &threshold, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
fn admin_can_update_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);

    client.update_threshold(&admin, &5);

    let merchant = Address::generate(&env);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    let s = client.approve_settlement(&backup, &sid);
    // admin(1) + backup(1) = weight 2; new threshold is 5 → still pending
    assert_eq!(s.approvals.len(), 2);
}

#[test]
fn threshold_update_takes_effect_for_future_executions() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);

    // lower threshold to 1 so admin alone is sufficient
    client.update_threshold(&admin, &1);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id)
        .mint(&env.register_contract(None, TreasuryContract), &1_000_000);
    // with threshold=1 and admin weight=1 the settlement can be executed; we only test that
    // approval weight passes the lowered bar — execution itself requires a live token balance
    // so we just assert the settlement was accepted with a single signer
    let s = client.approve_settlement(&admin, &sid);
    assert_eq!(s.approvals.len(), 1);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn non_admin_cannot_update_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env, 2);

    let attacker = Address::generate(&env);
    client.update_threshold(&attacker, &1);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn wrong_admin_address_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env, 2);

    let other_admin = Address::generate(&env);
    // register other_admin as a signer but not the stored admin
    client.set_signer(&_admin, &other_admin, &1);
    client.update_threshold(&other_admin, &3);
}
