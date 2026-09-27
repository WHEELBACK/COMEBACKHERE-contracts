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
mod validation;

pub use events::{EscrowReleasedEvent, InvoiceAmountUpdatedEvent, InvoiceExpiryExtendedEvent};
use invoice::StatusTransition;
pub use invoice::{
    BatchInvoiceParams, DataKey, Invoice, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
    MAX_BATCH_EXPIRE, MAX_BATCH_SIZE,
};

use soroban_sdk::{contract, Env, Vec};

/// Persistent TTL thresholds for active invoices, per `docs/storage-ttl-audit.md`.
///
/// `INVOICE_TTL_THRESHOLD` is the minimum remaining TTL (in ledgers) below which
/// an access extends the entry, and `INVOICE_TTL_EXTEND_TO` is the TTL the entry
/// is extended to. Both are expressed in ledgers (~5s each).
const INVOICE_TTL_THRESHOLD: u32 = 30 * 24 * 60 * 12; // ~30 days
const INVOICE_TTL_EXTEND_TO: u32 = 90 * 24 * 60 * 12; // ~90 days

/// Extend the persistent TTL of an invoice entry when it is accessed.
///
/// Only non-terminal invoices are bumped so that terminal invoices
/// (paid/cancelled/expired) can age out of storage naturally.
pub(crate) fn bump_invoice_ttl(env: &Env, id: u64) {
    let key = DataKey::Invoice(id);
    if !env.storage().persistent().has(&key) {
        return;
    }
    let invoice: Invoice = match env.storage().persistent().get(&key) {
        Some(inv) => inv,
        None => return,
    };
    if invoice.status.is_terminal() {
        return;
    }
    env.storage().persistent().extend_ttl(
        &key,
        INVOICE_TTL_THRESHOLD,
        INVOICE_TTL_EXTEND_TO,
    );
}

pub(crate) fn pending_index_add(env: &Env, id: u64) {
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&DataKey::PendingIndex)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(id);
    env.storage().persistent().set(&DataKey::PendingIndex, &ids);
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
}

#[contract]
pub struct InvoiceContract;

#[cfg(test)]
extern crate std;
