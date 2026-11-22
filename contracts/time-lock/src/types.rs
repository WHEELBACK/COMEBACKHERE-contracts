use soroban_sdk::{contracterror, contracttype, Address};

// Issue #946: modular, well-documented error enum with descriptive variant names
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
}
