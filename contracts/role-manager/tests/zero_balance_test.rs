//! #939: zero-balance states must not panic contract execution.
//!
//! A budget can legitimately be absent (never credited), exactly zero (fully
//! spent), or positive. Every mutating path is exercised at those boundaries:
//! reads use `unwrap_or(0)`, negative inputs and arithmetic failures are typed
//! errors, a zero-amount operation is a storage-free no-op, and exhausting a
//! budget removes its key so no zero-value entry costs rent.
//!
//! The proptest cases mirror those checks over a wide numeric range so the
//! handling is not just correct for a few hand-picked constants.

use proptest::prelude::*;
use role_manager::{
    DataKey, Role, RoleManagerContract, RoleManagerContractClient, RoleManagerError,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let role_manager = env.register(RoleManagerContract, ());
    let client = RoleManagerContractClient::new(&env, &role_manager);
    client.initialize(&admin);
    client.grant_role(&admin, &Role::Operator, &operator);

    (env, admin, operator, role_manager)
}

#[test]
fn uncredited_budget_reads_as_zero() {
    let (env, _admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    assert_eq!(client.get_budget(&Role::Operator, &operator), 0);
    assert!(!env.as_contract(&role_manager, || {
        env.storage()
            .persistent()
            .has(&DataKey::Budget(Role::Operator, operator.clone()))
    }));
}

#[test]
fn debit_from_uncredited_budget_is_a_typed_error_not_a_panic() {
    let (env, _admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    assert_eq!(
        client.try_debit_budget(&operator, &Role::Operator, &1),
        Err(Ok(RoleManagerError::InsufficientBudget))
    );
    assert_eq!(client.get_budget(&Role::Operator, &operator), 0);
}

#[test]
fn zero_amount_operations_do_not_create_storage_entries() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    assert_eq!(
        client.credit_budget(&admin, &Role::Operator, &operator, &0),
        0
    );
    assert_eq!(client.debit_budget(&operator, &Role::Operator, &0), 0);
    assert_eq!(client.get_budget(&Role::Operator, &operator), 0);
    assert!(!env.as_contract(&role_manager, || {
        env.storage()
            .persistent()
            .has(&DataKey::Budget(Role::Operator, operator.clone()))
    }));
}

#[test]
fn partial_debit_leaves_the_remaining_balance() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    assert_eq!(
        client.credit_budget(&admin, &Role::Operator, &operator, &10),
        10
    );
    assert_eq!(client.debit_budget(&operator, &Role::Operator, &4), 6);
    assert_eq!(client.get_budget(&Role::Operator, &operator), 6);
}

#[test]
fn exact_debit_removes_the_zero_balance_key() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    client.credit_budget(&admin, &Role::Operator, &operator, &5);
    assert_eq!(client.debit_budget(&operator, &Role::Operator, &5), 0);
    assert_eq!(client.get_budget(&Role::Operator, &operator), 0);
    assert!(!env.as_contract(&role_manager, || {
        env.storage()
            .persistent()
            .has(&DataKey::Budget(Role::Operator, operator.clone()))
    }));
}

#[test]
fn overdebit_preserves_the_existing_balance() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    client.credit_budget(&admin, &Role::Operator, &operator, &3);
    assert_eq!(
        client.try_debit_budget(&operator, &Role::Operator, &4),
        Err(Ok(RoleManagerError::InsufficientBudget))
    );
    assert_eq!(client.get_budget(&Role::Operator, &operator), 3);
}

#[test]
fn negative_amounts_are_rejected_without_mutating_state() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    client.credit_budget(&admin, &Role::Operator, &operator, &3);
    assert_eq!(
        client.try_credit_budget(&admin, &Role::Operator, &operator, &-1),
        Err(Ok(RoleManagerError::InvalidAmount))
    );
    assert_eq!(
        client.try_debit_budget(&operator, &Role::Operator, &-1),
        Err(Ok(RoleManagerError::InvalidAmount))
    );
    assert_eq!(client.get_budget(&Role::Operator, &operator), 3);
}

#[test]
fn overflowing_credit_is_a_typed_error() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    client.credit_budget(&admin, &Role::Operator, &operator, &i128::MAX);
    assert_eq!(
        client.try_credit_budget(&admin, &Role::Operator, &operator, &1),
        Err(Ok(RoleManagerError::BudgetOverflow))
    );
    assert_eq!(client.get_budget(&Role::Operator, &operator), i128::MAX);
}

#[test]
fn address_without_the_role_cannot_debit_a_budget() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);
    let stranger = Address::generate(&env);

    client.credit_budget(&admin, &Role::Operator, &operator, &9);
    assert_eq!(
        client.try_debit_budget(&stranger, &Role::Operator, &1),
        Err(Ok(RoleManagerError::RoleNotAssigned))
    );
    assert_eq!(client.get_budget(&Role::Operator, &operator), 9);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// For any credited balance and requested debit, the contract either
    /// succeeds with exactly `initial - amount` left or fails with the typed
    /// `InsufficientBudget` error and leaves the balance untouched. It must
    /// never trap or panic.
    #[test]
    fn prop_budget_debit_matches_model(
        initial in 0i128..1_000_000_000,
        amount in 0i128..2_000_000_000,
    ) {
        let (env, admin, operator, role_manager) = setup();
        let client = RoleManagerContractClient::new(&env, &role_manager);
        client.credit_budget(&admin, &Role::Operator, &operator, &initial);

        let result = client.try_debit_budget(&operator, &Role::Operator, &amount);
        let balance = client.get_budget(&Role::Operator, &operator);
        if amount <= initial {
            prop_assert!(result.is_ok());
            prop_assert_eq!(balance, initial - amount);
        } else {
            prop_assert!(matches!(
                result,
                Err(Ok(RoleManagerError::InsufficientBudget))
            ));
            prop_assert_eq!(balance, initial);
        }
    }

    /// Any negative amount is refused and cannot move a balance, regardless of
    /// how large the magnitude is.
    #[test]
    fn prop_negative_amounts_never_mutate(negative in -1_000_000_000i128..0) {
        let (env, admin, operator, role_manager) = setup();
        let client = RoleManagerContractClient::new(&env, &role_manager);
        client.credit_budget(&admin, &Role::Operator, &operator, &42);

        prop_assert_eq!(
            client.try_credit_budget(&admin, &Role::Operator, &operator, &negative),
            Err(Ok(RoleManagerError::InvalidAmount))
        );
        prop_assert_eq!(
            client.try_debit_budget(&operator, &Role::Operator, &negative),
            Err(Ok(RoleManagerError::InvalidAmount))
        );
        prop_assert_eq!(client.get_budget(&Role::Operator, &operator), 42);
    }
}
