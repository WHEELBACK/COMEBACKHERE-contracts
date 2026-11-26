//! Zero-state / empty-role boundary tests for `role-manager` (#939).

use role_manager::{RoleManagerContract, RoleManagerContractClient, RoleManagerError};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol};

#[test]
fn uninitialized_and_empty_role_states_do_not_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let unknown = Address::generate(&env);
    let id = env.register(RoleManagerContract, ());
    let client = RoleManagerContractClient::new(&env, &id);

    // Before initialization, read-only queries return safe defaults without panicking.
    assert_eq!(client.get_admin(), None);
    assert!(!client.is_paused());
    assert!(!client.has_role(&symbol_short!("operator"), &unknown));

    client.initialize(&admin);

    // Empty role symbol is rejected cleanly with typed error.
    let empty = Symbol::new(&env, "");
    assert_eq!(
        client.try_grant_role(&admin, &empty, &unknown),
        Err(Ok(RoleManagerError::EmptyRole))
    );
    assert_eq!(
        client.try_revoke_role(&admin, &empty, &unknown),
        Err(Ok(RoleManagerError::EmptyRole))
    );
    assert_eq!(
        client.try_require_role(&empty, &unknown),
        Err(Ok(RoleManagerError::EmptyRole))
    );

    // Revoking or requiring a role from an account with zero roles returns typed error.
    assert_eq!(
        client.try_revoke_role(&admin, &symbol_short!("operator"), &unknown),
        Err(Ok(RoleManagerError::RoleNotGranted))
    );
    assert_eq!(
        client.try_require_role(&symbol_short!("operator"), &unknown),
        Err(Ok(RoleManagerError::MissingRole))
    );
    assert_eq!(client.try_verify_account_roles(&unknown), Ok(Ok(())));
}
