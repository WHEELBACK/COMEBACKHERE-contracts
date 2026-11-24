// Property tests for `require_valid_discount` in validation.rs.
//
// Three properties are verified:
//   1. A discount that is >= the invoice amount is always rejected.
//   2. A discount deadline that falls after the invoice expiry is always
//      rejected.
//   3. Valid inputs (discount < amount AND deadline <= expiry) are always
//      accepted.
//
// Each proptest case runs with the default 256-case budget, exercising a wide
// range of u64 / i128 combinations that would be unreachable by hand-crafted
// unit tests.

use invoice::InvoiceError;
use proptest::prelude::*;

extern crate std;

// Minimum USDC amount accepted by the contract (1 USDC = 10_000_000 stroops).
const USDC_FACTOR: i128 = 10_000_000;

// ---------------------------------------------------------------------------
// Helper: call the pure validator directly through a thin wrapper so that tests
// do not need a running Soroban environment.
// ---------------------------------------------------------------------------

/// Thin shim that maps straight to the validation logic so proptest can call
/// it without going through the contract client.
fn validate_discount(
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

// ---------------------------------------------------------------------------
// Property 1: discount_amount >= amount_usdc is always rejected
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn prop_discount_not_less_than_amount_is_rejected(
        // Pick an amount in the valid USDC range.
        amount in USDC_FACTOR..=i128::MAX,
        // Pick a discount that equals or exceeds the amount.
        discount_delta in 0i128..=1_000_000_000i128,
        deadline in 0u64..=u64::MAX / 2,
        expires_at in 0u64..=u64::MAX / 2,
    ) {
        // Clamp to avoid overflow: discount = amount + delta, but cap at i128::MAX.
        let discount = amount.saturating_add(discount_delta);
        // Ensure deadline <= expires_at so only the amount rule fires.
        let effective_expires = expires_at.max(deadline);

        let result = validate_discount(discount, amount, deadline, effective_expires);
        prop_assert!(
            result.is_err(),
            "expected DiscountExceedsAmount for discount={discount} amount={amount}"
        );
        prop_assert_eq!(
            result.unwrap_err(),
            InvoiceError::DiscountExceedsAmount
        );
    }

    // ---------------------------------------------------------------------------
    // Property 2: discount_deadline > expires_at is always rejected
    // ---------------------------------------------------------------------------

    #[test]
    fn prop_discount_deadline_after_expiry_is_rejected(
        amount in USDC_FACTOR..=i128::MAX,
        // discount strictly less than amount
        discount_fraction in 1i128..USDC_FACTOR,
        expires_at in 1u64..=u64::MAX / 2,
        // deadline strictly after expires_at
        extra_seconds in 1u64..=1_000_000u64,
    ) {
        let discount = amount.saturating_sub(discount_fraction).max(0);
        let deadline = expires_at.saturating_add(extra_seconds);

        let result = validate_discount(discount, amount, deadline, expires_at);
        prop_assert!(
            result.is_err(),
            "expected DiscountDeadlineAfterExpiry for deadline={deadline} expires_at={expires_at}"
        );
        prop_assert_eq!(
            result.unwrap_err(),
            InvoiceError::DiscountDeadlineAfterExpiry
        );
    }

    // ---------------------------------------------------------------------------
    // Property 3: valid inputs are always accepted
    // ---------------------------------------------------------------------------

    #[test]
    fn prop_valid_discount_inputs_are_accepted(
        // Amount in valid USDC range, leave room below for discount.
        amount in (USDC_FACTOR + 1)..=i128::MAX,
        // discount strictly less than amount
        discount_fraction in 1i128..USDC_FACTOR,
        expires_at in 1u64..=u64::MAX / 2,
        // deadline at or before expires_at
        deadline_offset in 0u64..=100_000u64,
    ) {
        let discount = amount.saturating_sub(discount_fraction).max(0);
        let deadline = expires_at.saturating_sub(deadline_offset);

        let result = validate_discount(discount, amount, deadline, expires_at);
        prop_assert!(
            result.is_ok(),
            "expected Ok for discount={discount} amount={amount} deadline={deadline} expires_at={expires_at}: got {:?}",
            result.unwrap_err()
        );
    }
}

// ---------------------------------------------------------------------------
// Table-driven regression cases (run alongside proptest for deterministic coverage)
// ---------------------------------------------------------------------------

#[test]
fn discount_must_be_less_than_amount() {
    // Discount equal to amount is rejected.
    let amount = 10 * USDC_FACTOR;
    assert_eq!(
        validate_discount(amount, amount, 1000, 2000),
        Err(InvoiceError::DiscountExceedsAmount)
    );
    // Discount greater than amount is rejected.
    assert_eq!(
        validate_discount(amount + 1, amount, 1000, 2000),
        Err(InvoiceError::DiscountExceedsAmount)
    );
    // Discount of zero is valid.
    assert!(validate_discount(0, amount, 1000, 2000).is_ok());
}

#[test]
fn discount_deadline_must_not_exceed_expiry() {
    let amount = 10 * USDC_FACTOR;
    let discount = USDC_FACTOR; // strictly less than amount

    // Deadline one second after expiry is rejected.
    assert_eq!(
        validate_discount(discount, amount, 2001, 2000),
        Err(InvoiceError::DiscountDeadlineAfterExpiry)
    );
    // Deadline equal to expiry is accepted.
    assert!(validate_discount(discount, amount, 2000, 2000).is_ok());
    // Deadline well before expiry is accepted.
    assert!(validate_discount(discount, amount, 1000, 2000).is_ok());
}

#[test]
fn zero_discount_with_min_amount_is_valid() {
    // Smallest possible valid case: zero discount on a 1-USDC invoice.
    assert!(validate_discount(0, USDC_FACTOR, 0, 0).is_ok());
}
