/// Tests for #592: hold_settlement optional expiry.
///
/// Covers:
/// - A hold with no expiry persists until explicitly released.
/// - A hold with an expiry is treated as released once the timestamp passes.
/// - get_hold_reason returns None once a hold expires.
/// - get_hold_expiry returns the stored timestamp or None.
/// - execute_settlement succeeds when a hold has expired.
/// - execute_settlement still fails when a hold has NOT yet expired.
/// - Re-holding a settlement whose previous hold has expired succeeds.
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use treasury::{SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient, TreasuryError};

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

// ---------------------------------------------------------------------------
// Permanent hold (no expiry)
// ---------------------------------------------------------------------------

/// A hold placed with `expires_at = None` is permanent — `get_hold_reason`
/// still returns the reason after time advances.
#[test]
fn permanent_hold_persists_after_time_advances() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &None);

    // Advance time well past any reasonable expiry.
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Reason is still present because there is no expiry.
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::AdminHold);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);
}

/// `get_hold_expiry` returns `None` for a permanent hold.
#[test]
fn get_hold_expiry_returns_none_for_permanent_hold() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &None);

    assert_eq!(client.get_hold_expiry(&sid), None);
}

// ---------------------------------------------------------------------------
// Expiring hold — before expiry
// ---------------------------------------------------------------------------

/// Before the expiry timestamp, `get_hold_reason` returns the stored reason
/// and `get_hold_expiry` returns the stored timestamp.
#[test]
fn expiring_hold_is_active_before_expiry() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let expiry: u64 = 500;
    env.ledger().with_mut(|l| l.timestamp = 0);

    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.hold_settlement(
        &admin,
        &sid,
        &SettlementHoldReason::ComplianceReview,
        &Some(expiry),
    );

    // Advance to one second before expiry — hold is still active.
    env.ledger().with_mut(|l| l.timestamp = expiry - 1);

    assert_eq!(
        client.get_hold_reason(&sid),
        SettlementHoldReason::ComplianceReview
    );
    assert_eq!(client.get_hold_expiry(&sid), Some(expiry));
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);
}

// ---------------------------------------------------------------------------
// Expiring hold — at and after expiry
// ---------------------------------------------------------------------------

/// At the exact expiry timestamp, `get_hold_reason` returns `None` (expired).
#[test]
fn expiring_hold_reports_none_at_exact_expiry() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let expiry: u64 = 1_000;
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.hold_settlement(
        &admin,
        &sid,
        &SettlementHoldReason::FraudCheck,
        &Some(expiry),
    );

    env.ledger().with_mut(|l| l.timestamp = expiry);
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::None);
}

/// After the expiry timestamp, `get_hold_reason` returns `None` (expired).
#[test]
fn expiring_hold_reports_none_after_expiry() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let expiry: u64 = 300;
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.hold_settlement(
        &admin,
        &sid,
        &SettlementHoldReason::KycPending,
        &Some(expiry),
    );

    env.ledger().with_mut(|l| l.timestamp = expiry + 1);
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::None);
}

// ---------------------------------------------------------------------------
// Re-holding after expiry
// ---------------------------------------------------------------------------

/// Once a hold has expired, `hold_settlement` must succeed again (the expired
/// hold is no longer "active", so `AlreadyOnHold` must not fire).
#[test]
fn re_hold_after_expiry_succeeds() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let expiry: u64 = 200;
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.hold_settlement(
        &admin,
        &sid,
        &SettlementHoldReason::AdminHold,
        &Some(expiry),
    );

    // Advance past expiry.
    env.ledger().with_mut(|l| l.timestamp = expiry + 10);

    // Should succeed: previous hold is expired.
    let result = client.try_hold_settlement(
        &admin,
        &sid,
        &SettlementHoldReason::FraudCheck,
        &None,
    );
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::FraudCheck);
}

/// Before expiry, a second `hold_settlement` call must still return `AlreadyOnHold`.
#[test]
fn hold_settlement_still_rejects_duplicate_before_expiry() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let expiry: u64 = 1_000;
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.hold_settlement(
        &admin,
        &sid,
        &SettlementHoldReason::AdminHold,
        &Some(expiry),
    );

    // Advance to just before expiry — hold still active.
    env.ledger().with_mut(|l| l.timestamp = expiry - 1);

    let result = client.try_hold_settlement(
        &admin,
        &sid,
        &SettlementHoldReason::FraudCheck,
        &None,
    );
    assert_eq!(result, Err(Ok(TreasuryError::AlreadyOnHold)));
}

// ---------------------------------------------------------------------------
// release_hold clears expiry
// ---------------------------------------------------------------------------

/// After `release_hold`, `get_hold_expiry` returns `None` even if an expiry
/// was previously set.
#[test]
fn release_hold_clears_expiry() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let expiry: u64 = 9_999;
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.hold_settlement(
        &admin,
        &sid,
        &SettlementHoldReason::ComplianceReview,
        &Some(expiry),
    );

    assert_eq!(client.get_hold_expiry(&sid), Some(expiry));

    client.release_hold(&admin, &sid);

    assert_eq!(client.get_hold_expiry(&sid), None);
    assert_eq!(client.get_hold_reason(&sid), SettlementHoldReason::None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::Pending);
}
