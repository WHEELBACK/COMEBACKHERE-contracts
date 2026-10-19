// Regression snapshot for settlement-workflow's get_schema_version entrypoint (#779).
//
// Context
// -------
// settlement-workflow is given the same schema versioning as compliance (#779):
// `CURRENT_SCHEMA_VERSION`, `get_schema_version`, and `migrate` are added so that
// future storage changes have a tested migration path.
//
// Why this test exists
// --------------------
// Per docs/STORAGE_VERSIONING.md, SchemaVersion has real implications for how a
// future storage migration must be reasoned about and executed. Bumping it silently
// (without a deliberate, reviewed PR) would leave callers with no warning that
// stored data layouts may have shifted.
//
// This test hard-codes the *expected* schema version as a snapshot constant.
// If get_schema_version ever returns a different value the test fails immediately,
// forcing the author of that change to acknowledge the bump, update this constant,
// and explain the migration path in their PR — exactly the same spirit as the ABI
// snapshot-drift CI check but scoped to this semantically-important value.
//
// Updating this test
// ------------------
// If you are intentionally bumping the schema version:
//   1. Update EXPECTED_SCHEMA_VERSION below to match the new value.
//   2. Add an entry to CHANGELOG.md describing the storage migration path.
//   3. Include both changes in the same PR so reviewers see the full picture.

use settlement_workflow::{DataKey, SettlementWorkflowContract, SettlementWorkflowContractClient, WorkflowError};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// The schema version that initialize stores and get_schema_version returns.
/// This is intentionally a snapshot constant — if the value changes without
/// this constant being updated the test fails, which is the point.
const EXPECTED_SCHEMA_VERSION: u32 = 1;

fn setup() -> (Env, Address, SettlementWorkflowContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let compliance_id = Address::generate(&env);
    let treasury_id = Address::generate(&env);
    let id = env.register(SettlementWorkflowContract, ());
    let client = SettlementWorkflowContractClient::new(&env, &id);
    client.initialize(&admin, &compliance_id, &treasury_id);
    (env, admin, client)
}

/// Core regression: get_schema_version must return the snapshot value.
///
/// If this test fails after a deliberate schema bump, update
/// EXPECTED_SCHEMA_VERSION above and document the migration path in the PR.
#[test]
fn schema_version_matches_snapshot() {
    let (_env, _admin, client) = setup();
    assert_eq!(
        client.get_schema_version(),
        EXPECTED_SCHEMA_VERSION,
        "settlement-workflow schema version changed unexpectedly — \
         if this bump is intentional, update EXPECTED_SCHEMA_VERSION in \
         contracts/settlement-workflow/tests/schema_version_test.rs and document \
         the storage migration path in your PR (see #779)"
    );
}

/// get_schema_version must be callable before any settlements are executed,
/// immediately after initialization — it reads instance storage set by initialize.
#[test]
fn schema_version_available_immediately_after_init() {
    let (_env, _admin, client) = setup();
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
    let (_env, _admin, client) = setup();
    let first = client.get_schema_version();
    let second = client.get_schema_version();
    assert_eq!(
        first, second,
        "get_schema_version returned different values on successive reads: \
         {first} then {second}"
    );
}

// ── #779 migrate ──────────────────────────────────────────────────────────────

fn setup_with_version(version: u32) -> (Env, Address, SettlementWorkflowContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let compliance_id = Address::generate(&env);
    let treasury_id = Address::generate(&env);
    let id = env.register(SettlementWorkflowContract, ());
    let client = SettlementWorkflowContractClient::new(&env, &id);
    client.initialize(&admin, &compliance_id, &treasury_id);
    // Override the schema version written by initialize so we can test migration paths.
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
        Err(Ok(WorkflowError::UnexpectedSchemaVersion))
    );
    assert_eq!(client.get_schema_version(), EXPECTED_SCHEMA_VERSION + 5);
}

#[test]
fn migrate_rejects_non_admin() {
    let (env, _admin, client) = setup_with_version(EXPECTED_SCHEMA_VERSION - 1);
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_migrate(&stranger),
        Err(Ok(WorkflowError::Unauthorized))
    );
}
