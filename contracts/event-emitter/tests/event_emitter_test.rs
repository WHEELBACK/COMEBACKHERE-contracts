use event_emitter::{EventEmitterContract, EventEmitterContractClient, EventEmitterError};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, EventEmitterContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, EventEmitterContract);
    let client = EventEmitterContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

#[test]
fn initialize_twice_returns_already_initialized() {
    let (_env, admin, client) = setup();
    let err = client.try_initialize(&admin).unwrap_err().unwrap();
    assert_eq!(err, EventEmitterError::AlreadyInitialized);
}

#[test]
fn negative_emitted_amount_is_rejected() {
    let (_env, admin, client) = setup();
    let err = client
        .try_set_emitted_amount(&admin, &7, &(-5))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, EventEmitterError::InvalidAmount);
}

#[test]
fn underflow_in_partial_settlement_computation_returns_error() {
    // #950: `settlement_total - emitted_amount` must not panic when it
    // underflows; the call returns `Err(Underflow)` and leaves state intact.
    let (_env, admin, client) = setup();
    client.set_emitted_amount(&admin, &7, &200);
    let err = client
        .try_emit_partial_settlement(&7, &100)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, EventEmitterError::Underflow);
    // State is unchanged by the rejected emit.
    assert_eq!(client.get_emitted_amount(&7), 200);
}

#[test]
fn partial_settlement_returns_unemitted_remainder() {
    let (_env, admin, client) = setup();
    client.set_emitted_amount(&admin, &7, &40);
    client.emit_partial_settlement(&7, &100); // remaining = 100 - 40 = 60; stored → 100
    assert_eq!(client.get_emitted_amount(&7), 100);
    // Emitting again with a smaller total now underflows (10 - 100).
    let err = client
        .try_emit_partial_settlement(&7, &10)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, EventEmitterError::Underflow);
}
