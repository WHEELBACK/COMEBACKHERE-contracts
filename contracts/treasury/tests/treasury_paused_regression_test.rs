use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

fn paused_setup(env: &Env) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    client.pause(&admin);
    (client, admin)
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn paused_rejects_propose_settlement() {
    let env = Env::default();
    let (client, admin) = paused_setup(&env);
    let merchant = Address::generate(&env);
    client.propose_settlement(&admin, &merchant, &1_000);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn paused_rejects_approve_settlement() {
    let env = Env::default();
    let (client, admin) = paused_setup(&env);
    client.approve_settlement(&admin, &1);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn paused_rejects_execute_settlement() {
    let env = Env::default();
    let (client, admin) = paused_setup(&env);
    let token = Address::generate(&env);
    client.execute_settlement(&admin, &1, &token);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn paused_rejects_deposit() {
    let env = Env::default();
    let (client, admin) = paused_setup(&env);
    let token = Address::generate(&env);
    client.deposit(&admin, &token, &1_000);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn paused_rejects_withdraw() {
    let env = Env::default();
    let (client, admin) = paused_setup(&env);
    let token = Address::generate(&env);
    client.withdraw(&admin, &token, &1_000);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn paused_rejects_raise_dispute() {
    let env = Env::default();
    let (client, _admin) = paused_setup(&env);
    let claimant = Address::generate(&env);
    let counterparty = Address::generate(&env);
    client.raise_dispute(&claimant, &1, &counterparty, &1_000);
}

#[test]
#[should_panic(expected = "ContractPaused")]
fn paused_rejects_resolve_dispute() {
    let env = Env::default();
    let (client, admin) = paused_setup(&env);
    client.resolve_dispute(&admin, &1, &true);
}
