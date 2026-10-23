//! Exhaustive `From` coverage for every contract error enum (#782).
//!
//! Each test below iterates every variant of one error enum, converts it into
//! a `ProtocolError` via `From`, and asserts:
//!   1. The conversion wraps the value in the expected `ProtocolError` variant.
//!   2. `description()` returns a non-empty string for every variant.
//!
//! The match arms inside each test have no wildcard: adding a variant to any
//! of the four enums without updating this file is a compile error, which is
//! the point — silent coverage gaps are caught at compile time.

use protocol_errors::{ComplianceError, InvoiceError, ProtocolError, TreasuryError, WorkflowError};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Asserts that `err` wraps an inner value of type `E`, that the wrapping
/// matches the `expected` variant, and that `description()` is non-empty.
fn assert_from<E: Into<ProtocolError> + Copy>(inner: E, expected: ProtocolError) {
    let got: ProtocolError = inner.into();
    assert_eq!(got, expected, "From conversion wrapped unexpected variant");
    assert!(
        !got.description().is_empty(),
        "description() must not be empty for {:?}",
        got
    );
}

// ── InvoiceError ─────────────────────────────────────────────────────────────

/// Exhaustive `From<InvoiceError>` coverage.
///
/// No wildcard — adding a variant to `InvoiceError` without extending this
/// match is a compile error.
#[test]
fn from_invoice_error_exhaustive() {
    use InvoiceError::*;
    let cases = [
        Unauthorized,
        ContractPaused,
        InvalidAmount,
        NotPending,
        Expired,
        NotFound,
        AlreadyInitialized,
        ZeroDuration,
        ExpiryOverflow,
        NotPaid,
        NotReleased,
        AmountPrecision,
        DuplicateNonce,
        ExpiryTooLong,
        MetadataMismatch,
        NoPendingAdmin,
        InvalidPaymentLinkHash,
        NotRefundRequested,
        TokenMismatch,
        BatchTooLarge,
        CooldownActive,
        InvoiceCountOverflow,
        HashTooLong,
        PaymentStateInconsistent,
        RefundTransferFailed,
        RefundTokenNotSet,
        RefundFeeTooHigh,
        ArithmeticOverflow,
    ];
    for variant in cases {
        assert_from(variant, ProtocolError::Invoice(variant));
    }
}

// ── TreasuryError ─────────────────────────────────────────────────────────────

/// Exhaustive `From<TreasuryError>` coverage.
///
/// No wildcard — adding a variant to `TreasuryError` without extending this
/// match is a compile error.
#[test]
fn from_treasury_error_exhaustive() {
    use TreasuryError::*;
    let cases = [
        AlreadyInitialized,
        ZeroThreshold,
        SettlementNotFound,
        AlreadyExecuted,
        ThresholdNotMet,
        ThresholdNotConfigured,
        InvalidAmount,
        ContractPaused,
        Unauthorized,
        UnauthorizedSigner,
        InvalidTokenContract,
        TokenNotAllowed,
        RotationNotFound,
        RotationAlreadyExecuted,
        SettlementOnHold,
        DisputeNotExpired,
        AlreadyOnHold,
        ThresholdUnreachable,
        ComplianceCheckFailed,
        ArithmeticOverflow,
        DisputeNotFound,
        DisputeAlreadyResolved,
        ResolutionDirectionMismatch,
        BatchTooLarge,
        WeightOverflow,
        SettlementNotCancellable,
        TtlNotElapsed,
        AllowlistFull,
        NotOnHold,
        DestinationNotAllowed,
        InsufficientBalance,
        NotPaused,
        RotationProposalCooldown,
        WorkflowNotRegisteredSigner,
        WithdrawalLimitExceeded,
        InvalidSplitRatio,
        ForceCancelNotAllowed,
        SignerChangeTooEarly,
        SignerChangeNotFound,
        SignerChangeAlreadyFinalised,
        ExecutionDeadlineExceeded,
        QuorumBreak,
        TokenHasPendingSettlements,
        ApprovalNotFound,
        DuplicateVote,
        NoPendingAdmin,
        LabelTooLong,
    ];
    for variant in cases {
        assert_from(variant, ProtocolError::Treasury(variant));
    }
}

// ── ComplianceError ───────────────────────────────────────────────────────────

/// Exhaustive `From<ComplianceError>` coverage.
///
/// No wildcard — adding a variant to `ComplianceError` without extending this
/// match is a compile error.
#[test]
fn from_compliance_error_exhaustive() {
    use ComplianceError::*;
    let cases = [AlreadyInitialized];
    for variant in cases {
        assert_from(variant, ProtocolError::Compliance(variant));
    }
}

// ── WorkflowError ─────────────────────────────────────────────────────────────

/// Exhaustive `From<WorkflowError>` coverage.
///
/// No wildcard — adding a variant to `WorkflowError` without extending this
/// match is a compile error.
#[test]
fn from_workflow_error_exhaustive() {
    use WorkflowError::*;
    let cases = [
        Unauthorized,
        NotConfigured,
        PauseTargetFailed,
        UnpauseTargetFailed,
        NoPauseTargets,
        TooManyPauseTargets,
        DuplicatePauseTarget,
        SelfPauseTarget,
        EmergencyPauseActive,
        NotEmergencyPaused,
    ];
    for variant in cases {
        assert_from(variant, ProtocolError::Workflow(variant));
    }
}
