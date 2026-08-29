use soroban_sdk::{contracttype, Address, Bytes};

pub use invoice_errors::InvoiceError;

/// USDC on Stellar uses 7 decimal places: 1 USDC = 10_000_000 stroops.
pub const USDC_FACTOR: i128 = 10_000_000;

/// Maximum number of elements accepted by any batch entrypoint (batch_create_invoice,
/// batch_expire) per call, to bound per-invocation storage writes and gas.
pub const MAX_BATCH_SIZE: u32 = 50;

/// Maximum number of invoice IDs accepted by batch_expire per call.
pub const MAX_BATCH_EXPIRE: u32 = 100;

/// Maximum bytes accepted for optional invoice hash fields.
pub const MAX_HASH_BYTES: u32 = 64;

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
    /// Refund has been approved by admin; terminal status for disputed invoices.
    Refunded,
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
    /// Optional token contract address for multi-currency invoices.
    /// `None` means the invoice is denominated in the default (USDC).
    pub token_address: MaybeAddress,
}

/// Parameters for a single invoice within a batch_create_invoice call.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchInvoiceParams {
    pub amount_usdc: i128,
    pub gross_usdc: i128,
    pub expires_in_seconds: u64,
    pub metadata_hash: MaybeBytes,
    pub payment_link_hash: MaybeBytes,
    pub merchant_nonce: u64,
    pub token_address: MaybeAddress,
}

/// A single status transition recorded in an invoice's audit log.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusTransition {
    pub from: InvoiceStatus,
    pub to: InvoiceStatus,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Invoice(u64),
    InvoiceCount,
    Admin,
    PendingAdmin,
    Paused,
    /// Configurable grace window (seconds) added to expires_at during mark_paid.
    GraceWindow,
    /// Tracks used merchant nonces: (merchant_address, nonce) → bool.
    MerchantNonce(Address, u64),
    /// Count of invoices created by a merchant.
    MerchantInvoiceCount(Address),
    /// Secondary index: (merchant address, zero-based position) → invoice ID.
    MerchantInvoiceIndex(Address, u64),
    /// Ordered audit log of status transitions for an invoice.
    InvoiceHistory(u64),
    /// Global set of pending invoice IDs for efficient expiry enumeration.
    PendingIndex,
    /// Admin-tunable minimum seconds between successive create_invoice calls per merchant.
    CreationCooldown,
    /// Timestamp of the last successful create_invoice call for a given merchant.
    LastCreatedAt(Address),
}
