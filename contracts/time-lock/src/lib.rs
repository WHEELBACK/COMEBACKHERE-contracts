#![no_std]

mod errors;

pub use errors::TimeLockError;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

/// Lifecycle state of a timelocked proposal.
///
/// Transitions: `Active` → `Executed` (delay elapsed, `execute_timelock`
/// called), or `Active` → `Cancelled` (admin cancels). `Executed` and
/// `Cancelled` are final: while `Active`, no other transition is allowed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimelockStatus {
    /// Queued and executable once `executable_at` is reached.
    Active,
    /// Executed — the delayed action was completed.
    Executed,
    /// Cancelled by the admin while still active.
    Cancelled,
}

/// A timelocked proposal on a target: the delayed action is considered done
/// once the ledger timestamp reaches `executable_at`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelockProposal {
    /// The target the proposal applies to.
    pub target: Address,
    /// The delay (seconds) between `proposed_at` and `executable_at`.
    pub delay_secs: u64,
    /// Ledger timestamp at which the proposal was created.
    pub proposed_at: u64,
    /// Earliest ledger timestamp at which `execute_timelock` may succeed.
    pub executable_at: u64,
    /// Current lifecycle state.
    pub status: TimelockStatus,
}

/// Storage keys for the time-lock contract.
///
/// Used as keys for Soroban instance storage. Variants must not be reordered
/// or removed once deployed; new variants should be appended at the end so
/// that existing on-chain data (keyed by XDR-encoded discriminants) continues
/// to decode correctly.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Stored administrator (instance storage).
    Admin,
    /// Timelock proposal per target (instance storage).
    TimelockProposal(Address),
}

/// Generic timelocked action executor: contracts queue a delayed change on
/// themselves via `propose_timelock` and execute it via `execute_timelock`
/// only after the delay elapses.
#[contract]
pub struct TimeLockContract;

fn require_admin(env: &Env, admin: &Address) -> Result<(), TimeLockError> {
    admin.require_auth();
    match env.storage().instance().get(&DataKey::Admin) {
        Some(stored) if stored == *admin => Ok(()),
        _ => Err(TimeLockError::Unauthorized),
    }
}

#[contractimpl]
impl TimeLockContract {
    /// One-time initialization: stores `admin`.
    ///
    /// Errors: `AlreadyInitialized`.
    /// Emits: `time_lock_initialized`.
    pub fn initialize(env: Env, admin: Address) -> Result<(), TimeLockError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TimeLockError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.events()
            .publish((Symbol::new(&env, "time_lock_initialized"),), admin);
        Ok(())
    }

    /// Queues a timelocked proposal on `target` (admin-only).
    ///
    /// Strict validation: `delay_secs > 0` (`ZeroDelay`); the target must not
    /// have an active proposal (`TimelockActive`); `executable_at` arithmetic
    /// must not overflow (`ArithmeticOverflow`).
    /// Emits: `timelock_proposed`.
    pub fn propose_timelock(
        env: Env,
        admin: Address,
        target: Address,
        delay_secs: u64,
    ) -> Result<(), TimeLockError> {
        require_admin(&env, &admin)?;
        if delay_secs == 0 {
            return Err(TimeLockError::ZeroDelay);
        }
        if env
            .storage()
            .instance()
            .get(&DataKey::TimelockProposal(target.clone()))
            .is_some_and(|existing| existing.status == TimelockStatus::Active)
        {
            return Err(TimeLockError::TimelockActive);
        }
        let now = env.ledger().timestamp();
        let executable_at = now.checked_add(delay_secs).ok_or(TimeLockError::ArithmeticOverflow)?;
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

    /// Executes the timelocked proposal on `target` once the delay has
    /// elapsed (admin-only).
    ///
    /// Fails with `TimelockNotFound` if no proposal exists,
    /// `TimelockInactive` if it is not `Active`, and `TooEarly` if
    /// `env.ledger().timestamp() < executable_at`.
    /// Emits: `timelock_executed`.
    pub fn execute_timelock(
        env: Env,
        admin: Address,
        target: Address,
    ) -> Result<TimelockProposal, TimeLockError> {
        require_admin(&env, &admin)?;
        let mut proposal = env
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
        env.storage().instance().set(&DataKey::TimelockProposal(target.clone()), &proposal);
        env.events().publish((Symbol::new(&env, "timelock_executed"), target), proposal.clone());
        Ok(proposal)
    }

    /// Cancels the timelocked proposal on `target` (admin-only).
    ///
    /// Fails with `TimelockNotFound` if no proposal exists and
    /// `TimelockInactive` if it is not `Active`.
    /// Emits: `timelock_cancelled`.
    pub fn cancel_timelock(
        env: Env,
        admin: Address,
        target: Address,
    ) -> Result<TimelockProposal, TimeLockError> {
        require_admin(&env, &admin)?;
        let mut proposal = env
            .storage()
            .instance()
            .get(&DataKey::TimelockProposal(target.clone()))
            .ok_or(TimeLockError::TimelockNotFound)?;
        if proposal.status != TimelockStatus::Active {
            return Err(TimeLockError::TimelockInactive);
        }
        proposal.status = TimelockStatus::Cancelled;
        env.storage().instance().set(&DataKey::TimelockProposal(target.clone()), &proposal);
        env.events().publish((Symbol::new(&env, "timelock_cancelled"), target), proposal.clone());
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
            .get(&DataKey::TimelockProposal(target))
            .is_some_and(|proposal| proposal.status == TimelockStatus::Active)
    }
}
