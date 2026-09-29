use compliance::{ComplianceContract, ComplianceContractClient, ContractError, MAX_OPERATORS};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger as _},
    Address, Env, FromVal, Symbol, Vec,
};

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

// ── Legacy single-operator tests (set_operator / get_operator) ────────────────

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
fn operator_can_place_a_day_to_day_block() {
    let (env, admin, subject, client) = setup();
    let operator = Address::generate(&env);

    // Allow subject first
    client.allow_address(&admin, &subject);

    // Set operator
    client.set_operator(&admin, &operator);

    // The operator places a day-to-day block (#604) and is recorded as its placer.
    client.block_address(&operator, &subject, &None);
    assert!(!client.is_allowed(&subject));
    assert_eq!(client.get_block_placer(&subject), Some(operator));
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
fn operator_cannot_clear_an_admin_placed_block() {
    let (env, admin, subject, client) = setup();
    let operator = Address::generate(&env);

    // Allow subject first
    client.allow_address(&admin, &subject);

    // Set operator
    client.set_operator(&admin, &operator);

    // The admin places the block (e.g. a sanctions block).
    client.block_address(&admin, &subject, &None);
    assert_eq!(client.get_block_placer(&subject), Some(admin.clone()));

    // The operator must not be able to reverse it (#604).
    let result = client.try_clear_address(&operator, &subject);
    assert_eq!(
        result,
        Err(Ok(ContractError::OperatorCannotClearAdminBlock))
    );
    assert!(client.is_blocked(&subject));
    assert!(!client.is_allowed(&subject));
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

    // Operator may not grant access (allow), and may only reverse its own blocks:
    // neither subject has an operator-placed block, so the clear is refused.
    assert!(client.try_allow_address(&operator, &subject2).is_err());
    assert!(client.try_clear_address(&operator, &subject1).is_err());

    // Admin can still perform all operations
    client.block_address(&admin, &subject1, &None);
    assert!(!client.is_allowed(&subject1));

    // Still refused for the operator: the block is the admin's, not the operator's.
    assert_eq!(
        client.try_clear_address(&operator, &subject1),
        Err(Ok(ContractError::OperatorCannotClearAdminBlock))
    );

    client.clear_address(&admin, &subject1);
    assert!(client.is_allowed(&subject1));
}

// ── Multi-operator tests (add_operator / remove_operator / get_operators) ─────

#[test]
fn add_operator_allows_address_status_calls() {
    let (env, admin, subject, client) = setup();
    let op1 = Address::generate(&env);
    let op2 = Address::generate(&env);

    client.allow_address(&admin, &subject);
    client.add_operator(&admin, &op1);
    client.add_operator(&admin, &op2);

    // Both operators can call address_status
    assert!(client.try_address_status(&op1, &subject).is_ok());
    assert!(client.try_address_status(&op2, &subject).is_ok());
}

#[test]
fn add_operator_is_idempotent() {
    let (env, admin, _, client) = setup();
    let op = Address::generate(&env);

    client.add_operator(&admin, &op);
    // Adding the same operator again must not error and must not duplicate the entry.
    client.add_operator(&admin, &op);

    let ops = client.get_operators();
    assert_eq!(ops.len(), 1);
}

#[test]
fn remove_operator_revokes_address_status_access() {
    let (env, admin, subject, client) = setup();
    let op = Address::generate(&env);

    client.allow_address(&admin, &subject);
    client.add_operator(&admin, &op);

    // Operator can call address_status before removal.
    assert!(client.try_address_status(&op, &subject).is_ok());

    client.remove_operator(&admin, &op);

    // After removal, operator is rejected.
    let result = client.try_address_status(&op, &subject);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn remove_operator_is_idempotent() {
    let (env, admin, _, client) = setup();
    let op = Address::generate(&env);

    client.add_operator(&admin, &op);
    client.remove_operator(&admin, &op);
    // Removing an address that is no longer in the set must not error.
    client.remove_operator(&admin, &op);

    let ops = client.get_operators();
    assert_eq!(ops.len(), 0);
}

#[test]
fn get_operators_returns_all_added_operators() {
    let (env, admin, _, client) = setup();
    let op1 = Address::generate(&env);
    let op2 = Address::generate(&env);
    let op3 = Address::generate(&env);

    client.add_operator(&admin, &op1);
    client.add_operator(&admin, &op2);
    client.add_operator(&admin, &op3);

    let ops = client.get_operators();
    assert_eq!(ops.len(), 3);
}

#[test]
fn get_operators_returns_empty_when_none_added() {
    let (_env, _admin, _, client) = setup();
    let ops = client.get_operators();
    assert_eq!(ops.len(), 0);
}

#[test]
fn operators_cannot_allow_or_block_addresses() {
    let (env, admin, subject, client) = setup();
    let op = Address::generate(&env);

    client.add_operator(&admin, &op);

    // Operators have read-only privilege — cannot mutate allow/block state.
    assert_eq!(
        client.try_allow_address(&op, &subject),
        Err(Ok(ContractError::Unauthorized))
    );
    assert_eq!(
        client.try_block_address(&op, &subject, &None),
        Err(Ok(ContractError::Unauthorized))
    );
    assert_eq!(
        client.try_clear_address(&op, &subject),
        Err(Ok(ContractError::Unauthorized))
    );
}

#[test]
fn multiple_operators_each_with_independent_access() {
    let (env, admin, subject, client) = setup();
    let op1 = Address::generate(&env);
    let op2 = Address::generate(&env);
    let op3 = Address::generate(&env);

    client.allow_address(&admin, &subject);
    client.add_operator(&admin, &op1);
    client.add_operator(&admin, &op2);
    client.add_operator(&admin, &op3);

    // All three operators can read independently.
    assert!(client.try_address_status(&op1, &subject).is_ok());
    assert!(client.try_address_status(&op2, &subject).is_ok());
    assert!(client.try_address_status(&op3, &subject).is_ok());

    // Remove op2 — op1 and op3 should still have access.
    client.remove_operator(&admin, &op2);

    assert!(client.try_address_status(&op1, &subject).is_ok());
    assert_eq!(
        client.try_address_status(&op2, &subject),
        Err(Ok(ContractError::Unauthorized))
    );
    assert!(client.try_address_status(&op3, &subject).is_ok());
}

#[test]
fn add_operator_rejected_when_set_is_full() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    // Fill the operator set to the maximum.
    for _ in 0..MAX_OPERATORS {
        let op = Address::generate(&env);
        client.add_operator(&admin, &op);
    }

    // One more should fail.
    let extra = Address::generate(&env);
    let result = client.try_add_operator(&admin, &extra);
    assert_eq!(result, Err(Ok(ContractError::OperatorSetFull)));
}

#[test]
fn only_admin_can_add_operator() {
    let (env, admin, _, client) = setup();
    let op = Address::generate(&env);
    let non_admin = Address::generate(&env);

    let result = client.try_add_operator(&non_admin, &op);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));

    // Admin can add.
    client.add_operator(&admin, &op);
    let ops = client.get_operators();
    assert_eq!(ops.len(), 1);
}

#[test]
fn only_admin_can_remove_operator() {
    let (env, admin, _, client) = setup();
    let op = Address::generate(&env);
    let non_admin = Address::generate(&env);

    client.add_operator(&admin, &op);

    let result = client.try_remove_operator(&non_admin, &op);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));

    // Operator set should be unchanged.
    assert_eq!(client.get_operators().len(), 1);
}

#[test]
fn admin_and_operator_can_coexist_with_legacy_set_operator() {
    let (env, admin, subject, client) = setup();
    let legacy_op = Address::generate(&env);
    let new_op = Address::generate(&env);

    client.allow_address(&admin, &subject);

    // Set the legacy single operator.
    client.set_operator(&admin, &legacy_op);
    // Add a new operator via the multi-operator API.
    client.add_operator(&admin, &new_op);

    // Both should be able to call address_status.
    assert!(client.try_address_status(&legacy_op, &subject).is_ok());
    assert!(client.try_address_status(&new_op, &subject).is_ok());
}

#[test]
fn operator_set_preserved_across_remove_and_re_add() {
    let (env, admin, subject, client) = setup();
    let op = Address::generate(&env);

    client.allow_address(&admin, &subject);
    client.add_operator(&admin, &op);

    // Remove then re-add.
    client.remove_operator(&admin, &op);
    assert_eq!(
        client.try_address_status(&op, &subject),
        Err(Ok(ContractError::Unauthorized))
    );

    client.add_operator(&admin, &op);
    assert!(client.try_address_status(&op, &subject).is_ok());
}

#[test]
fn add_operator_emits_operator_added_event() {
    let (env, admin, _, client) = setup();
    let op = Address::generate(&env);

    client.add_operator(&admin, &op);

    let events = env.events().all();
    let (_, topics, _) = events.last().unwrap();
    let sym = Symbol::from_val(&env, &topics.get_unchecked(0));
    assert_eq!(sym, Symbol::new(&env, "operator_added"));
}

#[test]
fn remove_operator_emits_operator_removed_event() {
    let (env, admin, _, client) = setup();
    let op = Address::generate(&env);

    client.add_operator(&admin, &op);
    client.remove_operator(&admin, &op);

    let events = env.events().all();
    let (_, topics, _) = events.last().unwrap();
    let sym = Symbol::from_val(&env, &topics.get_unchecked(0));
    assert_eq!(sym, Symbol::new(&env, "operator_removed"));
}

#[test]
fn add_operator_permitted_while_paused() {
    let (env, admin, _, client) = setup();
    let op = Address::generate(&env);

    client.pause(&admin);

    // Role management must work while paused.
    assert!(client.try_add_operator(&admin, &op).is_ok());
}

#[test]
fn remove_operator_permitted_while_paused() {
    let (env, admin, _, client) = setup();
    let op = Address::generate(&env);

    client.add_operator(&admin, &op);
    client.pause(&admin);

    // Role management must work while paused.
    assert!(client.try_remove_operator(&admin, &op).is_ok());

    let ops = client.get_operators();
    assert_eq!(ops.len(), 0);
}

#[test]
fn multiple_operators_all_rejected_from_admin_operations() {
    let (env, admin, subject, client) = setup();
    let mut operators: Vec<Address> = Vec::new(&env);
    for _ in 0..3 {
        let op = Address::generate(&env);
        client.add_operator(&admin, &op);
        operators.push_back(op);
    }

    // None of the operators should be able to perform admin mutations.
    for op in operators.iter() {
        assert_eq!(
            client.try_allow_address(&op, &subject),
            Err(Ok(ContractError::Unauthorized)),
            "operator should not be able to call allow_address"
        );
        assert_eq!(
            client.try_block_address(&op, &subject, &None),
            Err(Ok(ContractError::Unauthorized)),
            "operator should not be able to call block_address"
        );
    }
}
