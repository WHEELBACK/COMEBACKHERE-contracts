use invoice::{InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes};
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

#[test]
fn test_zero_amount_rejected() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    assert!(client.try_create_invoice(&merchant, &0, &0, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None).is_err());
}

#[test]
fn test_negative_amount_rejected() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    assert!(client
        .try_create_invoice(&merchant, &-1, &-1, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None)
        .is_err());
}

#[test]
fn test_large_negative_amount_rejected() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    assert!(client
        .try_create_invoice(&merchant, &i128::MIN, &i128::MIN, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None)
        .is_err());
}

#[test]
fn test_gross_less_than_amount_rejected() {
    // gross < amount violates the fee/precision invariant → InvalidAmount
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    assert!(client
        .try_create_invoice(&merchant, &10_000_000, &9_999_999, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None)
        .is_err());
}

#[test]
fn test_zero_gross_with_positive_amount_rejected() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    assert!(client
        .try_create_invoice(&merchant, &10_000_000, &0, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None)
        .is_err());
}

#[test]
fn test_negative_gross_rejected() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    assert!(client
        .try_create_invoice(&merchant, &10_000_000, &-1, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None)
        .is_err());
}

#[test]
fn test_amount_one_gross_zero_rejected() {
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    assert!(client.try_create_invoice(&merchant, &1, &0, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None).is_err());
}

#[test]
fn test_amount_matrix() {
    // (amount, gross, expect_valid)
    // All invalid cases must return InvalidAmount (error code 3);
    // valid cases must succeed.
    let cases: &[(i128, i128, bool)] = &[
        (0, 0, false),
        (-1, -1, false),
        (-1, 0, false),
        (1, 0, false),
        (1, -1, false),
        (i128::MIN, i128::MIN, false),
        (10_000_000, 9_999_999, false),
        (10_000_000, -1, false),
        // valid
        (20_000_000, 20_000_000, true),
        (10_000_000, 10_000_000, true),
        (10_000_000, 10_250_000, true),
        (i128::MAX, i128::MAX, true),
    ];

    for &(amount, gross, expect_valid) in cases {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, InvoiceContract);
        let client = InvoiceContractClient::new(&env, &contract_id);
        client.initialize(&admin);
        let merchant = Address::generate(&env);
        let result = client.try_create_invoice(&merchant, &amount, &gross, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None);
        if expect_valid {
            assert!(
                result.is_ok(),
                "expected ok for amount={amount} gross={gross}"
            );
        } else {
            assert!(
                result.is_err(),
                "expected InvalidAmount for amount={amount} gross={gross}"
            );
        }
    }
}

#[test]
fn test_overflow_amount_i128_max_accepted() {
    // i128::MAX is a valid positive amount when gross == amount
    let (env, _, client) = setup();
    let merchant = Address::generate(&env);
    let result = client.try_create_invoice(&merchant, &i128::MAX, &i128::MAX, &3600, &MaybeBytes::None, &MaybeBytes::None, &0, &MaybeAddress::None);
    assert!(result.is_ok());
}
