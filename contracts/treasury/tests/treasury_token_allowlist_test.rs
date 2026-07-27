use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

#[contract]
struct FakeToken;
#[contractimpl]
impl FakeToken {
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
}

fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
fn execute_settlement_succeeds_with_allowed_token() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let token_id = env.register_contract(None, FakeToken);

    client.add_allowed_token(&admin, &token_id);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.execute_settlement(&admin, &sid, &token_id);
}

#[test]
fn execute_settlement_panics_when_token_not_in_allowlist() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let allowed_token = env.register_contract(None, FakeToken);
    // Allowlist check fires before any cross-contract call, so no real contract needed
    let other_token = Address::generate(&env);

    client.add_allowed_token(&admin, &allowed_token);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    assert!(client
        .try_execute_settlement(&admin, &sid, &other_token)
        .is_err());
}

#[test]
fn execute_settlement_with_empty_allowlist_succeeds() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let token_id = env.register_contract(None, FakeToken);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.execute_settlement(&admin, &sid, &token_id);
}

#[test]
fn remove_allowed_token_prevents_execution() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let token_a = env.register_contract(None, FakeToken);
    // token_b is an address-only entry; allowlist check fires before cross-contract transfer
    let token_b = Address::generate(&env);

    client.add_allowed_token(&admin, &token_a);
    client.add_allowed_token(&admin, &token_b);
    client.remove_allowed_token(&admin, &token_a);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    // token_a removed; allowlist still non-empty (token_b present) → rejected
    assert!(client
        .try_execute_settlement(&admin, &sid, &token_a)
        .is_err());
}
