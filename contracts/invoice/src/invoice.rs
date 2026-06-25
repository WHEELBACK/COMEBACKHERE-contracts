use soroban_sdk::{contracterror, contracttype, Address, Bytes};

/// USDC on Stellar uses 7 decimal places: 1 USDC = 10_000_000 stroops.
pub const USDC_FACTOR: i128 = 10_000_000;

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
    /// Invoice has not been released from escrow.
    NotReleased = 11,
    /// Amount is below the minimum USDC unit (must be >= USDC_FACTOR stroops).
    AmountPrecision = 12,
    /// Merchant nonce has already been used for a previous invoice.
    DuplicateNonce = 13,
    /// expires_in_seconds exceeds MAX_EXPIRY_SECONDS.
    ExpiryTooLong = 14,
    /// Provided metadata_hash does not match the stored hash on the invoice.
    MetadataMismatch = 15,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvoiceStatus {
    Pending,
    Paid,
    Expired,
    Cancelled,
    RefundRequested,
    /// Escrow funds have been released to the merchant after payment confirmation.
    Released,
}

// contracttype enum wrappers for optional complex types; Option<Address> and
// Option<Bytes> are not supported by the contracttype macro in soroban-sdk v20.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeAddress {
    None,
    Some(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeBytes {
    None,
    Some(Bytes),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invoice {
    pub id: u64,
    pub merchant: Address,
    pub amount_usdc: i128,
    pub gross_usdc: i128,
    pub status: InvoiceStatus,
    pub expires_at: u64,
    pub paid_at: Option<u64>,
    pub payer: MaybeAddress,
    pub metadata_hash: MaybeBytes,
    pub payment_link_hash: MaybeBytes,
    /// Merchant-supplied nonce for storefront idempotency (0 = no nonce).
    pub merchant_nonce: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Invoice(u64),
    InvoiceCount,
    Admin,
    Paused,
    /// Configurable grace window (seconds) added to expires_at during mark_paid.
    GraceWindow,
    /// Tracks used merchant nonces: (merchant_address, nonce) → bool.
    MerchantNonce(Address, u64),
}
