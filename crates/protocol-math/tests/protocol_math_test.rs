//! Integration coverage for the shared protocol math crate.
//!
//! #707: these assertions are written against the crate's *public* surface
//! (the way `contracts/treasury` consumes it) rather than its internals, so a
//! helper that stops being `pub` or changes its signature fails here instead of
//! only inside a contract's build.

use protocol_math::{
    apply_bps_i128, bps_denominator_i128, checked_sub_i128, is_valid_bps, split_bps_i128,
    BPS_DENOMINATOR,
};

#[test]
fn denominator_and_its_i128_widening_agree() {
    assert_eq!(BPS_DENOMINATOR, 10_000);
    assert_eq!(
        bps_denominator_i128(),
        i128::from(BPS_DENOMINATOR),
        "widening the denominator must be lossless"
    );
}

#[test]
fn a_split_can_never_exceed_the_amount() {
    for bps in [0, 1, 2_500, 5_000, 9_999, BPS_DENOMINATOR] {
        let amount = 9_999_999_999i128;
        let (first, remainder) = split_bps_i128(amount, bps).expect("in range");
        assert!(is_valid_bps(bps));
        assert_eq!(
            first + remainder,
            amount,
            "halves must reconstruct {amount}"
        );
        assert!(first >= 0 && first <= amount);
    }
}

#[test]
fn dispute_split_ratios_behave_like_the_pre_refactor_inline_math() {
    // The three ratios the treasury dispute split path is exercised with, each
    // computed the way `amount * bps / 10_000` computed it before #707.
    let amount = 10_000_000i128;
    assert_eq!(apply_bps_i128(amount, 0), Some(0));
    assert_eq!(apply_bps_i128(amount, 5_000), Some(5_000_000));
    assert_eq!(apply_bps_i128(amount, BPS_DENOMINATOR), Some(10_000_000));
}

#[test]
fn out_of_range_ratios_are_rejected_before_use() {
    assert!(!is_valid_bps(BPS_DENOMINATOR + 1));
    // The helper itself stays total so a caller-chosen error can be raised
    // instead; only the predicate gates the value.
    assert_eq!(apply_bps_i128(10_000, BPS_DENOMINATOR + 1), Some(10_001));
}

#[test]
fn checked_sub_returns_none_instead_of_underflowing() {
    // #959: partial settlement must never wrap; an underflowing subtraction is
    // reported as `None` so the caller can raise its own typed error.
    assert_eq!(checked_sub_i128(5, 3), Some(2));
    assert_eq!(checked_sub_i128(0, 0), Some(0));
    assert_eq!(checked_sub_i128(3, 5), None);
    assert_eq!(checked_sub_i128(i128::MIN, 1), None);
    assert_eq!(checked_sub_i128(i128::MIN, i128::MAX), None);
}

#[test]
fn checked_sub_handles_the_i128_boundaries() {
    assert_eq!(checked_sub_i128(i128::MAX, i128::MAX), Some(0));
    assert_eq!(checked_sub_i128(i128::MAX, 0), Some(i128::MAX));
    assert_eq!(checked_sub_i128(i128::MIN, 0), Some(i128::MIN));
    assert_eq!(checked_sub_i128(i128::MAX, i128::MIN), None);
    // The only representable difference at the lower bound.
    assert_eq!(checked_sub_i128(i128::MIN, i128::MIN), Some(0));
}

#[test]
fn partial_settlement_remainder_is_checked_against_the_amount() {
    // Mirrors the storage-layout partial settlement path: split the amount,
    // then subtract the settled half from the total. The remainder must be
    // non-negative and reconstruct the original amount.
    for bps in [0, 1, 2_500, 5_000, 9_999, BPS_DENOMINATOR] {
        let amount = 9_999_999_999i128;
        let (settled, _) = split_bps_i128(amount, bps).expect("in range");
        let remainder = checked_sub_i128(amount, settled).expect("settled <= amount");
        assert!(remainder >= 0);
        assert_eq!(settled + remainder, amount);
    }
}
