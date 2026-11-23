// #12 added property-test boundary coverage for mark_paid's grace-window
// logic, but implicitly assumed the ledger timestamp behaves the way most
// tests naturally construct it: monotonically increasing, one value per
// transaction. This suite uses soroban-sdk's Env testutils to force the
// ledger timestamp backward and to hold it flat across several transactions,
// and documents what the grace-window check (`timestamp >= expires_at +
// grace_window`, see mark_paid in
// contracts/invoice/src/entrypoints/lifecycle.rs) actually does under both
// scenarios, since the check has no explicit monotonicity assumption baked
// into its logic -- it reads env.ledger().timestamp() fresh on every call.

extern crate std;

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

fn create_invoice_expiring_in(env: &Env, client: &InvoiceContractClient, expires_in: u64) -> u64 {
    let merchant = Address::generate(env);
    client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &expires_in,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    )
}

// ── Backward-jumping timestamp ─────────────────────────────────────────────

/// The invoice is created while the ledger clock reads a *later* timestamp
/// than the one mark_paid is subsequently called at. Since the
/// effective-deadline check is a pure function of the current ledger
/// timestamp at call time (not of any elapsed delta since creation), payment
/// still succeeds here -- this documents that behavior rather than assuming
/// it.
#[test]
fn mark_paid_succeeds_when_ledger_timestamp_is_earlier_than_at_creation() {
    let (env, admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let payer = Address::generate(&env);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    // Ledger timestamp jumps backward relative to creation time, but is still
    // comfortably before expires_at.
    env.ledger().with_mut(|l| l.timestamp = 500);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
}

/// Documents the sharper edge of the same property: an invoice that was
/// already correctly rejected as Expired at a later timestamp can become
/// payable again if the ledger timestamp subsequently moves backward below
/// the effective deadline, because mark_paid re-evaluates
/// `timestamp >= expires_at + grace_window` fresh on every call, and a
/// rejected call never mutates invoice state (confirmed by the Pending
/// assertion below, consistent with the existing #55 boundary tests). This
/// is the "extending an effective payment window" scenario named in the
/// issue -- pinned down here as observed, documented behavior, not asserted
/// to be a bug, since real Stellar ledger timestamps are not expected to
/// move backward in production.
#[test]
fn mark_paid_rejected_then_backward_clock_jump_makes_it_payable_again() {
    let (env, admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let payer = Address::generate(&env);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    // Move forward past the deadline -- correctly rejected, no grace window set.
    env.ledger().with_mut(|l| l.timestamp = 1_200);
    let err = client
        .try_mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Expired);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);

    // Ledger timestamp now moves backward, below the effective deadline.
    env.ledger().with_mut(|l| l.timestamp = 900);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
}

// ── Flat (non-advancing) timestamp across multiple transactions ────────────

/// Several independent invoices are created and paid at the exact same
/// ledger timestamp, with no advance between any of the transactions.
/// Confirms mark_paid does not implicitly assume the ledger clock advances
/// between calls (e.g. no hidden "last processed timestamp" gate).
#[test]
fn mark_paid_succeeds_for_multiple_invoices_at_an_identical_flat_timestamp() {
    let (env, admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 5_000);

    let mut ids = std::vec::Vec::new();
    for _ in 0..5 {
        ids.push(create_invoice_expiring_in(&env, &client, 100)); // expires_at = 5_100
    }

    // Ledger timestamp never advances between any of these calls.
    for &id in ids.iter() {
        let payer = Address::generate(&env);
        client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
        assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
    }
}

/// Same flat-timestamp scenario, but at exactly the effective deadline --
/// every one of several invoices sharing a non-advancing timestamp must be
/// rejected identically and independently, confirming the boundary check
/// isn't skewed by call ordering when the clock doesn't move between calls.
#[test]
fn mark_paid_rejects_multiple_invoices_at_an_identical_flat_expiry_boundary() {
    let (env, admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 0);

    let mut ids = std::vec::Vec::new();
    for _ in 0..5 {
        ids.push(create_invoice_expiring_in(&env, &client, 100)); // expires_at = 100
    }

    env.ledger().with_mut(|l| l.timestamp = 100); // == expires_at, exclusive boundary
    for &id in ids.iter() {
        let payer = Address::generate(&env);
        let err = client
            .try_mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, InvoiceError::Expired);
        assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);
    }
}
