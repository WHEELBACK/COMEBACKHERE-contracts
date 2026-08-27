use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

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
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    // Merchant is never allowed, so compliance.is_allowed returns false.
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    let err = workflow
        .try_execute_with_compliance(
            &compliance_id,
            &treasury_id,
            &settlement_id,
            &token_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed);
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

#[test]
fn successful_path_executes_treasury_settlement() {
    let (
        env,
        admin,
        merchant,
        compliance,
        compliance_id,
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
            &compliance_id,
            &treasury_id,
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
fn execution_fails_with_clear_error_when_workflow_not_registered_signer() {
    // Regression test for #370: a first-time deployer who forgets to register the
    // workflow contract as a Treasury signer should see a specific, actionable error
    // (`WorkflowNotRegisteredSigner`) rather than Treasury's generic
    // `UnauthorizedSigner`, which gives no hint about the missing setup step.
    let (
        env,
        admin,
        merchant,
        compliance,
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup_with_signer(false);

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    let err = workflow
        .try_execute_with_compliance(
            &compliance_id,
            &treasury_id,
            &settlement_id,
            &token_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();

    // The exact error a first-time deployer will see — pin it down so the message
    // stays actionable if the precondition handling ever changes.
    assert_eq!(err, TreasuryError::WorkflowNotRegisteredSigner);
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

#[test]
fn settlement_not_found_when_compliance_allows_but_id_missing() {
    // #371: compliance genuinely allows the merchant, but the referenced
    // settlement_id does not exist in treasury (e.g. a mistyped/stale id from a
    // client bug). The failure must surface cleanly as Treasury's own
    // `SettlementNotFound`, not something less diagnosable.
    let (
        env,
        admin,
        merchant,
        compliance,
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    // No settlement is proposed; 999 is guaranteed not to exist.
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    let err = workflow
        .try_execute_with_compliance(
            &compliance_id,
            &treasury_id,
            &999,
            &token_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();

    assert_eq!(err, TreasuryError::SettlementNotFound);
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

#[test]
fn settlement_already_executed_when_executed_directly_via_treasury() {
    // #371: the merchant is compliance-allowed, but the settlement was already
    // executed via a direct Treasury call that bypassed this workflow. The nested
    // treasury call must propagate `AlreadyExecuted` (the compliance gate having
    // already passed one hop earlier).
    let (
        env,
        admin,
        merchant,
        compliance,
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    // Execute directly against treasury, bypassing the workflow's compliance gate.
    treasury.execute_settlement(&admin, &settlement_id, &token_id);

    let err = workflow
        .try_execute_with_compliance(
            &compliance_id,
            &treasury_id,
            &settlement_id,
            &token_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();

    assert_eq!(err, TreasuryError::AlreadyExecuted);
}

#[test]
fn executed_settlement_ids_are_recorded_and_paginated() {
    // #373: settlements executed through the workflow are discoverable via the
    // paginated read entrypoint, so an auditor can confirm they passed the gate
    // and cross-reference against treasury's full history.
    let (
        env,
        admin,
        merchant,
        compliance,
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let id1 = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    let id2 = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &15_000_000);

    workflow
        .try_execute_with_compliance(&compliance_id, &treasury_id, &id1, &token_id, &merchant)
        .unwrap()
        .unwrap();
    workflow
        .try_execute_with_compliance(&compliance_id, &treasury_id, &id2, &token_id, &merchant)
        .unwrap()
        .unwrap();

    let all = workflow.get_executed_settlement_ids_page(&0, &10);
    assert_eq!(all, soroban_sdk::Vec::from_array(&env, [id1, id2]));

    // Pagination: skip the first entry, limit 1.
    let page = workflow.get_executed_settlement_ids_page(&1, &1);
    assert_eq!(page, soroban_sdk::Vec::from_array(&env, [id2]));

    // A settlement executed directly via treasury is NOT recorded by the workflow.
    let id3 = treasury.propose_settlement(&admin, &merchant, &1_000_000);
    treasury.execute_settlement(&admin, &id3, &token_id);
    let still_two = workflow.get_executed_settlement_ids_page(&0, &10);
    assert_eq!(still_two, soroban_sdk::Vec::from_array(&env, [id1, id2]));
}
