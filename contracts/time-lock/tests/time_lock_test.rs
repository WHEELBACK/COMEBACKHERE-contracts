//! Unit and fuzz-style tests for the time-lock contract.
//!
//! The primary focus is the arithmetic-underflow fix introduced for issue #941:
//! every test that exercises `release` with boundary or adversarial amounts
//! asserts that `ArithmeticUnderflow` / `InsufficientDeposit` is returned
//! rather than a silent wrap-around.

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use time_lock::{LockStatus, TimeLockContract, TimeLockContractClient, TimeLockError};

// ── Test helpers ─────────────────────────────────────────────────────────────

fn setup(env: &Env) -> (TimeLockContractClient, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register(TimeLockContract, ());
    let client = TimeLockContractClient::new(env, &contract_id);
    client.initialize(&admin);
    (client, admin)
}

/// Advance the ledger timestamp by `delta` seconds.
fn advance(env: &Env, delta: u64) {
    env.ledger().with_mut(|li| li.timestamp += delta);
}

// ── Initialization ────────────────────────────────────────────────────────────

#[test]
fn double_initialize_returns_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(TimeLockError::AlreadyInitialized)));
}

// ── Happy-path: create and fully release ─────────────────────────────────────

#[test]
fn create_and_fully_release_lock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let amount = 1_000_000i128;
    let unlock_at = env.ledger().timestamp() + 100;

    let lock_id = client.create_lock(&depositor, &beneficiary, &amount, &unlock_at);

    // Advance past the unlock time.
    advance(&env, 101);

    let remaining = client.release(&beneficiary, &lock_id, &amount);
    assert_eq!(remaining, 0);

    let lock = client.get_lock(&lock_id);
    assert_eq!(lock.status, LockStatus::Released);
    assert_eq!(lock.released_total, amount);
}

// ── Partial release sequence ──────────────────────────────────────────────────

#[test]
fn partial_release_sequence_accumulates_correctly() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let total = 9_000_000i128;
    let unlock_at = env.ledger().timestamp() + 50;

    let lock_id = client.create_lock(&depositor, &beneficiary, &total, &unlock_at);

    advance(&env, 51);

    // Three sequential partial releases.
    let r1 = client.release(&beneficiary, &lock_id, &3_000_000);
    assert_eq!(r1, 6_000_000);

    let r2 = client.release(&beneficiary, &lock_id, &3_000_000);
    assert_eq!(r2, 3_000_000);

    let r3 = client.release(&beneficiary, &lock_id, &3_000_000);
    assert_eq!(r3, 0);

    let lock = client.get_lock(&lock_id);
    assert_eq!(lock.status, LockStatus::Released);
    assert_eq!(lock.released_total, total);
}

// ── Underflow guard (issue #941) ──────────────────────────────────────────────

/// Releasing more than the total deposited must return `InsufficientDeposit`,
/// not a silent wrap-around.
#[test]
fn release_exceeding_deposited_returns_insufficient_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let amount = 500_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &amount, &unlock_at);

    advance(&env, 11);

    let result = client.try_release(&beneficiary, &lock_id, &(amount + 1));
    assert_eq!(result, Err(Ok(TimeLockError::InsufficientDeposit)));
}

/// After a partial release, releasing an amount that would push `released_total`
/// past `deposited` must also return `InsufficientDeposit`.
#[test]
fn release_after_partial_exceeding_remaining_returns_insufficient_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let total = 1_000_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &total, &unlock_at);

    advance(&env, 11);

    // Release half.
    client.release(&beneficiary, &lock_id, &500_000);

    // Attempt to release more than the remaining 500_000.
    let result = client.try_release(&beneficiary, &lock_id, &500_001);
    assert_eq!(result, Err(Ok(TimeLockError::InsufficientDeposit)));
}

/// Releasing exactly the remaining amount after a partial release must succeed
/// (boundary condition).
#[test]
fn release_exactly_remaining_after_partial_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let total = 1_000_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &total, &unlock_at);

    advance(&env, 11);

    client.release(&beneficiary, &lock_id, &600_000);

    let remaining = client.release(&beneficiary, &lock_id, &400_000);
    assert_eq!(remaining, 0);

    let lock = client.get_lock(&lock_id);
    assert_eq!(lock.status, LockStatus::Released);
}

// ── Precondition guards ───────────────────────────────────────────────────────

#[test]
fn release_before_maturity_returns_lock_not_matured() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unlock_at = env.ledger().timestamp() + 1_000;

    let lock_id = client.create_lock(&depositor, &beneficiary, &100_000, &unlock_at);

    // Do NOT advance the ledger — unlock_at is still in the future.
    let result = client.try_release(&beneficiary, &lock_id, &100_000);
    assert_eq!(result, Err(Ok(TimeLockError::LockNotMatured)));
}

#[test]
fn release_on_already_released_lock_returns_lock_already_settled() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let amount = 100_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &amount, &unlock_at);

    advance(&env, 11);
    client.release(&beneficiary, &lock_id, &amount);

    let result = client.try_release(&beneficiary, &lock_id, &1);
    assert_eq!(result, Err(Ok(TimeLockError::LockAlreadySettled)));
}

#[test]
fn release_zero_amount_returns_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &100_000, &unlock_at);

    advance(&env, 11);

    let result = client.try_release(&beneficiary, &lock_id, &0);
    assert_eq!(result, Err(Ok(TimeLockError::InvalidAmount)));
}

#[test]
fn create_lock_with_zero_amount_returns_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unlock_at = env.ledger().timestamp() + 10;

    let result = client.try_create_lock(&depositor, &beneficiary, &0, &unlock_at);
    assert_eq!(result, Err(Ok(TimeLockError::InvalidAmount)));
}

#[test]
fn create_lock_with_unlock_in_past_returns_unlock_in_past() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    // Timestamp 0 is never in the future, so pass the current ledger timestamp
    // which equals unlock_at (not strictly greater).
    let unlock_at = env.ledger().timestamp();

    let result = client.try_create_lock(&depositor, &beneficiary, &100_000, &unlock_at);
    assert_eq!(result, Err(Ok(TimeLockError::UnlockInPast)));
}

// ── Cancel ────────────────────────────────────────────────────────────────────

#[test]
fn admin_can_cancel_active_lock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unlock_at = env.ledger().timestamp() + 100;

    let lock_id = client.create_lock(&depositor, &beneficiary, &100_000, &unlock_at);

    client.cancel_lock(&admin, &lock_id);

    let lock = client.get_lock(&lock_id);
    assert_eq!(lock.status, LockStatus::Cancelled);
}

#[test]
fn cancel_already_released_lock_returns_lock_already_settled() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let amount = 100_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &amount, &unlock_at);

    advance(&env, 11);
    client.release(&beneficiary, &lock_id, &amount);

    let result = client.try_cancel_lock(&admin, &lock_id);
    assert_eq!(result, Err(Ok(TimeLockError::LockAlreadySettled)));
}

#[test]
fn release_on_cancelled_lock_returns_lock_already_settled() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let amount = 100_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &amount, &unlock_at);

    client.cancel_lock(&admin, &lock_id);

    advance(&env, 11);
    let result = client.try_release(&beneficiary, &lock_id, &amount);
    assert_eq!(result, Err(Ok(TimeLockError::LockAlreadySettled)));
}

// ── Pause / unpause ───────────────────────────────────────────────────────────

#[test]
fn paused_contract_rejects_create_lock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    client.pause(&admin);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unlock_at = env.ledger().timestamp() + 10;

    let result = client.try_create_lock(&depositor, &beneficiary, &100_000, &unlock_at);
    assert_eq!(result, Err(Ok(TimeLockError::ContractPaused)));
}

#[test]
fn paused_contract_rejects_release() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let amount = 100_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &amount, &unlock_at);

    advance(&env, 11);
    client.pause(&admin);

    let result = client.try_release(&beneficiary, &lock_id, &amount);
    assert_eq!(result, Err(Ok(TimeLockError::ContractPaused)));
}

#[test]
fn unpausing_allows_operations_to_resume() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let amount = 100_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &amount, &unlock_at);

    advance(&env, 11);
    client.pause(&admin);
    client.unpause(&admin);

    let remaining = client.release(&beneficiary, &lock_id, &amount);
    assert_eq!(remaining, 0);
}

// ── View helpers ──────────────────────────────────────────────────────────────

#[test]
fn remaining_balance_matches_manual_calculation() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let total = 10_000_000i128;
    let partial = 3_000_000i128;
    let unlock_at = env.ledger().timestamp() + 10;

    let lock_id = client.create_lock(&depositor, &beneficiary, &total, &unlock_at);

    advance(&env, 11);
    client.release(&beneficiary, &lock_id, &partial);

    let remaining = client.remaining_balance(&lock_id);
    assert_eq!(remaining, total - partial);
}

#[test]
fn is_matured_returns_false_before_unlock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unlock_at = env.ledger().timestamp() + 100;

    let lock_id = client.create_lock(&depositor, &beneficiary, &100_000, &unlock_at);

    assert!(!client.is_matured(&lock_id));
}

#[test]
fn is_matured_returns_true_after_unlock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unlock_at = env.ledger().timestamp() + 100;

    let lock_id = client.create_lock(&depositor, &beneficiary, &100_000, &unlock_at);

    advance(&env, 100);
    assert!(client.is_matured(&lock_id));
}

#[test]
fn lock_count_increments_on_each_create() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let unlock_at = env.ledger().timestamp() + 10;

    assert_eq!(client.lock_count(), 0);
    client.create_lock(&depositor, &beneficiary, &100_000, &unlock_at);
    assert_eq!(client.lock_count(), 1);
    client.create_lock(&depositor, &beneficiary, &100_000, &unlock_at);
    assert_eq!(client.lock_count(), 2);
}

// ── Fuzz-style: boundary amounts ─────────────────────────────────────────────

/// Release amount of exactly 1 on a large deposit — minimum valid partial release.
#[test]
fn minimum_partial_release_of_one_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let total = i128::MAX;
    let unlock_at = env.ledger().timestamp() + 1;

    let lock_id = client.create_lock(&depositor, &beneficiary, &total, &unlock_at);

    advance(&env, 1);

    let remaining = client.release(&beneficiary, &lock_id, &1);
    assert_eq!(remaining, total - 1);
}

/// Releasing i128::MAX from a deposit of i128::MAX must succeed without overflow.
#[test]
fn full_release_of_i128_max_deposit_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let total = i128::MAX;
    let unlock_at = env.ledger().timestamp() + 1;

    let lock_id = client.create_lock(&depositor, &beneficiary, &total, &unlock_at);

    advance(&env, 1);

    let remaining = client.release(&beneficiary, &lock_id, &total);
    assert_eq!(remaining, 0);
}

/// Releasing `deposited + 1` for various deposit sizes always returns
/// `InsufficientDeposit`.
#[test]
fn release_one_over_deposited_always_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);

    for &total in &[1i128, 1_000i128, 1_000_000i128, i128::MAX - 1] {
        let unlock_at = env.ledger().timestamp() + 1;
        let lock_id = client.create_lock(&depositor, &beneficiary, &total, &unlock_at);
        advance(&env, 1);
        let result = client.try_release(&beneficiary, &lock_id, &(total + 1));
        assert_eq!(
            result,
            Err(Ok(TimeLockError::InsufficientDeposit)),
            "total={total}"
        );
    }
}

/// Multiple small releases summing to `deposited - 1` leave exactly 1 remaining.
#[test]
fn many_partial_releases_leave_correct_remainder() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let total = 1_000i128;
    let unlock_at = env.ledger().timestamp() + 1;

    let lock_id = client.create_lock(&depositor, &beneficiary, &total, &unlock_at);

    advance(&env, 1);

    // Release 1 at a time, 999 times.
    for i in 0..999 {
        let remaining = client.release(&beneficiary, &lock_id, &1);
        assert_eq!(remaining, total - (i as i128) - 1);
    }

    // One token remains.
    let rem = client.remaining_balance(&lock_id);
    assert_eq!(rem, 1);
}
