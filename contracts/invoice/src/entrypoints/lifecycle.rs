use crate::events::{self, InvoiceAmountUpdatedEvent};
use crate::validation::{
    require_admin, require_expiry_not_too_long, require_hash_not_too_long, require_not_paused,
    require_positive_amount, require_usdc_precision, require_valid_payment_link_hash,
};
use crate::{append_history, pending_index_add, pending_index_remove};
use crate::{
    DataKey, Invoice, InvoiceContract, InvoiceContractArgs, InvoiceContractClient, InvoiceError,
    InvoiceStatus, MaybeAddress, MaybeBytes,
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

        pending_index_add(&env, id);
        events::invoice_created(&env, id, &invoice);
        Ok(id)
    }

    // --- #553: transfer pending invoice ownership ---

    /// Transfer a pending invoice from the current merchant to a new merchant.
    /// Both the old and new merchant must authorize the transfer.
    /// Only pending invoices can be moved. The merchant index storage is
    /// updated in the same call and an `invoice_transferred` event is emitted.
    pub fn transfer_invoice(
        env: Env,
        old_merchant: Address,
        new_merchant: Address,
        id: u64,
    ) -> Result<(), InvoiceError> {
        old_merchant.require_auth();
        new_merchant.require_auth();
        require_not_paused(&env)?;

        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        if invoice.merchant != old_merchant {
            return Err(InvoiceError::Unauthorized);
        }

        if invoice.status != InvoiceStatus::Pending {
            return Err(InvoiceError::NotPending);
        }

        // Remove the invoice from the old merchant's index.
        let old_count_key = DataKey::MerchantInvoiceCount(old_merchant.clone());
        let old_count: u64 = env
            .storage()
            .persistent()
            .get(&old_count_key)
            .unwrap_or(0);
        let mut found = false;
        let mut i: u64 = 0;
        while i < old_count {
            let idx_key = DataKey::MerchantInvoiceIndex(old_merchant.clone(), i);
            if let Some(stored_id) = env.storage().persistent().get::<_, u64>(&idx_key) {
                if stored_id == id {
                    let last_key =
                        DataKey::MerchantInvoiceIndex(old_merchant.clone(), old_count - 1);
                    if i != old_count - 1 {
                        let last_id: u64 = env
                            .storage()
                            .persistent()
                            .get(&last_key)
                            .unwrap_or(0);
                        env.storage().persistent().set(&idx_key, &last_id);
                    }
                    env.storage().persistent().remove(&last_key);
                    env.storage()
                        .persistent()
                        .set(&old_count_key, &(old_count - 1));
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            return Err(InvoiceError::NotFound);
        }

        // Add the invoice to the new merchant's index.
        let new_count_key = DataKey::MerchantInvoiceCount(new_merchant.clone());
        let new_count: u64 = env
            .storage()
            .persistent()
            .get(&new_count_key)
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::MerchantInvoiceIndex(new_merchant.clone(), new_count),
            &id,
        );
        env.storage()
            .persistent()
            .set(&new_count_key, &(new_count + 1));

        invoice.merchant = new_merchant.clone();
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);

        events::invoice_transferred(&env, id, &old_merchant, &new_merchant);
        Ok(())
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
            .get(&DataKey::InvoiceCo

/* … truncated 7703 chars — edit only what you need near the top … */
