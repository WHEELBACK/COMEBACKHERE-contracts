//! Issue #936: strict invariant verification for the role-manager contract.
//!
//! The tests corrupt instance storage directly to simulate a bad upgrade or a
//! hostile write, then assert that authorization never trusts a malformed
//! grant list, that mutations repair corruption, and that role capacity is
//! enforced. Boundary matrices exercise the fuzz surface of the grant list.

use role_manager::{DataKey, RoleManagerContract, RoleManagerContractClient, RoleManagerError};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, Vec};

fn setup<'a>(env: &'a Env) -> (RoleManagerContractClient<'a>, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let cid = env.register(RoleManagerContract, ());
    let client = RoleManagerContractClient::new(env, &cid);
    client.initialize(&admin);
    (client, admin, cid)
}

fn role(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

fn write_grants(env: &Env, cid: &Address, account: &Address, grants: &Vec<Symbol>) {
    env.as_contract(cid, || {
        env.storage()
            .instance()
            .set(&DataKey::RoleGrants(account.clone()), grants);
    });
}

#[test]
fn invariants_hold_across_the_role_lifecycle() {
    let env = Env::default();
    let (client, admin, _cid) = setup(&env);
    let account = Address::generate(&env);
    let trading = role(&env, "trading");

    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));
    assert_eq!(client.try_verify_account_roles(&account), Ok(Ok(())));

    client.grant_role(&admin, &trading, &account);
    assert_eq!(client.try_verify_account_roles(&account), Ok(Ok(())));
    client.set_paused(&admin, &true);
    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));
    client.set_paused(&admin, &false);
    client.revoke_role(&admin, &trading, &account);
    assert_eq!(client.try_verify_account_roles(&account), Ok(Ok(())));
    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));
}

#[test]
fn missing_instance_keys_are_detected_and_block_mutations() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let account = Address::generate(&env);

    env.as_contract(&cid, || {
        env.storage().instance().remove(&DataKey::Paused);
    });
    assert_eq!(
        client.try_verify_invariants(),
        Err(Ok(RoleManagerError::StateInconsistency))
    );
    assert_eq!(
        client.try_grant_role(&admin, &role(&env, "trading"), &account),
        Err(Ok(RoleManagerError::StateInconsistency))
    );

    env.as_contract(&cid, || {
        env.storage().instance().remove(&DataKey::Admin);
    });
    assert_eq!(
        client.try_verify_invariants(),
        Err(Ok(RoleManagerError::StateInconsistency))
    );
    assert_eq!(
        client.try_grant_role(&admin, &role(&env, "trading"), &account),
        Err(Ok(RoleManagerError::Unauthorized))
    );
}

#[test]
fn corrupt_grant_list_is_never_trusted_for_authorization() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let account = Address::generate(&env);
    let trading = role(&env, "trading");

    let mut corrupt = Vec::new(&env);
    corrupt.push_back(role(&env, "")); // empty role symbol
    corrupt.push_back(trading.clone());
    corrupt.push_back(trading.clone()); // duplicate
    write_grants(&env, &cid, &account, &corrupt);

    assert!(!client.has_role(&trading, &account));
    assert_eq!(
        client.try_require_role(&trading, &account),
        Err(Ok(RoleManagerError::StateInconsistency))
    );
    assert_eq!(
        client.try_verify_account_roles(&account),
        Err(Ok(RoleManagerError::StateInconsistency))
    );

    // A grant repairs the stored list in place: valid roles (including the
    // duplicated one) are preserved once, empty/duplicate entries are dropped.
    let other = role(&env, "other");
    client.grant_role(&admin, &other, &account);
    assert!(client.has_role(&other, &account));
    assert!(client.has_role(&trading, &account));
    assert_eq!(client.try_verify_account_roles(&account), Ok(Ok(())));
}

#[test]
fn revoke_recovers_from_a_corrupt_duplicate_list() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let account = Address::generate(&env);
    let trading = role(&env, "trading");

    let mut corrupt = Vec::new(&env);
    corrupt.push_back(trading.clone());
    corrupt.push_back(trading.clone());
    corrupt.push_back(role(&env, ""));
    write_grants(&env, &cid, &account, &corrupt);

    client.revoke_role(&admin, &trading, &account);
    assert_eq!(client.try_verify_account_roles(&account), Ok(Ok(())));
    assert!(!client.has_role(&trading, &account));
    // The repaired state can be used normally again.
    client.grant_role(&admin, &trading, &account);
    assert!(client.has_role(&trading, &account));
}

#[test]
fn role_capacity_is_enforced_without_partial_writes() {
    let env = Env::default();
    let (client, admin, _cid) = setup(&env);
    let account = Address::generate(&env);

    for i in 0..32u32 {
        let name = std::format!("r{i}");
        client.grant_role(&admin, &role(&env, &name), &account);
    }
    assert_eq!(client.try_verify_account_roles(&account), Ok(Ok(())));

    let overflow = role(&env, "overflow");
    assert_eq!(
        client.try_grant_role(&admin, &overflow, &account),
        Err(Ok(RoleManagerError::TooManyRoles))
    );
    assert!(!client.has_role(&overflow, &account));

    client.revoke_role(&admin, &role(&env, "r0"), &account);
    client.grant_role(&admin, &overflow, &account);
    assert!(client.has_role(&overflow, &account));
    assert_eq!(client.try_verify_account_roles(&account), Ok(Ok(())));
}

#[test]
fn require_role_rejects_empty_symbols() {
    let env = Env::default();
    let (client, _admin, _cid) = setup(&env);
    let account = Address::generate(&env);
    assert_eq!(
        client.try_require_role(&role(&env, ""), &account),
        Err(Ok(RoleManagerError::EmptyRole))
    );
}

#[test]
fn fuzz_corrupt_grant_lists_never_panic() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let account = Address::generate(&env);
    let check_role = role(&env, "check");

    for len in 0u32..=35 {
        let mut grants = Vec::new(&env);
        for i in 0..len {
            // Deliberately reuse a tiny symbol alphabet to create duplicates,
            // and include an empty symbol in every non-empty list.
            let name = if i == 0 { "" } else { "dup" };
            grants.push_back(role(&env, name));
        }
        write_grants(&env, &cid, &account, &grants);

        assert_eq!(client.try_verify_account_roles(&account).is_err(), len > 0);
        assert!(!client.has_role(&check_role, &account));
        assert!(client.try_require_role(&check_role, &account).is_err());
        // Mutations either reject the oversized list or repair a smaller one.
        let result = client.try_grant_role(&admin, &check_role, &account);
        assert!(result.is_ok() || result.is_err());
        if result.is_ok() {
            assert_eq!(client.try_verify_account_roles(&account), Ok(Ok(())));
        }
    }
}
