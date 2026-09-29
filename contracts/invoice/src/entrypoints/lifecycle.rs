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

/// #556: upper bound for the configurable late fee, in basis points (10%).
pub const MAX_LATE_FEE_BPS: u32 = 1_000;

#[contractimpl]
impl InvoiceContract {
    // --- #58: merchant invoice nonce ---

    /// Create an invoice with an optional merchant-supplied nonce for idempotency.
    /// Pass `merchant_nonce = 0` to skip nonce enforcement.
    /// A non-zero nonce that has already been used for this merchant is rejected.
    ///
    /// #531: `token_address` stores the asset identifier for the invoice.
    /// When omitted, the configured USDC token is used for backwards
    /// compatibility.
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

        // #531: resolve the invoice token, defaulting to the configured USDC
        // address so existing callers that omit a token keep working.
        let token: Address = match token_address {
            MaybeAddress::Some(addr) => addr,
            MaybeAddress::None => env
                .storage()
                .instance()
                .get(&DataKey::UsdcToken)
                .ok_or(InvoiceError::NotInitialized)?,
        };

        // #57: token-aware decimal precision guardrail
        require_usdc_precision(&env, &token, amount_usdc, gross_usdc)?;
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

        // #537: enforce per-merchant open invoice limit
        let max_open: u64 = env
            .storage()
            .instance()
            .get(&DataKey::MaxOpenInvoices)
            .unwrap_or(0u64);
        if max_open != 0 {
            let open_key = DataKey::MerchantOpenInvoiceCount(merchant.clone());
            let open_count: u64 = env
                .storage()
                .persistent()
                .get(&open_key)
                .unwrap_or(0);
            if open_count >= max_open {
                return Err(InvoiceError::MerchantOpenInvoiceLimitReached);
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
            token_address: MaybeAddress::Some(token),
            amount_paid: 0,
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

        // #537: track open (pending) invoice count for this merchant
        let open_key = DataKey::MerchantOpenInvoiceCount(merchant.clone());
        let open_count: u64 = env
            .storage()
            .persistent()
            .get(&open_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&open_key, &(open_count + 1));

        pending_index_add(&env, id);
        events::invoice_created(&env, id, &invoice);
        Ok(id)
    }

    /// #558: lightweight invoice view returning only the essentials
    /// (id, status, amount and expiry) for list views. Reads from the same
    /// `DataKey::Invoice` storage as `get_invoice`, so it can never go out of
    /// sync with the full record.
    pub fn get_invoice_summary(env: Env, id: u64) -> Result<InvoiceSummary, InvoiceError> {
        let invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;
        Ok(InvoiceSummary {
            id: invoice.id,
            status: invoice.status,
            amount_usdc: invoice.amount_usdc,
            expires_at: invoice.expires_at,
        })
    }

    /// #556: configure the late fee (in basis points) applied to payments
    /// settled inside the grace window after expiry. Admin-only. The value is
    /// bounded by `MAX_LATE_FEE_BPS`; `0` disables the fee.
    pub fn set_late_fee_bps(env: Env, admin: Address, late_fee_bps: u32) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        require_not_paused(&env)?;
        if late_fee_bps > MAX_LATE_FEE_BPS {
            return Err(InvoiceError::LateFeeTooHigh);
        }
        env.storage()
            .instance()
            .set(&DataKey::LateFeeBps, &late_fee_bps);
        Ok(())
    }

    /// #556: read the currently configured late fee in basis points.
    pub fn get_late_fee_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::LateFeeBps)
            .unwrap_or(0u32)
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
        let now = env.ledger().timestamp();
        if now >= effective_deadline {
            return Err(InvoiceError::Expired);
        }

        // #556: apply the merchant-configured late fee only when the payment
        // lands inside the grace window (i.e. after expiry but before the
        // effective deadline). On-time payments are never charged a fee.
        if now > invoice.expires_at {
            let late_fee_bps: u32 = env
                .storage()
                .instance()
                .get(&DataKey::LateFeeBps)
                .unwrap_or(0u32);
            if late_fee_bps > 0 {
                let fee = invoice
                    .amount_usdc
                    .checked_mul(late_fee_bps as i128)
                    .ok_or(InvoiceError::AmountOverflow)?
                    / 10_000i128;
                invoice.amount_usdc = invoice
                    .amount_usdc
                    .checked_add(fee)
                    .ok_or(InvoiceError::AmountOverflow)?;
            }
        }

        invoice.status = InvoiceStatus::Paid;
        invoice.paid_at = Some(now);
        invoice.payer = MaybeAddress::Some(payer);
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        pending_index_remove(&env, id);
        // #537: decrement merchant open count on exit from pending
        Self::decrement_open_count(&env, &invoice.merchant);
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
        events::invoice_released(&env, id, &invoice);
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
        verify_payment_state(&invoice)?;

        let new_total = invoice
            .amount_paid
            .checked_add(amount)
            .ok_or(InvoiceError::Amoun

/* … truncated 3061 chars — edit only what you need near the top … */
