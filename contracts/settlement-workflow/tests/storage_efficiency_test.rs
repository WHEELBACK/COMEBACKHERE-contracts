//! #871 — Storage deserialization efficiency regression tests.
//!
//! Pins the CPU instruction budget for hot-path storage reads so a future
//! change that regresses deserialization efficiency is caught before it ships.

use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

/// Generous ceiling for the full execute_with_compliance call chain.
const MAX_EXECUTE_INSTRUCTIONS: u64 = 5_000_000;

/// Ceiling for a single `is_paused()` read — a lightweight instance storage read
/// should be well under this.
const MAX_IS_PAUSED_INSTRUCTIONS: u64 = 500_000;

fn setup_workflow(
    env: &Env,
) -> (
    SettlementWorkflowContractClient<'static>,
    Address,
    Address,
    TreasuryContractClient<'static>,
    ComplianceContractClient<'static>,
) {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let merchant = Address::generate(env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(env));

    let workflow_id = env.register(
        SettlementWorkflowContract,
        (admin.clone(), compliance_id.clone(), treasury_id.clone()),
    );
    let workflow = SettlementWorkflowContractClient::new(env, &workflow_id);
    treasury.set_signer(&admin, &workflow_id, &1);

    (workflow, admin, merchant, treasury, compliance)
}

/// `is_paused()` must complete within a tight CPU budget — it is a single
/// instance storage read and must not trigger unexpected deserialization work.
#[test]
fn hot_path_paused_check_uses_minimal_instructions() {
    let env = Env::default();
    let (workflow, ..) = setup_workflow(&env);

    env.cost_estimate().budget().reset_unlimited();
    env.cost_estimate().budget().reset_tracker();

    let _paused = workflow.is_paused();

    let instructions = env.cost_estimate().budget().cpu_instruction_cost();
    assert!(
        instructions <= MAX_IS_PAUSED_INSTRUCTIONS,
        "is_paused() used {instructions} instructions, expected <= {MAX_IS_PAUSED_INSTRUCTIONS}"
    );
}

/// The full `execute_with_compliance` call chain must stay within the same
/// instruction budget that the settlement_workflow_test.rs already pins.
#[test]
fn execute_with_compliance_instruction_budget_has_not_regressed() {
    let env = Env::default();
    let (workflow, admin, merchant, treasury, compliance) = setup_workflow(&env);

    let token_id = env.register_stellar_asset_contract(admin.clone());
    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    token::StellarAssetClient::new(&env, &token_id)
        .mint(&treasury.address, &10_000_000);

    env.cost_estimate().budget().reset_unlimited();
    env.cost_estimate().budget().reset_tracker();

    workflow.execute_with_compliance(&settlement_id, &token_id, &merchant);

    let instructions = env.cost_estimate().budget().cpu_instruction_cost();
    assert!(
        instructions <= MAX_EXECUTE_INSTRUCTIONS,
        "execute_with_compliance used {instructions} instructions, \
         expected <= {MAX_EXECUTE_INSTRUCTIONS}"
    );
}

/// `is_paused()` returns consistent results across repeated calls on the same env.
#[test]
fn storage_reads_are_idempotent() {
    let env = Env::default();
    let (workflow, ..) = setup_workflow(&env);

    let first = workflow.is_paused();
    let second = workflow.is_paused();
    assert_eq!(first, second, "is_paused() must return the same value on repeated reads");
}

/// After `pause`, `is_paused()` returns true; after `unpause`, false.
/// Verifies the write path is consistent with the optimized read path.
#[test]
fn paused_flag_round_trips_correctly() {
    let env = Env::default();
    let (workflow, admin, ..) = setup_workflow(&env);

    assert!(!workflow.is_paused());
    workflow.pause(&admin);
    assert!(workflow.is_paused());
    workflow.unpause(&admin);
    assert!(!workflow.is_paused());
}
