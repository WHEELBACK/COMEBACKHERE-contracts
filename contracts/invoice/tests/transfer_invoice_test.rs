//! Tests for #694: invoice ownership transfer entrypoint.
//!
//! Verifies:
//! * Payer can transfer a paid invoice to a new address.
//! * Non-payer callers are rejected.
//! * Transfers on non-Paid invoices (Pending, Cancelled, etc.) are rejected.
//! * Sender and receiver must differ.
//! * `invoice_transferred` event is emitted.
//! * Paused contract rejects transfers.

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
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

fn create_paid_invoice(
    env: &Env,
    admin: &Address,
    client: &InvoiceContractClient,
) -> (u64, Address, Address) {
    let merchant = Address::generate(env);
    let payer = Address::generate(env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    client.mark_paid(admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    (id, merchant, payer)
}

// ─── transfer_invoice ─────────────────────────────────────────────────────────

#[test]
fn test_transfer_invoice_by_payer_succeeds() {
    let (env, admin, client) = setup();
    let new_payer = Address::generate(&env);
    let (id, _merchant, payer) = create_paid_invoice(&env, &admin, &client);

    client.transfer_invoice(&payer, &id, &new_payer);

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.payer, MaybeAddress::Some(new_payer));
    assert_eq!(invoice.status, InvoiceStatus::Paid);
}

#[test]
fn test_transfer_invoice_by_non_payer_is_rejected() {
    let (env, admin, client) = setup();
    let (id, _merchant, _payer) = create_paid_invoice(&env, &admin, &client);
    let rogue = Address::generate(&env);
    let new_payer = Address::generate(&env);

    let err = client
        .try_transfer_invoice(&rogue, &id, &new_payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
}

#[test]
fn test_transfer_invoice_on_pending_invoice_is_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let new_payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );

    let err = client
        .try_transfer_invoice(&payer, &id, &new_payer)
        .unwrap_err()
        .unwrap();
    // Pending invoice has no payer set
    assert_eq!(err, InvoiceError::Unauthorized);
}

#[test]
fn test_transfer_invoice_to_self_is_rejected() {
    let (env, admin, client) = setup();
    let (id, _merchant, payer) = create_paid_invoice(&env, &admin, &client);

    let err = client
        .try_transfer_invoice(&payer, &id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::InvalidAmount);
}

#[test]
fn test_transfer_invoice_not_found_is_rejected() {
    let (env, _admin, client) = setup();
    let payer = Address::generate(&env);
    let new_payer = Address::generate(&env);

    let err = client
        .try_transfer_invoice(&payer, &999, &new_payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotFound);
}

#[test]
fn test_transfer_invoice_on_refund_requested_is_rejected() {
    let (env, admin, client) = setup();
    let (id, _merchant, payer) = create_paid_invoice(&env, &admin, &client);
    let new_payer = Address::generate(&env);

    client.request_refund(&payer, &id);
    assert_eq!(
        client.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );

    let err = client
        .try_transfer_invoice(&payer, &id, &new_payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotPaid);
}

#[test]
fn test_transfer_invoice_emits_invoice_transferred_event() {
    use soroban_sdk::testutils::Events;
    use soroban_sdk::{Symbol, TryFromVal};

    let (env, admin, client) = setup();
    let (id, _merchant, payer) = create_paid_invoice(&env, &admin, &client);
    let new_payer = Address::generate(&env);

    client.transfer_invoice(&payer, &id, &new_payer);

    let events = env.events().all();
    let (_, topics, _) = events.last().unwrap();
    assert_eq!(
        Symbol::try_from_val(&env, &topics.get_unchecked(0)).unwrap(),
        Symbol::new(&env, "invoice_transferred")
    );
}

#[test]
fn test_transfer_invoice_new_owner_can_request_refund() {
    let (env, admin, client) = setup();
    let (id, _merchant, payer) = create_paid_invoice(&env, &admin, &client);
    let new_payer = Address::generate(&env);

    client.transfer_invoice(&payer, &id, &new_payer);

    // Old payer can no longer request refund
    let err = client
        .try_request_refund(&payer, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);

    // New payer can request refund
    client.request_refund(&new_payer, &id);
    assert_eq!(
        client.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );
}

#[test]
fn test_transfer_invoice_when_paused_is_rejected() {
    let (env, admin, client) = setup();
    let (id, _merchant, payer) = create_paid_invoice(&env, &admin, &client);
    let new_payer = Address::generate(&env);

    client.pause(&admin);
    let err = client
        .try_transfer_invoice(&payer, &id, &new_payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}
