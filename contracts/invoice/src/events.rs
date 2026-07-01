// Event schema for Redis webhook delivery compatibility:
//
// Event Format:
// - Topics: Variable-length tuple of Symbols and data fields
// - Data: Serializable structs (Invoice, Address) or primitive types
//
// Redis Webhook Consumer Compatibility:
// All emitted events are compatible with JSON serialization for webhook delivery:
// - Symbol types serialize to strings
// - Address types serialize to account identifiers
// - Numeric types (u64, i128) serialize as JSON numbers or strings
// - Enum variants (InvoiceStatus) serialize to string representations
// - Structs (Invoice) serialize to JSON objects with field keys
// - Optional types (Option<u64>) serialize to null or value

use crate::invoice::Invoice;
use soroban_sdk::{contracttype, Address, Env, Symbol};

/// Emitted when an amendment changes an invoice's amount fields.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceAmountUpdatedEvent {
    pub id: u64,
    pub old_amount_usdc: i128,
    pub new_amount_usdc: i128,
    pub old_gross_usdc: i128,
    pub new_gross_usdc: i128,
}

pub fn invoice_created(env: &Env, id: u64, invoice: &Invoice) {
    env.events()
        .publish((Symbol::new(env, "invoice_created"), id), invoice.clone());
}

pub fn invoice_paid(env: &Env, id: u64, invoice: &Invoice) {
    env.events()
        .publish((Symbol::new(env, "invoice_paid"), id), invoice.clone());
}

pub fn invoice_expired(env: &Env, id: u64, invoice: &Invoice) {
    env.events()
        .publish((Symbol::new(env, "invoice_expired"), id), invoice.clone());
}

pub fn invoice_cancelled(env: &Env, id: u64, invoice: &Invoice) {
    env.events()
        .publish((Symbol::new(env, "invoice_cancelled"), id), invoice.clone());
}

pub fn invoice_refund_requested(env: &Env, id: u64, invoice: &Invoice) {
    env.events().publish(
        (Symbol::new(env, "invoice_refund_req"), id),
        invoice.clone(),
    );
}

pub fn refund_approved(env: &Env, id: u64, invoice: &Invoice) {
    env.events()
        .publish((Symbol::new(env, "refund_approved"), id), invoice.clone());
}

/// Minimal payload emitted when escrow is released for a paid invoice.
#[contracttype]
#[derive(Clone)]
pub struct EscrowReleasedEvent {
    pub id: u64,
    pub merchant: Address,
    pub amount_usdc: i128,
    pub released_at: u64,
}

pub fn escrow_released(env: &Env, id: u64, invoice: &Invoice) {
    let payload = EscrowReleasedEvent {
        id,
        merchant: invoice.merchant.clone(),
        amount_usdc: invoice.amount_usdc,
        released_at: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "escrow_released"), id), payload);
}

pub fn contract_paused(env: &Env, admin: &Address) {
    env.events()
        .publish((Symbol::new(env, "contract_paused"),), admin);
}

pub fn contract_unpaused(env: &Env, admin: &Address) {
    env.events()
        .publish((Symbol::new(env, "contract_unpaused"),), admin);
}

pub fn invoice_amended(env: &Env, event: &InvoiceAmountUpdatedEvent) {
    env.events().publish(
        (Symbol::new(env, "invoice_amended"), event.id),
        event.clone(),
    );
}
