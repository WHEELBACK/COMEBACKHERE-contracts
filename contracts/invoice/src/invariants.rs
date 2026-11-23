//! Strict state invariants for stored invoices (#882).
//!
//! Each status implies a fixed shape for the rest of the record. Entrypoints
//! maintain these invariants individually today, but nothing verified them as
//! a whole, so a future change to one entrypoint could persist a record that
//! is internally contradictory (for example a `Pending` invoice that already
//! has a payer, or a `Paid` invoice with no payment timestamp). Every invoice
//! write goes through [`crate::store_invoice`], which calls
//! [`verify_invoice_invariants`] first and refuses to persist a violating
//! record.

use crate::invoice::{Invoice, InvoiceError, InvoiceStatus, MaybeAddress};

/// Checks that `invoice` is internally consistent.
///
/// - **Amounts:** `0 < amount_usdc <= gross_usdc`, the same pair invariant
///   `create_invoice`/`amend_invoice` enforce on input.
/// - **Not yet paid** (`Pending`, `Expired`, `Cancelled`): no payment can have
///   been recorded, so `paid_at` and `payer` are both unset.
/// - **Paid or later** (`Paid`, `RefundRequested`, `Released`, `Refunded`): a
///   payment was recorded by `mark_paid`, so `paid_at` and `payer` are both set.
/// - **Payer independence:** a recorded payer is never the merchant (#717),
///   including after `transfer_invoice` reassigns the payer.
///
/// Errors: [`InvoiceError::InvariantViolation`] on any breach.
pub fn verify_invoice_invariants(invoice: &Invoice) -> Result<(), InvoiceError> {
    if invoice.amount_usdc <= 0 || invoice.gross_usdc < invoice.amount_usdc {
        return Err(InvoiceError::InvariantViolation);
    }

    let has_payer = matches!(invoice.payer, MaybeAddress::Some(_));
    let has_paid_at = invoice.paid_at.is_some();

    match invoice.status {
        InvoiceStatus::Pending | InvoiceStatus::Expired | InvoiceStatus::Cancelled => {
            if has_payer || has_paid_at {
                return Err(InvoiceError::InvariantViolation);
            }
        }
        InvoiceStatus::Paid
        | InvoiceStatus::RefundRequested
        | InvoiceStatus::Released
        | InvoiceStatus::Refunded => {
            if !has_payer || !has_paid_at {
                return Err(InvoiceError::InvariantViolation);
            }
        }
    }

    if let MaybeAddress::Some(payer) = &invoice.payer {
        if *payer == invoice.merchant {
            return Err(InvoiceError::InvariantViolation);
        }
    }

    Ok(())
}
