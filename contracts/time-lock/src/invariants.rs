//! Strict invariant verification for the time-lock contract (#945).
//!
//! A time-lock record and its key are two halves of the same piece of state.
//! If they ever disagree (because of a bad upgrade, a corrupted ledger entry,
//! or a hostile cross-contract write) the lifecycle entrypoints must refuse
//! to act on the record instead of silently moving funds under an invalid
//! identity. Every read that feeds a state transition is therefore validated
//! here before it is trusted.
//!
//! Verified invariants:
//! 1. The instance is fully initialized (admin, admin role, lock counter and
//!    pause flag are all present and mutually consistent).
//! 2. A lock record's `id` matches the storage key it was read from, and its
//!    amount and release timestamp are strictly positive.
//! 3. A proposal's `target` matches its storage key, its delay is positive,
//!    and `executable_at` is exactly `proposed_at + delay_secs`.
//! 4. The monotonic lock counter is present and never wraps.

use soroban_sdk::{Address, Env};

use crate::{DataKey, LockStatus, Role, TimeLock, TimeLockError, TimelockProposal, TimelockStatus};

/// Checks the instance-level invariants shared by every entrypoint.
///
/// This deliberately only reads instance keys (rather than scanning every
/// lock/proposal) so it adds no per-record footprint to transactions. Record
/// invariants are checked on access by [`get_lock`] and [`verify_proposal`].
pub fn check(env: &Env) -> Result<(), TimeLockError> {
    let instance = env.storage().instance();
    let admin: Address = instance
        .get(&DataKey::Admin)
        .ok_or(TimeLockError::StateInconsistency)?;
    if !matches!(instance.get(&DataKey::Role(admin)), Some(Role::Admin)) {
        return Err(TimeLockError::StateInconsistency);
    }
    if instance.get::<_, u64>(&DataKey::LockCount).is_none()
        || instance.get::<_, bool>(&DataKey::Paused).is_none()
    {
        return Err(TimeLockError::StateInconsistency);
    }
    Ok(())
}

/// Validates a lock record against the ID it is stored under.
pub fn verify_lock(lock: &TimeLock, expected_id: u64) -> Result<(), TimeLockError> {
    if lock.id == 0 || lock.id != expected_id || lock.amount <= 0 || lock.release_at == 0 {
        return Err(TimeLockError::StateInconsistency);
    }
    match lock.status {
        LockStatus::Active | LockStatus::Released | LockStatus::Cancelled => Ok(()),
    }
}

/// Reads a lock and verifies it before any lifecycle transition is attempted.
pub fn get_lock(env: &Env, lock_id: u64) -> Result<TimeLock, TimeLockError> {
    let lock: TimeLock = env
        .storage()
        .persistent()
        .get(&DataKey::Lock(lock_id))
        .ok_or(TimeLockError::LockNotFound)?;
    verify_lock(&lock, lock_id)?;
    Ok(lock)
}

/// Validates a proposal against the target key it is stored under.
///
/// The timing relation is recomputed with checked arithmetic so a corrupted
/// `executable_at` can never authorize an early execution.
pub fn verify_proposal(
    proposal: &TimelockProposal,
    expected_target: &Address,
) -> Result<(), TimeLockError> {
    if proposal.target != *expected_target || proposal.delay_secs == 0 {
        return Err(TimeLockError::StateInconsistency);
    }
    let expected_executable_at = proposal
        .proposed_at
        .checked_add(proposal.delay_secs)
        .ok_or(TimeLockError::StateInconsistency)?;
    if proposal.executable_at != expected_executable_at {
        return Err(TimeLockError::StateInconsistency);
    }
    match proposal.status {
        TimelockStatus::Active | TimelockStatus::Executed | TimelockStatus::Cancelled => Ok(()),
    }
}

/// Returns `true` only for a well-formed, still-active proposal.
pub fn proposal_is_active(proposal: &TimelockProposal, target: &Address) -> bool {
    verify_proposal(proposal, target).is_ok() && proposal.status == TimelockStatus::Active
}
