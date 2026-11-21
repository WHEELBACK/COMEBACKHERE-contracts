// contracts/invoice/src/lib.rs - Issue #859
#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, symbol_short};

#[derive(Clone)]
pub struct Invoice {
    pub id: u32,
    pub amount: i128,
    pub expires_at: u64,
    pub paid: bool,
}

#[contract]
pub struct InvoiceContract;

#[contractimpl]
impl InvoiceContract {
    pub fn create_invoice(env: Env, invoice_id: u32, amount: i128, expires_at: u64) -> Result<(), Symbol> {
        if amount <= 0 { return Err(symbol_short!("badamt")); }
        let invoice = Invoice { id: invoice_id, amount, expires_at, paid: false };
        env.storage().persistent().set(&invoice_id, &invoice);
        Ok(())
    }

    pub fn accept_payment(env: Env, invoice_id: u32, payer: Address, amount: i128) -> Result<(), Symbol> {
        payer.require_auth();
        let mut invoice: Invoice = env.storage().persistent().get(&invoice_id).ok_or(symbol_short!("notfnd"))?;

        // ✅ Check expiration against current ledger timestamp
        let current_timestamp = env.ledger().timestamp();
        if current_timestamp > invoice.expires_at { return Err(symbol_short!("expir")); }

        if amount != invoice.amount { return Err(symbol_short!("badamt")); }
        if invoice.paid { return Err(symbol_short!("paid")); }

        invoice.paid = true;
        env.storage().persistent().set(&invoice_id, &invoice);
        env.events().publish((symbol_short!("payment"),), &invoice_id);
        Ok(())
    }

    pub fn get_invoice(env: Env, invoice_id: u32) -> Result<Invoice, Symbol> {
        env.storage().persistent().get(&invoice_id).ok_or(symbol_short!("notfnd"))
    }

    pub fn is_expired(env: Env, invoice_id: u32) -> Result<bool, Symbol> {
        let invoice: Invoice = env.storage().persistent().get(&invoice_id).ok_or(symbol_short!("notfnd"))?;
        Ok(env.ledger().timestamp() > invoice.expires_at)
    }
}
