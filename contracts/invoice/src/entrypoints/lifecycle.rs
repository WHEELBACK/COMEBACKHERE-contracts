use crate::events::{self, InvoiceAmountUpdatedEvent};
use crate::validation::{
    require_admin, require_expiry_not_too_long, require_hash_not_too_long, require_not_paused,
    require_payer_not_merchant, require_positive_amount, require_usdc_precision, require_valid_payment_link_hash,
};
use crate::{append_history, payer_index_add, pending_index_add, pending_index_remove};
use crate::{
    DataKey, Invoice, InvoiceContract, InvoiceContractArgs, InvoiceContractClient, InvoiceError,
    InvoiceStatus, InvoiceSummary, MaybeAddress, MaybeBytes, MAX_PAGE_LIMIT,
};
use soroban_sdk::{contractimpl, Address, Env, Vec};

#[contractimpl]
impl InvoiceContract {
    // --- #58: merchant invoice nonce ---

    /// Create an invoice with an optional merchant-supplied nonce for idempotency.
    /// Pass `merchant_nonce = 0` to skip nonce enforcement.
    /// A non-zero nonce that has already been used for this merchant is rejected.
    #[allow(clippy::too_many_arguments)]
    pub fn create_invoice(
        env: Env,
        merchant: Address,
        amount_usdc: i128,
        gross_usdc: i128,
        expires_in_seconds: u64,
        metadata_hash: MaybeBytes,
        payment_link_hash: MaybeBytes,
        merchant_nonce: u64,
        token_address: MaybeAddress,
    ) -> Result<u64, InvoiceError> {
        merchant.require_auth();
        require_not_paused(&env)?;
        require_positive_amount(amount_usdc, gross_usdc)?;
        // #57: USDC decimal precision guardrail
        require_usdc_precision(amount_usdc, gross_usdc)?;
        require_hash_not_too_long(&metadata_hash)?;
        require_hash_not_too_long(&payment_link_hash)?;
        // #16: payment_link_hash must be exactly 32 bytes when provided
        require_valid_payment_link_hash(&payment_link_hash)?;

        if expires_in_seconds == 0 {
            return Err(InvoiceError::ZeroDuration);
        }
        require_expiry_not_too_long(expires_in_seconds)?;

        // #58: reject duplicate merchant nonce
        if merchant_nonce != 0 {
            let nonce_key = DataKey::MerchantNonce(merchant.clone(), merchant_nonce);
            if env.storage().persistent().has(&nonce_key) {
                return Err(InvoiceError::DuplicateNonce);
            }
        }

        create_invoice_record(
            &env,
            &merchant,
            amount_usdc,
            gross_usdc,
            expires_in_seconds,
            metadata_hash,
            payment_link_hash,
            merchant_nonce,
            token_address,
        )
    }

    pub fn mark_paid(
        env: Env,
        admin: Address,
        id: u64,
        payer: Address,
        provided_metadata_hash: MaybeBytes,
        payment_token: MaybeAddress,
    ) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        require_not_paused(&env)?;

        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        if invoice.status != InvoiceStatus::Pending {
            return Err(InvoiceError::NotPending);
        }

        // #717: reject self-pay, where the payer is the invoice's own merchant.
        require_payer_not_merchant(&payer, &invoice.merchant)?;

        if provided_metadata_hash != MaybeBytes::None
            && provided_metadata_hash != invoice.metadata_hash
        {
            return Err(InvoiceError::MetadataMismatch);
        }

        if let MaybeAddress::Some(expected) = &invoice.token_address {
            if payment_token != MaybeAddress::Some(expected.clone()) {
                return Err(InvoiceError::TokenMismatch);
            }
        }

        // #55: apply grace window — payment is valid up to expires_at + grace_window
        let grace: u64 = env
            .storage()
            .instance()
            .get(&DataKey::GraceWindow)
            .unwrap_or(0u64);
        let effective_deadline = invoice
            .expires_at
            .checked_add(grace)
            .unwrap_or(invoice.expires_at);
        if env.ledger().timestamp() >= effective_deadline {
            return Err(InvoiceError::Expired);
        }

        invoice.status = InvoiceStatus::Paid;
        invoice.paid_at = Some(env.ledger().timestamp());
        invoice.payer = MaybeAddress::Some(payer.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        // #711: the payer is recorded once, here, and never reassigned - there
        // is no entrypoint that moves an invoice to a different payer - so the
        // payer index never needs migrating. `reject_refund` returns the
        // invoice to `Paid` with the same payer, leaving the entry valid.
        payer_index_add(&env, &payer, id);
        pending_index_remove(&env, id);
        append_history(&env, id, InvoiceStatus::Pending, InvoiceStatus::Paid);
        events::invoice_paid(&env, id, &invoice);
        Ok(())
    }

    // --- #56: escrow release entrypoint ---

    /// Release escrow for a paid invoice. Admin-only. Transitions Paid → Released.
    pub fn release_escrow(env: Env, admin: Address, id: u64) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        require_not_paused(&env)?;

        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        if invoice.status != InvoiceStatus::Paid {
            return Err(InvoiceError::NotPaid);
        }

        invoice.status = InvoiceStatus::Released;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        append_history(&env, id, InvoiceStatus::Paid, InvoiceStatus::Released);
        events::escrow_released(&env, id, &invoice);
        Ok(())
    }

    pub fn get_invoice(env: Env, id: u64) -> Result<Invoice, InvoiceError> {
        env.storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)
    }

    pub fn get_invoice_status(env: Env, id: u64) -> Result<InvoiceStatus, InvoiceError> {
        let invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;
        Ok(invoice.status)
    }

    /// #712: lightweight invoice view for list screens.
    ///
    /// Returns the row fields a wallet needs - id, status, amount, expiry - plus
    /// `paid_usdc`/`remaining_usdc` so how much a payer still owes is readable
    /// in one call instead of by replaying `invoice_paid` and refund events.
    /// Reads the same `DataKey::Invoice` record as `get_invoice`, so it can never
    /// disagree with the full record.
    pub fn get_invoice_summary(env: Env, id: u64) -> Result<InvoiceSummary, InvoiceError> {
        let invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;
        Ok(InvoiceSummary::from(&invoice))
    }

    /// Return one status result per ID, preserving input order.
    pub fn batch_get_invoice_status(
        env: Env,
        ids: Vec<u64>,
    ) -> Vec<Result<InvoiceStatus, InvoiceError>> {
        let mut statuses = Vec::new(&env);
        for id in ids.iter() {
            statuses.push_back(Self::get_invoice_status(env.clone(), id));
        }
        statuses
    }

    /// Return up to `limit` invoices starting at `start_id` (inclusive).
    /// Gaps (IDs with no stored invoice) are skipped.
    pub fn get_invoices_page(env: Env, start_id: u64, limit: u64) -> Vec<Invoice> {
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::InvoiceCount)
            .unwrap_or(0);
        let end_id = start_id.saturating_add(limit).min(count + 1);
        let mut result = Vec::new(&env);
        let mut current = start_id;
        while current < end_id {
            if let Some(invoice) = env
                .storage()
                .persistent()
                .get::<DataKey, Invoice>(&DataKey::Invoice(current))
            {
                result.push_back(invoice);
            }
            current += 1;
        }
        result
    }

    /// Return the total number of invoices created so clients can page by id.
    pub fn get_invoice_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::InvoiceCount)
            .unwrap_or(0u64)
    }

    /// Return all IDs currently in the pending index.
    pub fn get_pending_ids(env: Env) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::PendingIndex)
            .unwrap_or_else(|| Vec::new(&env))
    }

    // Issue #49: merchant or admin may cancel a pending invoice
    pub fn cancel_invoice(env: Env, caller: Address, id: u64) -> Result<(), InvoiceError> {
        caller.require_auth();
        require_not_paused(&env)?;

        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        if caller != invoice.merchant && admin.as_ref() != Some(&caller) {
            return Err(InvoiceError::Unauthorized);
        }
        if invoice.status != InvoiceStatus::Pending {
            return Err(InvoiceError::NotPending);
        }

        invoice.status = InvoiceStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        pending_index_remove(&env, id);
        append_history(&env, id, InvoiceStatus::Pending, InvoiceStatus::Cancelled);
        events::invoice_cancelled(&env, id, &invoice);
        Ok(())
    }

    /// Amend a Pending invoice's amount fields before it has been paid or expired.
    /// Only the merchant who created the invoice may call this.
    pub fn amend_invoice(
        env: Env,
        merchant: Address,
        id: u64,
        new_amount_usdc: i128,
        new_gross_usdc: i128,
        new_expires_in_seconds: u64,
    ) -> Result<(), InvoiceError> {
        merchant.require_auth();
        require_not_paused(&env)?;
        require_positive_amount(new_amount_usdc, new_gross_usdc)?;
        require_usdc_precision(new_amount_usdc, new_gross_usdc)?;
        if new_expires_in_seconds == 0 {
            return Err(InvoiceError::ZeroDuration);
        }
        require_expiry_not_too_long(new_expires_in_seconds)?;

        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        if invoice.merchant != merchant {
            return Err(InvoiceError::Unauthorized);
        }
        if invoice.status != InvoiceStatus::Pending {
            return Err(InvoiceError::NotPending);
        }

        let event = InvoiceAmountUpdatedEvent {
            id,
            old_amount_usdc: invoice.amount_usdc,
            new_amount_usdc,
            old_gross_usdc: invoice.gross_usdc,
            new_gross_usdc,
        };

        invoice.amount_usdc = new_amount_usdc;
        invoice.gross_usdc = new_gross_usdc;
        invoice.expires_at = env
            .ledger()
            .timestamp()
            .checked_add(new_expires_in_seconds)
            .ok_or(InvoiceError::ExpiryOverflow)?;

        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        events::invoice_amended(&env, &event);
        Ok(())
    }

    // --- #9: paginated merchant invoice index read ---

    /// Return a page of invoice IDs for `merchant`.
    /// `start` is a zero-based offset; `limit` caps the returned slice.
    pub fn get_invoices_by_merchant(
        env: Env,
        merchant: Address,
        start: u32,
        limit: u32,
    ) -> Vec<u64> {
        let total: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantInvoiceCount(merchant.clone()))
            .unwrap_or(0);
        let start = u64::from(start).min(total);
        let end = start.saturating_add(u64::from(limit)).min(total);
        let mut page = Vec::new(&env);
        for i in start..end {
            if let Some(id) = env
                .storage()
                .persistent()
                .get::<DataKey, u64>(&DataKey::MerchantInvoiceIndex(merchant.clone(), i))
            {
                page.push_back(id);
            }
        }
        page
    }

    /// #711: return a page of invoice IDs that `payer` paid.
    ///
    /// The counterpart to `get_invoices_by_merchant`: it lets a wallet answer
    /// "what do I owe / what have I paid?" without scanning every invoice.
    ///
    /// `start` is a zero-based offset into the payer's index and `limit` caps
    /// the returned slice at [`MAX_PAGE_LIMIT`]; a larger `limit` is clamped
    /// rather than rejected, and a `start` past the end returns an empty page.
    /// Invoices with no designated payer (anything not yet `mark_paid`ed)
    /// belong to no index and never appear here.
    ///
    /// The index is bounded per payer by [`MAX_PAYER_INDEX_ENTRIES`]: it holds
    /// that many entries in `mark_paid` order, and payments beyond the cap are
    /// accepted but not indexed. Page `get_invoices_page` or the `invoice_paid`
    /// event stream for a payer's complete history.
    pub fn get_invoices_by_payer(env: Env, payer: Address, start: u32, limit: u32) -> Vec<u64> {
        let total: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::PayerIndexCount(payer.clone()))
            .unwrap_or(0);
        let start = u64::from(start).min(total);
        let limit = limit.min(MAX_PAGE_LIMIT);
        let end = start.saturating_add(u64::from(limit)).min(total);
        let mut page = Vec::new(&env);
        for i in start..end {
            if let Some(id) = env
                .storage()
                .persistent()
                .get::<DataKey, u64>(&DataKey::PayerIndexEntry(payer.clone(), i))
            {
                page.push_back(id);
            }
        }
        page
    }
}

/// Writes a new `Pending` invoice and returns its id: id allocation, nonce
/// recording, merchant index and pending index. Performs no authorization or
/// parameter validation — callers either validate and authenticate the
/// merchant themselves (`create_invoice`) or are the permissionless template
/// keeper acting on an already-authorized, already-validated template (#699).
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_invoice_record(
    env: &Env,
    merchant: &Address,
    amount_usdc: i128,
    gross_usdc: i128,
    expires_in_seconds: u64,
    metadata_hash: MaybeBytes,
    payment_link_hash: MaybeBytes,
    merchant_nonce: u64,
    token_address: MaybeAddress,
) -> Result<u64, InvoiceError> {
    let count: u64 = env
        .storage()
        .instance()
        .get(&DataKey::InvoiceCount)
        .unwrap_or(0);
    let id = count
        .checked_add(1)
        .ok_or(InvoiceError::InvoiceCountOverflow)?;
    let expires_at = env
        .ledger()
        .timestamp()
        .checked_add(expires_in_seconds)
        .ok_or(InvoiceError::ExpiryOverflow)?;
    if merchant_nonce != 0 {
        env.storage().persistent().set(
            &DataKey::MerchantNonce(merchant.clone(), merchant_nonce),
            &true,
        );
    }
    let invoice = Invoice {
        id,
        merchant: merchant.clone(),
        amount_usdc,
        gross_usdc,
        status: InvoiceStatus::Pending,
        expires_at,
        paid_at: None,
        payer: MaybeAddress::None,
        metadata_hash,
        payment_link_hash,
        merchant_nonce,
        token_address,
    };

    env.storage()
        .persistent()
        .set(&DataKey::Invoice(id), &invoice);
    env.storage().instance().set(&DataKey::InvoiceCount, &id);

    let merchant_count_key = DataKey::MerchantInvoiceCount(merchant.clone());
    let merchant_count: u64 = env
        .storage()
        .persistent()
        .get(&merchant_count_key)
        .unwrap_or(0);
    env.storage().persistent().set(
        &DataKey::MerchantInvoiceIndex(merchant.clone(), merchant_count),
        &id,
    );
    env.storage()
        .persistent()
        .set(&merchant_count_key, &(merchant_count + 1));

    pending_index_add(env, id);
    events::invoice_created(env, id, &invoice);
    Ok(id)
}
