//! #887: arithmetic underflow during partial settlement computations.
//!
//! Treasury's partial settlement path asks compliance whether a (partial)
//! amount is within the recipient's tier limit. A negative tier limit or a
//! non-positive amount (the result of an underflowed `remaining - executed`
//! style computation) must never be treated as within limits.

use compliance::{ComplianceContract, ComplianceContractClient, ComplianceError};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let subject = Address::generate(&env);
    let id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    client.allow_address(&admin, &subject);
    (env, admin, subject, client)
}

#[test]
fn negative_tier_limit_is_rejected() {
    let (_env, admin, _subject, client) = setup();
    assert_eq!(
        client.try_set_tier_limit(&admin, &0, &-1),
        Err(Ok(ComplianceError::InvalidTierLimit))
    );
    assert_eq!(
        client.try_set_tier_limit(&admin, &0, &i128::MIN),
        Err(Ok(ComplianceError::InvalidTierLimit))
    );
    assert_eq!(client.get_tier_limit(&0), None);
}

#[test]
fn zero_tier_limit_is_accepted_and_allows_nothing() {
    let (_env, admin, subject, client) = setup();
    client.set_tier_limit(&admin, &0, &0);
    assert_eq!(client.get_tier_limit(&0), Some(0));
    assert!(!client.is_allowed_for_amount(&subject, &1));
}

#[test]
fn non_positive_amounts_fail_closed() {
    let (_env, admin, subject, client) = setup();

    // Unlimited tier: still no free pass for an underflowed amount.
    assert!(!client.is_allowed_for_amount(&subject, &0));
    assert!(!client.is_allowed_for_amount(&subject, &-1));
    assert!(!client.is_allowed_for_amount(&subject, &i128::MIN));

    // Limited tier: a negative amount would trivially be `<= limit`.
    client.set_tier_limit(&admin, &0, &1_000);
    assert!(!client.is_allowed_for_amount(&subject, &0));
    assert!(!client.is_allowed_for_amount(&subject, &-500));
    assert!(!client.is_allowed_for_amount(&subject, &i128::MIN));
}

#[test]
fn partial_amounts_within_and_beyond_the_limit() {
    let (_env, admin, subject, client) = setup();
    client.set_tier_limit(&admin, &0, &1_000);

    assert!(client.is_allowed_for_amount(&subject, &1));
    assert!(client.is_allowed_for_amount(&subject, &999));
    assert!(client.is_allowed_for_amount(&subject, &1_000));
    assert!(!client.is_allowed_for_amount(&subject, &1_001));
    assert!(!client.is_allowed_for_amount(&subject, &i128::MAX));
}

#[test]
fn extreme_limit_does_not_underflow() {
    let (_env, admin, subject, client) = setup();
    client.set_tier_limit(&admin, &0, &i128::MAX);
    assert!(client.is_allowed_for_amount(&subject, &1));
    assert!(client.is_allowed_for_amount(&subject, &i128::MAX));
}
