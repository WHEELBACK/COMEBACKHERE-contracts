//! Pins down how an address's compliance tier interacts with a time-limited allow (#609).
//!
//! Semantics: the tier is metadata stored independently of allow status. It is never
//! cleared by allow expiry, so `get_address_tier` keeps returning the recorded tier
//! after `is_allowed` has flipped to `false`. Integrators must gate on `is_allowed`
//! first and only then consult the tier.

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup() -> (Env, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

#[test]
fn tier_then_expiring_allow_keeps_tier_after_expiry() {
    let (env, admin, client) = setup();
    let addr = Address::generate(&env);
    client.allow_address_with_tier(&admin, &addr, &2);
    client.allow_address_until(&admin, &addr, &2_000);

    assert!(client.is_allowed(&addr));
    assert_eq!(client.get_address_tier(&addr), 2);

    env.ledger().set_timestamp(2_000);
    assert!(!client.is_allowed(&addr));
    assert_eq!(client.get_address_tier(&addr), 2);
}

#[test]
fn expiring_allow_then_tier_makes_allow_permanent() {
    let (env, admin, client) = setup();
    let addr = Address::generate(&env);
    client.allow_address_until(&admin, &addr, &2_000);
    client.allow_address_with_tier(&admin, &addr, &1);

    env.ledger().set_timestamp(5_000);
    assert!(client.is_allowed(&addr));
    assert_eq!(client.get_address_tier(&addr), 1);
    assert_eq!(client.get_allow_expiry(&addr), None);
}

#[test]
fn sweep_expired_does_not_clear_tier() {
    let (env, admin, client) = setup();
    let addr = Address::generate(&env);
    client.allow_address_with_tier(&admin, &addr, &3);
    client.allow_address_until(&admin, &addr, &2_000);

    env.ledger().set_timestamp(3_000);
    client.sweep_expired(&admin);
    assert!(!client.is_allowed(&addr));
    assert_eq!(client.get_address_tier(&addr), 3);
}

#[test]
fn reallow_after_expiry_restores_access_with_same_tier() {
    let (env, admin, client) = setup();
    let addr = Address::generate(&env);
    client.allow_address_with_tier(&admin, &addr, &2);
    client.allow_address_until(&admin, &addr, &2_000);

    env.ledger().set_timestamp(3_000);
    assert!(!client.is_allowed(&addr));
    client.allow_address_until(&admin, &addr, &4_000);
    assert!(client.is_allowed(&addr));
    assert_eq!(client.get_address_tier(&addr), 2);
}

#[test]
fn untiered_expired_address_reports_default_tier() {
    let (env, admin, client) = setup();
    let addr = Address::generate(&env);
    client.allow_address_until(&admin, &addr, &2_000);
    env.ledger().set_timestamp(2_000);
    assert!(!client.is_allowed(&addr));
    assert_eq!(client.get_address_tier(&addr), 0);
}
