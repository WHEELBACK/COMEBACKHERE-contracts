use soroban_sdk::{contracttype, Address};

// Error types and their client-facing messages live in `crate::errors` (#946).

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
