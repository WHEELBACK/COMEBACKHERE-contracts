#[path = "reentrancy_suite/malicious_compliance.rs"]
mod malicious_compliance;

use compliance::{ComplianceContract, ComplianceContractClient};
use malicious_compliance::{MaliciousCompliance, MaliciousComplianceClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient, TreasuryError};

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

// --- Reentrancy suite: compliance-then-treasury call sequence ---
//
// #118's reentrancy suite (contracts/treasury/tests/reentrancy_suite/) only
// targets Treasury::execute_settlement's own token-transfer callback. The
// tests below target a structurally different surface: execute_with_compliance
// calls Compliance::is_allowed *first*, then Treasury::execute_settlement
// second, only if the first call passes. These tests use MaliciousCompliance
// (contracts/settlement-workflow/tests/reentrancy_suite/malicious_compliance.rs)
// to simulate a compromised compliance contract that reenters treasury from
// inside that first call, before execute_with_compliance's own, legitimate
// execute_settlement call ever runs.

fn setup_reentrancy_fixture<'a>(
    env: &'a Env,
) -> (
    Address,
    Address,
    TreasuryContractClient<'a>,
    Address,
    SettlementWorkflowContractClient<'a>,
    Address,
    Address,
    u64,
) {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let merchant = Address::generate(env);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(env));

    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(env, &workflow_id);
    // Same precondition as the legitimate flow: the workflow contract must
    // be a registered treasury signer since it executes settlements as itself.
    treasury.set_signer(&admin, &workflow_id, &1);

    let token_id = env.register_stellar_asset_contract(admin.clone());

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(env, &token_id).mint(&treasury_id, &10_000_000);

    (
        admin,
        merchant,
        treasury,
        treasury_id,
        workflow,
        workflow_id,
        token_id,
        settlement_id,
    )
}

#[test]
fn reentrant_compliance_allow_callback_reverts_atomically() {
    // Malicious compliance allows the merchant, but only after reentering
    // Treasury::execute_settlement directly during the `is_allowed` check.
    // That reentrant call runs to completion (transfer + status -> Executed)
    // before execute_with_compliance's own, legitimate execute_settlement
    // call is ever reached. That second call then finds the settlement
    // already Executed and panics with AlreadyExecuted, aborting the entire
    // top-level invocation.
    let env = Env::default();
    let (_admin, merchant, treasury, treasury_id, workflow, workflow_id, token_id, settlement_id) =
        setup_reentrancy_fixture(&env);

    let malicious_compliance_id = env.register_contract(None, MaliciousCompliance);
    let malicious_compliance = MaliciousComplianceClient::new(&env, &malicious_compliance_id);
    malicious_compliance.set_reentry_target(&treasury_id, &workflow_id, &settlement_id, &token_id);
    malicious_compliance.set_verdict(&true);

    let result = workflow.try_execute_with_compliance(
        &malicious_compliance_id,
        &treasury_id,
        &settlement_id,
        &token_id,
        &merchant,
    );

    assert!(
        result.is_err(),
        "a compliance callback that reenters treasury mid-check should abort the whole call"
    );

    // Soroban rolls back every storage write made during a failed top-level
    // invocation, including those made by the nested reentrant call — so the
    // settlement is untouched and no funds moved, despite the reentrant
    // execute_settlement having run to completion inside is_allowed.
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&treasury_id),
        10_000_000
    );
    assert_eq!(
        treasury.get_settlement(&settlement_id).status,
        SettlementStatus::Pending
    );
}

#[test]
fn reentrant_compliance_deny_callback_also_reverts_atomically() {
    // Same reentrant callback, but is_allowed ultimately denies the
    // merchant. execute_with_compliance short-circuits with
    // ComplianceCheckFailed before ever reaching its own execute_settlement
    // call, so there's no second call to panic against this time — but the
    // reentrant execute_settlement performed during the check is still
    // rolled back along with everything else once the outer call errors.
    let env = Env::default();
    let (_admin, merchant, treasury, treasury_id, workflow, workflow_id, token_id, settlement_id) =
        setup_reentrancy_fixture(&env);

    let malicious_compliance_id = env.register_contract(None, MaliciousCompliance);
    let malicious_compliance = MaliciousComplianceClient::new(&env, &malicious_compliance_id);
    malicious_compliance.set_reentry_target(&treasury_id, &workflow_id, &settlement_id, &token_id);
    malicious_compliance.set_verdict(&false);

    let err = workflow
        .try_execute_with_compliance(
            &malicious_compliance_id,
            &treasury_id,
            &settlement_id,
            &token_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed);

    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&treasury_id),
        10_000_000
    );
    assert_eq!(
        treasury.get_settlement(&settlement_id).status,
        SettlementStatus::Pending
    );
}
