// Issue #901 — Dedicated error enum with descriptive variant names.
// Every variant maps to a specific, named failure condition.
// No catch-all / generic errors.

use soroban_sdk::contracterror;

/// All failure conditions the fee-manager contract can surface to callers.
///
/// Variants are numbered starting at 1 and must only be appended (never
/// renumbered) to preserve on-chain ABI compatibility.
#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum FeeManagerError {
    /// `initialize` was called on a contract that has already been set up.
    AlreadyInitialized = 1,

    /// A fee rate outside the valid [0, 10_000] basis-point range was supplied.
    InvalidFeeRate = 2,

    /// The caller supplied a zero amount where a positive amount is required
    /// (deposit, collect_fees). Zero-amount operations are rejected explicitly
    /// rather than silently succeeding.
    ZeroAmount = 3,

    /// A signed integer arithmetic operation (checked_add / checked_sub) overflowed.
    ArithmeticOverflow = 4,

    /// The accumulated fee balance for the requested token is zero — there is
    /// nothing to collect.
    ZeroBalance = 5,

    /// The caller is not the stored administrator.
    Unauthorized = 6,

    /// A state-mutating call was made while the contract is paused.
    ContractPaused = 7,
}
