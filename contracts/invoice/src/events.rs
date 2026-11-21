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

use crate::invoice::{Invoice, MaybeAddress};
use crate::template::InvoiceTemplate;
use crate::refund::NetRefund;
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

/// Payload for `invoice_created`. Carries `token_address` explicitly (schema v2)
/// so indexers can learn the settlement asset without re-fetching the invoice (#718).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceCreatedEvent {
    pub id: u64,
    pub merchant: Address,
    pub amount_usdc: i128,
    pub gross_usdc: i128,
    pub expires_at: u64,
    pub merchant_nonce: u64,
    pub token_address: MaybeAddress,
}

pub fn invoice_created(env: &Env, id: u64, invoice: &Invoice) {
    let payload = InvoiceCreatedEvent {
        id,
        merchant: invoice.merchant.clone(),
        amount_usdc: invoice.amount_usdc,
        gross_usdc: invoice.gross_usdc,
        expires_at: invoice.expires_at,
        merchant_nonce: invoice.merchant_nonce,
        token_address: invoice.token_address.clone(),
    };
    env.events()
        .publish((Symbol::new(env, "invoice_created"), id), payload);
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

/// Emitted when a refund is processed on-chain and the net payout is
/// transferred to the payer (#71).
///
/// Carries the whole gross-vs-net breakdown rather than only the amount paid,
/// so an indexer or a support workflow can show the customer exactly what was
/// deducted (payment-gateway fee, network fee) instead of re-deriving it from
/// the invoice. Mirrors [`crate::NetRefund`], which is also stored under
/// `DataKey::RefundBreakdown` for the same invoice.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundProcessedEvent {
    pub id: u64,
    /// The payer the net amount was transferred to.
    pub payer: Address,
    pub gross_amount: i128,
    pub processing_fee: i128,
    pub network_fee: i128,
    pub net_amount: i128,
    /// Ledger timestamp the payout was executed at.
    pub processed_at: u64,
}

pub fn refund_processed(env: &Env, id: u64, payer: &Address, refund: &NetRefund) {
    let payload = RefundProcessedEvent {
        id,
        payer: payer.clone(),
        gross_amount: refund.gross_amount,
        processing_fee: refund.processing_fee,
        network_fee: refund.network_fee,
        net_amount: refund.net_amount,
        processed_at: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "refund_processed"), id), payload);
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

/// Emitted when the admin updates the grace window configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraceWindowUpdatedEvent {
    pub old_seconds: u64,
    pub new_seconds: u64,
}

pub fn grace_window_updated(env: &Env, event: &GraceWindowUpdatedEvent) {
    env.events()
        .publish((Symbol::new(env, "grace_window_updated"),), event.clone());
}

/// Emitted when the admin updates the max-open-invoices limit.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaxOpenInvoicesUpdatedEvent {
    pub old_limit: u32,
    pub new_limit: u32,
}

pub fn max_open_invoices_updated(env: &Env, event: &MaxOpenInvoicesUpdatedEvent) {
    env.events().publish(
        (Symbol::new(env, "max_open_invoices_updated"),),
        event.clone(),
    );
}

/// Emitted when the admin sets or clears a per-merchant open-invoice cap (#715).
/// `new_limit` is `None` when the override was cleared.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantMaxOpenInvoicesUpdatedEvent {
    pub merchant: Address,
    pub new_limit: Option<u32>,
}

pub fn merchant_max_open_invoices_updated(env: &Env, event: &MerchantMaxOpenInvoicesUpdatedEvent) {
    env.events().publish(
        (Symbol::new(env, "merchant_max_open_invoices_updated"),),
        event.clone(),
    );
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

#[cfg(test)]
mod tests {
    use super::{invoice_expiry_extended, InvoiceExpiryExtendedEvent};
    use soroban_sdk::{contract, testutils::Events, Env, Symbol, TryFromVal};

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
}

/// The admin changed the late fee: data is `(old_bps, new_bps)` (#704).
pub fn late_fee_updated(env: &Env, old_bps: u32, new_bps: u32) {
    env.events()
        .publish((Symbol::new(env, "late_fee_updated"),), (old_bps, new_bps));
}

/// A recurring invoice template was registered (#698).
pub fn template_created(env: &Env, id: u64, template: &InvoiceTemplate) {
    env.events()
        .publish((Symbol::new(env, "template_created"), id), template.clone());
}

/// A template was activated or deactivated: data is the new `active` flag.
pub fn template_activated(env: &Env, id: u64, active: bool) {
    env.events()
        .publish((Symbol::new(env, "template_activated"), id), active);
}

/// The keeper generated an invoice: topics are the template id and invoice id (#699).
pub fn template_generated(env: &Env, template_id: u64, invoice_id: u64) {
    env.events().publish(
        (
            Symbol::new(env, "template_generated"),
            template_id,
            invoice_id,
        ),
        (),
    );
}

/// Emitted when the admin changes the refund network fee (#701).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundNetworkFeeUpdatedEvent {
    pub old_fee: i128,
    pub new_fee: i128,
}

pub fn refund_network_fee_updated(env: &Env, old_fee: i128, new_fee: i128) {
    env.events().publish(
        (Symbol::new(env, "refund_network_fee_updated"),),
        RefundNetworkFeeUpdatedEvent { old_fee, new_fee },
    );
}

/// Emitted when the payer of a paid invoice transfers it to a new payer.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceTransferredEvent {
    pub id: u64,
    pub from: Address,
    pub to: Address,
}

pub fn invoice_transferred(env: &Env, id: u64, from: &Address, to: &Address) {
    env.events().publish(
        (Symbol::new(env, "invoice_transferred"), id),
        InvoiceTransferredEvent {
            id,
            from: from.clone(),
            to: to.clone(),
        },
    );
}
