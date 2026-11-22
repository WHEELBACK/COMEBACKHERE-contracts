// Issue #944: Fuzzing / property-based test suite for the time-lock contract
// covering malicious caller scenarios, boundary values, and invariant checks.
//
// The workspace ships `derive_arbitrary` but not a full proptest/quickcheck
// harness; following the established pattern in contracts/invoice, we use
// parametric iteration over hand-crafted adversarial cases to exercise every
// error path without panicking the contract.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use time_lock::{LockStatus, Role, TimeLockContract, TimeLockContractClient, TimeLockError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn deploy(env: &Env) -> (TimeLockContractClient<'static>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let cid = env.register_contract(None, TimeLockContract);
    let client = TimeLockContractClient::new(env, &cid);
    client.initialize(&admin);
    (client, admin)
}

/// Advance the mocked ledger timestamp by `seconds`.
fn advance_ledger(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

// ---------------------------------------------------------------------------
// Fuzz: malicious caller — unauthorized operations
// ---------------------------------------------------------------------------

/// A stranger (no role) must never be able to create a lock regardless of
/// the amount or duration passed.
#[test]
fn fuzz_stranger_create_lock_always_rejected() {
    let adversarial_amounts: &[i128] = &[
        0,
        -1,
        i128::MIN,
        1,
        1_000_000,
        i128::MAX,
    ];
    let adversarial_durations: &[u64] = &[0, 1, 3600, u64::MAX];

    for &amount in adversarial_amounts {
        for &duration in adversarial_durations {
            let env = Env::default();
            env.mock_all_auths();
            let (client, _) = deploy(&env);
            let stranger = Address::generate(&env);
            let beneficiary = Address::generate(&env);
            let result = client.try_create_lock(&stranger, &beneficiary, &amount, &duration);
            // Stranger must always be rejected — ZeroDeposit, ZeroDuration, or InsufficientRole
            assert!(
                result.is_err(),
                "stranger must never succeed: amount={amount} duration={duration}"
            );
        }
    }
}

/// An operator must NOT be able to cancel locks (admin-only operation).
#[test]
fn fuzz_operator_cancel_always_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = deploy(&env);
    let operator = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    client.set_role(&admin, &operator, &Role::Operator);

    // Create a valid lock as admin
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &3600);

    // Operator attempts to cancel — must fail
    let result = client.try_cancel_lock(&operator, &lock_id);
    assert!(
        result.is_err(),
        "operator must not cancel a lock — only admin can"
    );
}

/// A malicious actor trying to release a lock they don't own must always get
/// `BeneficiaryMismatch` regardless of the lock_id they attempt.
#[test]
fn fuzz_impostor_release_always_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = deploy(&env);
    let real_beneficiary = Address::generate(&env);

    // Create several locks
    let mut lock_ids = soroban_sdk::Vec::new(&env);
    for _ in 0..5u32 {
        let id = client.create_lock(&admin, &real_beneficiary, &1_000_000, &1);
        lock_ids.push_back(id);
    }

    // Advance ledger so all locks are mature
    advance_ledger(&env, 100);

    for id in lock_ids.iter() {
        let impostor = Address::generate(&env);
        let result = client.try_release(&impostor, &id);
        assert_eq!(
            result,
            Err(Ok(TimeLockError::BeneficiaryMismatch)),
            "impostor must always get BeneficiaryMismatch for lock {id}"
        );
    }
}

/// A malicious caller attempting to release non-existent lock IDs must always
/// get `LockNotFound`, never a panic.
#[test]
fn fuzz_release_nonexistent_ids_never_panics() {
    let ghost_ids: &[u64] = &[0, 1, 2, 999, u64::MAX / 2, u64::MAX];

    for &id in ghost_ids {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = deploy(&env);
        let caller = Address::generate(&env);
        let result = client.try_release(&caller, &id);
        assert_eq!(
            result,
            Err(Ok(TimeLockError::LockNotFound)),
            "ghost lock_id={id} must return LockNotFound, never panic"
        );
    }
}

// ---------------------------------------------------------------------------
// Fuzz: zero-balance / boundary amount states (issue #948)
// ---------------------------------------------------------------------------

/// Every non-positive amount must return `ZeroDeposit` without panicking,
/// regardless of who the caller is or what duration is supplied.
#[test]
fn fuzz_non_positive_amounts_always_zero_deposit() {
    let bad_amounts: &[i128] = &[0, -1, -1_000_000, i128::MIN, i128::MIN + 1];
    let durations: &[u64] = &[1, 3600, 86400, u64::MAX - 1];

    for &amount in bad_amounts {
        for &duration in durations {
            let env = Env::default();
            env.mock_all_auths();
            let (client, admin) = deploy(&env);
            let beneficiary = Address::generate(&env);
            let result = client.try_create_lock(&admin, &beneficiary, &amount, &duration);
            assert_eq!(
                result,
                Err(Ok(TimeLockError::ZeroDeposit)),
                "amount={amount} duration={duration} must yield ZeroDeposit"
            );
        }
    }
}

/// Zero duration is always rejected regardless of the amount.
#[test]
fn fuzz_zero_duration_always_rejected() {
    let positive_amounts: &[i128] = &[1, 1_000_000, i128::MAX / 2, i128::MAX];
    for &amount in positive_amounts {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin) = deploy(&env);
        let beneficiary = Address::generate(&env);
        let result = client.try_create_lock(&admin, &beneficiary, &amount, &0);
        assert_eq!(
            result,
            Err(Ok(TimeLockError::ZeroDuration)),
            "zero duration with amount={amount} must yield ZeroDuration"
        );
    }
}

/// Releasing a lock before it matures must never panic — always `LockNotMatured`.
#[test]
fn fuzz_premature_release_never_panics() {
    // Various durations — all larger than any ledger advance we do here.
    let durations: &[u64] = &[1, 60, 3600, 86400, u64::MAX / 2];
    for &duration in durations {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin) = deploy(&env);
        let beneficiary = Address::generate(&env);
        // We do NOT advance the ledger, so the lock is never mature.
        let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &duration);
        let result = client.try_release(&beneficiary, &lock_id);
        assert_eq!(
            result,
            Err(Ok(TimeLockError::LockNotMatured)),
            "premature release for duration={duration} must return LockNotMatured"
        );
    }
}

// ---------------------------------------------------------------------------
// Fuzz: invariants — stored state is never corrupted
// ---------------------------------------------------------------------------

/// Invariant: a released lock always records `LockStatus::Released` and its
/// amount is exactly what was deposited (no silent truncation or mutation).
#[test]
fn prop_released_lock_amount_matches_deposited() {
    let amounts: &[i128] = &[
        1,
        1_000_000,
        10_000_000,
        999_999_999,
        i128::MAX / 4,
        i128::MAX / 2,
        i128::MAX,
    ];
    for &amount in amounts {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin) = deploy(&env);
        let beneficiary = Address::generate(&env);
        let lock_id = client.create_lock(&admin, &beneficiary, &amount, &1);
        advance_ledger(&env, 10);
        let lock = client.release(&beneficiary, &lock_id);
        assert_eq!(
            lock.amount, amount,
            "released lock amount must equal deposited amount={amount}"
        );
        assert_eq!(lock.status, LockStatus::Released);
    }
}

/// Invariant: lock IDs are monotonically increasing with no gaps, regardless
/// of intermediate failures (failed create attempts must not advance the counter).
#[test]
fn prop_lock_ids_monotonic_no_gaps_despite_failures() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = deploy(&env);
    let beneficiary = Address::generate(&env);

    let mut next_expected = 1u64;
    for i in 0u32..10 {
        // Interleave valid creates with deliberately failing ones
        if i % 3 == 0 {
            // This fails — should not advance lock counter
            let _ = client.try_create_lock(&admin, &beneficiary, &0, &3600);
        } else {
            let id = client.create_lock(&admin, &beneficiary, &1_000_000, &3600);
            assert_eq!(
                id, next_expected,
                "expected lock id={next_expected} but got {id}"
            );
            next_expected += 1;
        }
    }
}

/// Invariant: a cancelled lock is never releasable, even after the ledger
/// advances well past the original release_at timestamp.
#[test]
fn prop_cancelled_lock_never_releasable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = deploy(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &3600);
    client.cancel_lock(&admin, &lock_id);
    // Fast-forward far past release_at
    advance_ledger(&env, 999_999);
    let result = client.try_release(&beneficiary, &lock_id);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::AlreadyReleased)),
        "cancelled lock must never become releasable"
    );
}

/// Invariant: pausing and unpausing the contract restores full functionality.
#[test]
fn prop_unpause_restores_operations() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = deploy(&env);
    let beneficiary = Address::generate(&env);
    client.pause(&admin);
    // Create must fail while paused
    let result = client.try_create_lock(&admin, &beneficiary, &1_000_000, &1);
    assert_eq!(result, Err(Ok(TimeLockError::ContractPaused)));
    // Unpause and retry
    client.unpause(&admin);
    let id = client.create_lock(&admin, &beneficiary, &1_000_000, &1);
    assert_eq!(id, 1u64, "first lock after unpause must have id=1");
}

/// Invariant: get_lock always reflects the latest status after each operation.
#[test]
fn prop_get_lock_reflects_latest_status() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = deploy(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &1);

    let lock = client.get_lock(&lock_id);
    assert_eq!(lock.status, LockStatus::Active, "newly created lock is Active");

    advance_ledger(&env, 100);
    client.release(&beneficiary, &lock_id);

    let lock = client.get_lock(&lock_id);
    assert_eq!(
        lock.status,
        LockStatus::Released,
        "get_lock must show Released after release"
    );
}
