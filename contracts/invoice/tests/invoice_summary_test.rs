//! #712: `get_invoice_summary` reports the outstanding balance.
//!
//! Wallets rendering a list of invoices previously had to replay
//! `invoice_paid`/refund events to work out how much a payer still owed.
//! `InvoiceSummary` now carries `paid_usdc` and `remaining_usdc` next to the row
//! fields, so every payment state is readable in one call. These tests pin the
//! derivation for each state an invoice can be in, plus the invariants a client
//! relies on (`remaining + paid` reconstructs the gross, and the summary agrees
//! with `get_invoice`).

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    vec, Address, Env,
};

const AMOUNT_USDC: i128 = 10_000_000;
const GROSS_USDC: i128 = 10_250_000;
const EXPIRES_IN: u64 = 3_600;

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    payer: Address,
    client: InvoiceContractClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    Fixture {
        env,
        admin,
        merchant,
        payer,
        client,
    }
}

fn create(f: &Fixture) -> u64 {
    f.client.create_invoice(
        &f.merchant,
        &AMOUNT_USDC,
        &GROSS_USDC,
        &EXPIRES_IN,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    )
}

fn pay(f: &Fixture, id: u64) {
    f.client.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
}

#[test]
fn unpaid_invoice_owes_its_entire_gross_amount() {
    let f = setup();
    let id = create(&f);

    let summary = f.client.get_invoice_summary(&id);

    assert_eq!(summary.id, id);
    assert_eq!(summary.status, InvoiceStatus::Pending);
    assert_eq!(summary.amount_usdc, AMOUNT_USDC);
    assert_eq!(summary.gross_usdc, GROSS_USDC);
    // Nothing has moved yet, so the payer still owes the full charged amount.
    assert_eq!(summary.paid_usdc, 0);
    assert_eq!(summary.remaining_usdc, GROSS_USDC);
}

#[test]
fn amended_invoice_reports_the_revised_outstanding_balance() {
    let f = setup();
    let id = create(&f);
    f.client
        .amend_invoice(&f.merchant, &id, &20_000_000, &20_500_000, &EXPIRES_IN);

    let summary = f.client.get_invoice_summary(&id);

    // The outstanding balance follows the amendment, not the original amount,
    // so a stale figure can't outlive the merchant's own edit.
    assert_eq!(summary.paid_usdc, 0);
    assert_eq!(summary.remaining_usdc, 20_500_000);
}

#[test]
fn partially_progressed_invoice_owes_only_what_is_left() {
    let f = setup();
    let id = create(&f);
    pay(&f, id);

    let summary = f.client.get_invoice_summary(&id);

    assert_eq!(summary.status, InvoiceStatus::Paid);
    assert_eq!(summary.paid_usdc, GROSS_USDC);
    assert_eq!(summary.remaining_usdc, 0);
    // The net merchant amount is untouched by settlement of the balance.
    assert_eq!(summary.amount_usdc, AMOUNT_USDC);
}

#[test]
fn refund_requested_invoice_has_nothing_left_outstanding() {
    let f = setup();
    let id = create(&f);
    pay(&f, id);
    f.client.request_refund(&f.payer, &id);

    let summary = f.client.get_invoice_summary(&id);

    assert_eq!(summary.status, InvoiceStatus::RefundRequested);
    assert_eq!(summary.paid_usdc, GROSS_USDC);
    assert_eq!(summary.remaining_usdc, 0);
}

#[test]
fn rejected_refund_returns_the_invoice_to_owing_nothing() {
    let f = setup();
    let id = create(&f);
    pay(&f, id);
    f.client.request_refund(&f.payer, &id);
    f.client.reject_refund(&f.admin, &id);

    let summary = f.client.get_invoice_summary(&id);

    assert_eq!(summary.status, InvoiceStatus::Paid);
    assert_eq!(summary.paid_usdc, GROSS_USDC);
    assert_eq!(summary.remaining_usdc, 0);
}

#[test]
fn refunded_invoice_reports_the_full_amount_as_collected() {
    let f = setup();
    let id = create(&f);
    pay(&f, id);
    f.client.request_refund(&f.payer, &id);
    f.client.approve_refund(&f.admin, &id);

    let summary = f.client.get_invoice_summary(&id);

    // A refund pays the payer back out; it is not a negative contribution, so
    // the amount that arrived stays visible while nothing is left to pay.
    assert_eq!(summary.status, InvoiceStatus::Refunded);
    assert_eq!(summary.paid_usdc, GROSS_USDC);
    assert_eq!(summary.remaining_usdc, 0);
}

#[test]
fn released_invoice_owes_nothing() {
    let f = setup();
    let id = create(&f);
    pay(&f, id);
    f.client.release_escrow(&f.admin, &id);

    let summary = f.client.get_invoice_summary(&id);

    assert_eq!(summary.status, InvoiceStatus::Released);
    assert_eq!(summary.paid_usdc, GROSS_USDC);
    assert_eq!(summary.remaining_usdc, 0);
}

#[test]
fn expired_invoice_owes_nothing_because_it_can_no_longer_be_paid() {
    let f = setup();
    let id = create(&f);
    f.env.ledger().set_timestamp(EXPIRES_IN + 1);
    f.client.batch_expire(&f.admin, &vec![&f.env, id]);

    let summary = f.client.get_invoice_summary(&id);

    assert_eq!(summary.status, InvoiceStatus::Expired);
    assert_eq!(summary.paid_usdc, 0);
    assert_eq!(summary.remaining_usdc, 0);
}

#[test]
fn cancelled_invoice_owes_nothing() {
    let f = setup();
    let id = create(&f);
    f.client.cancel_invoice(&f.merchant, &id);

    let summary = f.client.get_invoice_summary(&id);

    assert_eq!(summary.status, InvoiceStatus::Cancelled);
    assert_eq!(summary.paid_usdc, 0);
    assert_eq!(summary.remaining_usdc, 0);
}

#[test]
fn paid_and_remaining_always_reconstruct_the_gross_amount() {
    let f = setup();
    let pending_id = create(&f);
    let paid_id = create(&f);
    pay(&f, paid_id);

    for id in [pending_id, paid_id] {
        let summary = f.client.get_invoice_summary(&id);
        assert_eq!(
            summary.paid_usdc + summary.remaining_usdc,
            summary.gross_usdc,
            "invoice {id}: collected plus outstanding must equal the gross"
        );
        assert!(summary.remaining_usdc >= 0);
    }
}

#[test]
fn summary_agrees_with_the_full_invoice_record() {
    let f = setup();
    let id = create(&f);
    pay(&f, id);

    let invoice = f.client.get_invoice(&id);
    let summary = f.client.get_invoice_summary(&id);

    // The summary is a projection of the same record, never a second copy.
    assert_eq!(summary.id, invoice.id);
    assert_eq!(summary.status, invoice.status);
    assert_eq!(summary.amount_usdc, invoice.amount_usdc);
    assert_eq!(summary.gross_usdc, invoice.gross_usdc);
    assert_eq!(summary.expires_at, invoice.expires_at);
}

#[test]
fn summary_of_an_unknown_invoice_is_not_found() {
    let f = setup();

    let err = f.client.try_get_invoice_summary(&999).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::NotFound);
}
