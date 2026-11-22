//! Contract-level errors for the event-emitter contract.

use soroban_sdk::contracterror;

/// Errors for the event-emitter contract.
///
/// Discriminants are part of the on-chain ABI: variants are append-only and
/// must never be renumbered or removed (see `scripts/check-enum-ordering.sh`).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EventEmitterError {
    /// `initialize` was called more than once.
    AlreadyInitialized = 1,
    /// Caller is not the stored admin.
    Unauthorized = 2,
    /// Negative or otherwise invalid amount passed by the caller.
    InvalidAmount = 3,
    /// The partial-settlement subtraction underflowed: `settlement_total -
    /// emitted_amount` had no non-negative result. See #950.
    Underflow = 4,
    /// Granting a settlement agent that is already authorized. (#951)
    AgentAlreadyGranted = 5,
    /// Revoking a settlement agent that is not authorized. (#951)
    AgentNotGranted = 6,
}
