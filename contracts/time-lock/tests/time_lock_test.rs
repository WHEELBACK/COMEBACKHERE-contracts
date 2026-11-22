use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use time_lock::{LockStatus, Role, TimeLockContract, TimeLockContractClient, TimeLockError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup(env: &Env) -> (TimeLockContractClient<'static>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let cid = env.register_contract(None, TimeLockContract);
    let client = TimeLockContractClient::new(env, &cid);
    client.initialize(&admin);
    (client, admin)
}

fn setup_with_operator(env: &Env) -> (TimeLockContractClient<'static>, Address, Address) {
    let (client, admin) = setup(env);
    let operator = Address::generate(env);
    client.set_role(&admin, &operator, &Role::Operator);
    (client, admin, operator)
}

/// Advance the mocked ledger timestamp by `seconds`.
fn advance_ledger(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| li.timestamp += seconds);
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

#[test]
fn initialize_sets_admin_and_role() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let cid = env.register_contract(None, TimeLockContract);
    let client = TimeLockContractClient::new(&env, &cid);
    client.initialize(&admin);
    assert_eq!(client.get_role(&admin), Role::Admin);
}

#[test]
fn double_initialize_returns_error() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let result = client.try_initialize(&admin);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::AlreadyInitialized)),
        "second initialize must return AlreadyInitialized"
    );
}

// ---------------------------------------------------------------------------
// Issue #942: Role-based authorization
// ---------------------------------------------------------------------------

#[test]
fn admin_can_assign_operator_role() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let op = Address::generate(&env);
    client.set_role(&admin, &op, &Role::Operator);
    assert_eq!(client.get_role(&op), Role::Operator);
}

#[test]
fn unknown_address_has_no_role() {
    let env = Env::default();
    let (client, _) = setup(&env);
    let stranger = Address::generate(&env);
    assert_eq!(client.get_role(&stranger), Role::None);
}

#[test]
fn stranger_cannot_set_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup(&env);
    let stranger = Address::generate(&env);
    let target = Address::generate(&env);
    let result = client.try_set_role(&stranger, &target, &Role::Operator);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::Unauthorized)),
        "stranger must not be able to set roles"
    );
}

#[test]
fn operator_cannot_set_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, operator) = setup_with_operator(&env);
    let target = Address::generate(&env);
    let result = client.try_set_role(&operator, &target, &Role::Operator);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::Unauthorized)),
        "operator must not be able to set roles"
    );
}

#[test]
fn admin_can_assign_admin_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let other = Address::generate(&env);
    client.set_role(&admin, &other, &Role::Admin);
    assert_eq!(client.get_role(&other), Role::Admin);
}

#[test]
fn operator_can_create_lock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, operator) = setup_with_operator(&env);
    let beneficiary = Address::generate(&env);
    let id = client.create_lock(&operator, &beneficiary, &1_000_000, &3600);
    assert_eq!(id, 1u64);
}

#[test]
fn address_without_role_cannot_create_lock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup(&env);
    let stranger = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let result = client.try_create_lock(&stranger, &beneficiary, &1_000_000, &3600);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::InsufficientRole)),
        "stranger with no role must be rejected"
    );
}

#[test]
fn non_admin_cannot_cancel_lock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, operator) = setup_with_operator(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &500_000, &3600);
    // operator is not an admin — cancel must fail
    let result = client.try_cancel_lock(&operator, &lock_id);
    assert!(
        result.is_err(),
        "operator must not be able to cancel a lock"
    );
}

#[test]
fn admin_can_cancel_active_lock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &500_000, &3600);
    let lock = client.cancel_lock(&admin, &lock_id);
    assert_eq!(lock.status, LockStatus::Cancelled);
}

#[test]
fn non_admin_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, operator) = setup_with_operator(&env);
    let result = client.try_pause(&operator);
    assert!(result.is_err(), "operator must not be able to pause");
}

#[test]
fn wrong_beneficiary_cannot_release() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let impostor = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &1);
    // Advance ledger past release_at
    advance_ledger(&env, 10);
    let result = client.try_release(&impostor, &lock_id);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::BeneficiaryMismatch)),
        "impostor must be rejected"
    );
}

// ---------------------------------------------------------------------------
// Issue #948: Zero-balance / zero-deposit edge cases — no panics
// ---------------------------------------------------------------------------

#[test]
fn zero_deposit_returns_error_not_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let result = client.try_create_lock(&admin, &beneficiary, &0, &3600);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::ZeroDeposit)),
        "zero deposit must return ZeroDeposit, not panic"
    );
}

#[test]
fn negative_amount_returns_zero_deposit_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let result = client.try_create_lock(&admin, &beneficiary, &-1, &3600);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::ZeroDeposit)),
        "negative amount must return ZeroDeposit"
    );
}

#[test]
fn zero_duration_returns_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let result = client.try_create_lock(&admin, &beneficiary, &1_000_000, &0);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::ZeroDuration)),
        "zero duration must return ZeroDuration"
    );
}

#[test]
fn release_on_nonexistent_lock_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup(&env);
    let beneficiary = Address::generate(&env);
    let result = client.try_release(&beneficiary, &9999);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::LockNotFound)),
        "missing lock must return LockNotFound, not panic"
    );
}

#[test]
fn release_before_maturity_returns_not_matured() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &7200);
    // Ledger has not advanced past release_at
    let result = client.try_release(&beneficiary, &lock_id);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::LockNotMatured)),
        "early release must return LockNotMatured"
    );
}

#[test]
fn double_release_returns_already_released() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &1);
    advance_ledger(&env, 100);
    client.release(&beneficiary, &lock_id);
    let result = client.try_release(&beneficiary, &lock_id);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::AlreadyReleased)),
        "second release must return AlreadyReleased"
    );
}

#[test]
fn release_cancelled_lock_returns_already_released() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &3600);
    client.cancel_lock(&admin, &lock_id);
    advance_ledger(&env, 7200);
    let result = client.try_release(&beneficiary, &lock_id);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::AlreadyReleased)),
        "cancelled lock must not be releasable"
    );
}

#[test]
fn paused_contract_blocks_create_lock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    client.pause(&admin);
    let result = client.try_create_lock(&admin, &beneficiary, &1_000_000, &3600);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::ContractPaused)),
        "paused contract must reject create_lock"
    );
}

#[test]
fn paused_contract_blocks_release() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000_000, &1);
    advance_ledger(&env, 100);
    client.pause(&admin);
    let result = client.try_release(&beneficiary, &lock_id);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::ContractPaused)),
        "paused contract must reject release"
    );
}

#[test]
fn successful_release_after_maturity() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &10_000_000, &3600);
    advance_ledger(&env, 3601);
    let lock = client.release(&beneficiary, &lock_id);
    assert_eq!(lock.status, LockStatus::Released);
    assert_eq!(lock.amount, 10_000_000);
    assert_eq!(lock.beneficiary, beneficiary);
}

#[test]
fn lock_ids_are_sequential() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    for expected in 1u64..=5 {
        let id = client.create_lock(&admin, &beneficiary, &1_000_000, &3600);
        assert_eq!(id, expected, "lock IDs must be sequential");
    }
}

#[test]
fn min_i128_deposit_returns_zero_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    let result = client.try_create_lock(&admin, &beneficiary, &i128::MIN, &3600);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::ZeroDeposit)),
        "i128::MIN amount must be rejected as ZeroDeposit"
    );
}

#[test]
fn create_lock_with_huge_duration_returns_expiry_overflow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let beneficiary = Address::generate(&env);
    // Push the ledger timestamp to the very top of the u64 range so that
    // `now + duration` overflows while computing `release_at`.
    let now = env.ledger().timestamp();
    advance_ledger(&env, u64::MAX - now);
    let result = client.try_create_lock(&admin, &beneficiary, &1_000_000, &u64::MAX);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::ExpiryOverflow)),
        "release_at overflow must return ExpiryOverflow, not panic"
    );
}

#[test]
fn admin_ops_on_uninitialized_contract_return_error_not_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, TimeLockContract);
    let client = TimeLockContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let result = client.try_pause(&admin);
    assert_eq!(
        result,
        Err(Ok(TimeLockError::Unauthorized)),
        "admin ops before initialize must error, not panic"
    );
}
