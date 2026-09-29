//! End-to-end tests for the refund flow: `request_refund`, `approve_refund`
//! and `reject_refund`, covering every valid and invalid state transition.
//!
//! Emitted events are asserted against snapshots so future payload changes are
//! caught in review.

use comebackhere_invoice::{InvoiceContract, InvoiceContractClient, InvoiceStatus};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token, Address, Env, IntoVal, Symbol, TryFromVal,
};

const AMOUNT: i128 = 1_000;

struct Setup<'a> {
    env: Env,
    client: InvoiceContractClient<'a>,
    admin: Address,
    merchant: Address,
    payer: Address,
    token: Address,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    let token_id = env.register_stellar_asset_contract_v2(admin.clone());
    let token = token_id.address();
    token::StellarAssetClient::new(&env, &token).mint(&payer, &AMOUNT);

    let contract_id = env.register(InvoiceContract, ());
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token);

    Setup {
        env,
        client,
        admin,
        merchant,
        payer,
        token,
    }
}

/// Creates an invoice and pays it so it is in the `Paid` state.
fn paid_invoice(s: &Setup) -> u64 {
    let id = s.client.create_invoice(&s.merchant, &AMOUNT, &0);
    s.client.pay_invoice(&s.payer, &id);
    id
}

fn assert_last_event(s: &Setup, topic: &str, id: u64) {
    let (_, topics, _) = s.env.events().all().last().unwrap();
    assert_eq!(
        Symbol::try_from_val(&s.env, &topics.get_unchecked(0)).unwrap(),
        Symbol::new(&s.env, topic)
    );
    assert_eq!(
        u64::try_from_val(&s.env, &topics.get_unchecked(1)).unwrap(),
        id
    );
}

#[test]
fn request_refund_moves_paid_invoice_to_refund_requested() {
    let s = setup();
    let id = paid_invoice(&s);

    s.client.request_refund(&s.payer, &id);

    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::RefundRequested);
    assert_last_event(&s, "invoice_refund_requested", id);
}

#[test]
fn request_refund_twice_is_rejected() {
    let s = setup();
    let id = paid_invoice(&s);

    s.client.request_refund(&s.payer, &id);
    let result = s.client.try_request_refund(&s.payer, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::RefundRequested);
}

#[test]
fn request_refund_on_unpaid_invoice_is_rejected() {
    let s = setup();
    let id = s.client.create_invoice(&s.merchant, &AMOUNT, &0);

    let result = s.client.try_request_refund(&s.payer, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::Pending);
}

#[test]
fn request_refund_by_unauthorized_caller_is_rejected() {
    let s = setup();
    let id = paid_invoice(&s);
    let stranger = Address::generate(&s.env);

    let result = s.client.try_request_refund(&stranger, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::Paid);
}

#[test]
fn approve_refund_moves_invoice_to_refunded() {
    let s = setup();
    let id = paid_invoice(&s);
    s.client.request_refund(&s.payer, &id);

    s.client.approve_refund(&s.admin, &id);

    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_last_event(&s, "refund_approved", id);
}

#[test]
fn approve_refund_without_request_is_rejected() {
    let s = setup();
    let id = paid_invoice(&s);

    let result = s.client.try_approve_refund(&s.admin, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::Paid);
}

#[test]
fn approve_refund_by_unauthorized_caller_is_rejected() {
    let s = setup();
    let id = paid_invoice(&s);
    s.client.request_refund(&s.payer, &id);
    let stranger = Address::generate(&s.env);

    let result = s.client.try_approve_refund(&stranger, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::RefundRequested);
}

#[test]
fn reject_refund_moves_invoice_back_to_paid() {
    let s = setup();
    let id = paid_invoice(&s);
    s.client.request_refund(&s.payer, &id);

    s.client.reject_refund(&s.admin, &id);

    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::Paid);
    assert_last_event(&s, "refund_rejected", id);
}

#[test]
fn reject_refund_without_request_is_rejected() {
    let s = setup();
    let id = paid_invoice(&s);

    let result = s.client.try_reject_refund(&s.admin, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::Paid);
}

#[test]
fn reject_refund_by_unauthorized_caller_is_rejected() {
    let s = setup();
    let id = paid_invoice(&s);
    s.client.request_refund(&s.payer, &id);
    let stranger = Address::generate(&s.env);

    let result = s.client.try_reject_refund(&stranger, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::RefundRequested);
}

#[test]
fn approve_refund_after_rejection_is_rejected() {
    let s = setup();
    let id = paid_invoice(&s);
    s.client.request_refund(&s.payer, &id);
    s.client.reject_refund(&s.admin, &id);

    let result = s.client.try_approve_refund(&s.admin, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::Paid);
}

#[test]
fn reject_refund_after_approval_is_rejected() {
    let s = setup();
    let id = paid_invoice(&s);
    s.client.request_refund(&s.payer, &id);
    s.client.approve_refund(&s.admin, &id);

    let result = s.client.try_reject_refund(&s.admin, &id);

    assert!(result.is_err());
    assert_eq!(s.client.get_invoice(&id).status, InvoiceStatus::Refunded);
}

#[test]
fn refund_flow_event_payloads_match_snapshots() {
    let s = setup();
    let id = paid_invoice(&s);

    s.client.request_refund(&s.payer, &id);
    let (_, topics, data) = s.env.events().all().last().unwrap();
    assert_eq!(
        topics,
        (Symbol::new(&s.env, "invoice_refund_requested"), id).into_val(&s.env)
    );
    assert_eq!(data, s.client.get_invoice(&id).into_val(&s.env));

    s.client.approve_refund(&s.admin, &id);
    let (_, topics, data) = s.env.events().all().last().unwrap();
    assert_eq!(
        topics,
        (Symbol::new(&s.env, "refund_approved"), id).into_val(&s.env)
    );
    assert_eq!(data, s.client.get_invoice(&id).into_val(&s.env));

    let s2 = setup();
    let id2 = paid_invoice(&s2);
    s2.client.request_refund(&s2.payer, &id2);
    s2.client.reject_refund(&s2.admin, &id2);
    let (_, topics, data) = s2.env.events().all().last().unwrap();
    assert_eq!(
        topics,
        (Symbol::new(&s2.env, "refund_rejected"), id2).into_val(&s2.env)
    );
    assert_eq!(data, s2.client.get_invoice(&id2).into_val(&s2.env));
}
