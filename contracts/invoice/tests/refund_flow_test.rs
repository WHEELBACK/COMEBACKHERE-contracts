//! Tests for #695: cancel_invoice, request_refund, and get_invoice entrypoints.
//!
//! These tests cover the full refund lifecycle:
//! * cancel_invoice: merchant or admin can cancel a pending invoice
//! * request_refund: payer can request a refund on a paid invoice
//! * get_invoice: retrieve invoice by ID
//! * approve_refund/reject_refund: admin actions on a refund request

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

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

fn create_basic_invoice(
    env: &Env,
    client: &InvoiceContractClient,
    merchant: &Address,
) -> u64 {
    client.create_invoice(
        merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    )
}

// ─── get_invoice ──────────────────────────────────────────────────────────────

#[test]
fn test_get_invoice_returns_correct_invoice() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.id, id);
    assert_eq!(invoice.merchant, merchant);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.amount_usdc, 10_000_000);
    assert_eq!(invoice.gross_usdc, 10_250_000);
}

#[test]
fn test_get_invoice_not_found_returns_error() {
    let (_env, _admin, client) = setup();
    let err = client.try_get_invoice(&999).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::NotFound);
}

// ─── cancel_invoice ───────────────────────────────────────────────────────────

#[test]
fn test_cancel_invoice_by_merchant_succeeds() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.cancel_invoice(&merchant, &id);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Cancelled);
}

#[test]
fn test_cancel_invoice_by_admin_succeeds() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.cancel_invoice(&admin, &id);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Cancelled);
}

#[test]
fn test_cancel_invoice_by_unauthorized_caller_is_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let rogue = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    let err = client
        .try_cancel_invoice(&rogue, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
}

#[test]
fn test_cancel_invoice_on_paid_invoice_is_rejected() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    let err = client
        .try_cancel_invoice(&merchant, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotPending);
}

#[test]
fn test_cancel_invoice_not_found_returns_error() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let err = client
        .try_cancel_invoice(&merchant, &999)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotFound);
}

#[test]
fn test_cancel_invoice_emits_cancelled_event() {
    use soroban_sdk::testutils::Events;
    use soroban_sdk::{Symbol, TryFromVal};

    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.cancel_invoice(&merchant, &id);

    let events = env.events().all();
    let (_, topics, _) = events.last().unwrap();
    assert_eq!(
        Symbol::try_from_val(&env, &topics.get_unchecked(0)).unwrap(),
        Symbol::new(&env, "invoice_cancelled")
    );
}

// ─── request_refund ───────────────────────────────────────────────────────────

#[test]
fn test_request_refund_by_payer_succeeds() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    client.request_refund(&payer, &id);
    assert_eq!(
        client.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );
}

#[test]
fn test_request_refund_by_non_payer_is_rejected() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let rogue = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    let err = client
        .try_request_refund(&rogue, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
}

#[test]
fn test_request_refund_on_pending_invoice_is_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    let err = client
        .try_request_refund(&payer, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotPaid);
}

#[test]
fn test_request_refund_emits_refund_requested_event() {
    use soroban_sdk::testutils::Events;
    use soroban_sdk::{Symbol, TryFromVal};

    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    client.request_refund(&payer, &id);

    let events = env.events().all();
    let (_, topics, _) = events.last().unwrap();
    assert_eq!(
        Symbol::try_from_val(&env, &topics.get_unchecked(0)).unwrap(),
        Symbol::new(&env, "invoice_refund_requested")
    );
}

// ─── approve / reject refund ──────────────────────────────────────────────────

#[test]
fn test_approve_refund_transitions_to_refunded() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    client.request_refund(&payer, &id);
    client.approve_refund(&admin, &id);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Refunded);
}

#[test]
fn test_reject_refund_transitions_back_to_paid() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    client.request_refund(&payer, &id);
    client.reject_refund(&admin, &id);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
}

#[test]
fn test_approve_refund_on_non_refund_requested_invoice_is_rejected() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    // Attempt to approve without first requesting
    let err = client
        .try_approve_refund(&admin, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotRefundRequested);
}

#[test]
fn test_full_refund_lifecycle_pending_to_refunded() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    // 1. Create
    let id = create_basic_invoice(&env, &client, &merchant);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);

    // 2. Mark paid
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);

    // 3. Request refund
    client.request_refund(&payer, &id);
    assert_eq!(
        client.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );

    // 4. Admin approves
    client.approve_refund(&admin, &id);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Refunded);
}

#[test]
fn test_refund_when_contract_paused_is_rejected() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    client.pause(&admin);
    let err = client
        .try_request_refund(&payer, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}

#[test]
fn test_cancel_invoice_when_paused_is_rejected() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = create_basic_invoice(&env, &client, &merchant);
    client.pause(&admin);
    let err = client
        .try_cancel_invoice(&merchant, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}
