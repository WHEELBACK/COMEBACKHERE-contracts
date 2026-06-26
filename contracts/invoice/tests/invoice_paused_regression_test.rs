// #20: Regression test — every state-mutating entrypoint returns ContractPaused after pause.
use invoice::{InvoiceContract, InvoiceContractClient, InvoiceError, MaybeAddress, MaybeBytes};
use soroban_sdk::{testutils::Address as _, Address, Env};

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

fn make_invoice(env: &Env, client: &InvoiceContractClient) -> u64 {
    let merchant = Address::generate(env);
    client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    )
}

#[test]
fn test_create_invoice_rejected_when_paused() {
    let (env, admin, client) = setup();
    client.pause(&admin);
    let merchant = Address::generate(&env);
    let err = client
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}

#[test]
fn test_mark_paid_rejected_when_paused() {
    let (env, admin, client) = setup();
    let id = make_invoice(&env, &client);
    let payer = Address::generate(&env);
    client.pause(&admin);
    let err = client
        .try_mark_paid(&admin, &id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}

#[test]
fn test_release_escrow_rejected_when_paused() {
    let (env, admin, client) = setup();
    let id = make_invoice(&env, &client);
    let payer = Address::generate(&env);
    client.mark_paid(&admin, &id, &payer);
    client.pause(&admin);
    let err = client
        .try_release_escrow(&admin, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}

#[test]
fn test_cancel_invoice_rejected_when_paused() {
    let (env, admin, client) = setup();
    let id = make_invoice(&env, &client);
    client.pause(&admin);
    let err = client
        .try_cancel_invoice(&admin, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}

#[test]
fn test_request_refund_rejected_when_paused() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.mark_paid(&admin, &id, &payer);
    client.pause(&admin);
    let err = client
        .try_request_refund(&payer, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}

#[test]
fn test_batch_expire_rejected_when_paused() {
    let (env, admin, client) = setup();
    let id = make_invoice(&env, &client);
    client.pause(&admin);
    let ids = soroban_sdk::vec![&env, id];
    let err = client
        .try_batch_expire(&admin, &ids)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);
}
