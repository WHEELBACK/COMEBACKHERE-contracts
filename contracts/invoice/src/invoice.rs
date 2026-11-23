use soroban_sdk::{contracttype, Address, Bytes};

pub use invoice_errors::InvoiceError;

/// USDC on Stellar uses 7 decimal places: 1 USDC = 10_000_000 stroops.
pub const USDC_FACTOR: i128 = 10_000_000;

/// Maximum allowed late fee expressed in basis points.
///
/// `gross_usdc` may exceed `amount_usdc` by at most this many bps of
/// `amount_usdc`. 1 000 bps = 10 %. This is the single source of truth for
/// the late-fee cap; `validation.rs` imports and uses this constant rather
/// than redeclaring it.
pub const MAX_LATE_FEE_BPS: u32 = 1_000;

/// Maximum number of elements accepted by any batch entrypoint (batch_create_invoice,
/// batch_expire) per call, to bound per-invocation storage writes and gas.
pub const MAX_BATCH_SIZE: u32 = 50;

/// Maximum number of invoice IDs accepted by batch_expire per call.
pub const MAX_BATCH_EXPIRE: u32 = 100;

/// Ledger-count threshold below which a persistent entry's TTL is extended.
pub const INVOICE_TTL_THRESHOLD: u32 = 518_400; // ~30 days at 5s/ledger
/// Ledger-count a persistent entry's TTL is extended to once it is bumped.
pub const INVOICE_TTL_EXTEND_TO: u32 = 1_036_800; // ~60 days at 5s/ledger

/// Maximum bytes accepted for optional invoice hash fields.
pub const MAX_HASH_BYTES: u32 = 64;

/// Maximum `limit` accepted by a paginated invoice-index read
/// (`get_invoices_by_payer`). Caps how many storage entries one read can walk,
/// so a caller asking for the whole index has to page instead of pulling an
/// unbounded slice into a single invocation.
pub const MAX_PAGE_LIMIT: u32 = 100;

/// Maximum number of invoices recorded in a single payer's index.
///
/// The payer index is a read-model convenience, so it is bounded per payer: the
/// first `MAX_PAYER_INDEX_ENTRIES` invoices a payer pays are indexed, and once
/// that many are recorded further payments are still accepted but not indexed.
/// Without a cap, one payer's index would grow without bound and every
/// `mark_paid` would pay for an ever-larger write. Clients that need a payer's
/// complete history should page `get_invoices_page` or index the
/// `invoice_paid` event stream rather than rely on the payer index.
pub const MAX_PAYER_INDEX_ENTRIES: u64 = 500;

/// Lifecycle status of an invoice.
///
/// The typical happy path is: `Pending` → `Paid` → `Released`.
/// Unhappy paths include: `Pending` → `Expired` (timeout without payment),
/// `Pending` → `Cancelled` (merchant or admin cancellation), `Paid` →
/// `RefundRequested` → `Refunded` (dispute/refund flow).
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

impl InvoiceStatus {
    /// Returns true if this status is terminal: the invoice can no longer
    /// transition to any other status. Adding a new `InvoiceStatus` variant
    /// forces a deliberate decision here because the match below has no
    /// wildcard arm.
    pub fn is_terminal(&self) -> bool {
        match self {
            InvoiceStatus::Pending => false,
            InvoiceStatus::Paid => false,
            InvoiceStatus::Expired => true,
            InvoiceStatus::Cancelled => true,
            InvoiceStatus::RefundRequested => false,
            InvoiceStatus::Released => true,
            InvoiceStatus::Refunded => true,
        }
    }
}

#[cfg(test)]
mod is_terminal_tests {
    use super::InvoiceStatus;

    /// Exhaustively checks every `InvoiceStatus` variant against the expected
    /// terminal/non-terminal classification.
    #[test]
    fn is_terminal_covers_every_variant() {
        let variants = [
            (InvoiceStatus::Pending, false),
            (InvoiceStatus::Paid, false),
            (InvoiceStatus::Expired, true),
            (InvoiceStatus::Cancelled, true),
            (InvoiceStatus::RefundRequested, false),
            (InvoiceStatus::Released, true),
            (InvoiceStatus::Refunded, true),
        ];
        for (status, expected) in variants {
            assert_eq!(status.is_terminal(), expected, "is_terminal() for {:?}", status);
        }
    }
}

// contracttype enum wrappers for optional complex types; Option<Address> and
// Option<Bytes> are not supported by the contracttype macro in soroban-sdk v20.
/// Nullable `Address` wrapper compatible with `#[contracttype]`.
///
/// `Option<Address>` is not supported by the Soroban contract-type macro, so
/// this enum serves as a manual `Option` for address fields. `None` signals
/// absence; `Some(addr)` wraps a concrete address.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeAddress {
    None,
    Some(Address),
}

/// Nullable `Bytes` wrapper compatible with `#[contracttype]`.
///
/// `Option<Bytes>` is not supported by the Soroban contract-type macro, so
/// this enum serves as a manual `Option` for byte-string fields such as
/// metadata hashes and payment-link hashes. `None` signals absence;
/// `Some(bytes)` wraps a concrete byte string.
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

/// Gross amount already received from the payer on `invoice`.
///
/// `mark_paid` is all-or-nothing, so the only thing that has ever moved funds is
/// the transition into a paid state: `gross_usdc` from `Paid` onwards, and `0`
/// before that. Refunds pay the payer *out*, so `Refunded` keeps reporting the
/// gross that was collected rather than treating the refund as a negative
/// contribution.
fn collected_usdc(invoice: &Invoice) -> i128 {
    match invoice.status {
        InvoiceStatus::Paid
        | InvoiceStatus::RefundRequested
        | InvoiceStatus::Released
        | InvoiceStatus::Refunded => invoice.gross_usdc,
        InvoiceStatus::Pending | InvoiceStatus::Expired | InvoiceStatus::Cancelled => 0,
    }
}

/// Gross amount still owed on `invoice`, i.e. what the payer has yet to pay.
///
/// While an invoice is `Pending` the payer owes the whole `gross_usdc` (the
/// amount the merchant receives plus its fee); once funds have moved, or once
/// the invoice is no longer collectible (`Expired`, `Cancelled`), nothing is
/// outstanding. Partial payments are not recorded on-chain today (no
/// per-invoice collected-amount accounting exists), so an invoice has no
/// intermediate partially-paid state to report here: the figure goes from the
/// full `gross_usdc` to `0` when `mark_paid` fires. Clients that need
/// finer-grained progress keep replaying `invoice_paid`/refund events today,
/// and this field picks up partial amounts with no change to the summary's
/// shape once that accounting lands (#712).
fn outstanding_usdc(invoice: &Invoice) -> i128 {
    match invoice.status {
        // An expired or cancelled invoice can no longer be paid, so nothing is owed.
        InvoiceStatus::Expired | InvoiceStatus::Cancelled => 0,
        _ => (invoice.gross_usdc - collected_usdc(invoice)).max(0),
    }
}

/// Lightweight, read-only projection of an [`Invoice`] for list views.
///
/// Carries only the fields a wallet needs to render a row, and adds the two
/// amounts (#712): `paid_usdc` and the outstanding `remaining_usdc`, so a
/// partially-settled or fully-settled invoice can be shown without replaying
/// events. It is derived from the same `DataKey::Invoice` record `get_invoice`
/// reads - never a second copy - so it can never drift out of sync with the
/// full record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceSummary {
    pub id: u64,
    pub status: InvoiceStatus,
    /// Net amount the merchant receives once escrow is released.
    pub amount_usdc: i128,
    /// Total amount charged to the payer, fee included.
    pub gross_usdc: i128,
    /// Gross amount collected so far; 0 until funds move.
    pub paid_usdc: i128,
    /// Gross amount still owed to the merchant by the payer.
    pub remaining_usdc: i128,
    pub expires_at: u64,
}

impl From<&Invoice> for InvoiceSummary {
    fn from(invoice: &Invoice) -> Self {
        InvoiceSummary {
            id: invoice.id,
            status: invoice.status.clone(),
            amount_usdc: invoice.amount_usdc,
            gross_usdc: invoice.gross_usdc,
            paid_usdc: collected_usdc(invoice),
            remaining_usdc: outstanding_usdc(invoice),
            expires_at: invoice.expires_at,
        }
    }
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

/// A nullable `u64` wrapper compatible with `#[contracttype]`.
///
/// Used to represent an optional timestamp (e.g. `requested_at` before a
/// refund has been requested). `Option<u64>` is not supported by the
/// Soroban contract-type macro in the same way, so this enum mirrors the
/// `MaybeAddress`/`MaybeBytes` pattern already established in this file.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeU64 {
    None,
    Some(u64),
}

/// Consolidated refund state for an invoice, returned by `get_refund_status`.
///
/// Combines the invoice's refund-related status fields with the fee breakdown
/// stored under `DataKey::RefundBreakdown` (if any), so a client does not
/// need to call both `get_invoice` and `get_refund_breakdown` to render a
/// refund progress view.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundStatus {
    /// Current lifecycle status of the invoice.
    pub status: InvoiceStatus,
    /// Ledger timestamp at which `request_refund` was called, if applicable.
    /// `MaybeU64::None` when no refund has been requested.
    pub requested_at: MaybeU64,
    /// Gross amount the refund was (or would be) computed from.
    /// `0` when no refund breakdown has been recorded yet.
    pub gross_amount: i128,
    /// Payment-gateway processing fee deducted from the gross amount.
    /// `0` when no refund breakdown has been recorded yet.
    pub processing_fee: i128,
    /// Flat network fee deducted from the gross amount.
    /// `0` when no refund breakdown has been recorded yet.
    pub network_fee: i128,
    /// Net amount the payer received (or will receive).
    /// `0` when no refund breakdown has been recorded yet.
    pub net_amount: i128,
}

/// Storage keys for invoice contract state.
///
/// Used as keys for Soroban instance and persistent storage lookups. Variants
/// must not be reordered or removed after deployment; append new variants at
/// the end so that existing on-chain data keyed by XDR discriminant continues
/// to decode correctly.
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
    /// Fee breakdown of a processed refund: gross amount, processing fee,
    /// network fee and the net amount transferred to the payer (#71).
    RefundBreakdown(u64),
    /// Admin-tunable maximum number of open (pending) invoices a single merchant
    /// may hold at one time. 0 means no limit (default).
    MaxOpenInvoices,
    /// Admin-tunable per-merchant override of `MaxOpenInvoices` (#715). When
    /// absent, the merchant falls back to the global cap.
    MerchantMaxOpenInvoices(Address),
    /// Admin-tunable late-fee rate in basis points (1 bps = 0.01%).
    /// Applied to overdue invoices at the discretion of the settlement layer.
    LateFee,
    /// Recurring invoice template store: Template(id) → InvoiceTemplate (#698).
    Template(u64),
    /// Monotonic template id counter (#698).
    TemplateCount,
    /// Per-merchant template index in creation order (#698).
    MerchantTemplates(Address),
    /// Admin-configurable refund network fee in stroops (#701). Defaults to
    /// `REFUND_NETWORK_FEE` when unset.
    RefundNetworkFee,
    /// Admin-configured window (seconds) after payment within which a payer
    /// may call request_refund. Zero means no limit (current behaviour).
    RefundWindow,
    /// Ledger timestamp at which request_refund was called for this invoice.
    /// Stored so get_refund_status can surface it without re-reading the
    /// full invoice history.
    RefundRequestedAt(u64),
    /// Number of invoices recorded in a payer's index.
    PayerIndexCount(Address),
    /// Secondary index: (payer address, zero-based position) → invoice ID.
    /// Positions are assigned in `mark_paid` order and never reused; see
    /// [`MAX_PAYER_INDEX_ENTRIES`] for the growth cap.
    PayerIndexEntry(Address, u64),
}
