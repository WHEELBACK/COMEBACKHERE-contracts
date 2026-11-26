//! #880: batch entrypoints read and write the shared counters and the pending
//! index once per call instead of once per element. These tests pin that the
//! streamlined paths keep exactly the same observable state.

use invoice::{
    BatchInvoiceParams, InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeAddress,
    MaybeBytes,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    vec, Address, Env, Vec,
};

const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(InvoiceContract, ());
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

fn params(env: &Env, n: u32) -> Vec<BatchInvoiceParams> {
    let mut v = Vec::new(env);
    for i in 0..n {
        v.push_back(BatchInvoiceParams {
            amount_usdc: AMOUNT,
            gross_usdc: GROSS,
            expires_in_seconds: 100,
            metadata_hash: MaybeBytes::None,
            payment_link_hash: MaybeBytes::None,
            merchant_nonce: u64::from(i) + 1,
            token_address: MaybeAddress::None,
        });
    }
    v
}

#[test]
fn batch_create_updates_counters_and_pending_index_once() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let single = client.create_invoice(
        &merchant,
        &AMOUNT,
        &GROSS,
        &100,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );

    let ids = client.batch_create_invoice(&merchant, &params(&env, 3));

    assert_eq!(ids, vec![&env, single + 1, single + 2, single + 3]);
    assert_eq!(client.get_invoice_count(), single + 3);
    assert_eq!(
        client.get_pending_ids(),
        vec![&env, single, single + 1, single + 2, single + 3]
    );
    assert_eq!(
        client.get_invoices_by_merchant(&merchant, &0, &10),
        vec![&env, single, single + 1, single + 2, single + 3]
    );
}

#[test]
fn empty_batch_create_writes_nothing() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let ids = client.batch_create_invoice(&merchant, &Vec::new(&env));
    assert_eq!(ids.len(), 0);
    assert_eq!(client.get_invoice_count(), 0);
    assert_eq!(client.get_pending_ids().len(), 0);
}

#[test]
fn batch_cancel_removes_only_cancelled_ids_from_pending_index() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let ids = client.batch_create_invoice(&merchant, &params(&env, 4));

    client.batch_cancel_invoices(
        &merchant,
        &vec![&env, ids.get(1).unwrap(), ids.get(3).unwrap()],
        &MaybeBytes::None,
    );

    assert_eq!(
        client.get_pending_ids(),
        vec![&env, ids.get(0).unwrap(), ids.get(2).unwrap()]
    );
    assert_eq!(
        client.get_invoice(&ids.get(1).unwrap()).status,
        InvoiceStatus::Cancelled
    );
}

#[test]
fn batch_expire_removes_expired_ids_and_ignores_unknown_ones() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let ids = client.batch_create_invoice(&merchant, &params(&env, 3));
    env.ledger().with_mut(|l| l.timestamp += 1_000);

    let expired = client.batch_expire(
        &admin,
        &vec![&env, ids.get(0).unwrap(), 9_999, ids.get(2).unwrap()],
    );

    assert_eq!(expired, 2);
    assert_eq!(client.get_pending_ids(), vec![&env, ids.get(1).unwrap()]);
}

#[test]
fn removing_an_unindexed_id_leaves_the_index_untouched() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let ids = client.batch_create_invoice(&merchant, &params(&env, 2));
    let payer = Address::generate(&env);
    client.mark_paid(
        &admin,
        &ids.get(0).unwrap(),
        &payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    assert_eq!(client.get_pending_ids(), vec![&env, ids.get(1).unwrap()]);

    // Expiring an already-paid id is a no-op for the index.
    env.ledger().with_mut(|l| l.timestamp += 1_000);
    client.batch_expire(&admin, &vec![&env, ids.get(0).unwrap()]);
    assert_eq!(client.get_pending_ids(), vec![&env, ids.get(1).unwrap()]);
}
