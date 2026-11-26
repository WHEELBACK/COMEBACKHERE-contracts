//! Integration tests for the `storage-layout` Soroban contract.
//!
//! These tests exercise three concerns:
//!   * #961 — host memory / CPU instruction metering.
//!   * #963 — strict invariant verification (typed errors, no state mutation).
//!   * #966 — zero-balance edge cases must not panic contract execution.

use soroban_sdk::{testutils::Address as _, Address, Env};

use storage_layout::{StorageLayoutContract, StorageLayoutContractClient, StorageLayoutError};

fn setup<'a>(env: &'a Env) -> (StorageLayoutContractClient<'a>, Address) {
    let contract_id = env.register(StorageLayoutContract, ());
    let client = StorageLayoutContractClient::new(env, &contract_id);
    client.initialize();
    let user = Address::generate(env);
    (client, user)
}

const MAX_INSTRUCTIONS_PER_CYCLE: u64 = 2_000_000;
const MAX_MEMORY_PER_CYCLE: u64 = 1_000_000;

#[test]
fn read_write_cycle_stays_within_instruction_budget() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.set_balance(&user, &1);

    let before = env.cost_estimate().budget().cpu_instruction_cost();
    let mem_before = env.cost_estimate().budget().memory_bytes_cost();

    client.adjust_balance(&user, &42);
    let balance = client.get_balance(&user);
    assert_eq!(balance, 43);

    let after = env.cost_estimate().budget().cpu_instruction_cost();
    let mem_after = env.cost_estimate().budget().memory_bytes_cost();

    let cpu_delta = after.saturating_sub(before);
    let mem_delta = mem_after.saturating_sub(mem_before);

    assert!(
        cpu_delta <= MAX_INSTRUCTIONS_PER_CYCLE,
        "read/write cycle used {cpu_delta} CPU instructions, exceeds bound {MAX_INSTRUCTIONS_PER_CYCLE}"
    );
    assert!(
        mem_delta <= MAX_MEMORY_PER_CYCLE,
        "read/write cycle used {mem_delta} memory bytes, exceeds bound {MAX_MEMORY_PER_CYCLE}"
    );
}

#[test]
fn repeated_get_balance_does_not_grow_instruction_cost_per_call() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.set_balance(&user, &7);

    let _ = client.get_balance(&user);
    let start = env.cost_estimate().budget().cpu_instruction_cost();
    let _ = client.get_balance(&user);
    let single_read_cost = env
        .cost_estimate()
        .budget()
        .cpu_instruction_cost()
        .saturating_sub(start);

    let start2 = env.cost_estimate().budget().cpu_instruction_cost();
    let _ = client.get_balance(&user);
    let second_read_cost = env
        .cost_estimate()
        .budget()
        .cpu_instruction_cost()
        .saturating_sub(start2);

    assert!(
        second_read_cost <= single_read_cost.saturating_mul(2).max(1),
        "get_balance cost grew between calls: {single_read_cost} -> {second_read_cost}"
    );
}

#[test]
fn negative_balance_and_underflow_are_rejected_without_state_change() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.set_balance(&user, &100);
    let counter_before = client.get_counter();

    assert_eq!(
        client.try_set_balance(&user, &-1),
        Err(Ok(StorageLayoutError::InvalidAmount))
    );
    assert_eq!(
        client.try_adjust_balance(&user, &-101),
        Err(Ok(StorageLayoutError::InvalidAmount))
    );
    assert_eq!(client.get_balance(&user), 100);
    assert_eq!(client.get_counter(), counter_before);
}

#[test]
fn counter_is_strictly_monotonic_across_mutations() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    assert_eq!(client.get_counter(), 0);
    client.set_balance(&user, &10);
    assert_eq!(client.get_counter(), 1);
    client.adjust_balance(&user, &5);
    assert_eq!(client.get_counter(), 2);
    // Zero delta is a no-op and must not bump the counter.
    assert_eq!(client.adjust_balance(&user, &0), 15);
    assert_eq!(client.get_counter(), 2);
    client.clear_balance(&user);
    assert_eq!(client.get_counter(), 3);
    // Clearing absent balance is a no-op.
    client.clear_balance(&user);
    assert_eq!(client.get_counter(), 3);
}

#[test]
fn zero_balance_and_missing_account_edge_cases_do_not_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    assert_eq!(
        client.try_get_balance(&user),
        Err(Ok(StorageLayoutError::BalanceNotFound))
    );
    assert_eq!(
        client.try_adjust_balance(&user, &0),
        Err(Ok(StorageLayoutError::BalanceNotFound))
    );

    client.set_balance(&user, &0);
    assert_eq!(client.get_balance(&user), 0);
    assert_eq!(client.adjust_balance(&user, &0), 0);
    assert_eq!(
        client.try_adjust_balance(&user, &-1),
        Err(Ok(StorageLayoutError::InvalidAmount))
    );

    client.adjust_balance(&user, &50);
    assert_eq!(client.adjust_balance(&user, &-50), 0);
    assert_eq!(client.get_balance(&user), 0);
}

#[test]
fn overflow_protection_on_i128_max() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.set_balance(&user, &i128::MAX);
    assert_eq!(
        client.try_adjust_balance(&user, &1),
        Err(Ok(StorageLayoutError::InvalidAmount))
    );
    assert_eq!(client.get_balance(&user), i128::MAX);
}
