//! Contract-level errors for the time-lock contract.

use soroban_sdk::contracterror;

/// Errors for the time-lock contract.
///
/// Discriminants are part of the on-chain ABI: variants are append-only and
/// must never be renumbered or removed (see `scripts/check-enum-ordering.sh`).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TimeLockError {
    /// `initialize` was called more than once.
    AlreadyInitialized = 1,
    /// Caller is not the stored admin.
    Unauthorized = 2,
    /// The target already has an active proposal.
    TimelockActive = 3,
    /// No proposal exists for the target.
    TimelockNotFound = 4,
    /// The proposal is not active (already executed or cancelled).
    TimelockInactive = 5,
    /// The ledger timestamp has not reached `executable_at` yet.
    TooEarly = 6,
    /// `delay_secs` is zero.
    ZeroDelay = 7,
    /// `now + delay_secs` overflowed `u64`.
    ArithmeticOverflow = 8,
}
