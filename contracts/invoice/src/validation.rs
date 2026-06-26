use crate::invoice::{DataKey, InvoiceError, USDC_FACTOR};
use soroban_sdk::{Address, Env};

pub fn require_not_paused(env: &Env) -> Result<(), InvoiceError> {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        return Err(InvoiceError::ContractPaused);
    }
    Ok(())
}

pub fn require_admin(env: &Env, admin: &Address) -> Result<(), InvoiceError> {
    admin.require_auth();
    let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
    if stored != *admin {
        return Err(InvoiceError::Unauthorized);
    }
    Ok(())
}

pub fn require_positive_amount(amount_usdc: i128, gross_usdc: i128) -> Result<(), InvoiceError> {
    if amount_usdc <= 0 || gross_usdc < amount_usdc {
        return Err(InvoiceError::InvalidAmount);
    }
    Ok(())
}

/// Reject amounts below the minimum USDC unit (1 USDC = USDC_FACTOR stroops).
/// This guards against off-by-factor errors (e.g., passing dollar cents instead of stroops).
pub fn require_usdc_precision(amount_usdc: i128, gross_usdc: i128) -> Result<(), InvoiceError> {
    if amount_usdc < USDC_FACTOR || gross_usdc < USDC_FACTOR {
        return Err(InvoiceError::AmountPrecision);
    }
    Ok(())
}

/// If payment_link_hash is provided, it must be exactly 32 bytes (SHA-256 digest).
pub fn require_valid_payment_link_hash(
    hash: &crate::invoice::MaybeBytes,
) -> Result<(), InvoiceError> {
    if let crate::invoice::MaybeBytes::Some(ref bytes) = hash {
        if bytes.len() != 32 {
            return Err(InvoiceError::InvalidPaymentLinkHash);
        }
    }
    Ok(())
}
