#![no_std]

#[cfg(test)]
extern crate std;

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Error codes for the invoice contract.
    ///
    /// Variants are append-only and must not be renumbered; discriminants are
    /// part of the on-chain ABI and are matched by callers and off-chain systems.
    /// New variants must be added at the end with an explicit discriminant one
    /// higher than the current maximum; see `scripts/check-enum-ordering.sh`.
    pub enum InvoiceError {
        /// Caller is not authorized for this operation.
        Unauthorized = 1,
        /// Contract is currently paused.
        ContractPaused = 2,
        /// Invoice or refund amount is invalid (e.g. zero or negative).
        InvalidAmount = 3,
        /// Invoice is not in the `Pending` state.
        NotPending = 4,
        /// Invoice has expired.
        Expired = 5,
        /// Requested invoice or template does not exist.
        NotFound = 6,
        /// Contract has already been initialized.
        AlreadyInitialized = 7,
        /// Duration argument is zero.
        ZeroDuration = 8,
        /// Expiry timestamp calculation overflowed.
        ExpiryOverflow = 9,
        /// Invoice has not been paid yet.
        NotPaid = 10,
        /// Escrow has not been released yet.
        NotReleased = 11,
        /// Amount does not meet required precision constraints.
        AmountPrecision = 12,
        /// Nonce has already been used for this merchant.
        DuplicateNonce = 13,
        /// Requested expiry duration exceeds the configured maximum.
        ExpiryTooLong = 14,
        /// Supplied metadata does not match the invoice record.
        MetadataMismatch = 15,
        /// No pending admin transfer is active.
        NoPendingAdmin = 16,
        /// Payment link hash is invalid.
        InvalidPaymentLinkHash = 17,
        /// Invoice does not have an active refund request.
        NotRefundRequested = 18,
        /// Supplied token address does not match the invoice token.
        TokenMismatch = 19,
        /// Batch argument exceeds the maximum allowed size.
        BatchTooLarge = 20,
        /// Operation is still within its cooldown window.
        CooldownActive = 21,
        /// Invoice counter overflowed `u64::MAX`.
        InvoiceCountOverflow = 22,
        /// Hash argument exceeds the maximum permitted length.
        HashTooLong = 23,
        /// Stored invoice does not describe a completed payment (#70).
        PaymentStateInconsistent = 24,
        /// Cross-contract refund token transfer failed (#70).
        RefundTransferFailed = 25,
        /// Invoice was created without a `token_address` (#70).
        RefundTokenNotSet = 26,
        /// Refund fee exceeds `MAX_REFUND_FEE_BPS` (#71).
        RefundFeeTooHigh = 27,
        /// Refund fee arithmetic overflowed (#71).
        ArithmeticOverflow = 28,
        /// Discount amount exceeds the invoice amount (#708).
        DiscountExceedsAmount = 29,
        /// Discount deadline falls after the invoice expiry (#708).
        DiscountDeadlineAfterExpiry = 30,
        /// Memo exceeds `MAX_MEMO_BYTES` (#709).
        MemoTooLong = 31,
        /// Refund requested after the refund window elapsed (#702).
        RefundWindowClosed = 32,
        /// Payer cannot equal the invoice merchant (#717).
        SelfPayNotAllowed = 33,
        /// Invoice template has been deactivated (#694).
        TemplateDisabled = 34,
        /// Late fee exceeds 10_000 bps (#704).
        InvalidLateFee = 35,
        /// Invoice write would violate a contract invariant (#882).
        InvariantViolation = 36,
    }
}

// #883: client-facing debugging metadata lives in its own modules, so the
// ABI-bearing enum above stays a plain, append-only list of codes.
mod category;
mod message;

pub use category::InvoiceErrorCategory;
