//! Cross-contract invocation mock tests for the `access-control` contract.
//!
//! These tests exercise the `access-control` contract when it is invoked from
//! another contract (a mock callee) using the Soroban SDK test utilities. They
//! verify that role and authorization checks behave correctly across contract
//! boundaries, including failure paths for unauthorized callers.

#![cfg(test)]

use access_control::{AccessControl, AccessControlClient, Role};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, testutils::Address as _, Address, Env,
    IntoVal, Symbol,
};

/// A mock contract that invokes the `access-control` contract across a
/// contract boundary. It forwards calls to the access-control contract so that
/// the authorization context is the mock contract's address.
#[contract]
pub struct MockCallee;

#[contracttype]
#[derive(Clone)]
pub enum MockKey {
    AccessControl,
}

#[contractimpl]
impl MockCallee {
    /// Store the address of the access-control contract this mock will call.
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

    /// Grant `role` to `account` by invoking the access-control contract.
    /// The caller of the access-control contract is this mock contract.
    pub fn grant_role(env: Env, admin: Address, account: Address, role: Role) {
        let ac = Self::access_control(&env);
        let client = AccessControlClient::new(&env, &ac);
        client.grant_role(&admin, &account, &role);
    }

    /// Revoke `role` from `account` by invoking the access-control contract.
    pub fn revoke_role(env: Env, admin: Address, account: Address, role: Role) {
        let ac = Self::access_control(&env);
        let client = AccessControlClient::new(&env, &ac);
        client.revoke_role(&admin, &account, &role);
    }

    /// Check whether `account` has `role` by invoking the access-control
    /// contract.
    pub fn has_role(env: Env, account: Address, role: Role) -> bool {
        let ac = Self::access_control(&env);
        let client = AccessControlClient::new(&env, &ac);
        client.has_role(&account, &role)
    }

    /// Require that `account` has `role`, panicking otherwise. This mirrors a
    /// guarded entry point that relies on the access-control contract.
    pub fn require_role(env: Env, account: Address, role: Role) {
        let ac = Self::access_control(&env);
        let client = AccessControlClient::new(&env, &ac);
        client.require_role(&account, &role);
    }
}

/// Deploy the access-control contract and the mock callee, wiring the mock to
/// the access-control contract. Returns the access-control client, the mock
/// client, and the admin address.
fn setup(env: &Env) -> (AccessControlClient<'_>, MockCalleeClient<'_>, Address) {
    let admin = Address::generate(env);

    let ac_id = env.register_contract(None, AccessControl);
    let ac_client = AccessControlClient::new(env, &ac_id);

    // Initialize the access-control contract with the admin.
    ac_client.initialize(&admin);

    let mock_id = env.register_contract(None, MockCallee);
    let mock_client = MockCalleeClient::new(env, &mock_id);
    mock_client.init(&ac_id);

    (ac_client, mock_client, admin)
}

#[test]
fn cross_contract_grant_and_has_role() {
    let env = Env::default();
    env.mock_all_auths();

    let (ac_client, mock_client, admin) = setup(&env);
    let account = Address::generate(&env);
    let role = Role::Admin;

    // Initially the account has no role.
    assert!(!mock_client.has_role(&account, &role));

    // Grant the role via the mock contract (cross-contract invocation).
    mock_client.grant_role(&admin, &account, &role);

    // The role should now be visible both directly and via the mock.
    assert!(ac_client.has_role(&account, &role));
    assert!(mock_client.has_role(&account, &role));
}

#[test]
fn cross_contract_revoke_role() {
    let env = Env::default();
    env.mock_all_auths();

    let (ac_client, mock_client, admin) = setup(&env);
    let account = Address::generate(&env);
    let role = Role::Admin;

    mock_client.grant_role(&admin, &account, &role);
    assert!(ac_client.has_role(&account, &role));

    mock_client.revoke_role(&admin, &account, &role);
    assert!(!ac_client.has_role(&account, &role));
    assert!(!mock_client.has_role(&account, &role));
}

#[test]
fn cross_contract_require_role_succeeds_when_authorized() {
    let env = Env::default();
    env.mock_all_auths();

    let (_ac_client, mock_client, admin) = setup(&env);
    let account = Address::generate(&env);
    let role = Role::Admin;

    mock_client.grant_role(&admin, &account, &role);

    // Should not panic since the account has the role.
    mock_client.require_role(&account, &role);
}

#[test]
#[should_panic]
fn cross_contract_require_role_panics_when_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();

    let (_ac_client, mock_client, _admin) = setup(&env);
    let account = Address::generate(&env);
    let role = Role::Admin;

    // The account was never granted the role, so this must panic.
    mock_client.require_role(&account, &role);
}

#[test]
#[should_panic]
fn cross_contract_grant_role_unauthorized_admin_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (_ac_client, mock_client, _admin) = setup(&env);
    let attacker = Address::generate(&env);
    let account = Address::generate(&env);
    let role = Role::Admin;

    // `attacker` is not the admin, so granting a role must fail even though
    // the call is routed through the mock contract.
    mock_client.grant_role(&attacker, &account, &role);
}

#[test]
#[should_panic]
fn cross_contract_revoke_role_unauthorized_admin_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (_ac_client, mock_client, admin) = setup(&env);
    let attacker = Address::generate(&env);
    let account = Address::generate(&env);
    let role = Role::Admin;

    // First grant legitimately.
    mock_client.grant_role(&admin, &account, &role);

    // An unauthorized caller must not be able to revoke the role.
    mock_client.revoke_role(&attacker, &account, &role);
}

#[test]
fn cross_contract_has_role_is_read_only() {
    let env = Env::default();
    env.mock_all_auths();

    let (ac_client, mock_client, admin) = setup(&env);
    let account = Address::generate(&env);
    let role = Role::Admin;

    // Reading via the mock must not mutate state.
    assert!(!mock_client.has_role(&account, &role));
    assert!(!ac_client.has_role(&account, &role));

    mock_client.grant_role(&admin, &account, &role);

    // Repeated reads remain consistent.
    assert!(mock_client.has_role(&account, &role));
    assert!(mock_client.has_role(&account, &role));
    assert!(ac_client.has_role(&account, &role));
}

#[test]
fn cross_contract_multiple_roles_isolated() {
    let env = Env::default();
    env.mock_all_auths();

    let (ac_client, mock_client, admin) = setup(&env);
    let account = Address::generate(&env);
    let role_a = Role::Admin;
    let role_b = Role::Operator;

    mock_client.grant_role(&admin, &account, &role_a);

    assert!(ac_client.has_role(&account, &role_a));
    assert!(!ac_client.has_role(&account, &role_b));

    mock_client.grant_role(&admin, &account, &role_b);

    assert!(ac_client.has_role(&account, &role_a));
    assert!(ac_client.has_role(&account, &role_b));

    mock_client.revoke_role(&admin, &account, &role_a);

    assert!(!ac_client.has_role(&account, &role_a));
    assert!(ac_client.has_role(&account, &role_b));
}

#[test]
fn cross_contract_events_emitted_for_grant_and_revoke() {
    let env = Env::default();
    env.mock_all_auths();

    let (_ac_client, mock_client, admin) = setup(&env);
    let account = Address::generate(&env);
    let role = Role::Admin;

    mock_client.grant_role(&admin, &account, &role);
    mock_client.revoke_role(&admin, &account, &role);

    // The access-control contract should have emitted events for both the
    // grant and the revoke, even though the calls were routed through the
    // mock contract.
    let events = env.events().all();
    assert!(!events.is_empty(), "expected access-control events");
}

#[test]
fn cross_contract_symbol_roundtrip() {
    let env = Env::default();
    env.mock_all_auths();

    // Sanity check that symbols used by the contract round-trip through the
    // test environment without host metering issues.
    let sym: Symbol = symbol_short!("grant");
    let roundtrip: Symbol = sym.into_val(&env);
    assert_eq!(sym, roundtrip);
}