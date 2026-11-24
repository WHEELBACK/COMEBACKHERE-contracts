use role_manager::{RoleManagerContract, RoleManagerContractClient, RoleManagerError};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, Vec};

fn setup() -> (Env, Address, Address, RoleManagerContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let account = Address::generate(&env);
    let id = env.register_contract(None, RoleManagerContract);
    let client = RoleManagerContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, account, client)
}

#[test]
fn initialize_twice_returns_already_initialized() {
    let (_env, admin, _account, client) = setup();
    let err = client.try_initialize(&admin).unwrap_err().unwrap();
    assert_eq!(err, RoleManagerError::AlreadyInitialized);
}

#[test]
fn grant_has_and_revoke_roles() {
    let (env, admin, account, client) = setup();
    let role = Symbol::new(&env, "trading");
    assert!(!client.has_role(&role, &account));
    client.grant_role(&admin, &role, &account);
    assert!(client.has_role(&role, &account));
    assert!(!client.has_role(&Symbol::new(&env, "other"), &account));
    client.revoke_role(&admin, &role, &account);
    assert!(!client.has_role(&role, &account));
}

#[test]
fn grant_duplicate_role_fails_role_already_granted() {
    let (env, admin, account, client) = setup();
    let role = Symbol::new(&env, "trading");
    client.grant_role(&admin, &role, &account);
    let err = client
        .try_grant_role(&admin, &role, &account)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RoleManagerError::RoleAlreadyGranted);
}

#[test]
fn revoke_not_granted_role_fails_role_not_granted() {
    let (env, admin, account, client) = setup();
    let role = Symbol::new(&env, "trading");
    let err = client
        .try_revoke_role(&admin, &role, &account)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RoleManagerError::RoleNotGranted);
}

#[test]
fn require_role_fails_when_not_granted_and_succeeds_when_granted() {
    let (env, admin, account, client) = setup();
    let role = Symbol::new(&env, "trading");
    let err = client.try_require_role(&role, &account).unwrap_err().unwrap();
    assert_eq!(err, RoleManagerError::MissingRole);
    client.grant_role(&admin, &role, &account);
    assert!(client.try_require_role(&role, &account).is_ok());
}

#[test]
fn grant_and_revoke_invariants_hold_across_role_matrix() {
    // Invariant test across a matrix of roles: every role starts ungranted,
    // grant makes it held, revoke clears it.
    let (env, admin, account, client) = setup();
    let mut roles: Vec<Symbol> = Vec::new(&env);
    for name in ["alpha", "beta", "gamma", "delta", "epsilon"] {
        roles.push_back(Symbol::new(&env, name));
    }
    for role in roles.iter() {
        assert!(!client.has_role(&role, &account));
        client.grant_role(&admin, &role, &account);
        assert!(client.has_role(&role, &account));
        client.revoke_role(&admin, &role, &account);
        assert!(!client.has_role(&role, &account));
    }
}

#[test]
fn non_admin_grant_fails_unauthorized() {
    let (env, _admin, account, client) = setup();
    let rando = Address::generate(&env);
    let role = Symbol::new(&env, "trading");
    let err = client
        .try_grant_role(&rando, &role, &account)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RoleManagerError::Unauthorized);
}

#[test]
fn grant_and_revoke_blocked_while_paused() {
    let (env, admin, account, client) = setup();
    let role = Symbol::new(&env, "trading");
    client.set_paused(&admin, &true);
    assert!(client.is_paused());
    let err = client
        .try_grant_role(&admin, &role, &account)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RoleManagerError::ContractPaused);
    client.set_paused(&admin, &false);
    client.grant_role(&admin, &role, &account);
    assert!(client.has_role(&role, &account));
}

#[test]
fn get_admin_returns_stored_admin() {
    let (env, admin, _account, client) = setup();
    assert_eq!(client.get_admin(), Some(admin));
}
