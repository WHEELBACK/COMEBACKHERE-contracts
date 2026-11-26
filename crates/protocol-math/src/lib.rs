#![no_std]

//! Shared protocol math primitives.
//!
//! One definition of every protocol-wide numeric constant, so contracts can
//! never drift apart on what a ratio means. Today this crate owns the
//! basis-point denominator used by fee and split ratios (#707); settlement
//! amounts, fee schedules and dispute splits all read the same value instead
//! of each re-declaring their own copy of `10_000`.
//!
//! Like `crates/multisig`, this crate is compiled into the contracts that use it
//! but is never deployed on its own: it declares no `#[contract]`, no `DataKey`
//! and no storage, so it contributes no on-chain state of its own.

/// Basis-points denominator: 10 000 bps = 100.00%.
///
/// The single definition of "one whole ratio" for the protocol. Percentages are
/// expressed as `bps / BPS_DENOMINATOR`, so `5_000` is 50% and `10_000` is the
/// whole amount. Callers must reject any ratio above this value (see
/// [`is_valid_bps`]) - a larger ratio has no meaning and would make a split
/// exceed the amount being split.
pub const BPS_DENOMINATOR: u32 = 10_000;

/// [`BPS_DENOMINATOR`] widened to `i128`.
///
/// Split math runs on `i128` token amounts, so converting the denominator once
/// here keeps every call site free of an `as` cast that could silently change
/// width (`BPS_DENOMINATOR as i128` in one crate, a literal `10_000i128` in
/// another). Widening is lossless - `u32` always fits in `i128`.
pub const fn bps_denominator_i128() -> i128 {
    BPS_DENOMINATOR as i128
}

/// Returns `true` when `bps` is a usable ratio, i.e. `0..=BPS_DENOMINATOR`.
///
/// [`apply_bps_i128`] intentionally does *not* range-check `bps` so callers can
/// pick their own error for an out-of-range ratio (each contract has its own
/// error enum). Anything relying on it should gate on this predicate first.
pub const fn is_valid_bps(bps: u32) -> bool {
    bps <= BPS_DENOMINATOR
}

/// `bps` basis-point share of `amount`, in the same token units.
///
/// Returns `None` if the intermediate `amount * bps` multiplication overflows
/// `i128`, so a caller can map that onto its own overflow error rather than
/// relying on a release-mode wrap (`overflow-checks` is on for this workspace,
/// but contracts are still written to detect it explicitly).
///
/// The division truncates toward zero, matching the arithmetic every split did
/// before this helper existed, so adopting it changes no on-chain result.
pub fn apply_bps_i128(amount: i128, bps: u32) -> Option<i128> {
    amount
        .checked_mul(i128::from(bps))?
        .checked_div(bps_denominator_i128())
}

/// Splits `amount` into `(first, remainder)` where `first` is the `bps`
/// basis-point share and `remainder` is everything else.
///
/// The remainder is derived by subtraction rather than by a second division so
/// the two halves always sum back to exactly `amount` - no dust is created or
/// lost when `amount` is not divisible by the denominator. Returns `None` on
/// `i128` overflow, same as [`apply_bps_i128`].
pub fn split_bps_i128(amount: i128, bps: u32) -> Option<(i128, i128)> {
    let first = apply_bps_i128(amount, bps)?;
    Some((first, amount.checked_sub(first)?))
}

/// Checked `a - b` on `i128` token amounts.
///
/// Returns `None` when the subtraction would underflow or overflow `i128`
/// instead of wrapping, so settlement math can map that onto its own error
/// rather than relying on release-mode behaviour. This is the single audited
/// subtraction used by partial-settlement computations (see
/// [`split_bps_i128`], which derives its remainder the same way).
pub fn checked_sub_i128(a: i128, b: i128) -> Option<i128> {
    a.checked_sub(b)
}

/// Checked `a + b` on `i128` token amounts.
///
/// Returns `None` when the addition would overflow `i128` instead of wrapping.
/// Pairs with [`checked_sub_i128`] so settlement code never mixes a checked
/// subtraction with an unchecked addition (or vice versa).
pub fn checked_add_i128(a: i128, b: i128) -> Option<i128> {
    a.checked_add(b)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_bps_i128, bps_denominator_i128, checked_add_i128, checked_sub_i128, is_valid_bps,
        split_bps_i128, BPS_DENOMINATOR,
    };

    #[test]
    fn denominator_is_ten_thousand() {
        assert_eq!(BPS_DENOMINATOR, 10_000);
        assert_eq!(bps_denominator_i128(), 10_000);
    }

    #[test]
    fn valid_ratios_cover_the_whole_range() {
        assert!(is_valid_bps(0));
        assert!(is_valid_bps(BPS_DENOMINATOR));
        assert!(!is_valid_bps(BPS_DENOMINATOR + 1));
        assert!(!is_valid_bps(u32::MAX));
    }

    #[test]
    fn whole_ratio_returns_the_whole_amount() {
        assert_eq!(apply_bps_i128(1_234_567, BPS_DENOMINATOR), Some(1_234_567));
        assert_eq!(apply_bps_i128(1_234_567, 0), Some(0));
    }

    #[test]
    fn half_ratio_halves_the_amount() {
        assert_eq!(apply_bps_i128(1_000_000, 5_000), Some(500_000));
    }

    #[test]
    fn truncation_matches_the_previous_inline_arithmetic() {
        // 10_000_000 * 3_333 / 10_000 = 3_333_000 exactly; 33 bps of 7 is 0.0231,
        // truncating to 0 the same way `amount * bps / BPS_DENOMINATOR` did.
        assert_eq!(apply_bps_i128(7, 33), Some(0));
        assert_eq!(apply_bps_i128(10_000_000, 3_333), Some(3_333_000));
        assert_eq!(apply_bps_i128(-7, 33), Some(0));
    }

    #[test]
    fn overflow_is_reported_instead_of_wrapping() {
        assert_eq!(apply_bps_i128(i128::MAX, BPS_DENOMINATOR), None);
        assert_eq!(apply_bps_i128(i128::MAX, 2), None);
        assert_eq!(apply_bps_i128(i128::MIN, 2), None);
    }

    #[test]
    fn split_halves_reconstruct_the_whole_amount() {
        // 1 stroop split 50/50 must not lose the odd stroop.
        assert_eq!(split_bps_i128(1, 5_000), Some((0, 1)));
        assert_eq!(
            split_bps_i128(10_000_001, 3_333),
            Some((3_333_000, 6_667_001))
        );
        assert_eq!(
            split_bps_i128(-10_000_001, 3_333),
            Some((-3_333_000, -6_667_001))
        );
    }

    #[test]
    fn split_reports_overflow() {
        assert_eq!(split_bps_i128(i128::MAX, BPS_DENOMINATOR), None);
    }

    #[test]
    fn checked_sub_handles_ordinary_values() {
        assert_eq!(checked_sub_i128(1_000, 400), Some(600));
        assert_eq!(checked_sub_i128(1_000, 1_000), Some(0));
        assert_eq!(checked_sub_i128(0, 1_000), Some(-1_000));
        assert_eq!(checked_sub_i128(-1_000, -400), Some(-600));
    }

    #[test]
    fn checked_sub_reports_underflow_and_overflow() {
        // Underflow: subtracting a positive from i128::MIN.
        assert_eq!(checked_sub_i128(i128::MIN, 1), None);
        assert_eq!(checked_sub_i128(i128::MIN, i128::MAX), None);
        // Overflow: subtracting a negative from i128::MAX.
        assert_eq!(checked_sub_i128(i128::MAX, -1), None);
        assert_eq!(checked_sub_i128(i128::MAX, i128::MIN), None);
    }

    #[test]
    fn checked_sub_boundaries_are_exact() {
        assert_eq!(checked_sub_i128(i128::MAX, i128::MAX), Some(0));
        assert_eq!(checked_sub_i128(i128::MIN, i128::MIN), Some(0));
        assert_eq!(checked_sub_i128(i128::MAX, 0), Some(i128::MAX));
        assert_eq!(checked_sub_i128(i128::MIN, 0), Some(i128::MIN));
        assert_eq!(checked_sub_i128(0, i128::MIN), None);
        assert_eq!(checked_sub_i128(-1, i128::MAX), Some(i128::MIN));
        assert_eq!(checked_sub_i128(-2, i128::MAX), None);
    }

    #[test]
    fn checked_add_handles_ordinary_values() {
        assert_eq!(checked_add_i128(1_000, 400), Some(1_400));
        assert_eq!(checked_add_i128(1_000, -400), Some(600));
        assert_eq!(checked_add_i128(-1_000, -400), Some(-1_400));
        assert_eq!(checked_add_i128(0, 0), Some(0));
    }

    #[test]
    fn checked_add_reports_overflow_and_underflow() {
        assert_eq!(checked_add_i128(i128::MAX, 1), None);
        assert_eq!(checked_add_i128(i128::MAX, i128::MAX), None);
        assert_eq!(checked_add_i128(i128::MIN, -1), None);
        assert_eq!(checked_add_i128(i128::MIN, i128::MIN), None);
    }

    #[test]
    fn checked_add_boundaries_are_exact() {
        assert_eq!(checked_add_i128(i128::MAX, 0), Some(i128::MAX));
        assert_eq!(checked_add_i128(i128::MIN, 0), Some(i128::MIN));
        assert_eq!(checked_add_i128(i128::MAX, i128::MIN), Some(-1));
        assert_eq!(checked_add_i128(i128::MIN, i128::MAX), Some(-1));
        assert_eq!(checked_add_i128(i128::MAX, -i128::MAX), Some(0));
    }

    #[test]
    fn checked_helpers_round_trip() {
        // (a - b) + b == a whenever neither step overflows.
        let cases = [
            (0i128, 0i128),
            (1_234_567, 765_432),
            (-1_234_567, 765_432),
            (i128::MAX, i128::MAX),
            (i128::MIN, i128::MIN),
        ];
        for (a, b) in cases {
            let diff = checked_sub_i128(a, b).expect("difference fits");
            assert_eq!(checked_add_i128(diff, b), Some(a));
        }
    }
}
