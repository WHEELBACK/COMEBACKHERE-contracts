#![no_std]
// The soroban-sdk #[contractimpl] macro expands each contract method into
// additional generated items (e.g. the argument-spec helper) whose span
// clippy attributes to the macro invocation rather than the annotated
// function, so a function-level #[allow] doesn't suppress it — see
// create_invoice's 8-argument signature.
#![allow(clippy::too_many_arguments)]

mod entrypoints;
mod events;
mod invoice;
mod refund;
mod template;
mod validation;

pub use events::{
    EscrowReleasedEvent, GraceWindowUpdatedEvent, InvoiceAmountUpdatedEvent,
    InvoiceCreatedEvent, InvoiceExpiryExtendedEvent, InvoiceTransferredEvent, MaxOpenInvoicesUpdatedEvent, MerchantMaxOpenInvoicesUpdatedEvent, RefundNetworkFeeUpdatedEvent, RefundProcessedEvent,
};
pub use invoice::StatusTransition;
pub use invoice::{
    BatchInvoiceParams, DataKey, Invoice, InvoiceError, InvoiceStatus, InvoiceSummary,
    MaybeAddress, MaybeBytes, MaybeU64, RefundStatus, INVOICE_TTL_EXTEND_TO, INVOICE_TTL_THRESHOLD,
    MAX_BATCH_EXPIRE, MAX_BATCH_SIZE, MAX_LATE_FEE_BPS, MAX_PAGE_LIMIT, MAX_PAYER_INDEX_ENTRIES,
};
pub use refund::{
    calculate_net_refund, calculate_net_refund_with_network_fee, refund_recipient, transfer_net_refund, verify_payment_state, NetRefund,
    BPS_DENOMINATOR, MAX_REFUND_FEE_BPS, MAX_REFUND_NETWORK_FEE, REFUND_NETWORK_FEE,
};
pub use template::{
    InvoiceTemplate, RecurrenceInterval, TEMPLATE_KEEPER_BATCH_SIZE, TEMPLATE_PAGE_LIMIT,
};
pub use validation::require_valid_discount;
pub use validation::{require_memo_not_too_long, MAX_MEMO_BYTES};

use soroban_sdk::{contract, Address, Env, IntoVal, Val, Vec};

/// Extends the TTL of the persistent entry stored under `key` (#884).
///
/// Every persistent write in this contract goes through this helper so that no
/// invoice record, index or audit-log entry can silently age out of the ledger
/// and be evicted: once an entry's remaining TTL falls below
/// [`INVOICE_TTL_THRESHOLD`] ledgers it is bumped back to
/// [`INVOICE_TTL_EXTEND_TO`]. A key with no stored entry is skipped rather
/// than trapping, since `extend_ttl` on a missing key aborts the invocation.
pub(crate) fn bump_persistent<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    let storage = env.storage().persistent();
    if storage.has(key) {
        storage.extend_ttl(key, INVOICE_TTL_THRESHOLD, INVOICE_TTL_EXTEND_TO);
    }
}

/// Bumps the TTL of an invoice record and its per-invoice satellite entries
/// (#884), so an invoice is never evicted while its history, refund timestamp
/// or refund breakdown survive, or vice versa.
pub(crate) fn bump_invoice_ttl(env: &Env, id: u64) {
    bump_persistent(env, &DataKey::Invoice(id));
    bump_persistent(env, &DataKey::InvoiceHistory(id));
    bump_persistent(env, &DataKey::RefundRequestedAt(id));
    bump_persistent(env, &DataKey::RefundBreakdown(id));
}

pub(crate) fn pending_index_add(env: &Env, id: u64) {
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&DataKey::PendingIndex)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(id);
    env.storage().persistent().set(&DataKey::PendingIndex, &ids);
    bump_persistent(env, &DataKey::PendingIndex);
}

pub(crate) fn pending_index_remove(env: &Env, id: u64) {
    let ids: Vec<u64> = match env.storage().persistent().get(&DataKey::PendingIndex) {
        Some(v) => v,
        None => return,
    };
    let mut updated = Vec::new(env);
    for existing in ids.iter() {
        if existing != id {
            updated.push_back(existing);
        }
    }
    env.storage()
        .persistent()
        .set(&DataKey::PendingIndex, &updated);
    bump_persistent(env, &DataKey::PendingIndex);
}

/// Records `id` in `payer`'s index (#711).
///
/// Append-only by position, mirroring the merchant index: the count doubles as
/// the next free position, so a payment costs two writes regardless of how many
/// invoices the payer already has.
///
/// The index is bounded per payer by [`MAX_PAYER_INDEX_ENTRIES`]: once full, the
/// payment still succeeds and the invoice stays fully functional, it simply is
/// not added to this read model. Payment must never depend on a convenience
/// index having room, so hitting the cap is not an error.
pub(crate) fn payer_index_add(env: &Env, payer: &Address, id: u64) {
    let count_key = DataKey::PayerIndexCount(payer.clone());
    let count: u64 = env.storage().persistent().get(&count_key).unwrap_or(0);
    if count >= MAX_PAYER_INDEX_ENTRIES {
        return;
    }
    let entry_key = DataKey::PayerIndexEntry(payer.clone(), count);
    env.storage().persistent().set(&entry_key, &id);
    env.storage().persistent().set(&count_key, &(count + 1));
    bump_persistent(env, &entry_key);
    bump_persistent(env, &count_key);
}

pub(crate) fn append_history(env: &Env, id: u64, from: InvoiceStatus, to: InvoiceStatus) {
    let key = DataKey::InvoiceHistory(id);
    let mut history: Vec<StatusTransition> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    history.push_back(StatusTransition {
        from,
        to,
        timestamp: env.ledger().timestamp(),
    });
    env.storage().persistent().set(&key, &history);
    bump_persistent(env, &key);
}

#[contract]
pub struct InvoiceContract;

#[cfg(test)]
extern crate std;
