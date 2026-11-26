//! #938: automatic TTL bumps for persistent role-manager keys.
//!
//! Persistent entries are evicted when their TTL lapses, which for an access
//! control registry would silently revoke every role and budget. All persistent
//! reads and writes now go through helpers that extend a key's TTL once it
//! drops below [`PERSISTENT_TTL_THRESHOLD`]. The host only performs the write
//! when the threshold is actually crossed, so a hot key does not pay for a TTL
//! update on every access (the "no metering regression" acceptance criterion).
//!
//! The tests use `testutils::storage::{Instance, Persistent}::get_ttl` to read
//! the exact remaining lifetime after writes, reads, and ledger jumps.

use proptest::prelude::*;
use role_manager::{
    DataKey, Role, RoleManagerContract, RoleManagerContractClient, INSTANCE_TTL_EXTEND_TO,
    PERSISTENT_TTL_EXTEND_TO, PERSISTENT_TTL_THRESHOLD,
};
use soroban_sdk::{
    testutils::{storage::Instance as _, storage::Persistent as _, Address as _, Ledger},
    Address, Env,
};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let role_manager = env.register(RoleManagerContract, ());
    let client = RoleManagerContractClient::new(&env, &role_manager);
    client.initialize(&admin);

    (env, admin, operator, role_manager)
}

fn role_key(operator: &Address) -> DataKey {
    DataKey::RoleMember(Role::Operator, operator.clone())
}

fn role_ttl(env: &Env, role_manager: &Address, operator: &Address) -> u32 {
    let key = role_key(operator);
    env.as_contract(role_manager, || env.storage().persistent().get_ttl(&key))
}

fn budget_ttl(env: &Env, role_manager: &Address, operator: &Address) -> u32 {
    let key = DataKey::Budget(Role::Operator, operator.clone());
    env.as_contract(role_manager, || env.storage().persistent().get_ttl(&key))
}

#[test]
fn role_grant_extends_persistent_key_ttl() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    client.grant_role(&admin, &Role::Operator, &operator);

    assert_eq!(
        role_ttl(&env, &role_manager, &operator),
        PERSISTENT_TTL_EXTEND_TO
    );
}

#[test]
fn read_below_threshold_bumps_role_key_back_to_target() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);
    client.grant_role(&admin, &Role::Operator, &operator);

    // Land one ledger below the bump threshold.
    env.ledger().with_mut(|ledger| {
        ledger.sequence_number += PERSISTENT_TTL_EXTEND_TO - PERSISTENT_TTL_THRESHOLD + 1;
    });
    assert_eq!(
        role_ttl(&env, &role_manager, &operator),
        PERSISTENT_TTL_THRESHOLD - 1
    );

    assert!(client.has_role(&Role::Operator, &operator));
    assert_eq!(
        role_ttl(&env, &role_manager, &operator),
        PERSISTENT_TTL_EXTEND_TO
    );
}

#[test]
fn read_above_threshold_does_not_rewrite_ttl() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);
    client.grant_role(&admin, &Role::Operator, &operator);

    env.ledger()
        .with_mut(|ledger| ledger.sequence_number += 100);
    let before = role_ttl(&env, &role_manager, &operator);

    assert!(client.has_role(&Role::Operator, &operator));
    // No bump while healthy: the TTL only fell by the ledger advance.
    assert_eq!(role_ttl(&env, &role_manager, &operator), before);
}

#[test]
fn budget_reads_and_writes_bump_their_persistent_key() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);
    client.grant_role(&admin, &Role::Operator, &operator);
    client.credit_budget(&admin, &Role::Operator, &operator, &12);

    assert_eq!(
        budget_ttl(&env, &role_manager, &operator),
        PERSISTENT_TTL_EXTEND_TO
    );

    // Age the budget key below the threshold and read it back.
    env.ledger().with_mut(|ledger| {
        ledger.sequence_number += PERSISTENT_TTL_EXTEND_TO - PERSISTENT_TTL_THRESHOLD + 1;
    });
    assert_eq!(client.get_budget(&Role::Operator, &operator), 12);
    assert_eq!(
        budget_ttl(&env, &role_manager, &operator),
        PERSISTENT_TTL_EXTEND_TO
    );

    // Refresh the instance and the role key before the second long jump: on a
    // real network archived entries are restored by the host, but testutils
    // traps instead. Both are read by `debit_budget` below.
    client.grant_role(&admin, &Role::TreasurySigner, &operator);
    assert!(client.has_role(&Role::Operator, &operator));

    // A partial debit is a write and must bump the budget key again.
    env.ledger().with_mut(|ledger| {
        ledger.sequence_number += PERSISTENT_TTL_EXTEND_TO - PERSISTENT_TTL_THRESHOLD + 1;
    });
    assert_eq!(client.debit_budget(&operator, &Role::Operator, &2), 10);
    assert_eq!(
        budget_ttl(&env, &role_manager, &operator),
        PERSISTENT_TTL_EXTEND_TO
    );
}

#[test]
fn instance_ttl_is_extended_on_state_changing_entrypoints() {
    let (env, admin, operator, role_manager) = setup();
    let client = RoleManagerContractClient::new(&env, &role_manager);

    let instance_ttl = env.as_contract(&role_manager, || env.storage().instance().get_ttl());
    assert_eq!(instance_ttl, INSTANCE_TTL_EXTEND_TO);

    env.ledger().with_mut(|ledger| {
        ledger.sequence_number += INSTANCE_TTL_EXTEND_TO - PERSISTENT_TTL_THRESHOLD + 1;
    });
    client.grant_role(&admin, &Role::Operator, &operator);
    assert_eq!(
        env.as_contract(&role_manager, || env.storage().instance().get_ttl()),
        INSTANCE_TTL_EXTEND_TO
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// After any ledger advance shorter than the extended lifetime, a read must
    /// either leave a healthy TTL untouched or restore it to the target once it
    /// dips under the threshold. It can never end up below the threshold.
    #[test]
    fn prop_role_ttl_never_lapses_after_a_read(advance in 0u32..PERSISTENT_TTL_EXTEND_TO) {
        let (env, admin, operator, role_manager) = setup();
        let client = RoleManagerContractClient::new(&env, &role_manager);
        client.grant_role(&admin, &Role::Operator, &operator);

        env.ledger()
            .with_mut(|ledger| ledger.sequence_number += advance);
        assert!(client.has_role(&Role::Operator, &operator));

        let remaining = PERSISTENT_TTL_EXTEND_TO - advance;
        let expected = if remaining < PERSISTENT_TTL_THRESHOLD {
            PERSISTENT_TTL_EXTEND_TO
        } else {
            remaining
        };
        prop_assert_eq!(role_ttl(&env, &role_manager, &operator), expected);
    }
}
