//! Granular role-based authorization checks for `role-manager` (#933).

use role_manager::{RoleManagerContract, RoleManagerContractClient, RoleManagerError};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn setup() -> (Env, RoleManagerContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(RoleManagerContract, ());
    let client = RoleManagerContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn only_admin_can_grant_and_revoke_roles() {
    let (env, client, admin) = setup();
    let outsider = Address::generate(&env);
    let target = Address::generate(&env);
    let role = symbol_short!("treasurer");

    assert_eq!(
        client.try_grant_role(&outsider, &role, &target),
        Err(Ok(RoleManagerError::Unauthorized))
    );
    assert!(!client.has_role(&role, &target));

    client.grant_role(&admin, &role, &target);
    assert!(client.has_role(&role, &target));

    assert_eq!(
        client.try_revoke_role(&outsider, &role, &target),
        Err(Ok(RoleManagerError::Unauthorized))
    );
    assert!(client.has_role(&role, &target));

    client.revoke_role(&admin, &role, &target);
    assert!(!client.has_role(&role, &target));
}

#[test]
fn roles_are_isolated_per_symbol_and_per_account() {
    let (env, client, admin) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let pauser = symbol_short!("pauser");
    let minter = symbol_short!("minter");

    client.grant_role(&admin, &pauser, &alice);

    assert!(client.has_role(&pauser, &alice));
    assert!(!client.has_role(&minter, &alice));
    assert!(!client.has_role(&pauser, &bob));

    assert_eq!(client.try_require_role(&pauser, &alice), Ok(Ok(())));
    assert_eq!(
        client.try_require_role(&minter, &alice),
        Err(Ok(RoleManagerError::MissingRole))
    );
    assert_eq!(
        client.try_require_role(&pauser, &bob),
        Err(Ok(RoleManagerError::MissingRole))
    );
}

#[test]
fn pause_blocks_role_mutations_until_unpaused() {
    let (env, client, admin) = setup();
    let outsider = Address::generate(&env);
    let user = Address::generate(&env);
    let role = symbol_short!("auditor");

    assert_eq!(
        client.try_set_paused(&outsider, &true),
        Err(Ok(RoleManagerError::Unauthorized))
    );

    client.set_paused(&admin, &true);
    assert!(client.is_paused());

    assert_eq!(
        client.try_grant_role(&admin, &role, &user),
        Err(Ok(RoleManagerError::ContractPaused))
    );

    client.set_paused(&admin, &false);
    assert!(!client.is_paused());

    client.grant_role(&admin, &role, &user);
    assert!(client.has_role(&role, &user));
}
