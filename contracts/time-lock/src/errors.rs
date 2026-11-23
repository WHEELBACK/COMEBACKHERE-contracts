//! Error surface of the time-lock contract (#946).
//!
//! All failure information lives in this module so clients have a single place
//! to import:
//!
//! - [`TimeLockError`] is the contract-level error type returned by every
//!   fallible entrypoint. Its discriminants are part of the on-chain ABI and
//!   are append-only, so existing codes never change meaning.
//! - [`TimeLockError::message`] and [`message_for`] resolve a code to a stable,
//!   human-readable explanation. [`TimeLockContract::get_error_message`](crate::TimeLockContract::get_error_message)
//!   exposes the same mapping on-chain for clients that only received a raw
//!   `Error(Contract, #n)`.
//! - [`TimeLockError::category`] groups codes by failure domain, making it
//!   easier for monitoring tools to triage a failed transaction.

use soroban_sdk::contracterror;

/// Contract-level errors for the time-lock contract.
///
/// Discriminants are append-only: never renumber or remove a variant, since
/// clients and deployed ledgers depend on the numeric codes.
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
    /// Every error code this contract can return, in discriminant order.
    pub const ALL: [Self; 21] = [
        Self::AlreadyInitialized,
        Self::Unauthorized,
        Self::InsufficientRole,
        Self::ZeroDuration,
        Self::ZeroAmount,
        Self::LockNotFound,
        Self::LockNotMatured,
        Self::AlreadyReleased,
        Self::BeneficiaryMismatch,
        Self::ExpiryOverflow,
        Self::ContractPaused,
        Self::ZeroDeposit,
        Self::TimelockActive,
        Self::TimelockNotFound,
        Self::TimelockInactive,
        Self::TooEarly,
        Self::ZeroDelay,
        Self::ArithmeticOverflow,
        Self::StateInconsistency,
        Self::LockCountOverflow,
        Self::AdminRoleLoss,
    ];

    /// Numeric error code as reported in `Error(Contract, #n)`.
    pub fn code(self) -> u32 {
        self as u32
    }

    /// Recovers a typed error from a raw contract error code, or `None` if the
    /// code is not part of this contract's ABI.
    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.iter().copied().find(|e| e.code() == code)
    }

    /// Human-readable message for this variant, used by clients and developers
    /// to debug failures without parsing panic strings.
    pub fn message(&self) -> &'static str {
        match self {
            Self::AlreadyInitialized => "contract already initialized",
            Self::Unauthorized => "caller is not the contract admin",
            Self::InsufficientRole => "caller does not hold the required role",
            Self::ZeroDuration => "lock duration must be greater than zero",
            Self::ZeroAmount => "locked amount must be strictly positive",
            Self::LockNotFound => "no lock record exists for the given lock id",
            Self::LockNotMatured => "lock has not matured yet",
            Self::AlreadyReleased => "lock is already released or cancelled",
            Self::BeneficiaryMismatch => "claimed beneficiary does not match the lock record",
            Self::ExpiryOverflow => "release timestamp overflow",
            Self::ContractPaused => "contract is paused",
            Self::ZeroDeposit => "deposit of zero is not allowed; create no lock instead",
            Self::TimelockActive => "target already has an active timelock proposal",
            Self::TimelockNotFound => "no timelock proposal exists for target",
            Self::TimelockInactive => "timelock proposal is no longer active",
            Self::TooEarly => "timelock delay has not elapsed yet",
            Self::ZeroDelay => "timelock delay must be greater than zero",
            Self::ArithmeticOverflow => "timelock timestamp arithmetic overflowed",
            Self::StateInconsistency => "stored state failed an invariant check",
            Self::LockCountOverflow => "monotonic lock id counter is exhausted",
            Self::AdminRoleLoss => "stored admin cannot lose the admin role",
        }
    }

    /// Coarse failure domain so monitoring tools can triage failures without
    /// matching every variant.
    pub fn category(&self) -> &'static str {
        match self {
            Self::AlreadyInitialized
            | Self::LockNotFound
            | Self::AlreadyReleased
            | Self::StateInconsistency => "state",
            Self::Unauthorized | Self::InsufficientRole | Self::AdminRoleLoss => "authorization",
            Self::ZeroDuration | Self::ZeroAmount | Self::ZeroDeposit | Self::ZeroDelay => {
                "validation"
            }
            Self::LockNotMatured | Self::ContractPaused | Self::TooEarly => "timing",
            Self::ExpiryOverflow | Self::ArithmeticOverflow | Self::LockCountOverflow => {
                "arithmetic"
            }
            Self::BeneficiaryMismatch => "ownership",
            Self::TimelockActive | Self::TimelockNotFound | Self::TimelockInactive => "proposal",
        }
    }
}

impl core::fmt::Display for TimeLockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.message())
    }
}

/// Message for a raw error code, or `None` for a code this contract never
/// returns. Mirrors the on-chain `get_error_message` entrypoint.
pub fn message_for(code: u32) -> Option<&'static str> {
    TimeLockError::from_code(code).map(|error| error.message())
}

/// Failure domain for a raw error code.
pub fn category_for(code: u32) -> Option<&'static str> {
    TimeLockError::from_code(code).map(|error| error.category())
}
