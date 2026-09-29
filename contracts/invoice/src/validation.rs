use crate::invoice::{DataKey, InvoiceError, MaybeBytes, MAX_HASH_BYTES, MAX_MEMO_BYTES, USDC_FACTOR};
use soroban_sdk::{Address, Env};

/// Maximum allowed expiry duration: 5 years in seconds.
pub const MAX_EXPIRY_SECONDS: u64 = 5 * 365 * 24 * 60 * 60;

/// Maximum allowed late fee: 1000 basis points (10%).
pub const MAX_LATE_FEE_BPS: u32 = 1000;

/// Required length of a payment link hash in bytes.
pub const PAYMENT_LINK_HASH_BYTES: u32 = 32;

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

/// Resolve the number of decimal places for a token, defaulting to the
/// configured USDC token's decimals (6) when the token is not registered.
fn token_decimals(env: &Env, token: &Address) -> u32 {
    let usdc: Address = env
        .storage()
        .instance()
        .get(&DataKey::UsdcToken)
        .unwrap_or_else(|| token.clone());
    if *token == usdc {
        6
    } else {
        env.storage()
            .instance()
            .get(&DataKey::TokenDecimals(token.clone()))
            .unwrap_or(6)
    }
}

/// Reject amounts below the minimum unit for the invoice's token.
/// This guards against off-by-factor errors (e.g., passing dollar cents instead of stroops).
pub fn require_usdc_precision(
    env: &Env,
    token: &Address,
    amount_usdc: i128,
    gross_usdc: i128,
) -> Result<(), InvoiceError> {
    let factor = 10i128.pow(token_decimals(env, token));
    if amount_usdc < factor || gross_usdc < factor {
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

/// Reject late_fee_bps values that exceed MAX_LATE_FEE_BPS.
pub fn require_late_fee_within_bound(late_fee_bps: u32) -> Result<(), InvoiceError> {
    if late_fee_bps > MAX_LATE_FEE_BPS {
        return Err(InvoiceError::InvalidLateFee);
    }
    Ok(())
}

/// Reject optional invoice hash fields that exceed the storage/cost cap.
pub fn require_hash_not_too_long(hash: &MaybeBytes) -> Result<(), InvoiceError> {
    if let MaybeBytes::Some(bytes) = hash {
        if bytes.len() > MAX_HASH_BYTES {
            return Err(InvoiceError::HashTooLong);
        }
    }
    Ok(())
}

/// Reject a payment_link_hash that is provided but not exactly 32 bytes.
///
/// Explicitly rejects empty values, all-zero hashes and oversized inputs so
/// that invoices cannot be created with a hash that can never be matched to
/// an off-chain payment link.
pub fn require_valid_payment_link_hash(hash: &MaybeBytes) -> Result<(), InvoiceError> {
    if let MaybeBytes::Some(bytes) = hash {
        let len = bytes.len();
        if len == 0 {
            return Err(InvoiceError::InvalidPaymentLinkHash);
        }
        if len > PAYMENT_LINK_HASH_BYTES {
            return Err(InvoiceError::InvalidPaymentLinkHash);
        }
        if len != PAYMENT_LINK_HASH_BYTES {
            return Err(InvoiceError::InvalidPaymentLinkHash);
        }
        let mut all_zero = true;
        for i in 0..len {
            if bytes.get(i).unwrap_or(0) != 0 {
                all_zero = false;
                break;
            }
        }
        if all_zero {
            return Err(InvoiceError::InvalidPaymentLinkHash);
        }
    }
    Ok(())
}

/// Reject an optional memo that exceeds the storage/cost cap.
pub fn require_memo_not_too_long(memo: &Option<String>) -> Result<(), InvoiceError> {
    if let Some(m) = memo {
        if m.len() > MAX_MEMO_BYTES {
            return Err(InvoiceError::MemoTooLong);
        }
    }
    Ok(())
}
