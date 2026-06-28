use crate::invoice::{DataKey, InvoiceError, MaybeBytes, USDC_FACTOR};
use soroban_sdk::{Address, Env};

/// Maximum allowed expiry duration: 5 years in seconds.
pub const MAX_EXPIRY_SECONDS: u64 = 5 * 365 * 24 * 60 * 60;

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

/// Reject expires_in_seconds values that exceed MAX_EXPIRY_SECONDS.
pub fn require_expiry_not_too_long(expires_in_seconds: u64) -> Result<(), InvoiceError> {
    if expires_in_seconds > MAX_EXPIRY_SECONDS {
        return Err(InvoiceError::ExpiryTooLong);
    }
    Ok(())
}

/// Reject a payment_link_hash that is provided but not exactly 32 bytes.
pub fn require_valid_payment_link_hash(hash: &MaybeBytes) -> Result<(), InvoiceError> {
    if let MaybeBytes::Some(bytes) = hash {
        if bytes.len() != 32 {
            return Err(InvoiceError::InvalidPaymentLinkHash);
        }
    }
    Ok(())
}
