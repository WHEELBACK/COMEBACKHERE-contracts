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

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// The schema version that compliance::initialize stores and
/// get_schema_version returns.  This is intentionally a snapshot constant —
/// if the value changes without this constant being updated the test fails,
/// which is the point.
const EXPECTED_SCHEMA_VERSION: u32 = 1;

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
