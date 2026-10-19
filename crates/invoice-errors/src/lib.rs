#![no_std]

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Error codes for the invoice contract.
    ///
    /// Variants are append-only and must not be renumbered; discriminants are
    /// part of the on-chain ABI and are matched by callers and off-chain systems.
    /// New variants must be added at the end with an explicit discriminant one
    /// higher than the current maximum; see `scripts/check-enum-ordering.sh`.
    pub enum InvoiceError {
        Unauthorized = 1,
        ContractPaused = 2,
        InvalidAmount = 3,
        NotPending = 4,
        Expired = 5,
        NotFound = 6,
        AlreadyInitialized = 7,
        ZeroDuration = 8,
        ExpiryOverflow = 9,
        NotPaid = 10,
        NotReleased = 11,
        AmountPrecision = 12,
        DuplicateNonce = 13,
        ExpiryTooLong = 14,
        MetadataMismatch = 15,
        NoPendingAdmin = 16,
        InvalidPaymentLinkHash = 17,
        NotRefundRequested = 18,
        TokenMismatch = 19,
        BatchTooLarge = 20,
        CooldownActive = 21,
        InvoiceCountOverflow = 22,
        HashTooLong = 23,
        // Appended for #70: the stored invoice does not describe a completed
        // payment, so a refund must not be recorded or paid out against it. Raised
        // by `verify_payment_state` before any refund state is written.
        PaymentStateInconsistent = 24,
        // Appended for #70: the cross-contract call that executes the refund payout
        // failed (token contract paused, insufficient escrow balance, wrong token,
        // or not a SEP-41 contract at all). The refund is abandoned with no state
        // change rather than being marked `Refunded` without the customer being
        // paid.
        RefundTransferFailed = 25,
        // Appended for #70: the invoice was created without a `token_address`, so
        // there is no contract to execute the refund payout through.
        RefundTokenNotSet = 26,
        // Appended for #71: a refund fee above `MAX_REFUND_FEE_BPS` (10_000 bps =
        // 100%), which would make the deduction meaningless.
        RefundFeeTooHigh = 27,
        // Appended for #71: refund fee arithmetic (`gross_amount * fee_bps`)
        // overflowed, so the net payout cannot be computed.
        ArithmeticOverflow = 28,
        // Appended for #708: a discount amount that is larger than the invoice
        // amount, which would make the effective price negative or zero.
        DiscountExceedsAmount = 29,
        // Appended for #708: a discount early-pay deadline that falls after the
        // invoice expiry, making it impossible to honour the discount window.
        DiscountDeadlineAfterExpiry = 30,
        // Appended for #709: the memo field supplied to create_invoice exceeds
        // MAX_MEMO_BYTES, which would waste storage and make deserialization
        // unpredictable for off-chain consumers.
        MemoTooLong = 31,
        // Appended for #702: request_refund was called after the configurable
        // refund window (paid_at + refund_window_secs) has elapsed. Zero means
        // no limit, preserving the current behaviour.
        RefundWindowClosed = 32,
        // Appended for #717: mark_paid called with payer == the invoice's
        // merchant (self-pay), which could inflate volume metrics.
        SelfPayNotAllowed = 33,
        // Appended for #694: generate_from_template was called on a template
        // that has been deactivated.
        TemplateDisabled = 34,
        // Appended for #704: set_late_fee_bps called with more than 10_000 bps
        // (100% of the net amount).
        InvalidLateFee = 35,
    }
}
