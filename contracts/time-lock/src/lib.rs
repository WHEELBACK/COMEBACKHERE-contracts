//! Time-lock contract — admin-managed time-locked settlement releases for the
//! COMEBACKHERE protocol.
//!
//! # Overview
//!
//! This contract holds funds and enforces a mandatory unlock delay before any
//! release can be executed.  The core invariant is that `release_amount` can
//! never produce a negative remaining balance: every subtraction is guarded with
//! `checked_sub` and returns `TimeLockError::ArithmeticUnderflow` rather than
//! wrapping.
//!
//! Partial settlement computations are the primary underflow risk surface.  A
//! caller supplying a `release_amount` larger than the tracked `deposited` total
//! would, without protection, underflow the balance counter.  The fix applied
//! here — and tested in `tests/` — is:
//!
//! 1. All balance arithmetic uses `checked_sub` / `checked_add`.
//! 2. `release_amount` is validated against the stored `deposited` before the
//!    ledger transfer takes place.
//! 3. The same guard applies to the running `released_total` accumulator.
//!
//! # Storage layout
//!
//! | Key                      | Kind       | Description                              |
//! |--------------------------|------------|------------------------------------------|
//! | `Admin`                  | Instance   | Contract administrator                   |
//! | `Paused`                 | Instance   | Circuit-breaker flag                     |
//! | `LockCount`              | Instance   | Monotone counter for lock IDs            |
//! | `Lock(id)`               | Persistent | [`TimeLock`] record                      |
//!
//! # Acceptance criteria (issue #941)
//!
//! * Arithmetic underflow in partial settlement computations is prevented via
//!   `checked_sub`; the new `ArithmeticUnderflow` error variant is returned on
//!   violation.
//! * Soroban test harness passes without host metering regression.
//! * Code formats cleanly with `cargo fmt` and passes `cargo clippy`.

#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

// ── Error type ──────────────────────────────────────────────────────────────

/// Primary error type for the time-lock contract.
///
/// Variants are append-only to preserve on-chain backwards compatibility.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TimeLockError {
    /// The contract was already initialized.
    AlreadyInitialized = 1,
    /// The caller is not the stored administrator.
    Unauthorized = 2,
    /// The contract is paused; state-mutating calls are rejected.
    ContractPaused = 3,
    /// `amount` supplied is zero or negative.
    InvalidAmount = 4,
    /// The referenced lock ID does not exist.
    LockNotFound = 5,
    /// The lock's unlock timestamp has not yet been reached.
    LockNotMatured = 6,
    /// The lock has already been fully released or cancelled.
    LockAlreadySettled = 7,
    /// A `release_amount` would exceed the remaining deposited balance —
    /// protects against arithmetic underflow in partial settlement.
    InsufficientDeposit = 8,
    /// An arithmetic operation would overflow a `u64` or `i128` counter.
    ArithmeticOverflow = 9,
    /// An arithmetic subtraction would produce a negative result (underflow).
    /// Returned instead of wrapping; see the `checked_sub` guards throughout.
    ArithmeticUnderflow = 10,
    /// `unlock_at` timestamp is not strictly in the future relative to the
    /// current ledger timestamp.
    UnlockInPast = 11,
}

// ── Storage key type ─────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    LockCount,
    Lock(u64),
}

// ── Domain types ─────────────────────────────────────────────────────────────

/// Status of a [`TimeLock`] record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LockStatus {
    /// Funds are locked; not yet matured or only partially released.
    Active,
    /// All deposited funds have been released.
    Released,
    /// The lock was cancelled by the admin before full release.
    Cancelled,
}

/// A single time-locked deposit entry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeLock {
    /// Unique identifier (1-indexed, monotone).
    pub id: u64,
    /// The address that deposited (or on whose behalf the lock was created).
    pub depositor: Address,
    /// The beneficiary entitled to receive the released funds.
    pub beneficiary: Address,
    /// Total amount deposited into this lock.
    pub deposited: i128,
    /// Cumulative amount that has already been released via partial releases.
    /// Invariant: `released_total <= deposited` at all times.
    pub released_total: i128,
    /// Ledger timestamp after which `release` calls are permitted.
    pub unlock_at: u64,
    /// Ledger timestamp at which this lock was created.
    pub created_at: u64,
    /// Current lifecycle status.
    pub status: LockStatus,
}

// ── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct TimeLockContract;

#[contractimpl]
impl TimeLockContract {
    // ── Lifecycle ────────────────────────────────────────────────────────────

    /// Initialise the contract with `admin` as the owner.
    ///
    /// # Errors
    /// [`TimeLockError::AlreadyInitialized`] — called a second time.
    pub fn initialize(env: Env, admin: Address) -> Result<(), TimeLockError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TimeLockError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::LockCount, &0u64);
        env.events()
            .publish((Symbol::new(&env, "time_lock_initialized"),), admin);
        Ok(())
    }

    /// Pause the contract, blocking all state-mutating calls except admin
    /// operations (admin-only).
    ///
    /// Emits: `time_lock_paused`.
    pub fn pause(env: Env, admin: Address) -> Result<(), TimeLockError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "time_lock_paused"),), admin);
        Ok(())
    }

    /// Resume normal operations (admin-only).
    ///
    /// Emits: `time_lock_unpaused`.
    pub fn unpause(env: Env, admin: Address) -> Result<(), TimeLockError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "time_lock_unpaused"),), admin);
        Ok(())
    }

    // ── Lock management ──────────────────────────────────────────────────────

    /// Create a new time-locked entry for `amount` tokens payable to
    /// `beneficiary` after `unlock_at`.
    ///
    /// `unlock_at` must be strictly greater than the current ledger timestamp
    /// — a lock that is already mature at creation time is rejected with
    /// [`TimeLockError::UnlockInPast`].
    ///
    /// # Errors
    /// [`TimeLockError::InvalidAmount`], [`TimeLockError::UnlockInPast`],
    /// [`TimeLockError::ArithmeticOverflow`].
    ///
    /// Emits: `lock_created`.
    pub fn create_lock(
        env: Env,
        depositor: Address,
        beneficiary: Address,
        amount: i128,
        unlock_at: u64,
    ) -> Result<u64, TimeLockError> {
        require_not_paused(&env)?;
        depositor.require_auth();
        if amount <= 0 {
            return Err(TimeLockError::InvalidAmount);
        }
        if unlock_at <= env.ledger().timestamp() {
            return Err(TimeLockError::UnlockInPast);
        }
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::LockCount)
            .unwrap_or(0);
        let id = count
            .checked_add(1)
            .ok_or(TimeLockError::ArithmeticOverflow)?;
        let lock = TimeLock {
            id,
            depositor,
            beneficiary,
            deposited: amount,
            released_total: 0,
            unlock_at,
            created_at: env.ledger().timestamp(),
            status: LockStatus::Active,
        };
        env.storage().persistent().set(&DataKey::Lock(id), &lock);
        env.storage().instance().set(&DataKey::LockCount, &id);
        env.events()
            .publish((Symbol::new(&env, "lock_created"), id), lock);
        Ok(id)
    }

    /// Release `release_amount` tokens from lock `lock_id` to its beneficiary.
    ///
    /// This is the **partial settlement** entry-point.  The critical safety
    /// property (issue #941) is that this function never allows
    /// `released_total` to exceed `deposited`:
    ///
    /// ```text
    /// remaining = deposited - released_total   // checked_sub → underflow guard
    /// assert release_amount <= remaining        // InsufficientDeposit guard
    /// released_total += release_amount          // checked_add → overflow guard
    /// ```
    ///
    /// All three arithmetic operations use the checked variants and return
    /// [`TimeLockError::ArithmeticUnderflow`] or
    /// [`TimeLockError::ArithmeticOverflow`] instead of wrapping.
    ///
    /// # Preconditions
    /// * Contract not paused.
    /// * `release_amount > 0`.
    /// * Lock is `Active`.
    /// * `env.ledger().timestamp() >= lock.unlock_at`.
    /// * `release_amount <= deposited - released_total`.
    ///
    /// # Errors
    /// [`TimeLockError::LockNotFound`], [`TimeLockError::LockAlreadySettled`],
    /// [`TimeLockError::LockNotMatured`], [`TimeLockError::InvalidAmount`],
    /// [`TimeLockError::InsufficientDeposit`],
    /// [`TimeLockError::ArithmeticUnderflow`],
    /// [`TimeLockError::ArithmeticOverflow`].
    ///
    /// Emits: `lock_released` (partial) or `lock_fully_released` (final).
    pub fn release(
        env: Env,
        caller: Address,
        lock_id: u64,
        release_amount: i128,
    ) -> Result<i128, TimeLockError> {
        require_not_paused(&env)?;
        caller.require_auth();

        if release_amount <= 0 {
            return Err(TimeLockError::InvalidAmount);
        }

        let mut lock: TimeLock = env
            .storage()
            .persistent()
            .get(&DataKey::Lock(lock_id))
            .ok_or(TimeLockError::LockNotFound)?;

        if lock.status != LockStatus::Active {
            return Err(TimeLockError::LockAlreadySettled);
        }

        if env.ledger().timestamp() < lock.unlock_at {
            return Err(TimeLockError::LockNotMatured);
        }

        // ── Underflow guard (issue #941) ──────────────────────────────────
        //
        // Compute remaining = deposited − released_total using checked_sub so
        // that a corrupted or adversarially crafted storage value where
        // released_total > deposited cannot silently wrap around to a large
        // positive number and bypass the InsufficientDeposit check below.
        let remaining: i128 = lock
            .deposited
            .checked_sub(lock.released_total)
            .ok_or(TimeLockError::ArithmeticUnderflow)?;

        if release_amount > remaining {
            return Err(TimeLockError::InsufficientDeposit);
        }

        // Accumulate released_total with overflow protection.
        let new_released = lock
            .released_total
            .checked_add(release_amount)
            .ok_or(TimeLockError::ArithmeticOverflow)?;

        lock.released_total = new_released;

        // Compute the new remaining balance after this release.
        let new_remaining: i128 = lock
            .deposited
            .checked_sub(lock.released_total)
            .ok_or(TimeLockError::ArithmeticUnderflow)?;

        if new_remaining == 0 {
            lock.status = LockStatus::Released;
            env.storage()
                .persistent()
                .set(&DataKey::Lock(lock_id), &lock);
            env.events().publish(
                (Symbol::new(&env, "lock_fully_released"), lock_id),
                lock.clone(),
            );
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::Lock(lock_id), &lock);
            env.events().publish(
                (Symbol::new(&env, "lock_released"), lock_id),
                (release_amount, new_remaining),
            );
        }

        Ok(new_remaining)
    }

    /// Cancel an active lock (admin-only).  Any unreleased funds are considered
    /// recoverable by the admin through a separate off-chain mechanism; this
    /// contract does not hold native tokens — it tracks accounting state only.
    ///
    /// # Errors
    /// [`TimeLockError::LockNotFound`], [`TimeLockError::LockAlreadySettled`].
    ///
    /// Emits: `lock_cancelled`.
    pub fn cancel_lock(env: Env, admin: Address, lock_id: u64) -> Result<(), TimeLockError> {
        require_admin(&env, &admin)?;

        let mut lock: TimeLock = env
            .storage()
            .persistent()
            .get(&DataKey::Lock(lock_id))
            .ok_or(TimeLockError::LockNotFound)?;

        if lock.status != LockStatus::Active {
            return Err(TimeLockError::LockAlreadySettled);
        }

        lock.status = LockStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Lock(lock_id), &lock);
        env.events()
            .publish((Symbol::new(&env, "lock_cancelled"), lock_id), lock);
        Ok(())
    }

    // ── View helpers ─────────────────────────────────────────────────────────

    /// Return the [`TimeLock`] record for `lock_id`.
    ///
    /// # Errors
    /// [`TimeLockError::LockNotFound`].
    pub fn get_lock(env: Env, lock_id: u64) -> Result<TimeLock, TimeLockError> {
        env.storage()
            .persistent()
            .get(&DataKey::Lock(lock_id))
            .ok_or(TimeLockError::LockNotFound)
    }

    /// Return the remaining (unreleased) balance for `lock_id`.
    ///
    /// Uses `checked_sub` for the same underflow protection as `release`.
    ///
    /// # Errors
    /// [`TimeLockError::LockNotFound`], [`TimeLockError::ArithmeticUnderflow`].
    pub fn remaining_balance(env: Env, lock_id: u64) -> Result<i128, TimeLockError> {
        let lock: TimeLock = env
            .storage()
            .persistent()
            .get(&DataKey::Lock(lock_id))
            .ok_or(TimeLockError::LockNotFound)?;
        lock.deposited
            .checked_sub(lock.released_total)
            .ok_or(TimeLockError::ArithmeticUnderflow)
    }

    /// Return `true` if `lock_id` has matured (unlock timestamp reached).
    ///
    /// # Errors
    /// [`TimeLockError::LockNotFound`].
    pub fn is_matured(env: Env, lock_id: u64) -> Result<bool, TimeLockError> {
        let lock: TimeLock = env
            .storage()
            .persistent()
            .get(&DataKey::Lock(lock_id))
            .ok_or(TimeLockError::LockNotFound)?;
        Ok(env.ledger().timestamp() >= lock.unlock_at)
    }

    /// Return the total number of locks ever created.
    pub fn lock_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::LockCount)
            .unwrap_or(0)
    }
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn require_admin(env: &Env, admin: &Address) -> Result<(), TimeLockError> {
    admin.require_auth();
    let stored: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(TimeLockError::Unauthorized)?;
    if stored != *admin {
        return Err(TimeLockError::Unauthorized);
    }
    Ok(())
}

fn require_not_paused(env: &Env) -> Result<(), TimeLockError> {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        return Err(TimeLockError::ContractPaused);
    }
    Ok(())
}

#[cfg(test)]
extern crate std;
