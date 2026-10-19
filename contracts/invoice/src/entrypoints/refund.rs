//! Refund entrypoints for `InvoiceContract`, split out of `lifecycle.rs` for
//! readability (#719). No ABI change: soroban-sdk merges every submodule's
//! `#[contractimpl]` block into a single generated client.

use crate::events;
use crate::invoice::{MaybeU64, RefundStatus};
use crate::refund::{
    calculate_net_refund_with_network_fee, configured_network_fee, refund_recipient,
    transfer_net_refund, verify_payment_state,
};
use crate::validation::{require_admin, require_not_paused};
use crate::append_history;
use crate::{
    DataKey, Invoice, InvoiceContract, InvoiceContractArgs, InvoiceContractClient, InvoiceError,
    InvoiceStatus, MaybeAddress, NetRefund,
};
use soroban_sdk::{contractimpl, Address, Env};

#[contractimpl]
impl InvoiceContract {
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

        // #702: reject refund requests outside the configurable window.
        // A window of 0 means no limit (preserving the current behaviour).
        let refund_window: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RefundWindow)
            .unwrap_or(0u64);
        if refund_window > 0 {
            if let Some(paid_at) = invoice.paid_at {
                // paid_at + refund_window may overflow; use saturating_add so a
                // very large window never wraps to a timestamp in the past.
                let deadline = paid_at.saturating_add(refund_window);
                if env.ledger().timestamp() >= deadline {
                    return Err(InvoiceError::RefundWindowClosed);
                }
            }
        }

        let now = env.ledger().timestamp();
        invoice.status = InvoiceStatus::RefundRequested;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
        // Store the request timestamp for get_refund_status (#700).
        env.storage()
            .persistent()
            .set(&DataKey::RefundRequestedAt(id), &now);
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
        let refund = calculate_net_refund_with_network_fee(
            invoice.amount_usdc,
            fee_bps,
            configured_network_fee(&env),
        )?;

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
        env: Env,
        gross_amount: i128,
        fee_bps: u32,
    ) -> Result<NetRefund, InvoiceError> {
        calculate_net_refund_with_network_fee(gross_amount, fee_bps, configured_network_fee(&env))
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

    /// Return the consolidated refund state and fee breakdown for `id` in a
    /// single call (#700).
    ///
    /// Combines the invoice status, the ledger timestamp at which
    /// `request_refund` was called (if any), and the fee breakdown stored by
    /// `process_refund` (if any), so clients do not need to combine
    /// `get_invoice` and `get_refund_breakdown` separately.
    ///
    /// Read-only; no auth required. Works regardless of whether the contract
    /// is paused (quoting refund progress is a read and must remain available).
    ///
    /// - `requested_at` is `MaybeU64::None` when no refund has been requested.
    /// - Fee fields default to `0` when no breakdown has been stored yet
    ///   (e.g. the invoice is still in `Paid` status or was approved via
    ///   `approve_refund` rather than `process_refund`).
    ///
    /// Errors: [`InvoiceError::NotFound`] if no invoice exists for `id`.
    pub fn get_refund_status(env: Env, id: u64) -> Result<RefundStatus, InvoiceError> {
        let invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        let requested_at: MaybeU64 = env
            .storage()
            .persistent()
            .get(&DataKey::RefundRequestedAt(id))
            .map(MaybeU64::Some)
            .unwrap_or(MaybeU64::None);

        let breakdown: Option<NetRefund> = env
            .storage()
            .persistent()
            .get(&DataKey::RefundBreakdown(id));

        let (gross_amount, processing_fee, network_fee, net_amount) =
            breakdown.map_or((0, 0, 0, 0), |b| {
                (
                    b.gross_amount,
                    b.processing_fee,
                    b.network_fee,
                    b.net_amount,
                )
            });

        Ok(RefundStatus {
            status: invoice.status,
            requested_at,
            gross_amount,
            processing_fee,
            network_fee,
            net_amount,
        })
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
}
