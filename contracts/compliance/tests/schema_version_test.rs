// Regression snapshot for compliance's get_schema_version entrypoint.
//
// Context
// -------
// compliance is the only contract in this workspace that exposes a
// get_schema_version entrypoint. invoice and treasury have no equivalent
// (confirmed by grepping for `get_schema_version` and `SchemaVersion` across
// both crates — neither has a schema-version concept at all).
//
// Why this test exists
// --------------------
// Per the storage-schema migration-path documentation (#113), SchemaVersion has
// real implications for how a future storage migration must be reasoned about
// and executed. Bumping it silently (without a deliberate, reviewed PR) would
// leave callers with no warning that stored data layouts may have shifted.
//
// This test hard-codes the *expected* schema version as a snapshot constant.
// If get_schema_version ever returns a different value the test fails
// immediately, forcing the author of that change to acknowledge the bump,
// update this constant, and explain the migration path in their PR — exactly
// the same spirit as the ABI snapshot-drift CI check (#96) but scoped to this
// one semantically-important value.
//
// Updating this test
// ------------------
// If you are intentionally bumping the schema version:
//   1. Update EXPECTED_SCHEMA_VERSION below to match the new value.
//   2. Add an entry to CHANGELOG.md describing the storage migration path.
//   3. Include both changes in the same PR so reviewers see the full picture.

use compliance::{ComplianceContract, ComplianceContractClient, ComplianceError, DataKey};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// The schema version that compliance::initialize stores and
/// get_schema_version returns.  This is intentionally a snapshot constant —
/// if the value changes without this constant being updated the test fails,
/// which is the point.
const EXPECTED_SCHEMA_VERSION: u32 = 2;

fn setup() -> (Env, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, client)
}

/// Core regression: get_schema_version must return the snapshot value.
///
/// If this test fails after a deliberate schema bump, update
/// EXPECTED_SCHEMA_VERSION above and document the migration path in the PR.
#[test]
fn schema_version_matches_snapshot() {
    let (_env, client) = setup();
    assert_eq!(
        client.get_schema_version(),
        EXPECTED_SCHEMA_VERSION,
        "compliance schema version changed unexpectedly — \
         if this bump is intentional, update EXPECTED_SCHEMA_VERSION in \
         contracts/compliance/tests/schema_version_test.rs and document \
         the storage migration path in your PR (see #113)"
    );
}

/// get_schema_version must be callable before any addresses are managed,
/// immediately after initialization — it reads instance storage set by
/// initialize, so no further contract calls should be required.
#[test]
fn schema_version_available_immediately_after_init() {
    let (_env, client) = setup();
    // The entrypoint must not panic or return an unexpected default.
    let version = client.get_schema_version();
    assert!(
        version >= 1,
        "get_schema_version returned {version}, expected >= 1 after initialization"
    );
}

/// Repeated reads must be stable — get_schema_version is a pure storage read
/// and must not vary between calls on the same contract instance.
#[test]
fn schema_version_is_stable_across_repeated_reads() {
    let (_env, client) = setup();
    let first = client.get_schema_version();
    let second = client.get_schema_version();
    assert_eq!(
        first, second,
        "get_schema_version returned different values on successive reads: \
         {first} then {second}"
    );
}

// ── #610 migrate ──────────────────────────────────────────────────────────────

fn setup_with_version(version: u32) -> (Env, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    env.as_contract(&id, || {
        env.storage()
            .instance()
            .set(&DataKey::SchemaVersion, &version);
    });
    (env, admin, client)
}

#[test]
fn migrate_bumps_previous_version_to_current() {
    let (_env, admin, client) = setup_with_version(EXPECTED_SCHEMA_VERSION - 1);
    assert_eq!(client.migrate(&admin), EXPECTED_SCHEMA_VERSION);
    assert_eq!(client.get_schema_version(), EXPECTED_SCHEMA_VERSION);
}

#[test]
fn migrate_is_idempotent() {
    let (_env, admin, client) = setup_with_version(EXPECTED_SCHEMA_VERSION - 1);
    client.migrate(&admin);
    assert_eq!(client.migrate(&admin), EXPECTED_SCHEMA_VERSION);
    assert_eq!(client.get_schema_version(), EXPECTED_SCHEMA_VERSION);
}

#[test]
fn migrate_rejects_unexpected_version() {
    let (_env, admin, client) = setup_with_version(EXPECTED_SCHEMA_VERSION + 5);
    assert_eq!(
        client.try_migrate(&admin),
        Err(Ok(ComplianceError::UnexpectedSchemaVersion))
    );
    assert_eq!(client.get_schema_version(), EXPECTED_SCHEMA_VERSION + 5);
}

#[test]
fn migrate_rejects_non_admin() {
    let (env, _admin, client) = setup_with_version(EXPECTED_SCHEMA_VERSION - 1);
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_migrate(&stranger),
        Err(Ok(ComplianceError::Unauthorized))
    );
}

// ── #763 migrate from v1 to v2 with populated state ──────────────────────────

/// Seeds v1-shaped storage with allowed, blocked and expiring addresses,
/// runs `migrate`, and checks that every query returns the same answers.
#[test]
fn migrate_v1_to_v2_preserves_populated_state() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let allowed_addr = Address::generate(&env);
    let blocked_addr = Address::generate(&env);
    let expiring_addr = Address::generate(&env);
    let now = env.ledger().timestamp();
    let far_future = now + 3600;

    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    // Force storage to v1 while keeping the populated v1-shaped state.
    env.as_contract(&id, || {
        env.storage()
            .instance()
            .set(&DataKey::SchemaVersion, &1u32);
        env.storage().persistent().set(&DataKey::Allowed(allowed_addr.clone()), &true);
        env.storage().persistent().set(&DataKey::Blocked(blocked_addr.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(expiring_addr.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::AllowedUntil(expiring_addr.clone()), &far_future);
    });

    // Snapshot query answers before migration.
    let before_allowed = client.is_allowed(&allowed_addr);
    let before_blocked_allowed = client.is_allowed(&blocked_addr);
    let before_expiring_allowed = client.is_allowed(&expiring_addr);
    let before_blocked = client.is_blocked(&blocked_addr);
    let before_expiring_blocked = client.is_blocked(&expiring_addr);
    let before_expiring_status = client.address_status(&expiring_addr);

    // Migrate from v1 to v2.
    let version = client.migrate(&admin);
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
    assert_eq!(client.get_schema_version(), CURRENT_SCHEMA_VERSION);

    // Every query must return the same answers after migration.
    assert_eq!(client.is_allowed(&allowed_addr), before_allowed);
    assert_eq!(client.is_allowed(&blocked_addr), before_blocked_allowed);
    assert_eq!(client.is_allowed(&expiring_addr), before_expiring_allowed);
    assert_eq!(client.is_blocked(&blocked_addr), before_blocked);
    assert_eq!(client.is_blocked(&expiring_addr), before_expiring_blocked);
    assert_eq!(client.address_status(&expiring_addr), before_expiring_status);
}

/// `address_status` must be callable and consistent before and after migration
/// for an address that has both an allow expiry and a block override.
#[test]
fn migrate_preserves_address_status_for_blocked_and_expiring() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let addr = Address::generate(&env);
    let now = env.ledger().timestamp();
    let far_future = now + 3600;

    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    env.as_contract(&id, || {
        env.storage()
            .instance()
            .set(&DataKey::SchemaVersion, &1u32);
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(addr.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(addr.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::AllowedUntil(addr.clone()), &far_future);
    });

    let before = client.address_status(&addr);
    client.migrate(&admin);
    let after = client.address_status(&addr);
    assert_eq!(before, after);
    assert_eq!(client.get_schema_version(), CURRENT_SCHEMA_VERSION);
}

// ── #764 migrate at current schema version is a clean no-op ──────────────────

/// Calling `migrate` when already at `CURRENT_SCHEMA_VERSION` must be a clean
/// no-op: it returns the current version, emits no `schema_migrated` event, and
/// leaves storage untouched.
#[test]
fn migrate_at_current_version_is_noop() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    // Already at CURRENT_SCHEMA_VERSION (= 2) after initialize.
    assert_eq!(client.get_schema_version(), CURRENT_SCHEMA_VERSION);

    let before_events: std::vec::Vec<String> = env
        .events()
        .all()
        .iter()
        .map(|(_, topics, _)| Symbol::from_val(&env, &topics.get_unchecked(0)).to_string())
        .collect();
    assert!(!before_events.iter().any(|n| n == "schema_migrated"));

    let result = client.migrate(&admin);
    assert_eq!(result, CURRENT_SCHEMA_VERSION);
    assert_eq!(client.get_schema_version(), CURRENT_SCHEMA_VERSION);

    // No `schema_migrated` event should have been emitted by the no-op call.
    let after_events: std::vec::Vec<String> = env
        .events()
        .all()
        .iter()
        .map(|(_, topics, _)| Symbol::from_val(&env, &topics.get_unchecked(0)).to_string())
        .collect();
    assert_eq!(before_events, after_events);
}
