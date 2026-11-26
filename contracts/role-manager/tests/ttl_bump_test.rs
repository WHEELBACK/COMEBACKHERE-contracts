//! Storage lifecycle and ledger advancement tests for `role-manager` (#938).

use role_manager::{RoleManagerContract, RoleManagerContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

#[test]
fn role_grants_persist_across_ledger_advancement() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let id = env.register(RoleManagerContract, ());
    let client = RoleManagerContractClient::new(&env, &id);
    client.initialize(&admin);

    let role = symbol_short!("operator");
    client.grant_role(&admin, &role, &user);
    assert!(client.has_role(&role, &user));

    env.ledger().with_mut(|li| {
        li.sequence_number += 500;
        li.timestamp += 2_500;
    });

    assert!(client.has_role(&role, &user));
    assert_eq!(client.get_admin(), Some(admin.clone()));
    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));
    assert_eq!(client.try_verify_account_roles(&user), Ok(Ok(())));
}
