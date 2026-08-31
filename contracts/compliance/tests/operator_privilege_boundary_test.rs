use compliance::{ComplianceContract, ComplianceContractClient, ContractError};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let subject = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, subject, client)
}

#[test]
fn operator_can_call_address_status() {
    let (env, admin, subject, client) = setup();
    let operator = Address::generate(&env);

    // Allow a test subject
    client.allow_address(&admin, &subject);

    // Set operator
    client.set_operator(&admin, &operator);

    // Operator should be able to call address_status
    let result = client.try_address_status(&operator, &subject);
    assert!(result.is_ok());
}

#[test]
fn operator_rejected_from_allow_address() {
    let (env, admin, subject, client) = setup();
    let operator = Address::generate(&env);

    // Set operator
    client.set_operator(&admin, &operator);

    // Operator should NOT be able to call allow_address
    let result = client.try_allow_address(&operator, &subject);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn operator_rejected_from_block_address() {
    let (env, admin, subject, client) = setup();
    let operator = Address::generate(&env);

    // Allow subject first
    client.allow_address(&admin, &subject);

    // Set operator
    client.set_operator(&admin, &operator);

    // Operator should NOT be able to call block_address
    let result = client.try_block_address(&operator, &subject, &None);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn operator_rejected_from_clear_address() {
    let (env, admin, subject, client) = setup();
    let operator = Address::generate(&env);

    // Allow subject first
    client.allow_address(&admin, &subject);

    // Set operator
    client.set_operator(&admin, &operator);

    // Operator should NOT be able to call clear_address
    let result = client.try_clear_address(&operator, &subject);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn admin_can_call_all_operations() {
    let (env, admin, subject, client) = setup();
    let operator = Address::generate(&env);

    // Set operator
    client.set_operator(&admin, &operator);

    // Admin should still be able to call all operations
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));

    let result = client.try_address_status(&admin, &subject);
    assert!(result.is_ok());

    client.block_address(&admin, &subject, &None);
    assert!(!client.is_allowed(&subject));

    client.clear_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

#[test]
fn operator_privilege_correctly_distinguished_in_multiple_operations() {
    let (env, admin, subject1, client) = setup();
    let subject2 = Address::generate(&env);
    let operator = Address::generate(&env);

    // Setup initial state
    client.allow_address(&admin, &subject1);
    client.allow_address(&admin, &subject2);

    // Set operator
    client.set_operator(&admin, &operator);

    // Operator can read address_status
    let result1 = client.try_address_status(&operator, &subject1);
    assert!(result1.is_ok());

    let result2 = client.try_address_status(&operator, &subject2);
    assert!(result2.is_ok());

    // Operator cannot perform admin operations (block, allow, clear)
    assert!(client
        .try_block_address(&operator, &subject1, &None)
        .is_err());
    assert!(client.try_allow_address(&operator, &subject2).is_err());
    assert!(client.try_clear_address(&operator, &subject1).is_err());

    // Admin can still perform all operations
    client.block_address(&admin, &subject1, &None);
    assert!(!client.is_allowed(&subject1));

    client.clear_address(&admin, &subject1);
    assert!(client.is_allowed(&subject1));
}
