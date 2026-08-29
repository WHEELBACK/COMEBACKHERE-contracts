#![no_std]

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
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
}
