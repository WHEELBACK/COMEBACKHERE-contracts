// #475: Explicit, dedicated tests for MerchantNonce idempotency —
// the core guarantee that two create_invoice calls from the same merchant
// carrying the same non-zero nonce cannot result in two separate invoices.
//
// Coverage here is intentionally narrow: each test documents one facet of
// the nonce deduplication invariant in isolation, so failures point directly
// at the property that broke rather than surfacing as a side-effect of a
// broader scenario (such as the cancellation-specific edge case in #58).

use invoice::{InvoiceContract, InvoiceContractClient, InvoiceError, MaybeAddress, MaybeBytes};
use soroban_sdk::{testutils::Address as _, Address, Env};

extern crate std;

const USDC: i128 = 10_000_000;

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, admin, client)
}

fn create(client: &InvoiceContractClient, merchant: &Address, nonce: u64) -> u64 {
    client.create_invoice(
        merchant,
        &USDC,
        &USDC,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &nonce,
        &MaybeAddress::None,
    )
}

/// Attempt a create_invoice call and return the contract error.
/// Panics if the call unexpectedly succeeds.
fn expect_err(client: &InvoiceContractClient, merchant: &Address, nonce: u64) -> InvoiceError {
    client
        .try_create_invoice(
            merchant,
            &USDC,
            &USDC,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &nonce,
            &MaybeAddress::None,
        )
        .unwrap_err()
        .unwrap()
}

// --- Core property ---------------------------------------------------------

/// The fundamental guarantee: a second create_invoice call from the same
/// merchant with the same non-zero nonce is rejected with DuplicateNonce.
/// This is the foundational invariant that prevents duplicate invoice creation
/// across concurrent-looking calls.
#[test]
fn test_duplicate_nonce_same_merchant_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);

    create(&client, &merchant, 1);
    let err = expect_err(&client, &merchant, 1);
    assert_eq!(err, InvoiceError::DuplicateNonce);
}

/// The original invoice survives the duplicate attempt — the first call's
/// result is intact and fetchable after the second call is rejected.
#[test]
fn test_original_invoice_survives_duplicate_attempt() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);

    let id = create(&client, &merchant, 99);
    let _ = expect_err(&client, &merchant, 99);

    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.id, id);
    assert_eq!(invoice.merchant, merchant);
    assert_eq!(invoice.merchant_nonce, 99);
}

/// The duplicate attempt must not create a second invoice — the total invoice
/// count must remain 1, not 2, after the rejected call.
#[test]
fn test_duplicate_nonce_does_not_increment_invoice_count() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);

    create(&client, &merchant, 7);
    let _ = expect_err(&client, &merchant, 7);

    assert_eq!(client.get_invoice_count(), 1);
}

// --- Nonce isolation -------------------------------------------------------

/// Nonce deduplication is scoped to (merchant, nonce) — a different merchant
/// using the same nonce value must succeed independently. Nonces are not global.
#[test]
fn test_same_nonce_different_merchants_both_succeed() {
    let (env, _admin, client) = setup();
    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);

    let id_a = create(&client, &merchant_a, 42);
    let id_b = create(&client, &merchant_b, 42);

    assert_ne!(id_a, id_b, "each merchant must get a distinct invoice ID");
    assert_eq!(client.get_invoice(&id_a).merchant, merchant_a);
    assert_eq!(client.get_invoice(&id_b).merchant, merchant_b);
}

/// Multiple distinct nonce values from the same merchant must all succeed.
/// Only an identical (merchant, nonce) pair is rejected.
#[test]
fn test_different_nonces_same_merchant_all_succeed() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);

    let id1 = create(&client, &merchant, 1);
    let id2 = create(&client, &merchant, 2);
    let id3 = create(&client, &merchant, 3);

    assert_eq!(client.get_invoice_count(), 3);
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
}

// --- Sentinel value --------------------------------------------------------

/// Nonce 0 is the "no nonce" sentinel. The contract skips deduplication
/// entirely for zero-nonce calls, so two calls with nonce=0 both produce
/// independent invoices without any DuplicateNonce error.
#[test]
fn test_zero_nonce_is_exempt_from_deduplication() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);

    let id1 = create(&client, &merchant, 0);
    let id2 = create(&client, &merchant, 0);

    assert_ne!(
        id1, id2,
        "both zero-nonce calls must produce distinct invoices"
    );
    assert_eq!(client.get_invoice_count(), 2);
}

// --- Persistence across state transitions ----------------------------------

/// The nonce record persists even after the original invoice is paid —
/// the deduplication key outlives the invoice lifecycle state.
#[test]
fn test_nonce_rejected_after_invoice_is_paid() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    let id = create(&client, &merchant, 55);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);

    let err = expect_err(&client, &merchant, 55);
    assert_eq!(err, InvoiceError::DuplicateNonce);
}
