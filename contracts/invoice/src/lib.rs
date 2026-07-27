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

pub use events::{EscrowReleasedEvent, InvoiceAmountUpdatedEvent};
use invoice::StatusTransition;
pub use invoice::{
    BatchInvoiceParams, DataKey, Invoice, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};

use soroban_sdk::{contract, Address, Env, Vec};

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
