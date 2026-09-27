//! Invoice contract implementation.
//!
//! Provides invoice creation, payment, pausing, and recurring invoice
//! templates that can generate new invoices on a fixed interval.

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, String, Vec};

/// Storage keys used by the invoice contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Monotonic counter for invoice ids.
    InvoiceCount,
    /// Invoice record keyed by id.
    Invoice(u64),
    /// Monotonic counter for template ids.
    TemplateCount,
    /// Recurring invoice template keyed by id.
    Template(u64),
}

/// A single invoice.
#[contracttype]
#[derive(Clone)]
pub struct Invoice {
    pub id: u64,
    pub merchant: Address,
    pub payer: Address,
    pub amount: i128,
    pub memo: String,
    pub paid: bool,
    pub paused: bool,
}

/// A recurring invoice template.
///
/// Stores the invoice parameters plus a fixed interval (in ledgers).
/// New invoices are only generated when `generate_from_template` is called
/// and the interval has elapsed since the last generation.
#[contracttype]
#[derive(Clone)]
pub struct InvoiceTemplate {
    pub id: u64,
    pub merchant: Address,
    pub payer: Address,
    pub amount: i128,
    pub memo: String,
    /// Interval in ledgers between generations.
    pub interval: u64,
    /// Ledger sequence of the last generation (0 if never generated).
    pub last_generated: u64,
    /// Whether the template is active. Disabled templates cannot generate.
    pub enabled: bool,
}

#[contract]
pub struct InvoiceContract;

#[contractimpl]
impl InvoiceContract {
    /// Create a new invoice.
    pub fn create_invoice(
        env: Env,
        merchant: Address,
        payer: Address,
        amount: i128,
        memo: String,
    ) -> u64 {
        merchant.require_auth();

        let id = Self::next_invoice_id(&env);
        let invoice = Invoice {
            id,
            merchant,
            payer,
            amount,
            memo,
            paid: false,
            paused: false,
        };
        env.storage().persistent().set(&DataKey::Invoice(id), &invoice);
        env.events()
            .publish((symbol_short!("invoice"), symbol_short!("created")), id);
        id
    }

    /// Fetch an invoice by id.
    pub fn get_invoice(env: Env, id: u64) -> Invoice {
        env.storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .expect("invoice not found")
    }

    /// Pause an invoice so it cannot be paid.
    pub fn pause_invoice(env: Env, id: u64) {
        let mut invoice = Self::get_invoice(env.clone(), id);
        invoice.merchant.require_auth();
        invoice.paused = true;
        env.storage().persistent().set(&DataKey::Invoice(id), &invoice);
        env.events()
            .publish((symbol_short!("invoice"), symbol_short!("paused")), id);
    }

    /// Resume a paused invoice.
    pub fn resume_invoice(env: Env, id: u64) {
        let mut invoice = Self::get_invoice(env.clone(), id);
        invoice.merchant.require_auth();
        invoice.paused = false;
        env.storage().persistent().set(&DataKey::Invoice(id), &invoice);
        env.events()
            .publish((symbol_short!("invoice"), symbol_short!("resumed")), id);
    }

    /// Register a recurring invoice template.
    ///
    /// The template records the invoice parameters and a fixed interval (in
    /// ledgers). Generation is always call-triggered via
    /// `generate_from_template`; Soroban has no scheduler.
    pub fn create_template(
        env: Env,
        merchant: Address,
        payer: Address,
        amount: i128,
        memo: String,
        interval: u64,
    ) -> u64 {
        merchant.require_auth();
        assert!(interval > 0, "interval must be positive");

        let id = Self::next_template_id(&env);
        let template = InvoiceTemplate {
            id,
            merchant,
            payer,
            amount,
            memo,
            interval,
            last_generated: 0,
            enabled: true,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Template(id), &template);
        env.events().publish(
            (symbol_short!("template"), symbol_short!("created")),
            id,
        );
        id
    }

    /// Fetch a template by id.
    pub fn get_template(env: Env, id: u64) -> InvoiceTemplate {
        env.storage()
            .persistent()
            .get(&DataKey::Template(id))
            .expect("template not found")
    }

    /// Disable a template so it stops generating invoices.
    pub fn disable_template(env: Env, id: u64) {
        let mut template = Self::get_template(env.clone(), id);
        template.merchant.require_auth();
        template.enabled = false;
        env.storage()
            .persistent()
            .set(&DataKey::Template(id), &template);
        env.events().publish(
            (symbol_short!("template"), symbol_short!("disabled")),
            id,
        );
    }

    /// Generate the next invoice from a template.
    ///
    /// Only creates an invoice when the template is enabled and the interval
    /// has elapsed since the last generation. Returns the new invoice id.
    pub fn generate_from_template(env: Env, id: u64) -> u64 {
        let mut template = Self::get_template(env.clone(), id);
        assert!(template.enabled, "template disabled");

        let now = env.ledger().sequence() as u64;
        if template.last_generated != 0 {
            assert!(
                now >= template.last_generated + template.interval,
                "interval not elapsed"
            );
        }

        let invoice_id = Self::next_invoice_id(&env);
        let invoice = Invoice {
            id: invoice_id,
            merchant: template.merchant.clone(),
            payer: template.payer.clone(),
            amount: template.amount,
            memo: template.memo.clone(),
            paid: false,
            paused: false,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(invoice_id), &invoice);

        template.last_generated = now;
        env.storage()
            .persistent()
            .set(&DataKey::Template(id), &template);

        env.events().publish(
            (symbol_short!("template"), symbol_short!("generated")),
            (id, invoice_id),
        );
        invoice_id
    }

    fn next_invoice_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::InvoiceCount)
            .unwrap_or(0)
            + 1;
        env.storage().persistent().set(&DataKey::InvoiceCount, &id);
        id
    }

    fn next_template_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TemplateCount)
            .unwrap_or(0)
            + 1;
        env.storage().persistent().set(&DataKey::TemplateCount, &id);
        id
    }
}
