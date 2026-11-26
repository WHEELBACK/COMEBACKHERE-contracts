//! Coarse grouping of [`InvoiceError`] codes (#883).
//!
//! The ABI enum is one flat, append-only list of 30+ codes. Clients usually
//! want to react to the *kind* of failure (retry later, fix the input, ask an
//! admin) rather than to each code, so every variant is assigned exactly one
//! category here. The `match` is exhaustive, so a new variant cannot be added
//! without classifying it.

use crate::InvoiceError;

/// The kind of failure an [`InvoiceError`] represents.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum InvoiceErrorCategory {
    /// The caller lacks the required role or identity.
    Authorization,
    /// The invoice (or contract) is not in a state that allows the action.
    Lifecycle,
    /// An input argument was rejected.
    Validation,
    /// The refund path failed or was refused.
    Refund,
    /// A counter or amount computation would overflow.
    Arithmetic,
    /// Contract-level configuration or admin-transfer state.
    Configuration,
    /// Stored state failed an integrity check.
    Integrity,
}

impl InvoiceErrorCategory {
    /// Stable lowercase name, suitable for logs and metrics labels.
    pub const fn as_str(self) -> &'static str {
        match self {
            InvoiceErrorCategory::Authorization => "authorization",
            InvoiceErrorCategory::Lifecycle => "lifecycle",
            InvoiceErrorCategory::Validation => "validation",
            InvoiceErrorCategory::Refund => "refund",
            InvoiceErrorCategory::Arithmetic => "arithmetic",
            InvoiceErrorCategory::Configuration => "configuration",
            InvoiceErrorCategory::Integrity => "integrity",
        }
    }
}

impl InvoiceError {
    /// The category this error belongs to.
    pub const fn category(self) -> InvoiceErrorCategory {
        use InvoiceError::*;
        use InvoiceErrorCategory as C;
        match self {
            Unauthorized | SelfPayNotAllowed => C::Authorization,

            ContractPaused | NotPending | Expired | NotFound | NotPaid | NotReleased
            | CooldownActive | TemplateDisabled => C::Lifecycle,

            InvalidAmount
            | ZeroDuration
            | AmountPrecision
            | DuplicateNonce
            | ExpiryTooLong
            | MetadataMismatch
            | InvalidPaymentLinkHash
            | TokenMismatch
            | BatchTooLarge
            | HashTooLong
            | DiscountExceedsAmount
            | DiscountDeadlineAfterExpiry
            | MemoTooLong
            | InvalidLateFee => C::Validation,

            NotRefundRequested | RefundTransferFailed | RefundTokenNotSet | RefundFeeTooHigh
            | RefundWindowClosed => C::Refund,

            ExpiryOverflow | InvoiceCountOverflow | ArithmeticOverflow => C::Arithmetic,

            AlreadyInitialized | NoPendingAdmin => C::Configuration,

            PaymentStateInconsistent | InvariantViolation => C::Integrity,
        }
    }
}
