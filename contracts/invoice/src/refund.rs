//! Refund verification and the cross-contract payout (#70).
//!
//! A refund is an instruction to move the customer's money back to them, and it
//! is executed by *two* contracts: this one decides, and the token contract
//! moves the funds. Both halves are collected here so that every entrypoint
//! which can turn a `RefundRequested` invoice into a `Refunded` one enforces
//! the same rules, and so a failure on the far side of the contract boundary
//! cannot leave the two halves disagreeing.
//!
//! - [`verify_payment_state`] — the payment-completion check that must pass
//!   before any refund state is written.
//! - [`refund_recipient`] — the only address a refund may be paid to.
//! - [`transfer_net_refund`] — the only cross-contract call, made through
//!   `try_transfer` so a token-side failure is a returned
//!   [`InvoiceError::RefundTransferFailed`] rather than a panic that aborts the
//!   call with no error code.

use crate::invoice::{Invoice, InvoiceError, MaybeAddress};
use soroban_sdk::{token, Address, Env};

/// Verifies that the stored invoice describes a completed payment that can
/// safely be refunded (#70).
///
/// `request_refund` only accepts a `Paid` invoice, and `mark_paid` is the only
/// way to reach `Paid`, so every reachable invoice passes this. It is a
/// defence-in-depth check on the exact state an approver is about to act on: it
/// is cheap, and it means a refund can never be recorded as executed against an
/// invoice whose payment evidence is missing or self-inconsistent, whatever
/// produced that state.
///
/// Errors: [`InvoiceError::PaymentStateInconsistent`] if the payment was never
/// timestamped, if the payer is unknown (so there is no one to refund), or if
/// the amount pair is not internally consistent (`0 < amount_usdc <=
/// gross_usdc`) — the same invariant `require_positive_amount` enforces at
/// creation time.
pub fn verify_payment_state(invoice: &Invoice) -> Result<(), InvoiceError> {
    if invoice.paid_at.is_none() {
        return Err(InvoiceError::PaymentStateInconsistent);
    }
    if matches!(invoice.payer, MaybeAddress::None) {
        return Err(InvoiceError::PaymentStateInconsistent);
    }
    if invoice.amount_usdc <= 0 || invoice.gross_usdc < invoice.amount_usdc {
        return Err(InvoiceError::PaymentStateInconsistent);
    }
    Ok(())
}

/// The address a refund must be paid to: the payer recorded at `mark_paid`.
///
/// Errors: [`InvoiceError::PaymentStateInconsistent`] if no payer is recorded.
/// [`verify_payment_state`] enforces the same invariant; this exists so the
/// extraction cannot silently fall through to a default address.
pub fn refund_recipient(invoice: &Invoice) -> Result<Address, InvoiceError> {
    match &invoice.payer {
        MaybeAddress::Some(payer) => Ok(payer.clone()),
        MaybeAddress::None => Err(InvoiceError::PaymentStateInconsistent),
    }
}

/// Moves `amount` of `token_contract` from this contract's own escrow balance to
/// `recipient` (#70).
///
/// The transfer is made with `try_transfer` so that a failing payment/token
/// contract — paused, out of escrow balance, missing the `transfer` export, or
/// simply not a SEP-41 contract at all — comes back as a value this function
/// can classify. Every failure path returns
/// [`InvoiceError::RefundTransferFailed`] **before** the caller writes any
/// refund state, so a failed payout can never leave an invoice marked
/// `Refunded` while the customer was not paid.
///
/// The `from` side is always this contract's own address, never a
/// caller-supplied one: a refund is funded from the escrow the invoice contract
/// holds, and a caller must not be able to choose which balance is debited.
pub fn transfer_net_refund(
    env: &Env,
    token_contract: &Address,
    recipient: &Address,
    amount: i128,
) -> Result<(), InvoiceError> {
    let from = env.current_contract_address();
    let transfer = token::Client::new(env, token_contract).try_transfer(&from, recipient, &amount);
    match transfer {
        Ok(Ok(())) => Ok(()),
        // Either the invocation itself failed (revert, missing export, auth
        // failure) or the token rejected the transfer. Both mean no tokens
        // moved, so the refund must not be recorded.
        _ => Err(InvoiceError::RefundTransferFailed),
    }
}
