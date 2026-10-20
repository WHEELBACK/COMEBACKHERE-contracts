//! Boundary tests for the bulk-operation cooldown (`BULK_OP_COOLDOWN_SECS`, #754).
//!
//! Exercises the exact boundary: `BULK_OP_COOLDOWN_SECS - 1` (rejected) and
//! `BULK_OP_COOLDOWN_SECS` (accepted) after a bulk operation, and asserts that the
//! allow and block cooldowns are tracked independently.

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

use compliance::{ComplianceContract, ComplianceContractClient, ContractError, BULK_OP_COOLDOWN_SECS};

fn setup(env: &Env) -> (ComplianceContractClient<'_>, Address) {
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(env, &contract_id);
    client.initialize(&admin);
    (client, admin)
}

fn addresses(env: &Env, count: u32) -> Vec<Address> {
    let mut out = Vec::new(env);
    for _ in 0..count {
        out.push_back(Address::generate(env));
    }
    out
}

fn advance(env: &Env, secs: u64) {
    env.ledger().with_mut(|l| l.timestamp += secs);
}

#[test]
fn bulk_allow_cooldown_boundary() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    client.bulk_allow_addresses(&admin, &addresses(&env, 2));

    // cooldown - 1: still within the cooldown window, must be rejected.
    advance(&env, BULK_OP_COOLDOWN_SECS - 1);
    assert_eq!(
        client.try_bulk_allow_addresses(&admin, &addresses(&env, 2)),
        Err(Ok(ContractError::BulkOperationCooldown))
    );

    // exactly cooldown: window has elapsed, must be accepted.
    advance(&env, 1);
    client.bulk_allow_addresses(&admin, &addresses(&env, 2));

    // well past the window, must be accepted.
    advance(&env, BULK_OP_COOLDOWN_SECS + 1);
    client.bulk_allow_addresses(&admin, &addresses(&env, 2));
}

#[test]
fn bulk_block_cooldown_boundary() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    client.bulk_block_addresses(&admin, &addresses(&env, 2));

    advance(&env, BULK_OP_COOLDOWN_SECS - 1);
    assert_eq!(
        client.try_bulk_block_addresses(&admin, &addresses(&env, 2)),
        Err(Ok(ContractError::BulkOperationCooldown))
    );

    advance(&env, 1);
    client.bulk_block_addresses(&admin, &addresses(&env, 2));

    advance(&env, BULK_OP_COOLDOWN_SECS + 1);
    client.bulk_block_addresses(&admin, &addresses(&env, 2));
}

#[test]
fn bulk_allow_and_block_cooldowns_are_independent() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    client.bulk_allow_addresses(&admin, &addresses(&env, 2));
    // A bulk block right after a bulk allow is not affected by the allow cooldown.
    client.bulk_block_addresses(&admin, &addresses(&env, 2));

    advance(&env, BULK_OP_COOLDOWN_SECS - 1);
    assert_eq!(
        client.try_bulk_allow_addresses(&admin, &addresses(&env, 2)),
        Err(Ok(ContractError::BulkOperationCooldown))
    );
    assert_eq!(
        client.try_bulk_block_addresses(&admin, &addresses(&env, 2)),
        Err(Ok(ContractError::BulkOperationCooldown))
    );
}
