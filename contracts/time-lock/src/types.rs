use soroban_sdk::{contracterror, contracttype, Address};

// Issue #946 & #949: modular, well-documented error enum with descriptive variant names
// so client-side code can distinguish failure modes without parsing panic strings.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TimeLockError {
    /// Contract has already been initialized; re-initialization is rejected.
    AlreadyInitialized = 1,
    /// Caller is not the contract admin.
    Unauthorized = 2,
    /// Caller does not hold the required role for this operation.
    InsufficientRole = 3,
    /// Lock duration must be greater than zero ledger seconds.
    ZeroDuration = 4,
    /// The locked amount must be strictly positive.
    ZeroAmount = 5,
    /// No lock record exists for the given lock ID.
    LockNotFound = 6,
    /// Lock is still within its time window; release is premature.
    LockNotMatured = 7,
    /// Lock has already been released or cancelled.
    AlreadyReleased = 8,
    /// The claimed beneficiary does not match the lock record.
    BeneficiaryMismatch = 9,
    /// Arithmetic overflow computing the release ledger timestamp.
    ExpiryOverflow = 10,
    /// Contract is paused; all state-mutating operations are blocked.
    ContractPaused = 11,
    /// Deposit amount of zero is rejected; would create an unclaimable lock.
    ZeroDeposit = 12,
    /// The target already has an active proposal (#949).
    TimelockActive = 13,
    /// No proposal exists for the target (#949).
    TimelockNotFound = 14,
    /// The proposal is not active (already executed or cancelled) (#949).
    TimelockInactive = 15,
    /// The ledger timestamp has not reached `executable_at` yet (#949).
    TooEarly = 16,
    /// `delay_secs` is zero (#949).
    ZeroDelay = 17,
    /// `now + delay_secs` overflowed `u64` (#949).
    ArithmeticOverflow = 18,
    /// A stored record violated a state invariant (#945).
    StateInconsistency = 19,
    /// The monotonic lock-ID counter reached `u64::MAX` (#945).
    LockCountOverflow = 20,
    /// The stored admin may not be demoted below `Role::Admin` (#945).
    AdminRoleLoss = 21,
}

impl TimeLockError {
    /// Human-readable message for each error variant, provided so clients
    /// and developers can debug failures with context instead of parsing
    /// numeric codes.
    pub fn message(&self) -> &'static str {
        match self {
            TimeLockError::AlreadyInitialized => "contract already initialized",
            TimeLockError::Unauthorized => "caller is not the contract admin",
            TimeLockError::InsufficientRole => "caller does not hold the required role",
            TimeLockError::ZeroDuration => "lock duration must be greater than zero",
            TimeLockError::ZeroAmount => "locked amount must be strictly positive",
            TimeLockError::LockNotFound => "no lock record exists for the given lock id",
            TimeLockError::LockNotMatured => "lock has not matured yet",
            TimeLockError::AlreadyReleased => "lock is already released or cancelled",
            TimeLockError::BeneficiaryMismatch => {
                "claimed beneficiary does not match the lock record"
            }
            TimeLockError::ExpiryOverflow => "release timestamp overflow",
            TimeLockError::ContractPaused => "contract is paused",
            TimeLockError::ZeroDeposit => "deposit of zero is not allowed; create no lock instead",
            TimeLockError::TimelockActive => "target already has an active timelock proposal",
            TimeLockError::TimelockNotFound => "no timelock proposal exists for target",
            TimeLockError::TimelockInactive => "timelock proposal is no longer active",
            TimeLockError::TooEarly => "timelock delay has not elapsed yet",
            TimeLockError::ZeroDelay => "timelock delay must be greater than zero",
            TimeLockError::ArithmeticOverflow => "timelock timestamp arithmetic overflowed",
            TimeLockError::StateInconsistency => "stored state failed an invariant check",
            TimeLockError::LockCountOverflow => "monotonic lock id counter is exhausted",
            TimeLockError::AdminRoleLoss => "stored admin cannot lose the admin role",
        }
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LockStatus {
    /// Funds are locked and not yet claimable.
    Active,
    /// Funds have been released to the beneficiary.
    Released,
    /// Lock was cancelled before maturity (admin-only action).
    Cancelled,
}

/// Lifecycle state of a timelocked proposal (#949).
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

/// A timelocked proposal on a target (#949).
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

/// Role assigned to an address. Roles are ordered by privilege level:
/// `Operator` may create locks, `Admin` may also release and manage the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Role {
    /// No special privileges — default for unknown addresses.
    None,
    /// May deposit and create time-locks.
    Operator,
    /// Full administrative access: initialize, pause, cancel locks, assign roles.
    Admin,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeLock {
    pub id: u64,
    pub depositor: Address,
    pub beneficiary: Address,
    /// Locked token amount in the smallest token unit (stroops for XLM-based tokens).
    pub amount: i128,
    /// Ledger timestamp (seconds since epoch) after which release is permitted.
    pub release_at: u64,
    pub status: LockStatus,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Total number of locks ever created (monotonically increasing).
    LockCount,
    /// Individual lock record keyed by sequential ID.
    Lock(u64),
    /// Role assigned to a given address.
    Role(Address),
    /// Whether the contract is currently paused.
    Paused,
    /// Timelock proposal per target (instance storage, #949).
    TimelockProposal(Address),
}
