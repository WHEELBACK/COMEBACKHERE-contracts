// #540 adds deterministic boundary coverage for invoice expiry and the
// grace window. The existing grace_window_clock_test.rs covers the general
// behaviour (backward clock jumps, flat timestamps), but not every edge
// second around the effective deadline. This suite pins down the exact
// semantics at `expiry - 1`, `expiry`, `expiry + grace` and
// `expiry + grace + 1` for both `mark_paid` and `batch_expire`, so the
// intended behaviour is locked in and documented by the tests themselves.
//
// The effective-deadline check in mark_paid (see
// contracts/invoice/src/entrypoints/lifecycle.rs) is
// `timestamp >= expires_at + grace_window`, i.e. the boundary is exclusive:
// an invoice is still payable at `expiry - 1` and at `expiry` (when no grace
// window is set), and becomes Expired at `expiry + grace`.

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

// ── mark_paid at the exact expiry boundary (no grace window) ───────────────

/// One second before expiry the invoice is still payable.
#[test]
fn mark_paid_succeeds_at_expiry_minus_one() {
    let (env, admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let payer = Address::generate(&env);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    env.ledger().with_mut(|l| l.timestamp = 1_099); // expiry - 1
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
}

/// At exactly `expiry` (no grace window) the invoice is still payable: the
/// deadline check is exclusive (`timestamp >= expires_at`).
#[test]
fn mark_paid_succeeds_at_exact_expiry() {
    let (env, admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let payer = Address::generate(&env);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    env.ledger().with_mut(|l| l.timestamp = 1_100); // == expiry
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
}

// ── mark_paid at the exact grace-window boundary ───────────────────────────

/// With a grace window set, the invoice is still payable at exactly
/// `expiry + grace` -- the effective deadline is exclusive.
#[test]
fn mark_paid_succeeds_at_exact_expiry_plus_grace() {
    let (env, admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let payer = Address::generate(&env);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    env.ledger().with_mut(|l| l.timestamp = 1_150); // == expiry + grace (50)
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
}

/// One second past `expiry + grace` the invoice is Expired and the rejected
/// call leaves the invoice Pending.
#[test]
fn mark_paid_rejected_at_expiry_plus_grace_plus_one() {
    let (env, admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let payer = Address::generate(&env);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    env.ledger().with_mut(|l| l.timestamp = 1_151); // expiry + grace + 1
    let err = client
        .try_mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Expired);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);
}

// ── batch_expire at the exact expiry boundary ──────────────────────────────

/// `batch_expire` must not expire an invoice one second before its expiry.
#[test]
fn batch_expire_skips_invoice_at_expiry_minus_one() {
    let (env, _admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    env.ledger().with_mut(|l| l.timestamp = 1_099); // expiry - 1
    client.batch_expire(&std::vec![id].into());
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);
}

/// At exactly `expiry` the invoice is still within its window and must not be
/// expired by `batch_expire`.
#[test]
fn batch_expire_skips_invoice_at_exact_expiry() {
    let (env, _admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    env.ledger().with_mut(|l| l.timestamp = 1_100); // == expiry
    client.batch_expire(&std::vec![id].into());
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);
}

/// At exactly `expiry + grace` the invoice is still payable and must not be
/// expired by `batch_expire`.
#[test]
fn batch_expire_skips_invoice_at_exact_expiry_plus_grace() {
    let (env, _admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    env.ledger().with_mut(|l| l.timestamp = 1_150); // == expiry + grace (50)
    client.batch_expire(&std::vec![id].into());
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);
}

/// One second past `expiry + grace` the invoice is past its effective
/// deadline and `batch_expire` must transition it to Expired.
#[test]
fn batch_expire_expires_invoice_at_expiry_plus_grace_plus_one() {
    let (env, _admin, client) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = create_invoice_expiring_in(&env, &client, 100); // expires_at = 1_100

    env.ledger().with_mut(|l| l.timestamp = 1_151); // expiry + grace + 1
    client.batch_expire(&std::vec![id].into());
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Expired);
}
