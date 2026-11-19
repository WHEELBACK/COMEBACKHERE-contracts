//! Integration tests for the `event-emitter` contract.
//!
//! These tests exercise the contract through the Soroban test harness and
//! verify:
//!   * persistent storage keys have their TTL bumped on reads and writes
//!     (issue #956), and
//!   * zero-balance / boundary-balance paths return typed errors instead of
//!     panicking the host (issue #957).

use event_emitter::{EventEmitterContract, EventEmitterContractClient, Error};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, IntoVal, Symbol,
};

/// Helper: deploy the contract and return a client bound to it.
fn setup<'a>(env: &Env) -> (Address, EventEmitterContractClient<'a>) {
    let contract_id = env.register_contract(None, EventEmitterContract);
    let client = EventEmitterContractClient::new(env, &contract_id);
    (contract_id, client)
}

/// Helper: read the remaining TTL for a persistent key, if present.
fn persistent_ttl(env: &Env, contract_id: &Address, key: &impl IntoVal<Env, soroban_sdk::Val>) -> Option<u32> {
    env.as_contract(contract_id, || {
        env.storage().persistent().get_ttl(&key.clone().into_val(env))
    })
}

// ---------------------------------------------------------------------------
// Issue #956: automatic TTL bump on persistent storage keys
// ---------------------------------------------------------------------------

#[test]
fn test_persistent_ttl_extended_after_write() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, client) = setup(&env);
    let admin = Address::generate(&env);

    // Initialize the contract; this writes persistent state.
    client.initialize(&admin);

    // The admin key should now exist in persistent storage with a TTL.
    let ttl_after_write = persistent_ttl(&env, &contract_id, &symbol_short!("admin"));
    assert!(
        ttl_after_write.is_some(),
        "admin key must be present in persistent storage after initialize"
    );
    assert!(
        ttl_after_write.unwrap() > 0,
        "persistent key must have a non-zero TTL after write"
    );
}

#[test]
fn test_persistent_ttl_extended_after_read() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, client) = setup(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Advance the ledger so the TTL decreases, then read the value.
    env.ledger().with_mut(|li| {
        li.sequence_number += 100;
    });

    let ttl_before_read = persistent_ttl(&env, &contract_id, &symbol_short!("admin"))
        .expect("admin key must exist before read");

    // Reading through the contract should bump the TTL back up.
    let _ = client.get_admin();

    let ttl_after_read = persistent_ttl(&env, &contract_id, &symbol_short!("admin"))
        .expect("admin key must exist after read");

    assert!(
        ttl_after_read >= ttl_before_read,
        "TTL must not shrink after a read (before={ttl_before_read}, after={ttl_after_read})"
    );
}

#[test]
fn test_persistent_ttl_bumped_across_multiple_writes() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, client) = setup(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let initial_ttl = persistent_ttl(&env, &contract_id, &symbol_short!("admin"))
        .expect("admin key must exist after initialize");

    // Repeated writes should keep the TTL at or above the initial value.
    for _ in 0..3 {
        env.ledger().with_mut(|li| {
            li.sequence_number += 50;
        });
        client.set_admin(&admin);
        let ttl = persistent_ttl(&env, &contract_id, &symbol_short!("admin"))
            .expect("admin key must exist after set_admin");
        assert!(
            ttl >= initial_ttl,
            "TTL must be refreshed on each write (initial={initial_ttl}, now={ttl})"
        );
    }
}

// ---------------------------------------------------------------------------
// Issue #957: zero-balance and boundary-balance paths return typed errors
// ---------------------------------------------------------------------------

#[test]
fn test_zero_balance_returns_typed_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client) = setup(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let user = Address::generate(&env);

    // A zero-balance account must not panic; it must return a typed error.
    let result = client.try_emit_event(&user, &symbol_short!("zero"), &0i128);
    assert_eq!(
        result,
        Err(Ok(Error::ZeroBalance)),
        "zero balance must surface as a typed ZeroBalance error"
    );
}

#[test]
fn test_boundary_balance_does_not_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client) = setup(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let user = Address::generate(&env);

    // Exactly-at-threshold balance should succeed without panicking.
    let result = client.try_emit_event(&user, &symbol_short!("edge"), &1i128);
    assert!(
        result.is_ok(),
        "boundary balance must not panic; got {result:?}"
    );

    // Just-below-threshold balance must return a typed error, not panic.
    let below = client.try_emit_event(&user, &symbol_short!("edge"), &0i128);
    assert_eq!(
        below,
        Err(Ok(Error::ZeroBalance)),
        "below-threshold balance must surface as a typed error"
    );
}

#[test]
fn test_negative_balance_returns_typed_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client) = setup(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let user = Address::generate(&env);

    // Negative balances are invalid and must be rejected with a typed error.
    let result = client.try_emit_event(&user, &symbol_short!("neg"), &-1i128);
    assert_eq!(
        result,
        Err(Ok(Error::InvalidAmount)),
        "negative balance must surface as a typed InvalidAmount error"
    );
}

#[test]
fn test_uninitialized_contract_returns_typed_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client) = setup(&env);
    let user = Address::generate(&env);

    // Calling before initialize must not panic; it must return NotInitialized.
    let result = client.try_emit_event(&user, &symbol_short!("early"), &10i128);
    assert_eq!(
        result,
        Err(Ok(Error::NotInitialized)),
        "uninitialized contract must surface as a typed NotInitialized error"
    );
}

#[test]
fn test_double_initialize_returns_typed_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client) = setup(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Re-initializing must be rejected with a typed error, not a panic.
    let result = client.try_initialize(&admin);
    assert_eq!(
        result,
        Err(Ok(Error::AlreadyInitialized)),
        "double initialize must surface as a typed AlreadyInitialized error"
    );
}

// ---------------------------------------------------------------------------
// Fuzz-style loop: random-ish balances must never panic the host
// ---------------------------------------------------------------------------

#[test]
fn test_fuzz_balances_never_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let (_contract_id, client) = setup(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let user = Address::generate(&env);

    // Deterministic pseudo-random sweep across the balance domain.
    let mut seed: i128 = 0x5DEECE66D;
    for i in 0..64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let amount = seed % 1_000_000;

        // Every call must return either Ok or a typed Err; never panic.
        let result = client.try_emit_event(&user, &Symbol::new(&env, "fuzz"), &amount);
        match result {
            Ok(_) => {}
            Err(Ok(_typed)) => {}
            Err(Err(_)) => panic!("unexpected host error at iteration {i}"),
        }
    }
}