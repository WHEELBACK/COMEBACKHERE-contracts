//! `fmt_error` — human-readable debug metadata for `TreasuryError`.
//!
//! The canonical `TreasuryError` enum lives in `crates/multisig/src/lib.rs`
//! and is re-exported by `contracts/treasury/src/lib.rs`. This module adds
//! two companion types that clients can use for logging, dashboards, and
//! off-chain tooling **without** depending on `std`:
//!
//! * [`ErrorCategory`] — a `#[no_std]`-compatible enum that groups every
//!   `TreasuryError` discriminant into a broad category.
//! * [`error_category`] — maps a `TreasuryError` to its [`ErrorCategory`] and
//!   returns a fixed human-readable `&'static str` description suitable for
//!   structured log fields.
//!
//! Neither type is ever stored on-chain or included in the contract ABI.

use crate::TreasuryError;

/// Broad category groupings for [`TreasuryError`] variants.
///
/// Used by [`error_category`] to provide a stable, coarse-grained label that
/// client code can branch on without exhaustively matching every variant.
/// New variants should be added to the `Other` bucket first, then promoted to
/// a dedicated variant once a clear grouping emerges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCategory {
    /// The caller does not have permission to perform the requested action
    /// (e.g. wrong admin, unregistered signer, missing `require_auth`).
    Authorization,
    /// The request could not be satisfied because the targeted resource
    /// (settlement, dispute, rotation, signer, etc.) was not found or is
    /// already in a terminal state.
    State,
    /// A numeric argument or accumulated value is out of the accepted range
    /// (e.g. zero amount, weight overflow, invalid basis-point ratio).
    Validation,
    /// A contract-level configuration is missing or inconsistent
    /// (e.g. threshold not set, token not on allowlist, compliance check failed).
    Configuration,
    /// A ledger-time constraint was violated (e.g. proposal expired, hold
    /// expired, execution deadline exceeded, timelock not elapsed).
    Timing,
    /// An on-chain resource limit was reached (e.g. batch too large, allowlist full).
    Capacity,
    /// Errors that do not fit neatly into another category.
    Other,
}

/// Maps a [`TreasuryError`] to a `(category_label, description)` pair.
///
/// Both values are `&'static str` and are safe to use in `#[no_std]`
/// contexts.  The returned strings are suitable for structured log fields,
/// monitoring dashboards, and client-side error display.  They are **not**
/// on-chain values and may change between releases without breaking ABI
/// compatibility.
///
/// # Examples
///
/// ```rust,ignore
/// use treasury::{TreasuryError, errors::error_category};
///
/// let (category, description) = error_category(TreasuryError::Unauthorized);
/// assert_eq!(category, "Authorization");
/// assert_eq!(description, "caller is not authorized");
/// ```
pub fn error_category(e: TreasuryError) -> (&'static str, &'static str) {
    match e {
        // ── Authorization (discriminants 9, 10, 34) ──────────────────────
        TreasuryError::Unauthorized => (
            "Authorization",
            "caller is not authorized",
        ),
        TreasuryError::UnauthorizedSigner => (
            "Authorization",
            "signer is not registered or has zero weight",
        ),
        TreasuryError::WorkflowNotRegisteredSigner => (
            "Authorization",
            "settlement-workflow contract is not a registered signer",
        ),

        // ── State (discriminants 1, 3, 4, 13, 14, 15, 17, 21, 22, 26,
        //          29, 37, 39, 40, 44, 46) ─────────────────────────────────
        TreasuryError::AlreadyInitialized => (
            "State",
            "contract has already been initialized",
        ),
        TreasuryError::SettlementNotFound => (
            "State",
            "settlement not found",
        ),
        TreasuryError::AlreadyExecuted => (
            "State",
            "settlement is already in a terminal state",
        ),
        TreasuryError::RotationNotFound => (
            "State",
            "signer rotation proposal not found",
        ),
        TreasuryError::RotationAlreadyExecuted => (
            "State",
            "signer rotation proposal has already been executed",
        ),
        TreasuryError::SettlementOnHold => (
            "State",
            "settlement is currently on hold and cannot be executed",
        ),
        TreasuryError::AlreadyOnHold => (
            "State",
            "settlement is already on hold",
        ),
        TreasuryError::DisputeNotFound => (
            "State",
            "dispute not found",
        ),
        TreasuryError::DisputeAlreadyResolved => (
            "State",
            "dispute has already been resolved",
        ),
        TreasuryError::SettlementNotCancellable => (
            "State",
            "settlement cannot be cancelled in its current state",
        ),
        TreasuryError::NotOnHold => (
            "State",
            "settlement is not on hold",
        ),
        TreasuryError::ForceCancelNotAllowed => (
            "State",
            "settlement is in a terminal state and cannot be force-cancelled",
        ),
        TreasuryError::SignerChangeNotFound => (
            "State",
            "signer change proposal not found",
        ),
        TreasuryError::SignerChangeAlreadyFinalised => (
            "State",
            "signer change proposal has already been executed or cancelled",
        ),
        TreasuryError::ApprovalNotFound => (
            "State",
            "approval from this signer was not found",
        ),
        TreasuryError::NoPendingAdmin => (
            "State",
            "no pending admin transfer exists or it has expired",
        ),

        // ── Validation (discriminants 2, 7, 20, 23, 24, 25, 31, 36,
        //               42, 44, 45, 48, 49) ────────────────────────────────
        TreasuryError::ZeroThreshold => (
            "Validation",
            "approval threshold must be greater than zero",
        ),
        TreasuryError::InvalidAmount => (
            "Validation",
            "amount is invalid (zero, negative, or out of range)",
        ),
        TreasuryError::ArithmeticOverflow => (
            "Validation",
            "arithmetic operation overflowed",
        ),
        TreasuryError::ResolutionDirectionMismatch => (
            "Validation",
            "resolution direction does not match the existing resolution vote",
        ),
        TreasuryError::WeightOverflow => (
            "Validation",
            "accumulated signer weight overflowed u32",
        ),
        TreasuryError::InsufficientBalance => (
            "Validation",
            "depositor balance is insufficient for the requested withdrawal",
        ),
        TreasuryError::InvalidSplitRatio => (
            "Validation",
            "claimant split basis-points exceeds 10 000",
        ),
        TreasuryError::QuorumBreak => (
            "Validation",
            "removing this signer would drop total weight below the threshold",
        ),
        TreasuryError::DuplicateVote => (
            "Validation",
            "this signer has already voted on the dispute resolution",
        ),
        TreasuryError::InvalidTimelockDelay => (
            "Validation",
            "proposed timelock delay is outside the allowed bounds",
        ),
        TreasuryError::DuplicateReferenceId => (
            "Validation",
            "deposit reference id has already been used within its TTL window",
        ),
        TreasuryError::WithdrawalLimitExceeded => (
            "Validation",
            "withdrawal would exceed the per-window limit",
        ),

        // ── Configuration (discriminants 5, 6, 11, 12, 18, 19, 30, 43, 47, 50) ──
        TreasuryError::ThresholdNotMet => (
            "Configuration",
            "approval weight does not yet meet the threshold",
        ),
        TreasuryError::ThresholdNotConfigured => (
            "Configuration",
            "approval threshold has not been configured",
        ),
        TreasuryError::InvalidTokenContract => (
            "Configuration",
            "token contract address is invalid (e.g. self-reference)",
        ),
        TreasuryError::TokenNotAllowed => (
            "Configuration",
            "token is not on the treasury allowlist",
        ),
        TreasuryError::ThresholdUnreachable => (
            "Configuration",
            "new threshold exceeds total registered signer weight",
        ),
        TreasuryError::ComplianceCheckFailed => (
            "Configuration",
            "recipient failed the compliance allowlist check",
        ),
        TreasuryError::DestinationNotAllowed => (
            "Configuration",
            "withdrawal destination is not on the allowlist",
        ),
        TreasuryError::TokenHasPendingSettlements => (
            "Configuration",
            "token cannot be removed while pending settlements reference it",
        ),
        TreasuryError::LabelTooLong => (
            "Configuration",
            "signer label exceeds the maximum allowed length",
        ),
        TreasuryError::TokenHasBalance => (
            "Configuration",
            "token cannot be removed while depositors hold balances",
        ),

        // ── Timing (discriminants 8, 16, 27, 32, 33, 38, 41) ────────────
        TreasuryError::ContractPaused => (
            "Timing",
            "contract is paused; state-mutating operations are blocked",
        ),
        TreasuryError::DisputeNotExpired => (
            "Timing",
            "dispute has not yet expired",
        ),
        TreasuryError::TtlNotElapsed => (
            "Timing",
            "TTL window has not elapsed",
        ),
        TreasuryError::NotPaused => (
            "Timing",
            "operation requires the contract to be paused",
        ),
        TreasuryError::RotationProposalCooldown => (
            "Timing",
            "a rotation proposal for this signer is still within its cooldown window",
        ),
        TreasuryError::SignerChangeTooEarly => (
            "Timing",
            "signer change cannot be executed before its timelock delay has elapsed",
        ),
        TreasuryError::ExecutionDeadlineExceeded => (
            "Timing",
            "execution deadline set by the proposer has passed",
        ),

        // ── Capacity (discriminants 24, 28) ──────────────────────────────
        TreasuryError::BatchTooLarge => (
            "Capacity",
            "batch exceeds the maximum allowed size",
        ),
        TreasuryError::AllowlistFull => (
            "Capacity",
            "token allowlist has reached the maximum allowed size",
        ),
    }
}
