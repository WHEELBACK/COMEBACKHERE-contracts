use crate::invoice::{DataKey, InvoiceError, MaybeBytes, MAX_HASH_BYTES, USDC_FACTOR};
use soroban_sdk::{Address, Env};

/// Maximum allowed expiry duration: 5 years in seconds.
pub const MAX_EXPIRY_SECONDS: u64 = 5 * 365 * 24 * 60 * 60;

/// Maximum byte length for the optional invoice memo field.
pub const MAX_MEMO_BYTES: u32 = 256;

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
    // An uninitialized contract has no admin, so it fails closed as Unauthorized.
    let stored: Option<Address> = env.storage().instance().get(&DataKey::Admin);
    if stored.as_ref() != Some(admin) {
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

/// Reject optional invoice hash fields that exceed the storage/cost cap.
pub fn require_hash_not_too_long(hash: &MaybeBytes) -> Result<(), InvoiceError> {
    if let MaybeBytes::Some(bytes) = hash {
        if bytes.len() > MAX_HASH_BYTES {
            return Err(InvoiceError::HashTooLong);
        }
    }
    Ok(())
}

/// Reject a payer that is the same address as the invoice's merchant.
/// A merchant paying its own invoice can be used to inflate volume metrics
/// or test-fill limits (#717).
pub fn require_payer_not_merchant(payer: &Address, merchant: &Address) -> Result<(), InvoiceError> {
    if payer == merchant {
        return Err(InvoiceError::SelfPayNotAllowed);
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

/// Validate a discount against the invoice amount and expiry.
///
/// Two invariants must hold for a discount to be economically sound:
/// - `discount_amount` must be strictly less than `amount_usdc` so that the
///   discounted price stays positive.
/// - `discount_deadline` must be ≤ `expires_at` so there exists a time window
///   in which the payer can claim the discount before the invoice expires.
pub fn require_valid_discount(
    discount_amount: i128,
    amount_usdc: i128,
    discount_deadline: u64,
    expires_at: u64,
) -> Result<(), InvoiceError> {
    if discount_amount >= amount_usdc {
        return Err(InvoiceError::DiscountExceedsAmount);
    }
    if discount_deadline > expires_at {
        return Err(InvoiceError::DiscountDeadlineAfterExpiry);
    }
    Ok(())
}

/// Reject a memo that exceeds MAX_MEMO_BYTES bytes.
///
/// The limit is enforced in **bytes**, not characters: a 256-byte string
/// containing 128 two-byte UTF-8 code points is still 256 bytes and still
/// passes, but a string of 128 two-byte characters whose byte length exceeds
/// MAX_MEMO_BYTES will be rejected.
pub fn require_memo_not_too_long(memo: &MaybeBytes) -> Result<(), InvoiceError> {
    if let MaybeBytes::Some(bytes) = memo {
        if bytes.len() > MAX_MEMO_BYTES {
            return Err(InvoiceError::MemoTooLong);
        }
    }
    Ok(())
}
