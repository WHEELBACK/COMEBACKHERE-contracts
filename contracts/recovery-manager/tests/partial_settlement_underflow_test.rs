//! Tests for arithmetic underflow protection in recovery-manager partial settlement computations.
//!
//! These tests verify that partial settlement computations never panic due to
//! arithmetic underflow and instead return typed errors when computed amounts
//! exceed available balances.

use recovery_manager::{RecoveryManager, RecoveryManagerClient, Error};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, RecoveryManagerClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RecoveryManager);
    let client = RecoveryManagerClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    (env, client, admin, user)
}

/// Computed settlement amount equal to available balance must succeed.
#[test]
fn test_partial_settlement_equal_to_balance_succeeds() {
    let (env, client, _admin, user) = setup();

    let available: i128 = 1_000;
    let requested: i128 = 1_000;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_ok(), "equal settlement should not error");

    let settled = result.unwrap().unwrap();
    assert_eq!(settled, 1_000);
    assert_eq!(available - settled, 0);
    let _ = env;
}

/// Computed settlement amount below available balance must succeed and leave remainder.
#[test]
fn test_partial_settlement_below_balance_succeeds() {
    let (env, client, _admin, user) = setup();

    let available: i128 = 1_000;
    let requested: i128 = 400;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_ok(), "below-balance settlement should not error");

    let settled = result.unwrap().unwrap();
    assert_eq!(settled, 400);
    // Remainder computation must not underflow.
    let remainder = available.checked_sub(settled).expect("remainder underflow");
    assert_eq!(remainder, 600);
    let _ = env;
}

/// Computed settlement amount exceeding available balance must return a typed
/// error rather than panicking with an arithmetic underflow.
#[test]
fn test_partial_settlement_exceeding_balance_returns_typed_error() {
    let (env, client, _admin, user) = setup();

    let available: i128 = 500;
    let requested: i128 = 1_000;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_err(), "exceeding settlement must return an error");

    let err = result.unwrap_err().unwrap();
    assert_eq!(err, Error::InsufficientBalance);
    let _ = env;
}

/// Requested amount exactly one unit above available balance must return the
/// typed error (off-by-one boundary).
#[test]
fn test_partial_settlement_one_above_balance_returns_typed_error() {
    let (env, client, _admin, user) = setup();

    let available: i128 = 500;
    let requested: i128 = 501;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_err(), "one-above settlement must return an error");

    let err = result.unwrap_err().unwrap();
    assert_eq!(err, Error::InsufficientBalance);
    let _ = env;
}

/// Zero available balance with a positive request must return the typed error.
#[test]
fn test_partial_settlement_zero_balance_returns_typed_error() {
    let (env, client, _admin, user) = setup();

    let available: i128 = 0;
    let requested: i128 = 1;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_err(), "zero-balance settlement must return an error");

    let err = result.unwrap_err().unwrap();
    assert_eq!(err, Error::InsufficientBalance);
    let _ = env;
}

/// Zero requested amount against a positive balance must succeed with zero settled.
#[test]
fn test_partial_settlement_zero_request_succeeds() {
    let (env, client, _admin, user) = setup();

    let available: i128 = 1_000;
    let requested: i128 = 0;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_ok(), "zero-request settlement should not error");

    let settled = result.unwrap().unwrap();
    assert_eq!(settled, 0);
    let _ = env;
}

/// Negative available balance must be rejected with a typed error, not panic.
#[test]
fn test_partial_settlement_negative_balance_returns_typed_error() {
    let (env, client, _admin, user) = setup();

    let available: i128 = -1;
    let requested: i128 = 1;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_err(), "negative balance must return an error");

    let err = result.unwrap_err().unwrap();
    assert_eq!(err, Error::InvalidAmount);
    let _ = env;
}

/// Negative requested amount must be rejected with a typed error, not panic.
#[test]
fn test_partial_settlement_negative_request_returns_typed_error() {
    let (env, client, _admin, user) = setup();

    let available: i128 = 1_000;
    let requested: i128 = -1;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_err(), "negative request must return an error");

    let err = result.unwrap_err().unwrap();
    assert_eq!(err, Error::InvalidAmount);
    let _ = env;
}

/// Extreme boundary: i128::MAX available with i128::MAX requested must succeed
/// without overflow in remainder computation.
#[test]
fn test_partial_settlement_max_values_succeeds() {
    let (env, client, _admin, user) = setup();

    let available: i128 = i128::MAX;
    let requested: i128 = i128::MAX;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_ok(), "max-value settlement should not error");

    let settled = result.unwrap().unwrap();
    assert_eq!(settled, i128::MAX);
    let remainder = available.checked_sub(settled).expect("remainder underflow");
    assert_eq!(remainder, 0);
    let _ = env;
}

/// Extreme boundary: i128::MAX available with a request exceeding it is
/// impossible, but i128::MAX requested against i128::MAX - 1 must error.
#[test]
fn test_partial_settlement_max_minus_one_returns_typed_error() {
    let (env, client, _admin, user) = setup();

    let available: i128 = i128::MAX - 1;
    let requested: i128 = i128::MAX;

    let result = client.try_compute_partial_settlement(&user, &available, &requested);
    assert!(result.is_err(), "max-minus-one settlement must return an error");

    let err = result.unwrap_err().unwrap();
    assert_eq!(err, Error::InsufficientBalance);
    let _ = env;
}

/// Fuzz-style sweep across a range of available/requested pairs asserting that
/// no combination panics and that the typed error is returned whenever the
/// request exceeds the available balance.
#[test]
fn test_partial_settlement_fuzz_sweep_no_panic() {
    let (env, client, _admin, user) = setup();

    let values: [i128; 9] = [0, 1, 2, 10, 100, 1_000, 10_000, i128::MAX / 2, i128::MAX];

    for &available in values.iter() {
        for &requested in values.iter() {
            let result = client.try_compute_partial_settlement(&user, &available, &requested);

            if requested > available {
                assert!(
                    result.is_err(),
                    "request {requested} > available {available} must error"
                );
                let err = result.unwrap_err().unwrap();
                assert_eq!(err, Error::InsufficientBalance);
            } else {
                assert!(
                    result.is_ok(),
                    "request {requested} <= available {available} must succeed"
                );
                let settled = result.unwrap().unwrap();
                assert_eq!(settled, requested);
                // Remainder must be computable without underflow.
                let remainder = available
                    .checked_sub(settled)
                    .expect("remainder underflow in fuzz sweep");
                assert!(remainder >= 0);
            }
        }
    }
    let _ = env;
}