//! Tests for #701: admin-configurable refund network fee.
//!
//! Verifies:
//! * Default fee equals REFUND_NETWORK_FEE when unset.
//! * Admin can update the fee within bounds.
//! * A `refund_network_fee_updated` event is emitted on change.
//! * Fees above MAX_REFUND_NETWORK_FEE are rejected.
//! * Negative fees are rejected.
//! * Non-admin callers are rejected.

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, REFUND_NETWORK_FEE,
    MAX_REFUND_NETWORK_FEE,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

extern crate std;

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

#[test]
fn test_get_refund_network_fee_returns_default_when_unset() {
    let (_env, _admin, client) = setup();
    assert_eq!(client.get_refund_network_fee(), REFUND_NETWORK_FEE);
}

#[test]
fn test_set_refund_network_fee_updates_stored_value() {
    let (_env, admin, client) = setup();
    let new_fee: i128 = 500;
    client.set_refund_network_fee(&admin, &new_fee);
    assert_eq!(client.get_refund_network_fee(), new_fee);
}

#[test]
fn test_set_refund_network_fee_to_zero_is_accepted() {
    let (_env, admin, client) = setup();
    client.set_refund_network_fee(&admin, &0);
    assert_eq!(client.get_refund_network_fee(), 0);
}

#[test]
fn test_set_refund_network_fee_at_max_is_accepted() {
    let (_env, admin, client) = setup();
    client.set_refund_network_fee(&admin, &MAX_REFUND_NETWORK_FEE);
    assert_eq!(client.get_refund_network_fee(), MAX_REFUND_NETWORK_FEE);
}

#[test]
fn test_set_refund_network_fee_above_max_is_rejected() {
    let (_env, admin, client) = setup();
    let over_max = MAX_REFUND_NETWORK_FEE + 1;
    let err = client
        .try_set_refund_network_fee(&admin, &over_max)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::InvalidAmount);
}

#[test]
fn test_set_refund_network_fee_negative_is_rejected() {
    let (_env, admin, client) = setup();
    let err = client
        .try_set_refund_network_fee(&admin, &-1)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::InvalidAmount);
}

#[test]
fn test_set_refund_network_fee_unauthorized_is_rejected() {
    let (env, _admin, client) = setup();
    let rogue = Address::generate(&env);
    let err = client
        .try_set_refund_network_fee(&rogue, &500)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
}

#[test]
fn test_set_refund_network_fee_emits_event() {
    use soroban_sdk::testutils::Events;
    use soroban_sdk::{Symbol, TryFromVal};
    use invoice::RefundNetworkFeeUpdatedEvent;

    let (env, admin, client) = setup();
    let new_fee: i128 = 1_000_000;
    client.set_refund_network_fee(&admin, &new_fee);

    let events = env.events().all();
    let (_, topics, data) = events.last().unwrap();
    assert_eq!(
        Symbol::try_from_val(&env, &topics.get_unchecked(0)).unwrap(),
        Symbol::new(&env, "refund_network_fee_updated")
    );
    let payload = RefundNetworkFeeUpdatedEvent::try_from_val(&env, &data).unwrap();
    assert_eq!(payload.old_fee, REFUND_NETWORK_FEE);
    assert_eq!(payload.new_fee, new_fee);
}

#[test]
fn test_set_refund_network_fee_multiple_updates_accumulate() {
    let (_env, admin, client) = setup();
    client.set_refund_network_fee(&admin, &200);
    assert_eq!(client.get_refund_network_fee(), 200);
    client.set_refund_network_fee(&admin, &50_000_000);
    assert_eq!(client.get_refund_network_fee(), 50_000_000);
    client.set_refund_network_fee(&admin, &0);
    assert_eq!(client.get_refund_network_fee(), 0);
}
