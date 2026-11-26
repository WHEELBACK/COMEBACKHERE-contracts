//! Tests for arithmetic underflow protection in recovery-manager partial settlement computations.
//!
//! These tests verify that partial settlement computations never panic due to
//! arithmetic underflow and instead return typed errors when requested amounts
//! exceed available balances.

use recovery_manager::{RecoveryError, RecoveryManager, RecoveryManagerClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, RecoveryManagerClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(RecoveryManager, ());
    let client = RecoveryManagerClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);

    (env, client, admin, user)
}

#[test]
fn test_partial_settlement_equal_to_balance_succeeds() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &1_000);

    let result = client.try_partial_settle(&user, &1_000);
    assert_eq!(result, Ok(Ok(0)));
    assert_eq!(client.balance(&user), 0);
    assert_eq!(client.total_recovered(), 1_000);
}

#[test]
fn test_partial_settlement_below_balance_succeeds() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &1_000);

    let result = client.try_partial_settle(&user, &400);
    assert_eq!(result, Ok(Ok(600)));
    assert_eq!(client.balance(&user), 600);
    assert_eq!(client.total_recovered(), 400);
}

#[test]
fn test_partial_settlement_exceeding_balance_returns_typed_error() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &500);

    let result = client.try_partial_settle(&user, &1_000);
    assert_eq!(result, Err(Ok(RecoveryError::InsufficientBalance)));
    assert_eq!(client.balance(&user), 500);
}

#[test]
fn test_partial_settlement_one_above_balance_returns_typed_error() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &500);

    let result = client.try_partial_settle(&user, &501);
    assert_eq!(result, Err(Ok(RecoveryError::InsufficientBalance)));
}

#[test]
fn test_partial_settlement_zero_balance_returns_typed_error() {
    let (_env, client, _admin, user) = setup();

    let result = client.try_partial_settle(&user, &1);
    assert_eq!(result, Err(Ok(RecoveryError::InsufficientBalance)));
}

#[test]
fn test_partial_settlement_zero_and_negative_requests_return_invalid_amount() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &1_000);

    assert_eq!(
        client.try_partial_settle(&user, &0),
        Err(Ok(RecoveryError::InvalidAmount))
    );
    assert_eq!(
        client.try_partial_settle(&user, &-1),
        Err(Ok(RecoveryError::InvalidAmount))
    );
    assert_eq!(
        client.try_preview_partial_settle(&user, &0),
        Err(Ok(RecoveryError::InvalidAmount))
    );
    assert_eq!(
        client.try_preview_partial_settle(&user, &-1),
        Err(Ok(RecoveryError::InvalidAmount))
    );
}

#[test]
fn test_partial_settlement_max_values_succeeds() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &i128::MAX);

    let result = client.try_partial_settle(&user, &i128::MAX);
    assert_eq!(result, Ok(Ok(0)));
    assert_eq!(client.balance(&user), 0);
}

#[test]
fn test_partial_settlement_max_minus_one_returns_typed_error() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &(i128::MAX - 1));

    let result = client.try_partial_settle(&user, &i128::MAX);
    assert_eq!(result, Err(Ok(RecoveryError::InsufficientBalance)));
}

#[test]
fn test_partial_settlement_preview_fuzz_sweep_no_panic() {
    let (env, client, _admin, _user) = setup();

    let values: [i128; 8] = [1, 2, 10, 100, 1_000, 10_000, i128::MAX / 2, i128::MAX];

    for &available in values.iter() {
        let acct = Address::generate(&env);
        client.deposit(&acct, &available);
        for &requested in values.iter() {
            let result = client.try_preview_partial_settle(&acct, &requested);
            if requested > available {
                assert_eq!(result, Err(Ok(RecoveryError::InsufficientBalance)));
            } else {
                assert_eq!(result, Ok(Ok(available - requested)));
            }
        }
    }
}
