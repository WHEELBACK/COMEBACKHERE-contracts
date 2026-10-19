// Issue #704: late-fee changes must be visible to indexers.
use invoice::{InvoiceContract, InvoiceContractClient, InvoiceError};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, FromVal, Symbol, TryFromVal,
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

/// Returns the topic-0 symbol of the most recently emitted event.
fn last_event_symbol(env: &Env) -> String {
    let events = env.events().all();
    let last = events.last().expect("at least one event must be emitted");
    Symbol::from_val(env, &last.1.get_unchecked(0)).to_string()
}

/// Reads the `(old_bps, new_bps)` payload of the most recent event.
fn last_event_bps(env: &Env) -> (u32, u32) {
    let events = env.events().all();
    let last = events.last().expect("at least one event must be emitted");
    <(u32, u32)>::try_from_val(env, &last.2).expect("data must be an (old_bps, new_bps) tuple")
}

#[test]
fn late_fee_defaults_to_zero() {
    let (_env, _admin, client) = setup();

    assert_eq!(client.get_late_fee_bps(), 0);
}

#[test]
fn set_late_fee_bps_stores_and_returns_the_fee() {
    let (_env, admin, client) = setup();

    client.set_late_fee_bps(&admin, &250);

    assert_eq!(client.get_late_fee_bps(), 250);
}

#[test]
fn set_late_fee_bps_emits_old_and_new_bps() {
    let (env, admin, client) = setup();

    client.set_late_fee_bps(&admin, &250);

    assert_eq!(last_event_symbol(&env), "late_fee_updated");
    assert_eq!(last_event_bps(&env), (0, 250));

    client.set_late_fee_bps(&admin, &500);
    assert_eq!(last_event_bps(&env), (250, 500));
}

#[test]
fn late_fee_can_be_reset_to_zero() {
    let (env, admin, client) = setup();
    client.set_late_fee_bps(&admin, &250);

    client.set_late_fee_bps(&admin, &0);

    assert_eq!(client.get_late_fee_bps(), 0);
    assert_eq!(last_event_bps(&env), (250, 0));
}

#[test]
fn set_late_fee_bps_requires_admin() {
    let (env, _admin, client) = setup();
    let stranger = Address::generate(&env);

    let err = client
        .try_set_late_fee_bps(&stranger, &250)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
    assert_eq!(client.get_late_fee_bps(), 0);
}

#[test]
fn set_late_fee_bps_rejects_values_above_one_hundred_percent() {
    let (_env, admin, client) = setup();

    let err = client
        .try_set_late_fee_bps(&admin, &10_001)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::InvalidLateFee);
    assert_eq!(client.get_late_fee_bps(), 0);

    // 10_000 bps = 100% is still accepted.
    client.set_late_fee_bps(&admin, &10_000);
    assert_eq!(client.get_late_fee_bps(), 10_000);
}
