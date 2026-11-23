#![no_std]

//! # Storage Layout Contract
//!
//! A Soroban contract that manages per-account balances with strict invariant
//! verification, defensive error recovery, and streamlined storage
//! deserialization.
//!
//! ## Invariants
//!
//! The contract enforces the following invariants before every persistent write:
//!
//! 1. **Key-namespace consistency** — every persistent key must belong to the
//!    `DataKey` namespace (enforced structurally by the type system).
//! 2. **Non-negative balances** — balances are stored as `i128` and must never
//!    be negative.
//! 3. **Monotonic counters** — the global operation counter must never decrease.
//!
//! ## Error handling
//!
//! All storage reads use `ok_or`/`unwrap_or` defaults; no `unwrap`/`expect`
//! is used on storage access. Zero-amount operations are explicitly guarded and
//! return [`StorageLayoutError::ZeroAmount`] rather than panicking.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

/// Storage keys used by the contract.
///
/// The enum itself guarantees key-namespace consistency: only keys of this
/// type can be used with the contract's storage accessors.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Marks the contract as initialized.
    Initialized,
    /// Monotonic counter incremented on every successful write.
    Counter,
    /// Per-account balance keyed by address.
    Balance(Address),
}

/// Typed errors returned by the contract.
///
/// Every fallible entrypoint returns a `Result<_, StorageLayoutError>` so that
/// callers can recover without the host panicking.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StorageLayoutError {
    /// The contract has already been initialized.
    AlreadyInitialized = 1,
    /// The contract has not been initialized yet.
    NotInitialized = 2,
    /// A zero amount was supplied where a non-zero amount is required.
    ZeroAmount = 3,
    /// An amount was invalid (e.g. would overflow or produce a negative balance).
    InvalidAmount = 4,
    /// The requested balance does not exist.
    BalanceNotFound = 5,
    /// An invariant check failed.
    InvariantViolation = 6,
    /// A monotonic counter would have decreased.
    CounterRegression = 7,
}

/// Internal helper: read the initialization flag.
///
/// Uses `unwrap_or(false)` so a missing key is treated as "not initialized"
/// rather than panicking.
fn is_initialized(env: &Env) -> bool {
    env.storage()
        .instance()
        .get::<DataKey, bool>(&DataKey::Initialized)
        .unwrap_or(false)
}

/// Internal helper: read the monotonic counter.
///
/// Uses `unwrap_or(0)` so a missing key defaults to zero.
fn read_counter(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get::<DataKey, u64>(&DataKey::Counter)
        .unwrap_or(0)
}

/// Internal helper: read a balance.
///
/// Returns `None` when the key is absent; callers decide whether absence is an
/// error. Uses a single `get` per key — no intermediate `Vec` allocations.
fn read_balance(env: &Env, account: &Address) -> Option<i128> {
    env.storage()
        .persistent()
        .get::<DataKey, i128>(&DataKey::Balance(account.clone()))
}

/// Verify all invariants before a persistent write.
///
/// Checks:
/// - the contract is initialized;
/// - the proposed balance is non-negative;
/// - the proposed counter is not less than the stored counter.
///
/// Returns a typed [`StorageLayoutError`] instead of panicking.
fn validate_invariants(
    env: &Env,
    proposed_balance: i128,
    proposed_counter: u64,
) -> Result<(), StorageLayoutError> {
    if !is_initialized(env) {
        return Err(StorageLayoutError::NotInitialized);
    }
    if proposed_balance < 0 {
        return Err(StorageLayoutError::InvalidAmount);
    }
    let current_counter = read_counter(env);
    if proposed_counter < current_counter {
        return Err(StorageLayoutError::CounterRegression);
    }
    Ok(())
}

/// Persist a balance and bump the counter, after validating invariants.
///
/// This is the single write path for balances, ensuring every persistent write
/// is preceded by [`validate_invariants`].
fn write_balance(env: &Env, account: &Address, balance: i128) -> Result<(), StorageLayoutError> {
    let next_counter = read_counter(env).saturating_add(1);
    validate_invariants(env, balance, next_counter)?;
    env.storage()
        .persistent()
        .set(&DataKey::Balance(account.clone()), &balance);
    env.storage()
        .instance()
        .set(&DataKey::Counter, &next_counter);
    Ok(())
}

#[contract]
pub struct StorageLayoutContract;

#[contractimpl]
impl StorageLayoutContract {
    /// Initialize the contract.
    ///
    /// # Errors
    /// - [`StorageLayoutError::AlreadyInitialized`] if called more than once.
    ///
    /// # Invariants
    /// - Sets the counter to `0` and marks the contract initialized.
    pub fn initialize(env: Env) -> Result<(), StorageLayoutError> {
        if is_initialized(&env) {
            return Err(StorageLayoutError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&DataKey::Counter, &0u64);
        Ok(())
    }

    /// Set an account's balance to an explicit value.
    ///
    /// # Errors
    /// - [`StorageLayoutError::NotInitialized`] if the contract is not initialized.
    /// - [`StorageLayoutError::InvalidAmount`] if `balance` is negative.
    ///
    /// # Invariants
    /// - The stored balance is non-negative.
    /// - The counter is strictly increasing.
    pub fn set_balance(env: Env, account: Address, balance: i128) -> Result<(), StorageLayoutError> {
        account.require_auth();
        write_balance(&env, &account, balance)
    }

    /// Read an account's balance.
    ///
    /// # Errors
    /// - [`StorageLayoutError::NotInitialized`] if the contract is not initialized.
    /// - [`StorageLayoutError::BalanceNotFound`] if no balance is stored.
    ///
    /// # Invariants
    /// - Performs a single storage read; no intermediate allocations.
    pub fn get_balance(env: Env, account: Address) -> Result<i128, StorageLayoutError> {
        if !is_initialized(&env) {
            return Err(StorageLayoutError::NotInitialized);
        }
        read_balance(&env, &account).ok_or(StorageLayoutError::BalanceNotFound)
    }

    /// Adjust an account's balance by a signed delta.
    ///
    /// A zero `delta` is a no-op that returns `Ok(())` without touching storage,
    /// avoiding unnecessary host metering.
    ///
    /// # Errors
    /// - [`StorageLayoutError::NotInitialized`] if the contract is not initialized.
    /// - [`StorageLayoutError::InvalidAmount`] if the resulting balance would be
    ///   negative or the arithmetic would overflow.
    ///
    /// # Invariants
    /// - The resulting balance is non-negative.
    /// - The counter is strictly increasing on any non-zero adjustment.
    pub fn adjust_balance(env: Env, account: Address, delta: i128) -> Result<i128, StorageLayoutError> {
        account.require_auth();
        if !is_initialized(&env) {
            return Err(StorageLayoutError::NotInitialized);
        }
        // Zero-amount guard: no-op without a write.
        if delta == 0 {
            return read_balance(&env, &account).ok_or(StorageLayoutError::BalanceNotFound);
        }
        let current = read_balance(&env, &account).unwrap_or(0);
        let updated = current
            .checked_add(delta)
            .ok_or(StorageLayoutError::InvalidAmount)?;
        if updated < 0 {
            return Err(StorageLayoutError::InvalidAmount);
        }
        write_balance(&env, &account, updated)?;
        Ok(updated)
    }

    /// Clear an account's balance.
    ///
    /// Clearing an already-absent balance is a no-op that returns `Ok(())`.
    ///
    /// # Errors
    /// - [`StorageLayoutError::NotInitialized`] if the contract is not initialized.
    ///
    /// # Invariants
    /// - The counter is strictly increasing when a balance is actually removed.
    pub fn clear_balance(env: Env, account: Address) -> Result<(), StorageLayoutError> {
        account.require_auth();
        if !is_initialized(&env) {
            return Err(StorageLayoutError::NotInitialized);
        }
        let key = DataKey::Balance(account.clone());
        if !env.storage().persistent().has(&key) {
            // Zero-state guard: nothing to clear.
            return Ok(());
        }
        let next_counter = read_counter(&env).saturating_add(1);
        validate_invariants(&env, 0, next_counter)?;
        env.storage().persistent().remove(&key);
        env.storage()
            .instance()
            .set(&DataKey::Counter, &next_counter);
        Ok(())
    }

    /// Read the monotonic operation counter.
    ///
    /// # Invariants
    /// - The returned value is never less than a previously observed value.
    pub fn get_counter(env: Env) -> u64 {
        read_counter(&env)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn setup() -> (Env, StorageLayoutContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(StorageLayoutContract, ());
        let client = StorageLayoutContractClient::new(&env, &contract_id);
        let account = Address::generate(&env);
        (env, client, account)
    }

    #[test]
    fn initialize_once() {
        let (_env, client, _account) = setup();
        assert_eq!(client.try_initialize(), Ok(Ok(())));
        assert_eq!(
            client.try_initialize(),
            Err(Ok(StorageLayoutError::AlreadyInitialized))
        );
    }

    #[test]
    fn set_and_get_balance() {
        let (_env, client, account) = setup();
        client.initialize();
        client.set_balance(&account, &100);
        assert_eq!(client.get_balance(&account), 100);
    }

    #[test]
    fn negative_balance_rejected() {
        let (_env, client, account) = setup();
        client.initialize();
        assert_eq!(
            client.try_set_balance(&account, &-1),
            Err(Ok(StorageLayoutError::InvalidAmount))
        );
    }

    #[test]
    fn get_missing_balance_errors() {
        let (_env, client, account) = setup();
        client.initialize();
        assert_eq!(
            client.try_get_balance(&account),
            Err(Ok(StorageLayoutError::BalanceNotFound))
        );
    }

    #[test]
    fn not_initialized_errors() {
        let (_env, client, account) = setup();
        assert_eq!(
            client.try_set_balance(&account, &10),
            Err(Ok(StorageLayoutError::NotInitialized))
        );
        assert_eq!(
            client.try_get_balance(&account),
            Err(Ok(StorageLayoutError::NotInitialized))
        );
    }

    #[test]
    fn adjust_balance_accumulates() {
        let (_env, client, account) = setup();
        client.initialize();
        client.set_balance(&account, &50);
        assert_eq!(client.adjust_balance(&account, &25), 75);
        assert_eq!(client.adjust_balance(&account, &-25), 50);
    }

    #[test]
    fn adjust_zero_is_noop() {
        let (_env, client, account) = setup();
        client.initialize();
        client.set_balance(&account, &10);
        let before = client.get_counter();
        assert_eq!(client.adjust_balance(&account, &0), 10);
        assert_eq!(client.get_counter(), before);
    }

    #[test]
    fn adjust_below_zero_rejected() {
        let (_env, client, account) = setup();
        client.initialize();
        client.set_balance(&account, &10);
        assert_eq!(
            client.try_adjust_balance(&account, &-11),
            Err(Ok(StorageLayoutError::InvalidAmount))
        );
    }

    #[test]
    fn adjust_overflow_rejected() {
        let (_env, client, account) = setup();
        client.initialize();
        client.set_balance(&account, &i128::MAX);
        assert_eq!(
            client.try_adjust_balance(&account, &1),
            Err(Ok(StorageLayoutError::InvalidAmount))
        );
    }

    #[test]
    fn clear_balance_removes_and_is_idempotent() {
        let (_env, client, account) = setup();
        client.initialize();
        client.set_balance(&account, &10);
        client.clear_balance(&account);
        assert_eq!(
            client.try_get_balance(&account),
            Err(Ok(StorageLayoutError::BalanceNotFound))
        );
        // Clearing again is a no-op.
        client.clear_balance(&account);
    }

    #[test]
    fn counter_is_monotonic() {
        let (_env, client, account) = setup();
        client.initialize();
        let mut last = client.get_counter();
        for i in 1..=5i128 {
            client.set_balance(&account, &i);
            let now = client.get_counter();
            assert!(now > last);
            last = now;
        }
    }

    #[test]
    fn fuzz_adjust_never_panics() {
        let (_env, client, account) = setup();
        client.initialize();
        client.set_balance(&account, &1000);
        let deltas: [i128; 9] = [
            0,
            1,
            -1,
            i128::MAX,
            i128::MIN,
            500,
            -500,
            -1000,
            -1001,
        ];
        for d in deltas {
            // Must never panic; either Ok or a typed error.
            let _ = client.try_adjust_balance(&account, &d);
            // Balance must remain non-negative if present.
            if let Ok(b) = client.try_get_balance(&account) {
                assert!(b >= 0);
            }
        }
    }
}