use invoice::{
    EscrowReleasedEvent, InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    xdr::ToXdr,
    Address, Env,
};
use std::path::Path;

extern crate std;

fn setup_at(ts: u64) -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = ts);
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

fn snapshot_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
}

fn snapshot_path(event_name: &str) -> std::path::PathBuf {
    snapshot_dir().join(format!("{}.snap", event_name))
}

fn assert_snapshot(event_name: &str, hex: &str) {
    let path = snapshot_path(event_name);
    if path.exists() {
        let expected =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        assert_eq!(
            expected.trim(),
            hex,
            "XDR snapshot mismatch for {event_name}"
        );
    } else {
        std::fs::create_dir_all(snapshot_dir()).ok();
        std::fs::write(&path, hex).unwrap_or_else(|e| panic!("write {path:?}: {e}"));
        panic!("snapshot created at {path:?}; re-run to verify");
    }
}

fn to_hex(env: &Env, payload: impl ToXdr) -> String {
    let bytes = payload.to_xdr(env);
    hex::encode(bytes.into_iter().collect::<Vec<u8>>())
}

fn merchant(env: &Env) -> Address {
    Address::generate(env)
}

fn buyer(env: &Env) -> Address {
    Address::generate(env)
}

// --- invoice_created ---

#[test]
fn event_invoice_created_snapshot() {
    let (env, _admin, client) = setup_at(1000);
    let m = merchant(&env);
    let id = client.create_invoice(
        &m,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    assert_snapshot("invoice_created", &to_hex(&env, client.get_invoice(&id)));
}

// --- invoice_paid ---

#[test]
fn event_invoice_paid_snapshot() {
    let (env, admin, client) = setup_at(1000);
    let m = merchant(&env);
    let p = buyer(&env);
    let id = client.create_invoice(
        &m,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    env.ledger().with_mut(|l| l.timestamp = 2000);
    client.mark_paid(&admin, &id, &p, &MaybeBytes::None, &MaybeAddress::None);
    assert_snapshot("invoice_paid", &to_hex(&env, client.get_invoice(&id)));
}

// --- invoice_cancelled ---

#[test]
fn event_invoice_cancelled_snapshot() {
    let (env, _admin, client) = setup_at(1000);
    let m = merchant(&env);
    let id = client.create_invoice(
        &m,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    client.cancel_invoice(&m, &id);
    assert_snapshot("invoice_cancelled", &to_hex(&env, client.get_invoice(&id)));
}

// --- invoice_expired ---

#[test]
fn event_invoice_expired_snapshot() {
    let (env, admin, client) = setup_at(1000);
    let m = merchant(&env);
    let id = client.create_invoice(
        &m,
        &10_000_000,
        &10_250_000,
        &1,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    env.ledger().with_mut(|l| l.timestamp = 5000);
    let ids = soroban_sdk::vec![&env, id];
    client.batch_expire(&admin, &ids);
    assert_snapshot("invoice_expired", &to_hex(&env, client.get_invoice(&id)));
}

// --- invoice_refund_requested ---

#[test]
fn event_invoice_refund_requested_snapshot() {
    let (env, admin, client) = setup_at(1000);
    let m = merchant(&env);
    let p = buyer(&env);
    let id = client.create_invoice(
        &m,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    env.ledger().with_mut(|l| l.timestamp = 2000);
    client.mark_paid(&admin, &id, &p, &MaybeBytes::None, &MaybeAddress::None);
    client.request_refund(&p, &id);
    assert_snapshot(
        "invoice_refund_requested",
        &to_hex(&env, client.get_invoice(&id)),
    );
}

// --- escrow_released ---

#[test]
fn event_escrow_released_snapshot() {
    let (env, admin, client) = setup_at(1000);
    let m = merchant(&env);
    let p = buyer(&env);
    let id = client.create_invoice(
        &m,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    env.ledger().with_mut(|l| l.timestamp = 2000);
    client.mark_paid(&admin, &id, &p, &MaybeBytes::None, &MaybeAddress::None);
    env.ledger().with_mut(|l| l.timestamp = 3000);
    client.release_escrow(&admin, &id);
    let inv = client.get_invoice(&id);
    let payload = EscrowReleasedEvent {
        id,
        merchant: inv.merchant,
        amount_usdc: inv.amount_usdc,
        released_at: 3000,
    };
    assert_snapshot("escrow_released", &to_hex(&env, payload));
}

// --- contract_paused ---

#[test]
fn event_contract_paused_snapshot() {
    let (env, admin, client) = setup_at(1000);
    client.pause(&admin);
    assert_snapshot("contract_paused", &to_hex(&env, admin.clone()));
}

// --- contract_unpaused ---

#[test]
fn event_contract_unpaused_snapshot() {
    let (env, admin, client) = setup_at(1000);
    client.pause(&admin);
    client.unpause(&admin);
    assert_snapshot("contract_unpaused", &to_hex(&env, admin.clone()));
}
