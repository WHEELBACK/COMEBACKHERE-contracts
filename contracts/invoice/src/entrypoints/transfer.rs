//! Invoice ownership transfer entrypoints.
//!
//! Allows the current payer of a paid invoice to transfer their claim (and any
//! associated refund rights) to a different address. This is useful when a
//! paying party needs to assign their position to another entity.
//!
//! Only the payer may initiate a transfer; the target address must differ from
//! the current payer. Transfers are only valid on `Paid` invoices.

use crate::events;
use crate::validation::require_not_paused;
use crate::{
    DataKey, Invoice, InvoiceContract, InvoiceContractArgs, InvoiceContractClient, InvoiceError,
    InvoiceStatus, MaybeAddress,
};
use soroban_sdk::{contractimpl, Address, Env};

#[contractimpl]
impl InvoiceContract {
    // --- #695: invoice transfer entrypoint ---

    /// Transfer payer ownership of a paid invoice to a new address.
    ///
    /// Authorization rules:
    /// - Only the current payer (`from`) may call this.
    /// - The invoice must be in `Paid` status.
    /// - `to` must differ from `from`.
    ///
    /// Emits `invoice_transferred` on success.
    pub fn transfer_invoice(
        env: Env,
        from: Address,
        id: u64,
        to: Address,
    ) -> Result<(), InvoiceError> {
        from.require_auth();
        require_not_paused(&env)?;

        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(id))
            .ok_or(InvoiceError::NotFound)?;

        // Only the current payer may initiate a transfer
        if invoice.payer != MaybeAddress::Some(from.clone()) {
            return Err(InvoiceError::Unauthorized);
        }

        // Transfers are only valid on paid invoices
        if invoice.status != InvoiceStatus::Paid {
            return Err(InvoiceError::NotPaid);
        }

        // Target must differ from sender
        if from == to {
            return Err(InvoiceError::InvalidAmount);
        }

        invoice.payer = MaybeAddress::Some(to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);

        events::invoice_transferred(&env, id, &from, &to);
        Ok(())
    }
}
