//! Regression coverage for behaviors that changed between soroban-sdk 20.5.0
//! and 22.0.0 (commits 4a98822, da490a2, 233d01f, 5c4b648). These pin the
//! *current* SDK semantics our contracts and tests now depend on, so a
//! future downgrade or re-upgrade that silently reintroduces the same
//! fallout fails a test instead of failing CI in a confusing way six days
//! later.

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

// ── testutils events no longer accumulate across separate top-level calls ──
//
// Under soroban-sdk 20.5.0, `env.events().all()` accumulated every event
// published by every top-level contract invocation for the lifetime of the
// `Env`, so tests could make several calls and then search the whole log
// for each event by symbol (see the pre-fix version of
// `contracts/treasury/tests/task1_tests.rs::event_order_propose_approve_cancel`).
// Under 22.0.0, `events().all()` only reflects the most recent top-level
// call — the fix in 233d01f switched every such test to capture
// `events().all().last()` immediately after each call.

#[test]
fn events_do_not_accumulate_across_separate_top_level_calls() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);
    assert_eq!(
        env.events().all().len(),
        1,
        "expected exactly one event right after the propose call"
    );

    client.approve_settlement(&admin, &sid);
    assert_eq!(
        env.events().all().len(),
        1,
        "soroban-sdk 22 testutils does not accumulate events across separate \
         top-level calls: events().all() after approve_settlement should contain \
         only that call's event, not propose_settlement's event too"
    );
}

// ── nested (non-root) auth requires mock_all_auths_allowing_non_root_auth ──
//
// A `require_auth()` call made by a contract that is not the root of the
// invocation (e.g. a wrapper contract calling into compliance on the
// admin's behalf) is not satisfied by plain `mock_all_auths()` under
// soroban-sdk 22 — see the same requirement already relied on in
// `contracts/treasury/tests/pause_blocks_treasury_test.rs`. This wrapper
// reproduces the same nested-call shape against the compliance contract.

#[contract]
struct NestedAuthWrapper;

#[contractimpl]
impl NestedAuthWrapper {
    pub fn allow_via(env: Env, compliance_id: Address, admin: Address, address: Address) {
        let compliance = ComplianceContractClient::new(&env, &compliance_id);
        compliance.allow_address(&admin, &address);
    }
}

#[test]
#[should_panic]
fn nested_auth_is_rejected_by_plain_mock_all_auths() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let address = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let wrapper_id = env.register_contract(None, NestedAuthWrapper);
    let wrapper = NestedAuthWrapperClient::new(&env, &wrapper_id);

    // admin.require_auth() happens inside compliance.allow_address, invoked
    // from the wrapper rather than directly from the test — plain
    // mock_all_auths() does not authorize this nested call.
    wrapper.allow_via(&compliance_id, &admin, &address);
}

#[test]
fn nested_auth_succeeds_with_allowing_non_root_auth() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let address = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let wrapper_id = env.register_contract(None, NestedAuthWrapper);
    let wrapper = NestedAuthWrapperClient::new(&env, &wrapper_id);

    wrapper.allow_via(&compliance_id, &admin, &address);

    assert!(compliance.is_allowed(&address));
}

// ── compliance tier field widened from u8 to u32 ────────────────────────────
//
// soroban-sdk 22 removed `TryFromVal` for `u8`, so the compliance tier field
// was widened to `u32` (233d01f). Store a value outside u8's range to pin
// that the field is genuinely wide rather than silently truncated/wrapped
// by a narrower storage representation.

#[test]
fn compliance_tier_round_trips_values_beyond_u8_range() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let address = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let tier: u32 = 1_000_000;
    compliance.allow_address_with_tier(&admin, &address, &tier);

    assert_eq!(compliance.get_address_tier(&address), tier);
}
