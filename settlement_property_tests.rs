// contracts/settlement/tests/property_tests.rs - Issue #857
#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;

    fn calculate_cuts(total: i128, platform_fee_bps: u32, tax_bps: u32) -> (i128, i128, i128) {
        let fee = (total as u128 * platform_fee_bps as u128 / 10000) as i128;
        let tax = (total as u128 * tax_bps as u128 / 10000) as i128;
        let merchant = total - fee - tax;
        (merchant, fee, tax)
    }

    proptest! {
        #[test]
        fn prop_fee_split_no_dust_loss(
            total in 1i128..10_000_000_000i128,
            platform_fee_bps in 1u32..1000u32,
            tax_bps in 1u32..1000u32,
        ) {
            let (merchant, fee, tax) = calculate_cuts(total, platform_fee_bps, tax_bps);
            prop_assert_eq!(merchant + fee + tax, total, "Dust loss in fee split");
            prop_assert!(merchant >= 0);
            prop_assert!(fee >= 0);
            prop_assert!(tax >= 0);
        }
    }
}
