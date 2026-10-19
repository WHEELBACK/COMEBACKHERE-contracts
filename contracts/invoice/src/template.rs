use crate::invoice::MaybeBytes;
use soroban_sdk::{contracttype, Address};

/// Upper bound on the number of entries a paginated read may return.
pub const TEMPLATE_PAGE_LIMIT: u32 = 50;

/// Upper bound on the number of templates a single keeper call may process.
pub const TEMPLATE_KEEPER_BATCH_SIZE: u32 = 20;

/// Recurrence periods supported by an [`InvoiceTemplate`].
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecurrenceInterval {
    Daily,
    Weekly,
    /// 30-day period, so month lengths need no calendar arithmetic on-chain.
    Monthly,
}

impl RecurrenceInterval {
    /// Length of one recurrence period in seconds.
    pub fn period_seconds(&self) -> u64 {
        match self {
            RecurrenceInterval::Daily => 86_400,
            RecurrenceInterval::Weekly => 604_800,
            RecurrenceInterval::Monthly => 2_592_000,
        }
    }
}

/// A recurring invoice definition owned by a merchant.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceTemplate {
    pub id: u64,
    pub merchant: Address,
    pub amount_usdc: i128,
    pub gross_usdc: i128,
    pub interval: RecurrenceInterval,
    /// Validity window of each generated invoice, in seconds.
    pub expires_in_seconds: u64,
    pub metadata_hash: MaybeBytes,
    pub payment_link_hash: MaybeBytes,
    /// Inactive templates are skipped by the keeper.
    pub active: bool,
    /// Earliest ledger timestamp at which the keeper may generate the next invoice.
    pub next_run_at: u64,
}
