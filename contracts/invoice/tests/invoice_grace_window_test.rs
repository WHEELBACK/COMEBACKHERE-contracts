// #55: dedicated boundary tests for the grace window logic in mark_paid.
//
// The effective deadline is `expires_at + grace_window`. The expiry check is
// `timestamp >= effective_deadline`, so:
//   - timestamp == effective_deadline - 1  → accepted (last valid second)
//   - timestamp == effective_deadline       → rejected (boundary is exclusive)
//
// Three thresholds are exercised:
//   1. expires_at exactly, no grace window  → Expired
//   2. expires_at + grace - 1               → Paid
//   3. expires_at + grace                   → Expired
use invoice::{InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeBytes};
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
    )
}

// Boundary 1: payment at exactly expires_at with no grace window → Expired.
// effective_deadline = expires_at + 0 = expires_at.
// timestamp == expires_at satisfies timestamp >= effective_deadline → rejected.
#[test]
fn test_payment_at_exact_expiry_no_grace_is_rejected() {
    let (env, admin, client) = setup();
    let payer = Address::generate(&env);
    // Ledger starts at t=0; expires_in=100 → expires_at=100.
    let id = create_invoice_expiring_in(&env, &client, 100);
    // No grace window set (default = 0), effective_deadline = 100.
    env.ledger().with_mut(|l| l.timestamp = 100);
    let err = client
        .try_mark_paid(&admin, &id, &payer, &MaybeBytes::None)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Expired);
    // Invoice must remain Pending — mark_paid must not mutate on rejection.
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);
}

// Boundary 2: payment at expires_at + grace - 1 → Paid.
// This is the last second still within the grace window.
// effective_deadline = 100 + 30 = 130; timestamp = 129 < 130 → accepted.
#[test]
fn test_payment_one_second_before_grace_deadline_succeeds() {
    let (env, admin, client) = setup();
    let payer = Address::generate(&env);
    let id = create_invoice_expiring_in(&env, &client, 100);
    client.set_grace_window(&admin, &30);
    env.ledger().with_mut(|l| l.timestamp = 129);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
}

// Boundary 3: payment at exactly expires_at + grace → Expired.
// effective_deadline = 100 + 30 = 130; timestamp = 130 satisfies >= 130 → rejected.
#[test]
fn test_payment_at_exact_grace_deadline_is_rejected() {
    let (env, admin, client) = setup();
    let payer = Address::generate(&env);
    let id = create_invoice_expiring_in(&env, &client, 100);
    client.set_grace_window(&admin, &30);
    env.ledger().with_mut(|l| l.timestamp = 130);
    let err = client
        .try_mark_paid(&admin, &id, &payer, &MaybeBytes::None)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Expired);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Pending);
}
