//! Human-readable messages for [`InvoiceError`] (#883).
//!
//! The on-chain ABI only carries the numeric code. Clients, indexers and test
//! output need a stable description to make that code debuggable, and every
//! consumer used to keep its own copy (see `protocol-errors`). This module is
//! the single source of those strings, kept next to the enum so a new variant
//! cannot be added without a message: the `match` below is exhaustive.

use crate::InvoiceError;

impl InvoiceError {
    /// Every variant, in discriminant order. Useful for clients building a
    /// code → message table and for exhaustive tests.
    pub const ALL: [InvoiceError; 36] = [
        InvoiceError::Unauthorized,
        InvoiceError::ContractPaused,
        InvoiceError::InvalidAmount,
        InvoiceError::NotPending,
        InvoiceError::Expired,
        InvoiceError::NotFound,
        InvoiceError::AlreadyInitialized,
        InvoiceError::ZeroDuration,
        InvoiceError::ExpiryOverflow,
        InvoiceError::NotPaid,
        InvoiceError::NotReleased,
        InvoiceError::AmountPrecision,
        InvoiceError::DuplicateNonce,
        InvoiceError::ExpiryTooLong,
        InvoiceError::MetadataMismatch,
        InvoiceError::NoPendingAdmin,
        InvoiceError::InvalidPaymentLinkHash,
        InvoiceError::NotRefundRequested,
        InvoiceError::TokenMismatch,
        InvoiceError::BatchTooLarge,
        InvoiceError::CooldownActive,
        InvoiceError::InvoiceCountOverflow,
        InvoiceError::HashTooLong,
        InvoiceError::PaymentStateInconsistent,
        InvoiceError::RefundTransferFailed,
        InvoiceError::RefundTokenNotSet,
        InvoiceError::RefundFeeTooHigh,
        InvoiceError::ArithmeticOverflow,
        InvoiceError::DiscountExceedsAmount,
        InvoiceError::DiscountDeadlineAfterExpiry,
        InvoiceError::MemoTooLong,
        InvoiceError::RefundWindowClosed,
        InvoiceError::SelfPayNotAllowed,
        InvoiceError::TemplateDisabled,
        InvoiceError::InvalidLateFee,
        InvoiceError::InvariantViolation,
    ];

    /// The stable numeric code surfaced on-chain for this error.
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// Looks up a variant by its on-chain code, e.g. to decode a
    /// `HostError: Error(Contract, #N)` seen by a client.
    pub fn from_code(code: u32) -> Option<InvoiceError> {
        InvoiceError::ALL.iter().copied().find(|e| e.code() == code)
    }

    /// A short, user-facing description of the error.
    pub const fn message(self) -> &'static str {
        use InvoiceError::*;
        match self {
            Unauthorized => "Caller is not authorized for this action.",
            ContractPaused => "The invoice contract is paused.",
            InvalidAmount => "The invoice amount is invalid.",
            NotPending => "The invoice is not pending.",
            Expired => "The invoice has expired.",
            NotFound => "Invoice not found.",
            AlreadyInitialized => "The invoice contract is already initialized.",
            ZeroDuration => "The invoice duration must be greater than zero.",
            ExpiryOverflow => "The invoice expiry time is out of range.",
            NotPaid => "The invoice has not been paid.",
            NotReleased => "The invoice escrow has not been released.",
            AmountPrecision => "The amount has too many decimal places.",
            DuplicateNonce => "This invoice nonce has already been used.",
            ExpiryTooLong => "The invoice expiry is too far in the future.",
            MetadataMismatch => "The invoice metadata does not match.",
            NoPendingAdmin => "There is no pending admin transfer.",
            InvalidPaymentLinkHash => "The payment link hash is invalid.",
            NotRefundRequested => "No refund has been requested for this invoice.",
            TokenMismatch => "The payment token does not match the invoice.",
            BatchTooLarge => "Too many items in one batch.",
            CooldownActive => "Please wait before trying this action again.",
            InvoiceCountOverflow => "The invoice limit has been reached.",
            HashTooLong => "The provided hash is too long.",
            PaymentStateInconsistent => "The invoice does not describe a completed payment.",
            RefundTransferFailed => "The refund transfer failed.",
            RefundTokenNotSet => "The invoice has no token to refund through.",
            RefundFeeTooHigh => "The refund fee is too high.",
            ArithmeticOverflow => "The calculation overflowed.",
            DiscountExceedsAmount => "The discount amount exceeds the invoice amount.",
            DiscountDeadlineAfterExpiry => "The discount deadline is after the invoice expiry.",
            MemoTooLong => "The memo field is too long.",
            RefundWindowClosed => "The refund window for this invoice has closed.",
            SelfPayNotAllowed => "A merchant cannot pay its own invoice.",
            TemplateDisabled => "The invoice template is disabled.",
            InvalidLateFee => "The late fee exceeds 100%.",
            InvariantViolation => "The invoice state is internally inconsistent.",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{InvoiceError, InvoiceErrorCategory};
    use std::collections::BTreeSet;

    #[test]
    fn all_is_complete_and_in_code_order() {
        for (i, e) in InvoiceError::ALL.iter().enumerate() {
            assert_eq!(e.code(), i as u32 + 1, "{e:?} is out of order in ALL");
        }
    }

    #[test]
    fn every_variant_has_a_unique_non_empty_message() {
        let mut seen = BTreeSet::new();
        for e in InvoiceError::ALL {
            assert!(!e.message().is_empty(), "{e:?} has an empty message");
            assert!(seen.insert(e.message()), "{e:?} reuses another message");
        }
    }

    #[test]
    fn from_code_round_trips() {
        for e in InvoiceError::ALL {
            assert_eq!(InvoiceError::from_code(e.code()), Some(e));
        }
        assert_eq!(InvoiceError::from_code(0), None);
        assert_eq!(InvoiceError::from_code(InvoiceError::ALL.len() as u32 + 1), None);
    }

    #[test]
    fn every_variant_has_a_category() {
        for e in InvoiceError::ALL {
            let category: InvoiceErrorCategory = e.category();
            assert!(!category.as_str().is_empty());
        }
    }
}
