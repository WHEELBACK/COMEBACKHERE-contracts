//! Refund state consistency: what the invoice contract does with escrow once a
//! paid invoice has been refunded.
//!
//! The `release_escrow` guard (`status != Paid` → `NotPaid`) already prevents
//! paying a merchant out of escrow that has gone back to the payer, but nothing
//! pinned the `Paid` → `Refunded` → `release_escrow` path specifically: the
//! double-release path was covered, this one was not. A regression that
//! weakened the guard (comparing against the wrong status, or treating
//! `Refunded` as payable) would let escrow be released on a refunded invoice -
//! funds leaving the protocol twice - and would only show up as a failure in a
//! much longer end-to-end flow.
//!
//! Each test here asserts the exact `InvoiceError` variant rather than "some
//! error", so a guard that fails for the wrong reason (e.g. `NotFound` because
//! the invoice was dropped) is caught here too, and a control case proves the
//! entrypoint still works on a genuinely payable invoice.

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal,
};

const AMOUNT_USDC: i128 = 10_000_000;
const GROSS_USDC: i128 = 10_250_000;

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    payer: Address,
    invoice: InvoiceContractClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &contract_id);
    invoice.initialize(&admin);
    Fixture {
        env,
        admin,
        merchant,
        payer,
        invoice,
    }
}

fn paid_invoice(f: &Fixture) -> u64 {
    let id = f.invoice.create_invoice(
        &f.merchant,
        &AMOUNT_USDC,
        &GROSS_USDC,
        &3_600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    id
}

/// True when an `escrow_released` event was published by the last top-level
/// call. Payloads are deliberately ignored: only the topic symbol matters here.
fn escrow_released_emitted(env: &Env) -> bool {
    env.events().all().iter().any(|(_, topics, _)| {
        Symbol::try_from_val(env, &topics.get_unchecked(0))
            .map(|sym| sym == Symbol::new(env, "escrow_released"))
            .unwrap_or(false)
    })
}

#[test]
fn release_escrow_is_rejected_once_a_refund_has_been_approved() {
    let f = setup();
    let id = paid_invoice(&f);
    f.invoice.request_refund(&f.payer, &id);
    f.invoice.approve_refund(&f.admin, &id);
    assert_eq!(
        f.invoice.get_invoice_status(&id),
        InvoiceStatus::Refunded,
        "precondition: the refund must be approved before escrow is attempted"
    );

    let err = f.invoice.try_release_escrow(&f.admin, &id).unwrap_err();
    // `events().all()` only covers the most recent top-level call, so the
    // absence of an event is asserted before any other contract call.
    assert!(
        !escrow_released_emitted(&f.env),
        "a rejected release must not emit escrow_released"
    );

    // #710: assert the exact variant. `NotPaid` is the guard refusing a
    // non-payable status; anything else means the invoice record itself went
    // missing or the call failed for an unrelated reason.
    assert_eq!(err.unwrap(), InvoiceError::NotPaid);
    assert_eq!(
        f.invoice.get_invoice_status(&id),
        InvoiceStatus::Refunded,
        "the failed release must not move the invoice out of Refunded"
    );
}

#[test]
fn release_escrow_is_rejected_while_a_refund_request_is_open() {
    let f = setup();
    let id = paid_invoice(&f);
    f.invoice.request_refund(&f.payer, &id);

    let err = f.invoice.try_release_escrow(&f.admin, &id).unwrap_err();
    assert!(!escrow_released_emitted(&f.env));

    // RefundRequested is just as unpayable as Refunded: releasing here would
    // hand the merchant funds the payer is about to get back.
    assert_eq!(err.unwrap(), InvoiceError::NotPaid);
    assert_eq!(
        f.invoice.get_invoice_status(&id),
        InvoiceStatus::RefundRequested
    );
}

#[test]
fn a_rejected_refund_makes_the_invoice_payable_again() {
    let f = setup();
    let id = paid_invoice(&f);
    f.invoice.request_refund(&f.payer, &id);
    f.invoice.reject_refund(&f.admin, &id);

    // Control case for the two tests above: the guard keys on the live status,
    // so rejecting the refund restores the Paid -> Released path unchanged.
    f.invoice.release_escrow(&f.admin, &id);
    // Checked straight after the call: under soroban-sdk 22 `events().all()`
    // only reflects the most recent top-level invocation.
    assert!(escrow_released_emitted(&f.env));

    assert_eq!(f.invoice.get_invoice_status(&id), InvoiceStatus::Released);
}

#[test]
fn release_escrow_stays_rejected_after_a_rejected_refund_and_refund_loop() {
    let f = setup();
    let id = paid_invoice(&f);
    // Two request/reject rounds: the second refund attempt is approved, so the
    // invoice ends Refunded regardless of how many times it bounced through Paid.
    f.invoice.request_refund(&f.payer, &id);
    f.invoice.reject_refund(&f.admin, &id);
    f.invoice.request_refund(&f.payer, &id);
    f.invoice.approve_refund(&f.admin, &id);

    let err = f.invoice.try_release_escrow(&f.admin, &id).unwrap_err();
    assert!(!escrow_released_emitted(&f.env));

    assert_eq!(err.unwrap(), InvoiceError::NotPaid);
    assert_eq!(f.invoice.get_invoice_status(&id), InvoiceStatus::Refunded);
}

#[test]
fn refunded_invoice_summary_reports_nothing_outstanding() {
    let f = setup();
    let id = paid_invoice(&f);
    f.invoice.request_refund(&f.payer, &id);
    f.invoice.approve_refund(&f.admin, &id);

    // The read model agrees with the guard: once refunded, no balance is owed.
    let summary = f.invoice.get_invoice_summary(&id);
    assert_eq!(summary.status, InvoiceStatus::Refunded);
    assert_eq!(summary.remaining_usdc, 0);
}
