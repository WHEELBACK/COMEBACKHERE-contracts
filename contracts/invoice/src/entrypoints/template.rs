//! Recurring invoice template entrypoints (#698, #699): template writes and
//! reads, plus the permissionless keeper that turns due templates into invoices.

use super::lifecycle::create_invoice_record;
use crate::events;
use crate::template::{
    InvoiceTemplate, RecurrenceInterval, TEMPLATE_KEEPER_BATCH_SIZE, TEMPLATE_PAGE_LIMIT,
};
use crate::validation::{
    require_expiry_not_too_long, require_hash_not_too_long, require_not_paused,
    require_positive_amount, require_usdc_precision, require_valid_payment_link_hash,
};
use crate::{DataKey, InvoiceContract, InvoiceError, MaybeAddress, MaybeBytes};
#[allow(unused_imports)]
use crate::{InvoiceContractArgs, InvoiceContractClient};
use soroban_sdk::{contractimpl, Address, Env, Vec};

#[contractimpl]
impl InvoiceContract {
    /// Register a recurring invoice template for `merchant` and return its id.
    /// The first run is one full period after creation, so a new template is
    /// never due immediately.
    /// Errors: `ContractPaused`, `InvalidAmount`, `AmountPrecision`, `ZeroDuration`,
    ///         `ExpiryOverflow`, `InvoiceCountOverflow`.
    /// Emits: `template_created`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_template(
        env: Env,
        merchant: Address,
        amount_usdc: i128,
        gross_usdc: i128,
        interval: RecurrenceInterval,
        expires_in_seconds: u64,
        metadata_hash: MaybeBytes,
        payment_link_hash: MaybeBytes,
    ) -> Result<u64, InvoiceError> {
        merchant.require_auth();
        require_not_paused(&env)?;
        require_positive_amount(amount_usdc, gross_usdc)?;
        require_usdc_precision(amount_usdc, gross_usdc)?;
        require_hash_not_too_long(&metadata_hash)?;
        require_hash_not_too_long(&payment_link_hash)?;
        require_valid_payment_link_hash(&payment_link_hash)?;
        if expires_in_seconds == 0 {
            return Err(InvoiceError::ZeroDuration);
        }
        require_expiry_not_too_long(expires_in_seconds)?;

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TemplateCount)
            .unwrap_or(0);
        let id = count
            .checked_add(1)
            .ok_or(InvoiceError::InvoiceCountOverflow)?;
        let next_run_at = env
            .ledger()
            .timestamp()
            .checked_add(interval.period_seconds())
            .ok_or(InvoiceError::ExpiryOverflow)?;

        let template = InvoiceTemplate {
            id,
            merchant: merchant.clone(),
            amount_usdc,
            gross_usdc,
            interval,
            expires_in_seconds,
            metadata_hash,
            payment_link_hash,
            active: true,
            next_run_at,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Template(id), &template);
        env.storage().instance().set(&DataKey::TemplateCount, &id);
        index_template(&env, &merchant, id);
        events::template_created(&env, id, &template);
        Ok(id)
    }

    /// Return the recurring invoice template registered under `id`.
    /// Errors: `NotFound`.
    pub fn get_invoice_template(env: Env, id: u64) -> Result<InvoiceTemplate, InvoiceError> {
        env.storage()
            .persistent()
            .get(&DataKey::Template(id))
            .ok_or(InvoiceError::NotFound)
    }

    /// Return one page of `merchant`'s templates in creation order, skipping the
    /// first `start` entries. `limit` is capped at [`TEMPLATE_PAGE_LIMIT`].
    pub fn list_templates_by_merchant(
        env: Env,
        merchant: Address,
        start: u64,
        limit: u64,
    ) -> Vec<InvoiceTemplate> {
        let capped = core::cmp::min(limit, TEMPLATE_PAGE_LIMIT as u64);
        let ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantTemplates(merchant))
            .unwrap_or_else(|| Vec::new(&env));

        let mut page = Vec::new(&env);
        for id in ids.iter().skip(start as usize) {
            if (page.len() as u64) >= capped {
                break;
            }
            if let Some(template) = env
                .storage()
                .persistent()
                .get::<DataKey, InvoiceTemplate>(&DataKey::Template(id))
            {
                page.push_back(template);
            }
        }
        page
    }

    /// Activate or deactivate a template (merchant or admin). Deactivated
    /// templates keep their schedule but are skipped by the keeper.
    /// Errors: `NotFound`, `Unauthorized`.
    /// Emits: `template_activated`.
    pub fn set_template_active(
        env: Env,
        caller: Address,
        id: u64,
        active: bool,
    ) -> Result<(), InvoiceError> {
        caller.require_auth();
        let mut template: InvoiceTemplate = env
            .storage()
            .persistent()
            .get(&DataKey::Template(id))
            .ok_or(InvoiceError::NotFound)?;
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        if caller != template.merchant && admin.as_ref() != Some(&caller) {
            return Err(InvoiceError::Unauthorized);
        }
        template.active = active;
        env.storage()
            .persistent()
            .set(&DataKey::Template(id), &template);
        events::template_activated(&env, id, active);
        Ok(())
    }

    /// Generate an invoice from an active template on demand, outside its
    /// recurrence schedule (#694). The owning merchant must authorise the call.
    /// Does not advance `next_run_at`: the keeper's schedule is unaffected.
    /// Returns the new invoice ID.
    /// Errors: `ContractPaused`, `NotFound`, `Unauthorized`, `TemplateDisabled`.
    /// Emits: `template_generated`.
    pub fn generate_from_template(
        env: Env,
        merchant: Address,
        template_id: u64,
    ) -> Result<u64, InvoiceError> {
        merchant.require_auth();
        require_not_paused(&env)?;
        let template: InvoiceTemplate = env
            .storage()
            .persistent()
            .get(&DataKey::Template(template_id))
            .ok_or(InvoiceError::NotFound)?;
        if template.merchant != merchant {
            return Err(InvoiceError::Unauthorized);
        }
        if !template.active {
            return Err(InvoiceError::TemplateDisabled);
        }
        let invoice_id = create_invoice_record(
            &env,
            &template.merchant,
            template.amount_usdc,
            template.gross_usdc,
            template.expires_in_seconds,
            template.metadata_hash,
            template.payment_link_hash,
            0,
            MaybeAddress::None,
        )?;
        events::template_generated(&env, template_id, invoice_id);
        Ok(invoice_id)
    }

    /// Return the total number of templates created.
    pub fn get_template_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::TemplateCount)
            .unwrap_or(0)
    }

    /// Permissionless keeper call: create one invoice for each due template in
    /// `template_ids` and advance its schedule past `now`. Unknown, inactive and
    /// not-yet-due templates are skipped, so a period can never be invoiced twice.
    /// Returns the number of invoices generated.
    /// Errors: `BatchTooLarge` when more than [`TEMPLATE_KEEPER_BATCH_SIZE`] ids are supplied.
    /// Emits: `template_generated` per generated invoice.
    pub fn generate_due_invoices(env: Env, template_ids: Vec<u64>) -> Result<u32, InvoiceError> {
        if template_ids.len() > TEMPLATE_KEEPER_BATCH_SIZE {
            return Err(InvoiceError::BatchTooLarge);
        }
        require_not_paused(&env)?;

        let now = env.ledger().timestamp();
        let mut generated: u32 = 0;
        for id in template_ids.iter() {
            let Some(mut template) = env
                .storage()
                .persistent()
                .get::<DataKey, InvoiceTemplate>(&DataKey::Template(id))
            else {
                continue;
            };
            if !template.active || now < template.next_run_at {
                continue;
            }

            let invoice_id = create_invoice_record(
                &env,
                &template.merchant,
                template.amount_usdc,
                template.gross_usdc,
                template.expires_in_seconds,
                template.metadata_hash.clone(),
                template.payment_link_hash.clone(),
                0,
                MaybeAddress::None,
            )?;

            // Advance past every period already elapsed so the next run is in the
            // future: one call can never mint two invoices for the same period.
            let period = template.interval.period_seconds();
            let mut next_run_at = template.next_run_at;
            while next_run_at <= now {
                match next_run_at.checked_add(period) {
                    Some(advanced) => next_run_at = advanced,
                    None => {
                        next_run_at = u64::MAX;
                        break;
                    }
                }
            }
            template.next_run_at = next_run_at;
            env.storage()
                .persistent()
                .set(&DataKey::Template(id), &template);
            events::template_generated(&env, id, invoice_id);
            generated += 1;
        }
        Ok(generated)
    }
}

/// Appends `id` to the merchant's template index, preserving creation order.
fn index_template(env: &Env, merchant: &Address, id: u64) {
    let key = DataKey::MerchantTemplates(merchant.clone());
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(id);
    env.storage().persistent().set(&key, &ids);
}
