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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceExpiryExtendedEvent {
    pub id: u64,
    pub old_expires_at: u64,
    pub new_expires_at: u64,
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
        (Symbol::new(env, "invoice_refund_requested"), id),
        invoice.clone(),
    );
}

pub fn refund_approved(env: &Env, id: u64, invoice: &Invoice) {
    env.events()
        .publish((Symbol::new(env, "refund_approved"), id), invoice.clone());
}

pub fn refund_rejected(env: &Env, id: u64, invoice: &Invoice) {
    env.events()
        .publish((Symbol::new(env, "refund_rejected"), id), invoice.clone());
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

pub fn invoice_expiry_extended(env: &Env, event: &InvoiceExpiryExtendedEvent) {
    env.events().publish(
        (Symbol::new(env, "invoice_expiry_extended"), event.id),
        event.clone(),
    );
}

/// Emitted when a recurring invoice template is created.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplateCreatedEvent {
    pub template_id: u64,
    pub merchant: Address,
    pub interval: u64,
}

/// Emitted when a recurring invoice template is disabled.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplateDisabledEvent {
    pub template_id: u64,
    pub merchant: Address,
}

/// Emitted each time a new invoice is generated from a template.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplateGeneratedEvent {
    pub template_id: u64,
    pub invoice_id: u64,
    pub generated_at: u64,
}

pub fn template_created(env: &Env, event: &TemplateCreatedEvent) {
    env.events().publish(
        (Symbol::new(env, "template_created"), event.template_id),
        event.clone(),
    );
}

pub fn template_disabled(env: &Env, event: &TemplateDisabledEvent) {
    env.events().publish(
        (Symbol::new(env, "template_disabled"), event.template_id),
        event.clone(),
    );
}

pub fn template_generated(env: &Env, event: &TemplateGeneratedEvent) {
    env.events().publish(
        (Symbol::new(env, "template_generated"), event.template_id),
        event.clone(),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        invoice_expiry_extended, template_created, template_disabled, template_generated,
        InvoiceExpiryExtendedEvent, TemplateCreatedEvent, TemplateDisabledEvent,
        TemplateGeneratedEvent,
    };
    use soroban_sdk::{contract, testutils::Address as _, testutils::Events, Address, Env, Symbol, TryFromVal};

    #[contract]
    struct TestContract;

    #[test]
    fn invoice_expiry_extended_emits_event() {
        let env = Env::default();
        let contract_id = env.register(TestContract, ());
        env.as_contract(&contract_id, || {
            invoice_expiry_extended(
                &env,
                &InvoiceExpiryExtendedEvent {
                    id: 1,
                    old_expires_at: 100,
                    new_expires_at: 200,
                },
            );
        });

        let (_, topics, _) = env.events().all().last().unwrap();
        assert_eq!(
            Symbol::try_from_val(&env, &topics.get_unchecked(0)).unwrap(),
            Symbol::new(&env, "invoice_expiry_extended")
        );
    }

    #[test]
    fn template_lifecycle_emits_events() {
        let env = Env::default();
        let contract_id = env.register(TestContract, ());
        let merchant = Address::generate(&env);
        env.as_contract(&contract_id, || {
            template_created(
                &env,
                &TemplateCreatedEvent {
                    template_id: 1,
                    merchant: merchant.clone(),
                    interval: 86_400,
                },
            );
            template_generated(
                &env,
                &TemplateGeneratedEvent {
                    template_id: 1,
                    invoice_id: 10,
                    generated_at: 1_000,
                },
            );
            template_disabled(
                &env,
                &TemplateDisabledEvent {
                    template_id: 1,
                    merchant: merchant.clone(),
                },
            );
        });

        let events = env.events().all();
        let (_, created_topics, _) = events.get(events.len() - 3).unwrap();
        assert_eq!(
            Symbol::try_from_val(&env, &created_topics.get_unchecked(0)).unwrap(),
            Symbol::new(&env, "template_created")
        );
        let (_, generated_topics, _) = events.get(events.len() - 2).unwrap();
        assert_eq!(
            Symbol::try_from_val(&env, &generated_topics.get_unchecked(0)).unwrap(),
            Symbol::new(&env, "template_generated")
        );
        let (_, disabled_topics, _) = events.get(events.len() - 1).unwrap();
        assert_eq!(
            Symbol::try_from_val(&env, &disabled_topics.get_unchecked(0)).unwrap(),
            Symbol::new(&env, "template_disabled")
        );
    }
}
