use invoice::{
    BatchInvoiceParams, InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

extern crate std;

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, admin, client)
}

fn make_invoice(_env: &Env, client: &InvoiceContractClient, merchant: &Address) -> u64 {
    client.create_invoice(merchant, &10_000_000, &10_250_000, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None)
}

// --- add behaviour ---

#[test]
fn test_create_invoice_adds_to_pending_index() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = make_invoice(&env, &client, &merchant);
    let pending = client.get_pending_ids();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending.get(0).unwrap(), id);
}

#[test]
fn test_batch_create_adds_all_to_pending_index() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let mut params = Vec::new(&env);
    for _ in 0..3u32 {
        params.push_back(BatchInvoiceParams {
            amount_usdc: 10_000_000,
            gross_usdc: 10_250_000,
            expires_in_seconds: 3600,
            metadata_hash: MaybeBytes::None,
            payment_link_hash: MaybeBytes::None,
            merchant_nonce: 0,
            token_address: MaybeAddress::None,
        });
    }
    let ids = client.batch_create_invoice(&merchant, &params);
    let pending = client.get_pending_ids();
    assert_eq!(pending.len(), ids.len());
}

// --- remove behaviour ---

#[test]
fn test_mark_paid_removes_from_pending_index() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = make_invoice(&env, &client, &merchant);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    let pending = client.get_pending_ids();
    assert_eq!(pending.len(), 0);
}

#[test]
fn test_cancel_invoice_removes_from_pending_index() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = make_invoice(&env, &client, &merchant);
    client.cancel_invoice(&merchant, &id);
    let pending = client.get_pending_ids();
    assert_eq!(pending.len(), 0);
}

#[test]
fn test_batch_expire_removes_expired_ids_from_pending_index() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = make_invoice(&env, &client, &merchant);

    // advance past expiry
    env.ledger().with_mut(|l| l.timestamp += 7200);

    let mut ids = Vec::new(&env);
    ids.push_back(id);
    client.batch_expire(&admin, &ids);

    let pending = client.get_pending_ids();
    assert_eq!(pending.len(), 0);
}

#[test]
fn test_batch_expire_leaves_unexpired_ids_in_index() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let id_soon = make_invoice(&env, &client, &merchant);   // expires in 3600s
    // Create a long-lived invoice
    let id_long = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &86400,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );

    // advance past the first invoice's expiry only
    env.ledger().with_mut(|l| l.timestamp += 7200);

    let mut ids = Vec::new(&env);
    ids.push_back(id_soon);
    ids.push_back(id_long);
    client.batch_expire(&admin, &ids);

    let pending = client.get_pending_ids();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending.get(0).unwrap(), id_long);
}

// --- get_pending_ids empty state ---

#[test]
fn test_get_pending_ids_empty_on_init() {
    let (_env, _admin, client) = setup();
    let pending = client.get_pending_ids();
    assert_eq!(pending.len(), 0);
}

// --- regression: existing operations unaffected ---

#[test]
fn test_release_escrow_does_not_affect_pending_index() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = make_invoice(&env, &client, &merchant);

    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    // index already empty after mark_paid
    assert_eq!(client.get_pending_ids().len(), 0);

    client.release_escrow(&admin, &id);
    // still empty, no double-remove panic
    assert_eq!(client.get_pending_ids().len(), 0);
}

#[test]
fn test_multiple_invoices_partial_removal() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    let id1 = make_invoice(&env, &client, &merchant);
    let id2 = make_invoice(&env, &client, &merchant);
    let id3 = make_invoice(&env, &client, &merchant);

    // pay id2, cancel id1 — id3 stays pending
    client.mark_paid(&admin, &id2, &payer, &MaybeBytes::None, &MaybeAddress::None);
    client.cancel_invoice(&merchant, &id1);

    let pending = client.get_pending_ids();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending.get(0).unwrap(), id3);
}
