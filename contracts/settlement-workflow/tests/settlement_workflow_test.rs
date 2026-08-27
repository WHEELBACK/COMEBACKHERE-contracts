use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{
    SettlementWorkflowContract, SettlementWorkflowContractClient, SettlementWorkflowError,
};
use soroban_sdk::{testutils::Address as _, token, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

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
    workflow.initialize(&admin, &compliance_id, &treasury_id);

    // The workflow contract executes settlements as itself, so it must be an
    // authorized Treasury signer.
    treasury.set_signer(&admin, &workflow_id, &1);

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
        .try_execute_with_compliance(
            &settlement_id,
            &token_id,
            &merchant,
        )
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

    workflow
        .try_execute_with_compliance(
            &settlement_id,
            &token_id,
            &merchant,
        )
        .unwrap()
        .unwrap();

    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );
}

#[test]
fn get_config_returns_stored_values() {
    let (
        env,
        admin,
        _merchant,
        _compliance,
        compliance_id,
        _treasury,
        treasury_id,
        workflow,
        _token_id,
    ) = setup();

    let config = workflow.get_config();
    assert_eq!(config.admin, admin);
    assert_eq!(config.compliance_id, compliance_id);
    assert_eq!(config.treasury_id, treasury_id);
    assert_eq!(config.paused, false);
}

#[test]
fn paused_contract_blocks_execution() {
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
        .try_execute_with_compliance(
            &settlement_id,
            &token_id,
            &merchant,
        )
        .unwrap()
        .unwrap();

    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );
}

#[test]
fn pause_rejects_non_admin() {
    let (
        env,
        _admin,
        _merchant,
        _compliance,
        _compliance_id,
        _treasury,
        _treasury_id,
        workflow,
        _token_id,
    ) = setup();

    let stranger = Address::generate(&env);
    let err = workflow.try_pause(&stranger).unwrap_err().unwrap();
    assert_eq!(err, SettlementWorkflowError::Unauthorized);
}

#[test]
fn initialize_rejects_double_init() {
    let (
        env,
        admin,
        _merchant,
        _compliance,
        compliance_id,
        _treasury,
        treasury_id,
        workflow,
        _token_id,
    ) = setup();

    let err = workflow
        .try_initialize(&admin, &compliance_id, &treasury_id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, SettlementWorkflowError::AlreadyInitialized);
}
