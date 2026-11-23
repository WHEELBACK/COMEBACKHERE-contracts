#![no_std]

mod events;
mod types;

pub use types::{
    DataKey, LockStatus, Role, TimeLock, TimeLockError, TimelockProposal, TimelockStatus,
};

use soroban_sdk::{contract, contractimpl, Address, Env, Symbol};

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
        env.events()
            .publish((Symbol::new(&env, "time_lock_initialized"),), admin);
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
        env.storage().instance().set(&DataKey::Role(target), &role);
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
    pub fn create_lock(
        env: Env,
        depositor: Address,
        beneficiary: Address,
        amount: i128,
        duration_seconds: u64,
    ) -> Result<u64, TimeLockError> {
        Self::require_at_least_operator(&env, &depositor)?;
        depositor.require_auth();
        Self::require_not_paused(&env)?;

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

        env.storage().persistent().set(&DataKey::Lock(id), &lock);
        env.storage().instance().set(&DataKey::LockCount, &id);
        events::lock_created(&env, id, &lock);
        Ok(id)
    }

    /// Release a matured lock to its beneficiary.
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
    pub fn cancel_lock(env: Env, admin: Address, lock_id: u64) -> Result<TimeLock, TimeLockError> {
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
    // Issue #949: Generic timelocked proposal lifecycle
    // -------------------------------------------------------------------------

    /// Queues a timelocked proposal on `target` (admin-only).
    pub fn propose_timelock(
        env: Env,
        admin: Address,
        target: Address,
        delay_secs: u64,
    ) -> Result<(), TimeLockError> {
        Self::require_admin(&env, &admin)?;
        if delay_secs == 0 {
            return Err(TimeLockError::ZeroDelay);
        }
        if env
            .storage()
            .instance()
            .get::<_, TimelockProposal>(&DataKey::TimelockProposal(target.clone()))
            .is_some_and(|existing| existing.status == TimelockStatus::Active)
        {
            return Err(TimeLockError::TimelockActive);
        }
        let now = env.ledger().timestamp();
        let executable_at = now
            .checked_add(delay_secs)
            .ok_or(TimeLockError::ArithmeticOverflow)?;
        let proposal = TimelockProposal {
            target: target.clone(),
            delay_secs,
            proposed_at: now,
            executable_at,
            status: TimelockStatus::Active,
        };
        env.storage()
            .instance()
            .set(&DataKey::TimelockProposal(target.clone()), &proposal);
        env.events()
            .publish((Symbol::new(&env, "timelock_proposed"), target), proposal);
        Ok(())
    }

    /// Executes the timelocked proposal on `target` once the delay has elapsed (admin-only).
    pub fn execute_timelock(
        env: Env,
        admin: Address,
        target: Address,
    ) -> Result<TimelockProposal, TimeLockError> {
        Self::require_admin(&env, &admin)?;
        let mut proposal: TimelockProposal = env
            .storage()
            .instance()
            .get(&DataKey::TimelockProposal(target.clone()))
            .ok_or(TimeLockError::TimelockNotFound)?;
        if proposal.status != TimelockStatus::Active {
            return Err(TimeLockError::TimelockInactive);
        }
        let now = env.ledger().timestamp();
        if now < proposal.executable_at {
            return Err(TimeLockError::TooEarly);
        }
        proposal.status = TimelockStatus::Executed;
        env.storage()
            .instance()
            .set(&DataKey::TimelockProposal(target.clone()), &proposal);
        env.events().publish(
            (Symbol::new(&env, "timelock_executed"), target),
            proposal.clone(),
        );
        Ok(proposal)
    }

    /// Cancels the timelocked proposal on `target` (admin-only).
    pub fn cancel_timelock(
        env: Env,
        admin: Address,
        target: Address,
    ) -> Result<TimelockProposal, TimeLockError> {
        Self::require_admin(&env, &admin)?;
        let mut proposal: TimelockProposal = env
            .storage()
            .instance()
            .get(&DataKey::TimelockProposal(target.clone()))
            .ok_or(TimeLockError::TimelockNotFound)?;
        if proposal.status != TimelockStatus::Active {
            return Err(TimeLockError::TimelockInactive);
        }
        proposal.status = TimelockStatus::Cancelled;
        env.storage()
            .instance()
            .set(&DataKey::TimelockProposal(target.clone()), &proposal);
        env.events().publish(
            (Symbol::new(&env, "timelock_cancelled"), target),
            proposal.clone(),
        );
        Ok(proposal)
    }

    /// Returns the timelock proposal for `target`, if any.
    pub fn get_timelock(env: Env, target: Address) -> Option<TimelockProposal> {
        env.storage()
            .instance()
            .get(&DataKey::TimelockProposal(target))
    }

    /// Returns `true` if `target` has an active (non-final) proposal.
    pub fn has_active_timelock(env: Env, target: Address) -> bool {
        env.storage()
            .instance()
            .get::<_, TimelockProposal>(&DataKey::TimelockProposal(target))
            .is_some_and(|proposal| proposal.status == TimelockStatus::Active)
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

    fn require_admin(env: &Env, caller: &Address) -> Result<(), TimeLockError> {
        caller.require_auth();
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(TimeLockError::Unauthorized)?;
        if stored != *caller {
            return Err(TimeLockError::Unauthorized);
        }
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
