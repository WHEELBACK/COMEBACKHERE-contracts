use crate::events;
use crate::validation::{
    require_admin, require_expiry_not_too_long, require_not_paused, require_positive_amount,
    require_usdc_precision, require_valid_payment_link_hash,
};
use crate::{append_history, pending_index_add, pending_index_remove};
use crate::{
    BatchInvoiceParams, DataKey, Invoice, InvoiceContract, InvoiceError, InvoiceStatus,
    MaybeAddress,
};
use soroban_sdk::{contractimpl, Address, Env, Vec};

#[contractimpl]
impl InvoiceContract {
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
        let mut batch_nonces: Vec<u64> = Vec::new(&env);
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
                if env.storage().persistent().has(&nonce_key)
                    || batch_nonces.contains(p.merchant_nonce)
                {
                    return Err(InvoiceError::DuplicateNonce);
                }
                batch_nonces.push_back(p.merchant_nonce);
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
                token_address: p.token_address.clone(),
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

            pending_index_add(&env, id);
            events::invoice_created(&env, id, &invoice);
            ids.push_back(id);
        }
        Ok(ids)
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
                    pending_index_remove(&env, id);
                    append_history(&env, id, InvoiceStatus::Pending, InvoiceStatus::Expired);
                    events::invoice_expired(&env, id, &invoice);
                    expired_count += 1;
                }
            }
        }
        Ok(expired_count)
    }
}
