//! Initialization and invariant snapshot tests for `settlement-workflow`.

use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    SettlementWorkflowContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let compliance_id = Address::generate(&env);
    let treasury_id = Address::generate(&env);
    let id = env.register(
        SettlementWorkflowContract,
        (admin.clone(), compliance_id.clone(), treasury_id.clone()),
    );
    let client = SettlementWorkflowContractClient::new(&env, &id);
    (env, admin, compliance_id, treasury_id, client)
}

#[test]
fn constructor_pins_ids_and_satisfies_invariants() {
    let (_env, admin, compliance_id, treasury_id, client) = setup();
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_compliance_id(), compliance_id);
    assert_eq!(client.get_treasury_id(), treasury_id);
    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));
}

#[test]
fn repeated_reads_are_stable() {
    let (_env, _admin, compliance_id, treasury_id, client) = setup();
    assert_eq!(client.get_compliance_id(), compliance_id);
    assert_eq!(client.get_compliance_id(), compliance_id);
    assert_eq!(client.get_treasury_id(), treasury_id);
    assert_eq!(client.get_treasury_id(), treasury_id);
}
