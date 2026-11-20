I need to understand the contract's API to write accurate tests. Since I only have the test file to create, I'll write tests based on the described contract interface (`get_balance`, `deposit`, `withdraw`, `refund`, `StorageLayoutError` with variants like `ZeroAmount`, `InvalidAmount`, `NegativeBalance`, `NonMonotonicCounter`, `KeyNamespaceMismatch`).

```rust
//! Integration tests for the `storage-layout` Soroban contract.
//!
//! These tests exercise three concerns:
//!   * #961 — host memory / CPU instruction metering and ledger footprint.
//!   * #963 — strict invariant verification (typed errors, no state mutation).
//!   * #966 — zero-balance edge cases must not panic contract execution.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, IntoVal, Symbol,
};

use storage_layout::{StorageLayoutContract, StorageLayoutContractClient, StorageLayoutError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Registers the contract and returns a client plus a funded test account.
fn setup<'a>(env: &'a Env) -> (StorageLayoutContractClient<'a>, Address) {
    let contract_id = env.register_contract(None, StorageLayoutContract);
    let client = StorageLayoutContractClient::new(env, &contract_id);
    let user = Address::generate(env);
    (client, user)
}

/// Documented upper bound (in CPU instructions) for a single read/write cycle.
///
/// This bound is intentionally generous but tight enough to catch regressions
/// in storage deserialization (see #961). If the contract legitimately needs
/// more instructions, update this constant *and* the rationale in the PR.
const MAX_INSTRUCTIONS_PER_CYCLE: u64 = 2_000_000;

/// Documented upper bound (in memory bytes) for a single read/write cycle.
const MAX_MEMORY_PER_CYCLE: u64 = 1_000_000;

// ---------------------------------------------------------------------------
// #961 — metering and footprint
// ---------------------------------------------------------------------------

#[test]
fn read_write_cycle_stays_within_instruction_budget() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    // Warm up so that one-time contract instantiation costs are excluded.
    client.deposit(&user, &1);

    let before = env.budget().cpu_instruction_cost();
    let mem_before = env.budget().memory_bytes_cost();

    // A single read/write cycle: write then read back.
    client.deposit(&user, &42);
    let balance = client.get_balance(&user);
    assert_eq!(balance, 43);

    let after = env.budget().cpu_instruction_cost();
    let mem_after = env.budget().memory_bytes_cost();

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
fn repeated_get_balance_does_not_grow_footprint() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.deposit(&user, &100);

    // First read establishes the footprint.
    let _ = client.get_balance(&user);
    let footprint_after_first = env.ledger().get().footprint_len();

    // Many repeated reads must not add ledger entries.
    for _ in 0..25 {
        let balance = client.get_balance(&user);
        assert_eq!(balance, 100);
    }

    let footprint_after_many = env.ledger().get().footprint_len();
    assert_eq!(
        footprint_after_first, footprint_after_many,
        "repeated get_balance calls grew the ledger footprint"
    );
}

#[test]
fn repeated_get_balance_does_not_grow_instruction_cost_per_call() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.deposit(&user, &7);

    // Measure the cost of a single read after warm-up.
    let _ = client.get_balance(&user);
    let start = env.budget().cpu_instruction_cost();
    let _ = client.get_balance(&user);
    let single_read_cost = env.budget().cpu_instruction_cost().saturating_sub(start);

    // A second read should cost approximately the same; allow a small margin.
    let start2 = env.budget().cpu_instruction_cost();
    let _ = client.get_balance(&user);
    let second_read_cost = env.budget().cpu_instruction_cost().saturating_sub(start2);

    assert!(
        second_read_cost <= single_read_cost.saturating_mul(2),
        "get_balance cost grew between calls: {single_read_cost} -> {second_read_cost}"
    );
}

// ---------------------------------------------------------------------------
// #963 — strict invariant verification
// ---------------------------------------------------------------------------

#[test]
fn negative_balance_write_is_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.deposit(&user, &50);
    assert_eq!(client.get_balance(&user), 50);

    // Attempting to write a negative balance must fail with a typed error.
    let result = client.try_set_balance(&user, &-1);
    assert_eq!(result, Err(Ok(StorageLayoutError::NegativeBalance)));

    // State must be untouched.
    assert_eq!(client.get_balance(&user), 50);
}

#[test]
fn non_monotonic_counter_is_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.bump_counter(&user);
    client.bump_counter(&user);
    let counter_before = client.get_counter(&user);
    assert_eq!(counter_before, 2);

    // Setting the counter backwards violates monotonicity.
    let result = client.try_set_counter(&user, &1);
    assert_eq!(result, Err(Ok(StorageLayoutError::NonMonotonicCounter)));

    // State must be untouched.
    assert_eq!(client.get_counter(&user), counter_before);
}

#[test]
fn mismatched_key_namespace_is_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.deposit(&user, &10);

    // Writing under a namespace that does not match the key must be rejected.
    let result = client.try_write_namespaced(
        &user,
        &Symbol::new(&env, "wrong_namespace"),
        &123_i128,
    );
    assert_eq!(result, Err(Ok(StorageLayoutError::KeyNamespaceMismatch)));

    // The balance under the correct namespace is unchanged.
    assert_eq!(client.get_balance(&user), 10);
}

#[test]
fn invariant_holds_across_many_operations() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    // A sequence of valid operations must keep the invariant: balance >= 0
    // and counter strictly increasing.
    let mut expected_balance: i128 = 0;
    let mut expected_counter: u32 = 0;

    for i in 0..10 {
        client.deposit(&user, &(i as i128 + 1));
        expected_balance += i as i128 + 1;
        client.bump_counter(&user);
        expected_counter += 1;

        assert_eq!(client.get_balance(&user), expected_balance);
        assert_eq!(client.get_counter(&user), expected_counter);
        assert!(client.get_balance(&user) >= 0);
    }
}

// ---------------------------------------------------------------------------
// #966 — zero-balance edge cases
// ---------------------------------------------------------------------------

#[test]
fn zero_amount_deposit_returns_zero_amount_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    let result = client.try_deposit(&user, &0);
    assert_eq!(result, Err(Ok(StorageLayoutError::ZeroAmount)));

    // No state change and no panic.
    assert_eq!(client.get_balance(&user), 0);
}

#[test]
fn zero_amount_withdraw_returns_zero_amount_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.deposit(&user, &10);

    let result = client.try_withdraw(&user, &0);
    assert_eq!(result, Err(Ok(StorageLayoutError::ZeroAmount)));

    assert_eq!(client.get_balance(&user), 10);
}

#[test]
fn zero_amount_refund_returns_zero_amount_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.deposit(&user, &10);

    let result = client.try_refund(&user, &0);
    assert_eq!(result, Err(Ok(StorageLayoutError::ZeroAmount)));

    assert_eq!(client.get_balance(&user), 10);
}

#[test]
fn withdraw_more_than_balance_returns_invalid_amount_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.deposit(&user, &5);

    let result = client.try_withdraw(&user, &10);
    assert_eq!(result, Err(Ok(StorageLayoutError::InvalidAmount)));

    // Balance must be untouched; no underflow / panic.
    assert_eq!(client.get_balance(&user), 5);
}

#[test]
fn negative_amount_deposit_returns_invalid_amount_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    let result = client.try_deposit(&user, &-5);
    assert_eq!(result, Err(Ok(StorageLayoutError::InvalidAmount)));

    assert_eq!(client.get_balance(&user), 0);
}

#[test]
fn clearing_already_zero_balance_is_noop() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    // Balance starts at zero.
    assert_eq!(client.get_balance(&user), 0);

    // Clearing a zero balance must succeed without panicking and stay zero.
    let result = client.try_clear_balance(&user);
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_balance(&user), 0);

    // Clearing again is still a no-op.
    let result = client.try_clear_balance(&user);
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_balance(&user), 0);
}

#[test]
fn withdraw_full_balance_then_clear_is_noop() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    client.deposit(&user, &25);
    client.withdraw(&user, &25);
    assert_eq!(client.get_balance(&user), 0);

    // Clearing an already-zero balance after a full withdrawal is a no-op.
    let result = client.try_clear_balance(&user);
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_balance(&user), 0);
}

// ---------------------------------------------------------------------------
// Fuzz-style randomized invariant checks
// ---------------------------------------------------------------------------

#[test]
fn fuzz_random_operations_preserve_invariants() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    // Deterministic pseudo-random sequence (no external rng dependency).
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut expected_balance: i128 = 0;

    for _ in 0..200 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let amount = (seed % 1000) as i128;

        match seed % 4 {
            0 => {
                // Deposit a positive amount.
                let amt = amount.max(1);
                let res = client.try_deposit(&user, &amt);
                assert_eq!(res, Ok(Ok(())));
                expected_balance += amt;
            }
            1 => {
                // Withdraw; may fail with InvalidAmount if insufficient.
                let res = client.try_withdraw(&user, &amount);
                if amount <= expected_balance && amount > 0 {
                    assert_eq!(res, Ok(Ok(())));
                    expected_balance -= amount;
                } else if amount == 0 {
                    assert_eq!(res, Err(Ok(StorageLayoutError::ZeroAmount)));
                } else {
                    assert_eq!(res, Err(Ok(StorageLayoutError::InvalidAmount)));
                }
            }
            2 => {
                // Zero-amount deposit must always be rejected.
                let res = client.try_deposit(&user, &0);
                assert_eq!(res, Err(Ok(StorageLayoutError::ZeroAmount)));
            }
            _ => {
                // Read must never panic and must match tracked state.
                assert_eq!(client.get_balance(&user), expected_balance);
            }
        }

        // Invariant: balance is never negative.
        assert!(client.get_balance(&user) >= 0);
    }
}

// ---------------------------------------------------------------------------
// Defensive: invalid inputs never panic
// ---------------------------------------------------------------------------

#[test]
fn extreme_amounts_do_not_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, user) = setup(&env);

    // i128::MAX deposit should either succeed or return a typed error, never panic.
    let res = client.try_deposit(&user, &i128::MAX);
    match res {
        Ok(Ok(())) => assert_eq!(client.get_balance(&user), i128::MAX),
        Err(Ok(StorageLayoutError::InvalidAmount)) => {
            assert_eq!(client.get_balance(&user), 0)
        }
        other => panic!("unexpected result for i128::MAX deposit: {other:?}"),
    }

    // i128::MIN is negative and must be rejected as InvalidAmount.
    let res = client.try_deposit(&user, &i128::MIN);
    assert_eq!(res, Err(Ok(StorageLayoutError::InvalidAmount)));
}