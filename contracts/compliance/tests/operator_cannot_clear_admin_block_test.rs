//! Tests for #604: an operator must not be able to reverse a block placed by the
//! admin.
//!
//! Operators handle day-to-day compliance and place their own blocks; some blocks
//! are placed by the admin for serious reasons such as sanctions. Recording who
//! placed each block keeps that boundary explicit: an operator may undo its own
//! action, and only the admin may reverse an admin-placed block.
use compliance::{ComplianceContract, ComplianceContractClient, ContractError};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    client.set_operator(&admin, &operator);
    (env, admin, operator, client)
}

// ─── the boundary #604 asks for ───────────────────────────────────────────────

/// The admin's block survives an operator `clear_address`, and the call reports the
/// dedicated error rather than a generic `Unauthorized`, so a caller can tell the
/// two apart.
#[test]
fn operator_cannot_clear_admin_block() {
    let (_env, admin, operator, client) = setup();
    let subject = Address::generate(&_env);

    client.block_address(&admin, &subject, &None);

    assert_eq!(
        client.try_clear_address(&operator, &subject),
        Err(Ok(ContractError::OperatorCannotClearAdminBlock))
    );
    // Nothing changed: still blocked, still not allowed.
    assert!(client.is_blocked(&subject));
    assert!(!client.is_allowed(&subject));
    assert_eq!(client.get_block_placer(&subject), Some(admin));
}

/// Same for a time-bound admin block: the auto-expiry is the admin's decision to
/// make, not the operator's.
#[test]
fn operator_cannot_clear_admin_block_until() {
    let (env, admin, operator, client) = setup();
    let subject = Address::generate(&env);

    let unblock_at = env.ledger().timestamp() + 1_000;
    client.block_address_until(&admin, &subject, &unblock_at, &None);

    assert_eq!(
        client.try_clear_address(&operator, &subject),
        Err(Ok(ContractError::OperatorCannotClearAdminBlock))
    );
    assert!(client.is_blocked(&subject));
    assert_eq!(client.get_allow_expiry(&subject), None);
}

/// Blocks placed through the admin-only bulk entrypoint are admin-placed too, so
/// an operator cannot clear them either.
#[test]
fn operator_cannot_clear_bulk_admin_block() {
    let (env, admin, operator, client) = setup();
    let subject = Address::generate(&env);
    let addresses = soroban_sdk::vec![&env, subject.clone()];

    client.bulk_block_addresses(&admin, &addresses);
    assert_eq!(client.get_block_placer(&subject), Some(admin));

    assert_eq!(
        client.try_clear_address(&operator, &subject),
        Err(Ok(ContractError::OperatorCannotClearAdminBlock))
    );
    assert!(client.is_blocked(&subject));
}

/// The admin can still clear any block, including operator-placed ones — the change
/// only narrows operator privileges.
#[test]
fn admin_can_clear_operator_block() {
    let (env, admin, operator, client) = setup();
    let subject = Address::generate(&env);

    client.block_address(&operator, &subject, &None);
    assert_eq!(client.get_block_placer(&subject), Some(operator));

    client.clear_address(&admin, &subject);
    assert!(!client.is_blocked(&subject));
    assert!(client.is_allowed(&subject));
}

/// The admin can also reverse its own block, as before.
#[test]
fn admin_can_clear_admin_block() {
    let (env, admin, _operator, client) = setup();
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &None);
    client.clear_address(&admin, &subject);

    assert!(!client.is_blocked(&subject));
    assert!(client.is_allowed(&subject));
}

// ─── what the operator may still do ───────────────────────────────────────────

/// An operator can reverse the block it placed itself — day-to-day compliance
/// stays reversible for the operator that acted.
#[test]
fn operator_can_clear_its_own_block() {
    let (env, _admin, operator, client) = setup();
    let subject = Address::generate(&env);

    client.block_address(&operator, &subject, &None);
    assert!(!client.is_allowed(&subject));

    client.clear_address(&operator, &subject);
    assert!(!client.is_blocked(&subject));
    assert!(client.is_allowed(&subject));
}

/// Same for a time-bound block the operator placed.
#[test]
fn operator_can_clear_its_own_block_until() {
    let (env, _admin, operator, client) = setup();
    let subject = Address::generate(&env);

    let unblock_at = env.ledger().timestamp() + 1_000;
    client.block_address_until(&operator, &subject, &unblock_at, &None);
    client.clear_address(&operator, &subject);

    assert!(!client.is_blocked(&subject));
    assert!(client.is_allowed(&subject));
}

/// A refusal must not be a blanket operator block: after the admin hands the same
/// address to the operator's own block, the operator's clear succeeds.
#[test]
fn operator_is_not_locked_out_after_its_own_block() {
    let (env, admin, operator, client) = setup();
    let subject = Address::generate(&env);

    // Admin block: refused.
    client.block_address(&admin, &subject, &None);
    assert!(client.try_clear_address(&operator, &subject).is_err());

    // Admin clears it, then the operator places its own block and clears it.
    client.clear_address(&admin, &subject);
    client.block_address(&operator, &subject, &None);
    client.clear_address(&operator, &subject);

    assert!(client.is_allowed(&subject));
}

/// An operator clearing an address that was never blocked is refused: there is no
/// operator-placed block to reverse, and an allow is an admin action.
#[test]
fn operator_cannot_clear_an_unblocked_address() {
    let (env, admin, operator, client) = setup();
    let subject = Address::generate(&env);
    client.allow_address(&admin, &subject);

    assert_eq!(
        client.try_clear_address(&operator, &subject),
        Err(Ok(ContractError::Unauthorized))
    );
    assert!(client.is_allowed(&subject));
}

/// A non-admin, non-operator caller cannot clear anything.
#[test]
fn outsider_cannot_clear_a_block() {
    let (env, admin, _operator, client) = setup();
    let subject = Address::generate(&env);
    let outsider = Address::generate(&env);

    client.block_address(&admin, &subject, &None);

    assert_eq!(
        client.try_clear_address(&outsider, &subject),
        Err(Ok(ContractError::Unauthorized))
    );
    assert!(client.is_blocked(&subject));
}

// ─── provenance bookkeeping ───────────────────────────────────────────────────

/// The placer is readable, and reflects the caller that actually placed each block.
#[test]
fn get_block_placer_reflects_the_placing_caller() {
    let (env, admin, operator, client) = setup();
    let by_admin = Address::generate(&env);
    let by_operator = Address::generate(&env);
    let (admin, operator) = (admin.clone(), operator.clone());
    let never_blocked = Address::generate(&env);

    client.block_address(&admin, &by_admin, &None);
    client.block_address(&operator, &by_operator, &None);

    assert_eq!(client.get_block_placer(&by_admin), Some(admin));
    assert_eq!(
        client.get_block_placer(&by_operator),
        Some(operator.clone())
    );
    assert_eq!(client.get_block_placer(&never_blocked), None);
}

/// Clearing drops the provenance, and a later block records its own placer rather
/// than inheriting the previous one — otherwise a stale attribution could
/// misattribute the new block.
#[test]
fn clearing_drops_provenance_and_reblock_records_its_own_placer() {
    let (env, admin, operator, client) = setup();
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &None);
    client.clear_address(&admin, &subject);
    assert_eq!(client.get_block_placer(&subject), None);

    client.block_address(&operator, &subject, &None);
    assert_eq!(client.get_block_placer(&subject), Some(operator.clone()));
    // And the operator can now reverse that one, but not a re-admin block.
    client.clear_address(&operator, &subject);
    client.block_address(&admin, &subject, &None);
    assert_eq!(
        client.try_clear_address(&operator, &subject),
        Err(Ok(ContractError::OperatorCannotClearAdminBlock))
    );
}

/// The boundary holds while the contract is paused: `clear_address` is permitted
/// while paused (emergency policy) for the admin, and the operator's refusal is a
/// privilege decision that pause does not change.
#[test]
fn admin_block_stays_protected_while_paused() {
    let (env, admin, operator, client) = setup();
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &None);
    client.pause(&admin);

    assert_eq!(
        client.try_clear_address(&operator, &subject),
        Err(Ok(ContractError::OperatorCannotClearAdminBlock))
    );
    assert!(client.is_blocked(&subject));

    // The admin can still remediate while paused.
    client.clear_address(&admin, &subject);
    assert!(!client.is_blocked(&subject));
}

/// The boundary survives an admin transfer: the new admin clears blocks, and a
/// block placed by the *previous* admin is not handed to the operator.
#[test]
fn provenance_survives_admin_transfer() {
    let (env, admin, operator, client) = setup();
    let new_admin = Address::generate(&env);
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &None);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    // The operator still cannot clear it, even though `admin` in storage changed.
    assert_eq!(
        client.try_clear_address(&operator, &subject),
        Err(Ok(ContractError::OperatorCannotClearAdminBlock))
    );
    assert!(client.is_blocked(&subject));

    // The new admin can.
    client.clear_address(&new_admin, &subject);
    assert!(!client.is_blocked(&subject));
}

/// A block reason set by the admin survives an operator's refused clear — the
/// refusal must not partially mutate state.
#[test]
fn refused_operator_clear_leaves_block_reason_intact() {
    let (env, admin, operator, client) = setup();
    let subject = Address::generate(&env);
    let reason = soroban_sdk::Bytes::from_slice(&env, b"OFAC-SDN");

    client.block_address(&admin, &subject, &Some(reason.clone()));

    assert!(client.try_clear_address(&operator, &subject).is_err());
    assert_eq!(client.get_block_reason(&subject), Some(reason));
    assert!(client.is_blocked(&subject));
}
