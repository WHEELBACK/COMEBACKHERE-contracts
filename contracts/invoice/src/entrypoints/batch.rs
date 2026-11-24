use crate::events;
use crate::validation::{
    require_admin, require_expiry_not_too_long, require_hash_not_too_long, require_not_paused,
    require_positive_amount, require_usdc_precision, require_valid_payment_link_hash,
};
use crate::{append_history, pending_index_add_many, pending_index_remove_many};
use crate::{
    BatchInvoiceParams, DataKey, Invoice, InvoiceContract, InvoiceContractArgs,
    InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes, INVOICE_TTL_EXTEND_TO, INVOICE_TTL_THRESHOLD,
    MAX_BATCH_EXPIRE, MAX_BATCH_SIZE,
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
            require_hash_not_too_long(&p.metadata_hash)?;
            require_hash_not_too_long(&p.payment_link_hash)?;
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

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::InvoiceCount)
            .unwrap_or(0);
        count
            .checked_add(params.len() as u64)
            .ok_or(InvoiceError::InvoiceCountOverflow)?;

        // #880: the invoice counter, the merchant's index counter and the
        // pending index are each read once and written once for the whole
        // batch, instead of being deserialized and re-serialized per element.
        let merchant_count_key = DataKey::MerchantInvoiceCount(merchant.clone());
        let mut merchant_count: u64 = env
            .storage()
            .persistent()
            .get(&merchant_count_key)
            .unwrap_or(0);

        let mut ids = Vec::new(&env);
        for p in params.iter() {
            let id = count
                .checked_add(1)
                .ok_or(InvoiceError::InvoiceCountOverflow)?;
            count = id;
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
            crate::store_invoice(&env, &invoice)?;
            env.storage().persistent().extend_ttl(
                &DataKey::Invoice(id),
                INVOICE_TTL_THRESHOLD,
                INVOICE_TTL_EXTEND_TO,
            );

            if p.merchant_nonce != 0 {
                let nonce_key = DataKey::MerchantNonce(merchant.clone(), p.merchant_nonce);
                env.storage().persistent().set(&nonce_key, &true);
                crate::bump_persistent(&env, &nonce_key);
            }

            let merchant_index_key =
                DataKey::MerchantInvoiceIndex(merchant.clone(), merchant_count);
            env.storage().persistent().set(&merchant_index_key, &id);
            crate::bump_persistent(&env, &merchant_index_key);
            merchant_count += 1;

            events::invoice_created(&env, id, &invoice);
            ids.push_back(id);
        }

        if !ids.is_empty() {
            env.storage().instance().set(&DataKey::InvoiceCount, &count);
            env.storage()
                .persistent()
                .set(&merchant_count_key, &merchant_count);
            crate::bump_persistent(&env, &merchant_count_key);
            pending_index_add_many(&env, &ids);
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
        if ids.len() > MAX_BATCH_EXPIRE {
            return Err(InvoiceError::BatchTooLarge);
        }
        let now = env.ledger().timestamp();
        let mut expired_count: u32 = 0;
        // #880: drop expired ids from the pending index in one pass.
        let mut expired_ids: Vec<u64> = Vec::new(&env);
        for id in ids.iter() {
            let key = DataKey::Invoice(id);
            if let Some(mut invoice) = env.storage().persistent().get::<DataKey, Invoice>(&key) {
                if invoice.status == InvoiceStatus::Pending && now >= invoice.expires_at {
                    invoice.status = InvoiceStatus::Expired;
                    crate::store_invoice(&env, &invoice)?;
                    env.storage().persistent().extend_ttl(
                        &key,
                        INVOICE_TTL_THRESHOLD,
                        INVOICE_TTL_EXTEND_TO,
                    );
                    expired_ids.push_back(id);
                    append_history(&env, id, InvoiceStatus::Pending, InvoiceStatus::Expired);
                    events::invoice_expired(&env, id, &invoice);
                    expired_count += 1;
                }
            }
        }
        pending_index_remove_many(&env, &expired_ids);
        Ok(expired_count)
    }

    /// Cancel several pending invoices belonging to `merchant` in one call (#720).
    ///
    /// Atomic: every id must exist, belong to `merchant`, and be `Pending`, or
    /// the whole batch fails with no storage writes. Capped at `MAX_BATCH_SIZE`.
    /// Emits one `invoice_cancelled` event per cancelled id. `reason` is caller
    /// context for off-chain consumers; it is not persisted on-chain.
    #[allow(unused_variables)]
    pub fn batch_cancel_invoices(
        env: Env,
        merchant: Address,
        ids: Vec<u64>,
        reason: MaybeBytes,
    ) -> Result<(), InvoiceError> {
        merchant.require_auth();
        require_not_paused(&env)?;

        if ids.len() > MAX_BATCH_SIZE {
            return Err(InvoiceError::BatchTooLarge);
        }

        // Validate every id before writing anything (atomicity).
        let mut invoices: Vec<Invoice> = Vec::new(&env);
        for id in ids.iter() {
            let invoice: Invoice = env
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
            invoices.push_back(invoice);
        }

        let mut cancelled_ids: Vec<u64> = Vec::new(&env);
        for mut invoice in invoices.iter() {
            invoice.status = InvoiceStatus::Cancelled;
            crate::store_invoice(&env, &invoice)?;
            cancelled_ids.push_back(invoice.id);
            append_history(
                &env,
                invoice.id,
                InvoiceStatus::Pending,
                InvoiceStatus::Cancelled,
            );
            events::invoice_cancelled(&env, invoice.id, &invoice);
        }
        // #880: one read and one write of the pending index for the batch.
        pending_index_remove_many(&env, &cancelled_ids);

        Ok(())
    }
}
