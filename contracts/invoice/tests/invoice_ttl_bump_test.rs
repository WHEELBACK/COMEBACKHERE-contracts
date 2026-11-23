//! #884: automatic TTL bump for persistent storage keys.
//!
//! Every persistent write the invoice contract makes must leave the entry with
//! a TTL of [`INVOICE_TTL_EXTEND_TO`] ledgers, so that invoices, their audit
//! logs, the indexes that point at them and their refund records cannot be
//! evicted from the ledger while they are still in use.

use invoice::{
    DataKey, InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes,
    INVOICE_TTL_EXTEND_TO, INVOICE_TTL_THRESHOLD,
};
use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Ledger},
    Address, Env,
};

const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

fn setup() -> (Env, Address, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(InvoiceContract, ());
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, admin, contract_id, client)
}

fn ttl(env: &Env, contract_id: &Address, key: &DataKey) -> u32 {
    env.as_contract(contract_id, || env.storage().persistent().get_ttl(key))
}

fn create(client: &InvoiceContractClient, merchant: &Address, nonce: u64) -> u64 {
    client.create_invoice(
        merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &nonce,
        &MaybeAddress::None,
    )
}

#[test]
fn create_invoice_bumps_invoice_and_index_keys() {
    let (env, _admin, contract_id, client) = setup();
    let merchant = Address::generate(&env);
    let id = create(&client, &merchant, 7);

    for key in [
        DataKey::Invoice(id),
        DataKey::PendingIndex,
        DataKey::MerchantInvoiceIndex(merchant.clone(), 0),
        DataKey::MerchantInvoiceCount(merchant.clone()),
        DataKey::MerchantNonce(merchant.clone(), 7),
    ] {
        assert_eq!(ttl(&env, &contract_id, &key), INVOICE_TTL_EXTEND_TO);
    }
}

#[test]
fn mark_paid_bumps_history_and_payer_index() {
    let (env, admin, contract_id, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create(&client, &merchant, 0);

    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);

    for key in [
        DataKey::Invoice(id),
        DataKey::InvoiceHistory(id),
        DataKey::PayerIndexEntry(payer.clone(), 0),
        DataKey::PayerIndexCount(payer.clone()),
    ] {
        assert_eq!(ttl(&env, &contract_id, &key), INVOICE_TTL_EXTEND_TO);
    }
}

#[test]
fn request_refund_bumps_refund_timestamp() {
    let (env, admin, contract_id, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create(&client, &merchant, 0);
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    client.request_refund(&payer, &id);

    assert_eq!(
        ttl(&env, &contract_id, &DataKey::RefundRequestedAt(id)),
        INVOICE_TTL_EXTEND_TO
    );
}

/// An invoice that is touched again after its TTL has decayed below the
/// threshold is bumped back to the full extension, so a long-lived invoice
/// survives as long as it keeps being used.
#[test]
fn a_decayed_invoice_is_bumped_back_on_its_next_write() {
    let (env, admin, contract_id, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = create(&client, &merchant, 0);

    // Advance far enough that the remaining TTL is below the threshold, while
    // keeping the invoice within its expiry so it can still be paid.
    // Instance storage (admin, counters) is outside this issue's scope; keep it
    // live so only the persistent entries under test decay.
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .extend_ttl(INVOICE_TTL_EXTEND_TO, INVOICE_TTL_EXTEND_TO)
    });
    let advance = INVOICE_TTL_EXTEND_TO - INVOICE_TTL_THRESHOLD + 1;
    env.ledger().with_mut(|l| l.sequence_number += advance);
    assert!(ttl(&env, &contract_id, &DataKey::Invoice(id)) < INVOICE_TTL_THRESHOLD);

    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    assert_eq!(
        ttl(&env, &contract_id, &DataKey::Invoice(id)),
        INVOICE_TTL_EXTEND_TO
    );
}

#[test]
fn cancel_invoice_bumps_invoice_and_history() {
    let (env, _admin, contract_id, client) = setup();
    let merchant = Address::generate(&env);
    let id = create(&client, &merchant, 0);
    client.cancel_invoice(&merchant, &id);

    assert_eq!(
        ttl(&env, &contract_id, &DataKey::Invoice(id)),
        INVOICE_TTL_EXTEND_TO
    );
    assert_eq!(
        ttl(&env, &contract_id, &DataKey::InvoiceHistory(id)),
        INVOICE_TTL_EXTEND_TO
    );
}
