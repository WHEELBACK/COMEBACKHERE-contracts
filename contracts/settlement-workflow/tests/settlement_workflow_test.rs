#[path = "reentrancy_suite/malicious_compliance.rs"]
mod malicious_compliance;

use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{
    SettlementWorkflowContract, SettlementWorkflowContractClient, SettlementWorkflowError,
};
use soroban_sdk::{testutils::Address as _, token, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

/// Generous CPU-instruction ceiling for the two-hop cross-contract call chain
/// (Compliance::is_allowed → Treasury::execute_settlement). Native/test-host
/// numbers are far lower; this bound is wide enough to avoid flakiness while
/// still catching a large, unintended regression in the composed call chain (#368).
const MAX_EXECUTE_INSTRUCTIONS: u64 = 5_000_000;

fn setup() -> (
    Env,
    Address,
    Address,
    ComplianceContractClient<'static>,
    Address,
    TreasuryContractClient<'static>,
    Address,
    SettlementWorkflowContractClient<'static>,
    Address,
) {
    setup_with_signer(true)
}

/// `register_workflow_signer` controls whether the workflow contract is registered
/// as a Treasury signer. Pass `false` to exercise the #370 precondition path where
/// the workflow's own address has not been registered via `Treasury::set_signer`.
fn setup_with_signer(register_workflow_signer: bool) -> (
    Env,
    Address,
    Address,
    ComplianceContractClient<'static>,
    Address,
    TreasuryContractClient<'static>,
    Address,
    SettlementWorkflowContractClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    // Pin the trusted compliance/treasury instances once at init (#364).
    workflow.initialize(&compliance_id, &treasury_id);
    // The workflow contract executes settlements as itself, so it must be an
    // authorized Treasury signer.
    if register_workflow_signer {
        treasury.set_signer(&admin, &workflow_id, &1);
    }

    let token_id = env.register_stellar_asset_contract(admin.clone());

    (
        env,
        admin,
        merchant,
        compliance,
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    )
}

#[test]
fn execution_blocked_when_compliance_returns_false() {
    let (
        env,
        admin,
        merchant,
        _compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    let err = workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, SettlementWorkflowError::ComplianceCheckFailed);
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

#[test]
fn successful_path_executes_treasury_settlement() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    workflow.pause(&admin);

    let err = workflow
        .try_execute_with_compliance(
            &settlement_id,
            &token_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, SettlementWorkflowError::ContractPaused);
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

#[test]
fn unpause_restores_execution() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    workflow.pause(&admin);
    workflow.unpause(&admin);

    workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap()
        .unwrap();

    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );
}

#[test]
fn emits_settlement_workflow_executed_event() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    workflow.execute_with_compliance(&settlement_id, &token_id, &merchant);

    let emitted = env
        .events()
        .all()
        .iter()
        .any(|(topics, _data)| {
            topics
                .first()
                .map(|t| t == soroban_sdk::Val::from(soroban_sdk::Symbol::new(&env, "settlement_workflow_executed")))
                .unwrap_or(false)
        });
    assert!(
        emitted,
        "expected a settlement_workflow_executed event to be emitted"
    );
}

#[test]
fn initialize_is_idempotent_and_pins_trusted_instances() {
    let env = Env::default();
    env.mock_all_auths();
    let compliance_id = Address::generate(&env);
    let treasury_id = Address::generate(&env);
    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);

    workflow.initialize(&compliance_id, &treasury_id);
    // Second initialize must trap with AlreadyInitialized.
    let err = workflow
        .try_initialize(&compliance_id, &treasury_id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::AlreadyInitialized);
}

#[test]
fn batch_executes_multiple_settlements_and_skips_invalid_ids() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);

    let good_1 = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    let good_2 = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    // A settlement that does not exist.
    let bogus: u64 = 999;
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(good_1);
    ids.push_back(bogus);
    ids.push_back(good_2);

    let executed = workflow.execute_with_compliance_batch(&ids, &token_id, &merchant);
    assert_eq!(executed, soroban_sdk::Vec::from_array(&env, [good_1, good_2]));
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );
}

#[test]
fn batch_rejected_when_compliance_fails() {
    let (
        env,
        admin,
        merchant,
        _compliance,
        _compliance_id,
        treasury,
        _treasury_id,
        workflow,
        _token_id,
    ) = setup();

    let good = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(good);

    let err = workflow
        .try_execute_with_compliance_batch(&ids, &Address::generate(&env), &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed);
}

#[test]
fn execute_with_compliance_stays_under_instruction_budget() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    // Lift budget limits so the call chain is measured, not artificially capped.
    env.cost_estimate().budget().reset_unlimited();
    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);
    env.cost_estimate().budget().reset_tracker();

    workflow
        .execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap();

    let instructions = env.cost_estimate().budget().cpu_instruction_cost();
    assert!(
        instructions <= MAX_EXECUTE_INSTRUCTIONS,
        "execute_with_compliance used {instructions} instructions, expected <= {MAX_EXECUTE_INSTRUCTIONS}"
    );
}
