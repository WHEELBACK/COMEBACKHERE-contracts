use invoice::{BatchInvoiceParams, InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeBytes};
use soroban_sdk::{testutils::Address as _, vec, Address, Env, Vec};

const USDC: i128 = 10_000_000;

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

fn param(env: &Env, amount: i128, gross: i128) -> BatchInvoiceParams {
    BatchInvoiceParams {
        amount_usdc: amount,
        gross_usdc: gross,
        expires_in_seconds: 3600,
        metadata_hash: MaybeBytes::None,
        payment_link_hash: MaybeBytes::None,
        merchant_nonce: 0,
    }
}

// Happy path: N invoices created, IDs sequential, stored fields correct.
#[test]
fn test_batch_creates_invoices_and_returns_ids() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);

    let params = vec![
        &env,
        param(&env, USDC, USDC),
        param(&env, 2 * USDC, 2 * USDC),
        param(&env, 3 * USDC, 3 * USDC),
    ];
    let ids = client.batch_create_invoice(&merchant, &params);

    assert_eq!(ids.len(), 3);
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(id, (i as u64) + 1);
        let inv = client.get_invoice(&id);
        assert_eq!(inv.status, InvoiceStatus::Pending);
        assert_eq!(inv.merchant, merchant);
    }
}

// IDs pick up where any prior invoices left off.
#[test]
fn test_batch_ids_continue_from_existing_count() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);

    client.create_invoice(&merchant, &USDC, &USDC, &3600, &MaybeBytes::None, &MaybeBytes::None, &0);

    let params = vec![&env, param(&env, USDC, USDC), param(&env, USDC, USDC)];
    let ids = client.batch_create_invoice(&merchant, &params);

    assert_eq!(ids.get(0).unwrap(), 2);
    assert_eq!(ids.get(1).unwrap(), 3);
}

// A single invalid param causes the whole batch to be rejected.
#[test]
fn test_batch_rejects_on_any_invalid_amount() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);

    // Second param has zero amount.
    let params = vec![
        &env,
        param(&env, USDC, USDC),
        param(&env, 0, 0),
        param(&env, USDC, USDC),
    ];
    assert!(client.try_batch_create_invoice(&merchant, &params).is_err());
    // Nothing should have been stored.
    assert_eq!(client.get_invoice_count(), 0);
}

// Sub-USDC-factor amounts are rejected for any element.
#[test]
fn test_batch_rejects_sub_usdc_precision() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);

    let params = vec![
        &env,
        param(&env, USDC, USDC),
        param(&env, USDC - 1, USDC),
    ];
    assert!(client.try_batch_create_invoice(&merchant, &params).is_err());
    assert_eq!(client.get_invoice_count(), 0);
}

// gross < amount is rejected.
#[test]
fn test_batch_rejects_gross_less_than_amount() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);

    let params = vec![&env, param(&env, USDC, USDC - 1)];
    assert!(client.try_batch_create_invoice(&merchant, &params).is_err());
}

// Duplicate merchant nonce across the same batch is rejected.
#[test]
fn test_batch_rejects_duplicate_nonce_within_batch() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);

    let mut p1 = param(&env, USDC, USDC);
    p1.merchant_nonce = 42;
    let mut p2 = param(&env, USDC, USDC);
    p2.merchant_nonce = 42; // duplicate

    let params = vec![&env, p1, p2];
    assert!(client.try_batch_create_invoice(&merchant, &params).is_err());
    assert_eq!(client.get_invoice_count(), 0);
}

// A nonce already used by a prior create_invoice call is rejected.
#[test]
fn test_batch_rejects_nonce_already_used() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);

    client.create_invoice(&merchant, &USDC, &USDC, &3600, &MaybeBytes::None, &MaybeBytes::None, &7);

    let mut p = param(&env, USDC, USDC);
    p.merchant_nonce = 7;
    let params = vec![&env, p];
    assert!(client.try_batch_create_invoice(&merchant, &params).is_err());
}

// Paused contract rejects batch.
#[test]
fn test_batch_rejected_when_paused() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    client.pause(&admin);

    let params = vec![&env, param(&env, USDC, USDC)];
    assert!(client.try_batch_create_invoice(&merchant, &params).is_err());
}

// Amount and gross fields are stored exactly as provided.
#[test]
fn test_batch_amounts_stored_exactly() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);

    let params = vec![
        &env,
        param(&env, 5 * USDC, 6 * USDC),
        param(&env, 99 * USDC, 100 * USDC),
    ];
    let ids = client.batch_create_invoice(&merchant, &params);

    let inv0 = client.get_invoice(&ids.get(0).unwrap());
    assert_eq!(inv0.amount_usdc, 5 * USDC);
    assert_eq!(inv0.gross_usdc, 6 * USDC);

    let inv1 = client.get_invoice(&ids.get(1).unwrap());
    assert_eq!(inv1.amount_usdc, 99 * USDC);
    assert_eq!(inv1.gross_usdc, 100 * USDC);
}
