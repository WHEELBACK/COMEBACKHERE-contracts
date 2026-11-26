//! Cross-contract invocation mock tests for the `access-control` contract.
//!
//! These tests exercise `AccessControl` when invoked across a contract boundary
//! from a mock callee contract using Soroban SDK test utilities.

#![cfg(test)]

use access_control::{AccessControl, AccessControlClient};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, testutils::Address as _, Address, Env,
    Symbol,
};

#[contract]
pub struct MockCallee;

#[contracttype]
#[derive(Clone)]
pub enum MockKey {
    AccessControl,
}

#[contractimpl]
impl MockCallee {
    pub fn init(env: Env, access_control: Address) {
        env.storage()
            .instance()
            .set(&MockKey::AccessControl, &access_control);
    }

    fn access_control(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&MockKey::AccessControl)
            .expect("access-control not initialized")
    }

    pub fn grant_role(env: Env, role: Symbol, account: Address) {
        let ac = Self::access_control(&env);
        let client = AccessControlClient::new(&env, &ac);
        client.grant_role(&role, &account);
    }

    pub fn revoke_role(env: Env, role: Symbol, account: Address) {
        let ac = Self::access_control(&env);
        let client = AccessControlClient::new(&env, &ac);
        client.revoke_role(&role, &account);
    }

    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        let ac = Self::access_control(&env);
        let client = AccessControlClient::new(&env, &ac);
        client.has_role(&role, &account)
    }

    pub fn require_role(env: Env, role: Symbol, account: Address) {
        let ac = Self::access_control(&env);
        let client = AccessControlClient::new(&env, &ac);
        client.require_role(&role, &account);
    }
}

fn setup(env: &Env) -> (AccessControlClient<'_>, MockCalleeClient<'_>, Address) {
    let admin = Address::generate(env);

    let ac_id = env.register(AccessControl, ());
    let ac_client = AccessControlClient::new(env, &ac_id);
    ac_client.initialize(&admin);

    let mock_id = env.register(MockCallee, ());
    let mock_client = MockCalleeClient::new(env, &mock_id);
    mock_client.init(&ac_id);

    (ac_client, mock_client, admin)
}

#[test]
fn cross_contract_grant_and_has_role() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (ac_client, mock_client, _admin) = setup(&env);
    let account = Address::generate(&env);
    let role = symbol_short!("admin");

    assert!(!mock_client.has_role(&role, &account));

    mock_client.grant_role(&role, &account);

    assert!(ac_client.has_role(&role, &account));
    assert!(mock_client.has_role(&role, &account));
}

#[test]
fn cross_contract_revoke_role() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (ac_client, mock_client, _admin) = setup(&env);
    let account = Address::generate(&env);
    let role = symbol_short!("admin");

    mock_client.grant_role(&role, &account);
    assert!(ac_client.has_role(&role, &account));

    mock_client.revoke_role(&role, &account);
    assert!(!ac_client.has_role(&role, &account));
    assert!(!mock_client.has_role(&role, &account));
}

#[test]
fn cross_contract_require_role_succeeds_when_authorized() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (_ac_client, mock_client, _admin) = setup(&env);
    let account = Address::generate(&env);
    let role = symbol_short!("admin");

    mock_client.grant_role(&role, &account);
    mock_client.require_role(&role, &account);
}

#[test]
#[should_panic]
fn cross_contract_require_role_panics_when_unauthorized() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (_ac_client, mock_client, _admin) = setup(&env);
    let account = Address::generate(&env);
    let role = symbol_short!("admin");

    mock_client.require_role(&role, &account);
}

#[test]
#[should_panic]
fn cross_contract_grant_unconfigured_role_panics() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (_ac_client, mock_client, _admin) = setup(&env);
    let account = Address::generate(&env);
    let unconfigured = symbol_short!("unknown");

    mock_client.grant_role(&unconfigured, &account);
}

#[test]
fn cross_contract_has_role_is_read_only() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (ac_client, mock_client, _admin) = setup(&env);
    let account = Address::generate(&env);
    let role = symbol_short!("admin");

    assert!(!mock_client.has_role(&role, &account));
    assert!(!ac_client.has_role(&role, &account));

    mock_client.grant_role(&role, &account);

    assert!(mock_client.has_role(&role, &account));
    assert!(mock_client.has_role(&role, &account));
    assert!(ac_client.has_role(&role, &account));
}

#[test]
fn cross_contract_multiple_roles_isolated() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (ac_client, mock_client, _admin) = setup(&env);
    let account = Address::generate(&env);
    let role_a = symbol_short!("admin");
    let role_b = symbol_short!("operator");

    mock_client.grant_role(&role_a, &account);
    assert!(ac_client.has_role(&role_a, &account));
    assert!(!ac_client.has_role(&role_b, &account));

    mock_client.revoke_role(&role_a, &account);
    assert!(!ac_client.has_role(&role_a, &account));
    assert!(!ac_client.has_role(&role_b, &account));
}

#[test]
fn cross_contract_symbol_equality() {
    let env = Env::default();
    let sym = Symbol::new(&env, "admin");
    assert_eq!(sym, symbol_short!("admin"));
}
