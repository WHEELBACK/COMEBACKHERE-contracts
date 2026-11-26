//! Issue #946: modular error enums and custom client-facing messages.
//!
//! The error type lives in its own module (`time_lock::errors` through the
//! crate re-export), every code round-trips through `from_code`, and the same
//! message is available off-chain (`message`/`Display`) and on-chain
//! (`get_error_message`).

use soroban_sdk::{testutils::Address as _, Address, Env, String as SorobanString};
use time_lock::{TimeLockContract, TimeLockContractClient, TimeLockError};

fn setup<'a>(env: &'a Env) -> (TimeLockContractClient<'a>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let cid = env.register(TimeLockContract, ());
    let client = TimeLockContractClient::new(env, &cid);
    client.initialize(&admin);
    (client, admin)
}

#[test]
fn every_error_code_round_trips_from_raw_contract_code() {
    for (expected_code, error) in TimeLockError::ALL.iter().enumerate() {
        let code = error.code();
        assert_eq!(code, expected_code as u32 + 1);
        assert_eq!(TimeLockError::from_code(code), Some(*error));
    }
    assert_eq!(TimeLockError::from_code(0), None);
    assert_eq!(TimeLockError::from_code(u32::MAX), None);
}

#[test]
fn messages_and_categories_are_stable_and_unique() {
    let mut messages = std::vec::Vec::new();
    for error in TimeLockError::ALL {
        assert!(!error.message().is_empty());
        assert!(!error.category().is_empty());
        assert_eq!(
            error.to_string(),
            error.message(),
            "Display must expose the custom message for {error:?}"
        );
        assert!(!messages.contains(&error.message()), "duplicate message");
        messages.push(error.message());
    }
}

#[test]
fn message_for_matches_each_variant() {
    for error in TimeLockError::ALL {
        assert_eq!(time_lock::message_for(error.code()), Some(error.message()));
        assert_eq!(
            time_lock::category_for(error.code()),
            Some(error.category())
        );
    }
    assert_eq!(time_lock::message_for(0), None);
    assert_eq!(time_lock::message_for(999), None);
}

#[test]
fn on_chain_getter_returns_custom_messages() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    for error in TimeLockError::ALL {
        assert_eq!(
            client.get_error_message(&error.code()),
            Some(SorobanString::from_str(&env, error.message())),
            "on-chain message for {error:?} must match the off-chain message"
        );
    }
    assert_eq!(client.get_error_message(&0), None);
    assert_eq!(client.get_error_message(&999), None);
}

#[test]
fn message_resolves_a_real_failed_call() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let error = client.try_initialize(&admin).unwrap_err().unwrap();
    assert_eq!(error, TimeLockError::AlreadyInitialized);
    assert_eq!(
        client.get_error_message(&error.code()),
        Some(SorobanString::from_str(&env, error.message()))
    );
}
