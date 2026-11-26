#![no_std]
#![allow(deprecated)]

mod errors;
mod events;
mod invariants;
mod proposals;
mod types;

pub use errors::{category_for, message_for, TimeLockError};
pub use types::{DataKey, LockStatus, Role, TimeLock, TimelockProposal, TimelockStatus};

use soroban_sdk::{contract, contractimpl, Address, Env, String, Symbol};

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
        // Post-condition: the freshly written instance must already satisfy
        // every invariant (#945).
        invariants::check(&env)?;
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
        let stored_admin = Self::require_admin(&env, &admin)?;
        // The stored administrator is the only address that can administer the
        // contract. Demoting it would permanently brick every admin operation
        // (an unrecoverable state inconsistency), so reject it up front (#945).
        if target == stored_admin && role != Role::Admin {
            return Err(TimeLockError::AdminRoleLoss);
        }
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

        // The counter is written by `initialize`; a missing counter means the
        // instance is inconsistent, not that we should silently start over at
        // zero (which could overwrite an existing lock). Use checked
        // arithmetic so an exhausted counter returns a typed error instead of
        // panicking or wrapping (#945).
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::LockCount)
            .ok_or(TimeLockError::StateInconsistency)?;
        let id = count
            .checked_add(1)
            .ok_or(TimeLockError::LockCountOverflow)?;
        if env.storage().persistent().has(&DataKey::Lock(id)) {
            return Err(TimeLockError::StateInconsistency);
        }

        let lock = TimeLock {
            id,
            depositor: depositor.clone(),
            beneficiary: beneficiary.clone(),
            amount,
            release_at,
            status: LockStatus::Active,
        };
        invariants::verify_lock(&lock, id)?;

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

        let mut lock = invariants::get_lock(&env, lock_id)?;

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

        let mut lock = invariants::get_lock(&env, lock_id)?;

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

    /// Return a lock record by ID. Corrupt records are reported through the
    /// same typed error as lifecycle operations (#945).
    pub fn get_lock(env: Env, lock_id: u64) -> Result<TimeLock, TimeLockError> {
        invariants::get_lock(&env, lock_id)
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
        // Streamlined lookup (#943): a finalized proposal needs only its
        // status field, so its target/timestamps are never deserialized.
        if let proposals::ProposalLookup::Active(_) = proposals::load(&env, &target)? {
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
        invariants::verify_proposal(&proposal, &target)?;
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
        let mut proposal = match proposals::load(&env, &target)? {
            proposals::ProposalLookup::Missing => return Err(TimeLockError::TimelockNotFound),
            proposals::ProposalLookup::Final => return Err(TimeLockError::TimelockInactive),
            proposals::ProposalLookup::Active(proposal) => proposal,
        };
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
        let mut proposal = match proposals::load(&env, &target)? {
            proposals::ProposalLookup::Missing => return Err(TimeLockError::TimelockNotFound),
            proposals::ProposalLookup::Final => return Err(TimeLockError::TimelockInactive),
            proposals::ProposalLookup::Active(proposal) => proposal,
        };
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

    /// Returns the timelock proposal for `target`, if it is well-formed.
    /// Corrupt records are hidden rather than handed to a client (#945).
    pub fn get_timelock(env: Env, target: Address) -> Option<TimelockProposal> {
        env.storage()
            .instance()
            .get::<_, TimelockProposal>(&DataKey::TimelockProposal(target.clone()))
            .and_then(|proposal| {
                invariants::verify_proposal(&proposal, &target)
                    .ok()
                    .map(|_| proposal)
            })
    }

    /// Returns `true` only if `target` has a well-formed, active proposal.
    ///
    /// Uses the single-read status projection from #943 instead of decoding
    /// the entire record; active proposals are still strictly verified.
    pub fn has_active_timelock(env: Env, target: Address) -> bool {
        matches!(
            proposals::load(&env, &target),
            Ok(proposals::ProposalLookup::Active(_))
        )
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
    // Invariant audit (#945)
    // -------------------------------------------------------------------------

    /// Read-only audit of the instance-level invariants. Operators and
    /// monitors can call this on a live deployment to detect corrupted state
    /// before attempting a state-changing operation.
    pub fn verify_invariants(env: Env) -> Result<(), TimeLockError> {
        invariants::check(&env)
    }

    // -------------------------------------------------------------------------
    // Client error diagnostics (#946)
    // -------------------------------------------------------------------------

    /// Human-readable explanation of a raw time-lock error code (#946).
    ///
    /// A client that only sees `Error(Contract, #n)` can call this read-only
    /// entrypoint to display a stable message instead of an opaque number.
    /// Returns `None` for codes this contract never returns.
    pub fn get_error_message(env: Env, code: u32) -> Option<String> {
        errors::message_for(code).map(|message| String::from_str(&env, message))
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Authenticates `caller` as the stored admin and returns that admin.
    /// A stored admin without the admin role is treated as corrupted state
    /// rather than silently falling back to an unprivileged role (#945).
    fn require_admin(env: &Env, caller: &Address) -> Result<Address, TimeLockError> {
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
            .get(&DataKey::Role(stored.clone()))
            .ok_or(TimeLockError::StateInconsistency)?;
        if role != Role::Admin {
            return Err(TimeLockError::StateInconsistency);
        }
        Ok(stored)
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
