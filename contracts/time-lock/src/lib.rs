#![no_std]

mod events;
mod types;

pub use types::{DataKey, LockStatus, Role, TimeLock, TimeLockError};

use soroban_sdk::{contract, contractimpl, Address, Env};

#[contract]
pub struct TimeLockContract;

#[contractimpl]
impl TimeLockContract {
    // -------------------------------------------------------------------------
    // Initialization
    // -------------------------------------------------------------------------

    /// Initialize the contract, assigning `admin` as the sole admin.
    /// Returns `AlreadyInitialized` if called more than once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), TimeLockError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TimeLockError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Role(admin.clone()), &Role::Admin);
        env.storage().instance().set(&DataKey::LockCount, &0u64);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Issue #942: Granular role-based authorization
    // -------------------------------------------------------------------------

    /// Assign a role to an address. Only the contract admin may call this.
    pub fn set_role(
        env: Env,
        admin: Address,
        target: Address,
        role: Role,
    ) -> Result<(), TimeLockError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Role(target), &role);
        Ok(())
    }

    /// Return the role currently assigned to `who` (defaults to `Role::None`).
    pub fn get_role(env: Env, who: Address) -> Role {
        env.storage()
            .instance()
            .get(&DataKey::Role(who))
            .unwrap_or(Role::None)
    }

    // -------------------------------------------------------------------------
    // Issue #948: Zero-balance / zero-deposit guard
    // Core lock lifecycle
    // -------------------------------------------------------------------------

    /// Create a time-lock holding `amount` token units for `beneficiary`.
    /// The caller must hold at least `Role::Operator`.
    ///
    /// # Edge cases (issue #948)
    /// - `amount == 0`: returns `ZeroDeposit` instead of panicking.
    /// - `duration_seconds == 0`: returns `ZeroDuration`.
    /// - `release_at` timestamp overflow: returns `ExpiryOverflow`.
    pub fn create_lock(
        env: Env,
        depositor: Address,
        beneficiary: Address,
        amount: i128,
        duration_seconds: u64,
    ) -> Result<u64, TimeLockError> {
        // Issue #942: operator-or-admin role required
        Self::require_at_least_operator(&env, &depositor)?;
        depositor.require_auth();
        Self::require_not_paused(&env)?;

        // Issue #948: reject zero deposits without panicking
        if amount <= 0 {
            return Err(TimeLockError::ZeroDeposit);
        }

        if duration_seconds == 0 {
            return Err(TimeLockError::ZeroDuration);
        }

        let now = env.ledger().timestamp();
        let release_at = now
            .checked_add(duration_seconds)
            .ok_or(TimeLockError::ExpiryOverflow)?;

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::LockCount)
            .unwrap_or(0);
        let id = count + 1;

        let lock = TimeLock {
            id,
            depositor: depositor.clone(),
            beneficiary: beneficiary.clone(),
            amount,
            release_at,
            status: LockStatus::Active,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Lock(id), &lock);
        env.storage().instance().set(&DataKey::LockCount, &id);
        events::lock_created(&env, id, &lock);
        Ok(id)
    }

    /// Release a matured lock to its beneficiary.
    /// The `beneficiary` address must match the lock record.
    ///
    /// # Edge cases (issue #948)
    /// - Lock with zero-amount that somehow exists is handled gracefully —
    ///   the check on `LockNotMatured` and `AlreadyReleased` applies
    ///   regardless of the stored amount, so no panic path exists.
    pub fn release(
        env: Env,
        beneficiary: Address,
        lock_id: u64,
    ) -> Result<TimeLock, TimeLockError> {
        beneficiary.require_auth();
        Self::require_not_paused(&env)?;

        let mut lock: TimeLock = env
            .storage()
            .persistent()
            .get(&DataKey::Lock(lock_id))
            .ok_or(TimeLockError::LockNotFound)?;

        if lock.beneficiary != beneficiary {
            return Err(TimeLockError::BeneficiaryMismatch);
        }

        if lock.status != LockStatus::Active {
            return Err(TimeLockError::AlreadyReleased);
        }

        let now = env.ledger().timestamp();
        if now < lock.release_at {
            return Err(TimeLockError::LockNotMatured);
        }

        lock.status = LockStatus::Released;
        env.storage()
            .persistent()
            .set(&DataKey::Lock(lock_id), &lock);
        events::lock_released(&env, lock_id, &lock);
        Ok(lock)
    }

    /// Cancel an active lock before it matures. Admin-only.
    /// Issue #942: only an admin-role address may cancel.
    pub fn cancel_lock(
        env: Env,
        admin: Address,
        lock_id: u64,
    ) -> Result<TimeLock, TimeLockError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;

        let mut lock: TimeLock = env
            .storage()
            .persistent()
            .get(&DataKey::Lock(lock_id))
            .ok_or(TimeLockError::LockNotFound)?;

        if lock.status != LockStatus::Active {
            return Err(TimeLockError::AlreadyReleased);
        }

        lock.status = LockStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Lock(lock_id), &lock);
        events::lock_cancelled(&env, lock_id, &lock);
        Ok(lock)
    }

    /// Return a lock record by ID.
    pub fn get_lock(env: Env, lock_id: u64) -> Result<TimeLock, TimeLockError> {
        env.storage()
            .persistent()
            .get(&DataKey::Lock(lock_id))
            .ok_or(TimeLockError::LockNotFound)
    }

    // -------------------------------------------------------------------------
    // Pause / unpause (admin-only)
    // -------------------------------------------------------------------------

    pub fn pause(env: Env, admin: Address) -> Result<(), TimeLockError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        events::contract_paused(&env);
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), TimeLockError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        events::contract_unpaused(&env);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Issue #942: require the caller to be the stored contract admin.
    fn require_admin(env: &Env, caller: &Address) -> Result<(), TimeLockError> {
        caller.require_auth();
        // Issue #948: defensive — no panic if the contract is uninitialized.
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(TimeLockError::Unauthorized)?;
        if stored != *caller {
            return Err(TimeLockError::Unauthorized);
        }
        // Double-check via role table for defence-in-depth.
        let role: Role = env
            .storage()
            .instance()
            .get(&DataKey::Role(caller.clone()))
            .unwrap_or(Role::None);
        if role != Role::Admin {
            return Err(TimeLockError::InsufficientRole);
        }
        Ok(())
    }

    /// Issue #942: require `Role::Operator` or `Role::Admin`.
    fn require_at_least_operator(env: &Env, caller: &Address) -> Result<(), TimeLockError> {
        let role: Role = env
            .storage()
            .instance()
            .get(&DataKey::Role(caller.clone()))
            .unwrap_or(Role::None);
        match role {
            Role::Operator | Role::Admin => Ok(()),
            Role::None => Err(TimeLockError::InsufficientRole),
        }
    }

    fn require_not_paused(env: &Env) -> Result<(), TimeLockError> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            Err(TimeLockError::ContractPaused)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
extern crate std;
