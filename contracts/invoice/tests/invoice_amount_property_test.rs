// Fuzz-style property tests for require_positive_amount and require_usdc_precision.
// Covers zero, negative, sub-USDC-factor, and above-7-decimal-precision inputs
// systematically via parametric tables.
use invoice::{InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// USDC_FACTOR as known from invoice::invoice: 1 USDC = 10_000_000 stroops.
const USDC_FACTOR: i128 = 10_000_000;

fn client() -> (Env, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let c = InvoiceContractClient::new(&env, &id);
    c.initialize(&admin);
    (env, c)
}

// ---------------------------------------------------------------------------
// require_positive_amount: zero and negative amounts must be rejected
// ---------------------------------------------------------------------------

#[test]
fn prop_zero_and_negative_amounts_rejected() {
    // Every (amount, gross) pair here has amount <= 0 and must fail.
    let cases: &[(i128, i128)] = &[
        (0, 0),
        (0, USDC_FACTOR),
        (0, i128::MAX),
        (-1, USDC_FACTOR),
        (-1, -1),
        (-USDC_FACTOR, USDC_FACTOR),
        (i128::MIN, i128::MAX),
        (i128::MIN, i128::MIN),
        // Large negative range sweep
        (-1_000_000_000, -1_000_000_000),
        (-1_000_000_000, 0),
        (-1_000_000_000, USDC_FACTOR),
    ];

    for &(amount, gross) in cases {
        let (env, c) = client();
        let merchant = Address::generate(&env);
        assert!(
            c.try_create_invoice(
                &merchant,
                &amount,
                &gross,
                &3600,
                &MaybeBytes::None,
                &MaybeBytes::None,
                &0,
                &MaybeAddress::None
            )
            .is_err(),
            "expected rejection for zero/negative amount={amount} gross={gross}"
        );
    }
}

// ---------------------------------------------------------------------------
// require_positive_amount: gross < amount must be rejected
// ---------------------------------------------------------------------------

#[test]
fn prop_gross_less_than_amount_rejected() {
    let cases: &[(i128, i128)] = &[
        (USDC_FACTOR, USDC_FACTOR - 1),
        (USDC_FACTOR + 1, USDC_FACTOR),
        (2 * USDC_FACTOR, 2 * USDC_FACTOR - 1),
        (i128::MAX, i128::MAX - 1),
        (1_000 * USDC_FACTOR, 999 * USDC_FACTOR),
    ];

    for &(amount, gross) in cases {
        let (env, c) = client();
        let merchant = Address::generate(&env);
        assert!(
            c.try_create_invoice(
                &merchant,
                &amount,
                &gross,
                &3600,
                &MaybeBytes::None,
                &MaybeBytes::None,
                &0,
                &MaybeAddress::None
            )
            .is_err(),
            "expected rejection when gross < amount: amount={amount} gross={gross}"
        );
    }
}

// ---------------------------------------------------------------------------
// require_usdc_precision: amounts below USDC_FACTOR must be rejected
// (sub-7-decimal-precision / passing dollar-cent values instead of stroops)
// ---------------------------------------------------------------------------

#[test]
fn prop_below_usdc_factor_rejected() {
    // Any amount or gross below USDC_FACTOR violates precision and must fail.
    // The table sweeps powers-of-ten and off-by-one boundaries.
    let sub_factor_values: &[i128] = &[
        1,
        2,
        9,
        10,
        99,
        100,
        999,
        1_000,
        9_999,
        10_000,
        99_999,
        100_000,
        999_999,
        1_000_000,
        USDC_FACTOR - 1, // 9_999_999 — one stroop below minimum
    ];

    for &v in sub_factor_values {
        // amount below factor, gross at factor — should fail on amount precision
        let (env, c) = client();
        let merchant = Address::generate(&env);
        assert!(
            c.try_create_invoice(
                &merchant,
                &v,
                &USDC_FACTOR,
                &3600,
                &MaybeBytes::None,
                &MaybeBytes::None,
                &0,
                &MaybeAddress::None
            )
            .is_err(),
            "expected AmountPrecision rejection for amount={v} (below USDC_FACTOR)"
        );

        // gross below factor, amount at factor — should fail on gross precision
        let (env, c) = client();
        let merchant = Address::generate(&env);
        assert!(
            c.try_create_invoice(
                &merchant,
                &USDC_FACTOR,
                &v,
                &3600,
                &MaybeBytes::None,
                &MaybeBytes::None,
                &0,
                &MaybeAddress::None
            )
            .is_err(),
            "expected AmountPrecision rejection for gross={v} (below USDC_FACTOR)"
        );
    }
}

// ---------------------------------------------------------------------------
// Combined: values that satisfy both validators must be accepted
// ---------------------------------------------------------------------------

#[test]
fn prop_valid_amounts_accepted() {
    let cases: &[(i128, i128)] = &[
        (USDC_FACTOR, USDC_FACTOR),             // exact minimum
        (USDC_FACTOR, USDC_FACTOR + 1),         // gross one stroop above minimum
        (USDC_FACTOR, 2 * USDC_FACTOR),         // gross = 2 USDC
        (10 * USDC_FACTOR, 10 * USDC_FACTOR),   // 10 USDC
        (100 * USDC_FACTOR, 100 * USDC_FACTOR), // 100 USDC
        (i128::MAX / 2, i128::MAX / 2),
        (i128::MAX, i128::MAX),
    ];

    for &(amount, gross) in cases {
        let (env, c) = client();
        let merchant = Address::generate(&env);
        assert!(
            c.try_create_invoice(
                &merchant,
                &amount,
                &gross,
                &3600,
                &MaybeBytes::None,
                &MaybeBytes::None,
                &0,
                &MaybeAddress::None
            )
            .is_ok(),
            "expected acceptance for amount={amount} gross={gross}"
        );
    }
}
