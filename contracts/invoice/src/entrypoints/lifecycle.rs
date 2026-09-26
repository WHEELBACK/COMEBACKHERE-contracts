use crate::events::{self, InvoiceAmountUpdatedEvent};
use crate::refund::{
    calculate_net_refund, refund_recipient, transfer_net_refund, verify_payment_state,
};
use crate::validation::{
    require_admin, require_expiry_not_too_long, require_hash_not_too_long, require_not_paused,
    require_positive_amount, require_usdc_precision, require_valid_payment_link_hash,
};
use crate::{append_history, pending_index_add, pending_index_remove};
use crate::{
    DataKey, Invoice, InvoiceContract, InvoiceContractArgs, InvoiceContractClient, InvoiceError,
    InvoiceStatus, MaybeAddress, MaybeBytes, NetRefund,
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
    ///
    /// Records the refund decision on-chain only: no tokens move. Use
    /// [`Self::process_refund`] to approve *and* pay the payer out in the same
    /// transaction.
    ///
    /// #70: the invoice's payment state is verified first, so a refund can
    /// never be approved against an invoice that does not describe a completed
    /// payment. Errors: `NotRefundRequested`, `PaymentStateInconsistent`.
    /// Emits: `refund_approved`.
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
        verify_payment_state(&invoice)?;

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

    /// Approve a refund **and** pay the payer out on-chain. Admin-only.
    /// Transitions `RefundRequested` → `Refunded`.
    ///
    /// This is the refund path that actually moves money, and it spans a
    /// contract boundary: the amount is transferred by the invoice's own
    /// `token_address` contract from the escrow balance this contract holds
    /// (#70). Four properties make the boundary safe:
    ///
    /// 1. **The payment state is verified before anything else.** An invoice
    ///    that does not describe a completed payment is rejected with
    ///    `PaymentStateInconsistent`, so no payout is ever attempted against it.
    /// 2. **The payout is the net of the documented fees, not the gross.** The
    ///    `fee_bps` argument is the merchant's payment-gateway policy; the
    ///    customer's `net_amount` is computed once by
    ///    [`calculate_net_refund`] and is the exact amount transferred (#71).
    /// 3. **The transfer is the last thing that can fail, and it fails
    ///    safely.** `try_transfer` turns any token-side failure into
    ///    `RefundTransferFailed`, and because the status transition is written
    ///    only after the transfer returns `Ok`, a failed payout leaves the
    ///    invoice in `RefundRequested` — retryable, and not falsely recorded as
    ///    refunded.
    /// 4. **The funds debited are not caller-chosen.** They come from this
    ///    contract's own escrow balance, and the recipient is the payer recorded
    ///    at `mark_paid`, not anything the caller supplies.
    ///
    /// Returns the [`NetRefund`] that was applied, which is also stored under
    /// `DataKey::RefundBreakdown` and published in the `refund_processed` event.
    ///
    /// Errors: `NotRefundRequested`, `PaymentStateInconsistent`,
    /// `RefundTokenNotSet`, `RefundFeeTooHigh`, `RefundTransferFailed`.
    /// Emits: `refund_approved`, `refund_processed`.
    pub fn process_refund(
        env: Env,
        admin: Address,
        id: u64,
        fee_bps: u32,
    ) -> Result<NetRefund, InvoiceError> {
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
        // Verify before resolving the recipient and the token, so an
        // inconsistent invoice cannot reach the cross-contract call at all.
        verify_payment_state(&invoice)?;
        let payer = refund_recipient(&invoice)?;
        let token_id = match &invoice.token_address {
            MaybeAddress::Some(token_id) => token_id.clone(),
            MaybeAddress::None => return Err(InvoiceError::RefundTokenNotSet),
        };
        // Reject an out-of-range fee before the transfer, not after.
        let refund = calculate_net_refund(invoice.amount_usdc, fee_bps)?;

        // Payout first: on failure this returns `Err` and none of the writes
        // below run, which is what keeps the two contracts from disagreeing.
        transfer_net_refund(&env, &token_id, &payer, refund.net_amount)?;

        invoice.status = InvoiceStatus::Refunded;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        env.storage()
            .persistent()
            .set(&DataKey::RefundBreakdown(id), &refund);
        append_history(
            &env,
            id,
            InvoiceStatus::RefundRequested,
            InvoiceStatus::Refunded,
        );
        events::refund_approved(&env, id, &invoice);
        events::refund_processed(&env, id, &payer, &refund);
        Ok(refund)
    }

    /// Read-only preview of the fee arithmetic `process_refund` would apply to a
    /// refund of `gross_amount` under `fee_bps` (#71), so a merchant UI can show
    /// the customer what they will actually receive before approving.
    ///
    /// Deliberately not gated by `require_not_paused`: quoting a refund is a
    /// read, and an operator working out what a paused contract owes its
    /// customers is exactly the case where that read has to keep working.
    ///
    /// Errors: `RefundFeeTooHigh`, `InvalidAmount`, `ArithmeticOverflow`.
    pub fn calculate_net_refund(
        _env: Env,
        gross_amount: i128,
        fee_bps: u32,
    ) -> Result<NetRefund, InvoiceError> {
        calculate_net_refund(gross_amount, fee_bps)
    }

    /// The fee breakdown recorded by `process_refund` for `id`, if any.
    /// Returns `NotFound` for an invoice that was approved without a payout
    /// (`approve_refund`) or has not been refunded yet.
    pub fn get_refund_breakdown(env: Env, id: u64) -> Result<NetRefund, InvoiceError> {
        env.storage()
            .persistent()
            .get(&DataKey::RefundBreakdown(id))
            .ok_or(InvoiceError::NotFound)
    }

    /// Reject a refund request. Admin-only. Transitions RefundRequested → Paid.
    /// #70: the payment state is verified for the same reason as on
    /// `approve_refund` — a refund round-trip must not be able to land on an
    /// invoice that never described a completed payment.
    pub fn reject_refund(env: Env, admin: Address, id: u64) -> Result<(), InvoiceError> {
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
        verify_payment_state(&invoice)?;

        invoice.status = InvoiceStatus::Paid;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        append_history(
            &env,
            id,
            InvoiceStatus::RefundRequested,
            InvoiceStatus::Paid,
        );
        events::refund_rejected(&env, id, &invoice);
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
}
