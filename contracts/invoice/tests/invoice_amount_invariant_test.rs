// Property-style tests verifying invoice amount invariants across representative
// value ranges. Uses iterative parametric coverage in lieu of a dedicated
// property-testing harness since the workspace only ships derive_arbitrary.
use invoice::{InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeAddress, MaybeBytes};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

// Invariant: stored gross_usdc is always >= stored amount_usdc for valid inputs.
#[test]
fn prop_gross_always_gte_amount() {
    let cases: &[(i128, i128)] = &[
        (10_000_000, 10_000_000),
        (10_000_000, 20_000_000),
        (15_000_000, 15_000_000),
        (15_000_000, 15_010_000),
        (10_000_000, 10_000_000),
        (10_000_000, 10_250_000),
        (100_000_000, 100_000_000),
        (999_999_999, 1_000_000_000),
        (i128::MAX / 2, i128::MAX / 2),
        (i128::MAX / 2, i128::MAX),
        (i128::MAX, i128::MAX),
    ];
    for &(amount, gross) in cases {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let cid = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &cid);
        client.initialize(&admin);
        let merchant = Address::generate(&env);
        let id = client.create_invoice(
            &merchant,
            &amount,
            &gross,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
            &MaybeAddress::None,
        );
        let inv = client.get_invoice(&id);
        assert!(
            inv.gross_usdc >= inv.amount_usdc,
            "gross < amount stored: amount={amount} gross={gross}"
        );
    }
}

// Invariant: amount fields are never mutated by mark_paid.
#[test]
fn prop_paid_does_not_mutate_amounts() {
    let cases: &[(i128, i128)] = &[
        (10_000_000, 10_000_000),
        (10_000_000, 10_250_000),
        (10_000_000, 11_000_000),
        (i128::MAX / 4, i128::MAX / 4),
        (i128::MAX, i128::MAX),
    ];
    for &(amount, gross) in cases {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let cid = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &cid);
        client.initialize(&admin);
        let merchant = Address::generate(&env);
        let payer = Address::generate(&env);
        let id = client.create_invoice(
            &merchant,
            &amount,
            &gross,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
            &MaybeAddress::None,
        );
        client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
        let inv = client.get_invoice(&id);
        assert_eq!(
            inv.amount_usdc, amount,
            "amount_usdc mutated after mark_paid"
        );
        assert_eq!(inv.gross_usdc, gross, "gross_usdc mutated after mark_paid");
        assert_eq!(inv.status, InvoiceStatus::Paid);
    }
}

// Invariant: invoice IDs are always sequential starting at 1 (no gaps, no skips).
#[test]
fn prop_invoice_ids_are_sequential() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    for expected_id in 1u64..=20 {
        let id = client.create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
            &MaybeAddress::None,
        );
        assert_eq!(
            id, expected_id,
            "non-sequential id at position {expected_id}"
        );
    }
}

// Invariant: amount and gross stored exactly as provided — no rounding or truncation.
#[test]
fn prop_amounts_stored_exactly() {
    let cases: &[(i128, i128)] = &[
        (10_000_000, 10_000_000),
        (10_000_007, 10_000_013),
        (123_456_789, 987_654_321),
        (i128::MAX / 3, i128::MAX / 2),
        (i128::MAX, i128::MAX),
    ];
    for &(amount, gross) in cases {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let cid = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &cid);
        client.initialize(&admin);
        let merchant = Address::generate(&env);
        let id = client.create_invoice(
            &merchant,
            &amount,
            &gross,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
            &MaybeAddress::None,
        );
        let inv = client.get_invoice(&id);
        assert_eq!(inv.amount_usdc, amount);
        assert_eq!(inv.gross_usdc, gross);
    }
}

// Invariant: every valid (amount, gross) accepted by the validator satisfies gross >= amount > 0.
#[test]
fn prop_validator_accepts_iff_positive_and_gross_gte_amount() {
    let accept_cases: &[(i128, i128)] = &[
        (10_000_000, 10_000_000),
        (10_000_000, i128::MAX),
        (50_000_000, 50_000_000),
        (10_000_000, 10_000_001),
    ];
    let reject_cases: &[(i128, i128)] = &[
        (0, 0),
        (-1, 1),
        (1, 0),
        (1_000_000, 999_999),
        (i128::MIN, i128::MAX),
    ];

    for &(amount, gross) in accept_cases {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let cid = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &cid);
        client.initialize(&admin);
        let merchant = Address::generate(&env);
        assert!(
            client
                .try_create_invoice(
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
            "expected accept for amount={amount} gross={gross}"
        );
    }
    for &(amount, gross) in reject_cases {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let cid = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &cid);
        client.initialize(&admin);
        let merchant = Address::generate(&env);
        assert!(
            client
                .try_create_invoice(
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
            "expected reject for amount={amount} gross={gross}"
        );
    }
}
