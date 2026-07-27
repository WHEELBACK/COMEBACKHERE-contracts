use crate::events::{self, InvoiceAmountUpdatedEvent};
use crate::validation::{
    require_admin, require_expiry_not_too_long, require_not_paused, require_positive_amount,
    require_usdc_precision, require_valid_payment_link_hash,
};
use crate::{append_history, pending_index_add, pending_index_remove};
use crate::{DataKey, Invoice, InvoiceContract, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes};
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
            env.storage().persistent().set(&nonce_key, &true);
        }

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::InvoiceCount)
            .unwrap_or(0);
        let id = count + 1;
        let expires_at = env
            .ledger()
            .timestamp()
            .checked_add(expires_in_seconds)
            .ok_or(InvoiceError::ExpiryOverflow)?;
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

        // #9: maintain merchant invoice index
        let idx_key = DataKey::MerchantInvoices(merchant.clone());
        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&idx_key)
            .unwrap_or(Vec::new(&env));
        ids.push_back(id);
        env.storage().persistent().set(&idx_key, &ids);

        pending_index_add(&env, id);
        events::invoice_created(&env, id, &invoice);
        Ok(id)
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
        invoice.payer = MaybeAddress::Some(payer);
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
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

        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if caller != invoice.merchant && caller != admin {
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

    // payer may request a refund on a paid invoice (escrow dispute)
    pub fn request_refund(env: Env, payer: Address, id: u64) -> Result<(), InvoiceError> {
        payer.require_auth();
        require_not_paused(&env)?;

        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        if invoice.status != InvoiceStatus::Paid {
            return Err(InvoiceError::NotPaid);
        }
        if invoice.payer != MaybeAddress::Some(payer.clone()) {
            return Err(InvoiceError::Unauthorized);
        }

        invoice.status = InvoiceStatus::RefundRequested;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        append_history(
            &env,
            id,
            InvoiceStatus::Paid,
            InvoiceStatus::RefundRequested,
        );
        events::invoice_refund_requested(&env, id, &invoice);
        Ok(())
    }

    /// Approve a refund request. Admin-only. Transitions RefundRequested → Refunded.
    pub fn approve_refund(env: Env, admin: Address, id: u64) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        require_not_paused(&env)?;

        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        if invoice.status != InvoiceStatus::RefundRequested {
            return Err(InvoiceError::NotRefundRequested);
        }

        invoice.status = InvoiceStatus::Refunded;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        append_history(
            &env,
            id,
            InvoiceStatus::RefundRequested,
            InvoiceStatus::Refunded,
        );
        events::refund_approved(&env, id, &invoice);
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
        let ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantInvoices(merchant))
            .unwrap_or(Vec::new(&env));
        let total = ids.len();
        let start = start.min(total);
        let end = (start + limit).min(total);
        let mut page = Vec::new(&env);
        for i in start..end {
            page.push_back(ids.get(i).unwrap());
        }
        page
    }
}
