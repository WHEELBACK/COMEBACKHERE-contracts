#![no_std]

mod events;
mod invoice;
mod validation;

pub use events::InvoiceAmountUpdatedEvent;
pub use invoice::{BatchInvoiceParams, DataKey, Invoice, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes};

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
use validation::{
    require_admin, require_expiry_not_too_long, require_not_paused, require_positive_amount,
    require_usdc_precision,
};

fn append_history(env: &Env, id: u64, from: InvoiceStatus, to: InvoiceStatus) {
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

#[contractimpl]
impl InvoiceContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), InvoiceError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(InvoiceError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::InvoiceCount, &0u64);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    // --- #55: configurable grace window ---

    /// Set the grace window (seconds) added to expires_at when checking payment validity.
    /// Allows a short buffer after quote expiry for in-flight payments.
    pub fn set_grace_window(env: Env, admin: Address, seconds: u64) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::GraceWindow, &seconds);
        Ok(())
    }

    /// Return the current grace window in seconds (0 if not set).
    pub fn get_grace_window(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::GraceWindow)
            .unwrap_or(0u64)
    }

    /// Return the total number of invoices created so clients can page by id.
    pub fn get_invoice_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::InvoiceCount)
            .unwrap_or(0u64)
    }

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

        events::invoice_created(&env, id, &invoice);
        Ok(id)
    }

    /// Create multiple invoices atomically in a single invocation.
    /// All validations run on every element before any storage is written.
    /// Returns a Vec of assigned IDs in the same order as the input params.
    pub fn batch_create_invoice(
        env: Env,
        merchant: Address,
        params: Vec<BatchInvoiceParams>,
    ) -> Result<Vec<u64>, InvoiceError> {
        merchant.require_auth();
        require_not_paused(&env)?;

        // Validate all params before touching storage (atomicity).
        for p in params.iter() {
            require_positive_amount(p.amount_usdc, p.gross_usdc)?;
            require_usdc_precision(p.amount_usdc, p.gross_usdc)?;
            require_valid_payment_link_hash(&p.payment_link_hash)?;
            if p.expires_in_seconds == 0 {
                return Err(InvoiceError::ZeroDuration);
            }
            require_expiry_not_too_long(p.expires_in_seconds)?;
            if p.merchant_nonce != 0 {
                let nonce_key = DataKey::MerchantNonce(merchant.clone(), p.merchant_nonce);
                if env.storage().persistent().has(&nonce_key) {
                    return Err(InvoiceError::DuplicateNonce);
                }
            }
        }

        let mut ids = Vec::new(&env);
        for p in params.iter() {
            let count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::InvoiceCount)
                .unwrap_or(0);
            let id = count + 1;
            let expires_at = env
                .ledger()
                .timestamp()
                .checked_add(p.expires_in_seconds)
                .ok_or(InvoiceError::ExpiryOverflow)?;
            let invoice = Invoice {
                id,
                merchant: merchant.clone(),
                amount_usdc: p.amount_usdc,
                gross_usdc: p.gross_usdc,
                status: InvoiceStatus::Pending,
                expires_at,
                paid_at: None,
                payer: MaybeAddress::None,
                metadata_hash: p.metadata_hash.clone(),
                payment_link_hash: p.payment_link_hash.clone(),
                merchant_nonce: p.merchant_nonce,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Invoice(id), &invoice);
            env.storage().instance().set(&DataKey::InvoiceCount, &id);

            if p.merchant_nonce != 0 {
                env.storage().persistent().set(
                    &DataKey::MerchantNonce(merchant.clone(), p.merchant_nonce),
                    &true,
                );
            }

            let idx_key = DataKey::MerchantInvoices(merchant.clone());
            let mut merchant_ids: Vec<u64> = env
                .storage()
                .persistent()
                .get(&idx_key)
                .unwrap_or(Vec::new(&env));
            merchant_ids.push_back(id);
            env.storage().persistent().set(&idx_key, &merchant_ids);

            events::invoice_created(&env, id, &invoice);
            ids.push_back(id);
        }
        Ok(ids)
    }

    pub fn mark_paid(
        env: Env,
        admin: Address,
        id: u64,
        payer: Address,
        provided_metadata_hash: MaybeBytes,
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
        append_history(&env, id, InvoiceStatus::Pending, InvoiceStatus::Cancelled);
        events::invoice_cancelled(&env, id, &invoice);
        Ok(())
    }

    /// Expire all pending invoices whose `expires_at` has passed.
    ///
    /// IDs that do not correspond to an existing invoice are silently skipped,
    /// allowing callers to pass stale or cached ID lists without the call failing.
    /// Only invoices in `Pending` status that have passed their expiry timestamp
    /// are transitioned to `Expired`; all others (including missing IDs) are ignored.
    /// Returns the count of invoices actually expired.
    pub fn batch_expire(env: Env, admin: Address, ids: Vec<u64>) -> Result<u32, InvoiceError> {
        require_admin(&env, &admin)?;
        require_not_paused(&env)?;
        let now = env.ledger().timestamp();
        let mut expired_count: u32 = 0;
        for id in ids.iter() {
            let key = DataKey::Invoice(id);
            if let Some(mut invoice) = env.storage().persistent().get::<DataKey, Invoice>(&key) {
                if invoice.status == InvoiceStatus::Pending && now >= invoice.expires_at {
                    invoice.status = InvoiceStatus::Expired;
                    env.storage().persistent().set(&key, &invoice);
                    append_history(&env, id, InvoiceStatus::Pending, InvoiceStatus::Expired);
                    events::invoice_expired(&env, id, &invoice);
                    expired_count += 1;
                }
            }
        }
        Ok(expired_count)
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
        append_history(&env, id, InvoiceStatus::Paid, InvoiceStatus::RefundRequested);
        events::invoice_refund_requested(&env, id, &invoice);
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

    // --- #15: two-step admin transfer ---

    /// Initiate admin transfer. Current admin nominates `new_admin`.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        Ok(())
    }

    /// Complete admin transfer. Must be called by the pending admin.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), InvoiceError> {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(InvoiceError::NoPendingAdmin)?;
        if pending != new_admin {
            return Err(InvoiceError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    pub fn pause(env: Env, admin: Address) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        events::contract_paused(&env, &admin);
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        events::contract_unpaused(&env, &admin);
        Ok(())
    }
}

#[cfg(test)]
extern crate std;
