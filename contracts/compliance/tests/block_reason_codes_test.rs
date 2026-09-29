/// Tests for #594: standardised block reason codes.
///
/// Verifies that every BlockReason variant can be stored and retrieved via
/// block_address / get_block_reason, that block_address_until also stores a
/// reason, and that blocking without a reason leaves get_block_reason returning
/// None.
use compliance::{BlockReason, ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup(env: &Env) -> (ComplianceContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(env, &id);
    client.initialize(&admin);
    (client, admin)
}

// ---------------------------------------------------------------------------
// No reason (None)
// ---------------------------------------------------------------------------

/// Blocking an address without a reason stores no reason — get_block_reason
/// returns None.
#[test]
fn block_address_without_reason_stores_none() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &None);

    assert_eq!(client.get_block_reason(&subject), None);
}

// ---------------------------------------------------------------------------
// BlockReason::Sanctions
// ---------------------------------------------------------------------------

#[test]
fn block_address_sanctions_reason_is_stored_and_retrieved() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &Some(BlockReason::Sanctions));

    assert_eq!(
        client.get_block_reason(&subject),
        Some(BlockReason::Sanctions)
    );
}

// ---------------------------------------------------------------------------
// BlockReason::Fraud
// ---------------------------------------------------------------------------

#[test]
fn block_address_fraud_reason_is_stored_and_retrieved() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &Some(BlockReason::Fraud));

    assert_eq!(client.get_block_reason(&subject), Some(BlockReason::Fraud));
}

// ---------------------------------------------------------------------------
// BlockReason::ManualReview
// ---------------------------------------------------------------------------

#[test]
fn block_address_manual_review_reason_is_stored_and_retrieved() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &Some(BlockReason::ManualReview));

    assert_eq!(
        client.get_block_reason(&subject),
        Some(BlockReason::ManualReview)
    );
}

// ---------------------------------------------------------------------------
// BlockReason::CourtOrder
// ---------------------------------------------------------------------------

#[test]
fn block_address_court_order_reason_is_stored_and_retrieved() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &Some(BlockReason::CourtOrder));

    assert_eq!(
        client.get_block_reason(&subject),
        Some(BlockReason::CourtOrder)
    );
}

// ---------------------------------------------------------------------------
// BlockReason::Other
// ---------------------------------------------------------------------------

#[test]
fn block_address_other_reason_is_stored_and_retrieved() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &Some(BlockReason::Other));

    assert_eq!(client.get_block_reason(&subject), Some(BlockReason::Other));
}

// ---------------------------------------------------------------------------
// block_address_until with reason
// ---------------------------------------------------------------------------

/// block_address_until stores both the expiry timestamp and the reason code.
#[test]
fn block_address_until_with_sanctions_reason_is_stored_and_retrieved() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    let unblock_at: u64 = 9_999_999;
    client.block_address_until(
        &admin,
        &subject,
        &unblock_at,
        &Some(BlockReason::Sanctions),
    );

    assert_eq!(
        client.get_block_reason(&subject),
        Some(BlockReason::Sanctions)
    );
    // Confirm the address is actually blocked.
    assert!(!client.is_allowed(&subject));
}

/// block_address_until without a reason stores no reason.
#[test]
fn block_address_until_without_reason_stores_none() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    let unblock_at: u64 = 9_999_999;
    client.block_address_until(&admin, &subject, &unblock_at, &None);

    assert_eq!(client.get_block_reason(&subject), None);
}

// ---------------------------------------------------------------------------
// Reason survives is_blocked check
// ---------------------------------------------------------------------------

/// After blocking with a reason, is_blocked returns true and get_block_reason
/// still returns the stored reason.
#[test]
fn blocked_address_reason_survives_is_blocked_check() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let subject = Address::generate(&env);

    client.block_address(&admin, &subject, &Some(BlockReason::Fraud));

    assert!(client.is_blocked(&subject));
    assert_eq!(client.get_block_reason(&subject), Some(BlockReason::Fraud));
}

// ---------------------------------------------------------------------------
// Reason on non-blocked address
// ---------------------------------------------------------------------------

/// An address that has never been blocked has no reason stored.
#[test]
fn never_blocked_address_has_no_reason() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    let subject = Address::generate(&env);

    assert_eq!(client.get_block_reason(&subject), None);
}
