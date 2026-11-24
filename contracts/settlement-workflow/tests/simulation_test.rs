//! Tests for `simulate_with_compliance`, the read-only dry run (#618).
//!
//! The point of the simulation is that it agrees with the real execution path:
//! when it says a settlement would succeed, executing it succeeds; when it
//! reports a failing check, executing it fails for that same reason.

use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{
    SettlementWorkflowContract, SettlementWorkflowContractClient, SimulationFailure,
    SimulationResult,
};
use soroban_sdk::{testutils::Address as _, token, Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient};

struct Setup {
    env: Env,
    admin: Address,
    merchant: Address,
    cosigner: Address,
    compliance: ComplianceContractClient<'static>,
    treasury: TreasuryContractClient<'static>,
    treasury_id: Address,
    workflow: SettlementWorkflowContractClient<'static>,
    token_id: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let cosigner = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    let mut signers = Vec::new(&env);
    signers.push_back((admin.clone(), 1u32));
    signers.push_back((cosigner.clone(), 1u32));
    treasury.initialize(&admin, &2, &signers);

    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    workflow.initialize(&admin, &compliance_id, &treasury_id);
    treasury.set_signer(&admin, &workflow_id, &1);

    let token_id = env.register_stellar_asset_contract(admin.clone());

    Setup {
        env,
        admin,
        merchant,
        cosigner,
        compliance,
        treasury,
        treasury_id,
        workflow,
        token_id,
    }
}

#[test]
fn simulation_reports_compliance_failure_and_execution_agrees() {
    let s = setup();

    // Merchant is not allowlisted.
    let settlement = s
        .treasury
        .propose_settlement(&s.admin, &s.merchant, &10_000_000, &0_u64);

    assert_eq!(
        s.workflow
            .simulate_with_compliance(&settlement, &s.token_id, &s.merchant),
        SimulationResult::WouldFail(SimulationFailure::ComplianceCheckFailed)
    );

    // The real path fails for the same reason.
    let err = s
        .workflow
        .try_execute_with_compliance(&settlement, &s.token_id, &s.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        settlement_workflow::TreasuryError::ComplianceCheckFailed
    );
}

#[test]
fn simulation_reports_treasury_rejection_for_unknown_settlement() {
    let s = setup();

    s.compliance.allow_address(&s.admin, &s.merchant);
    let unknown: u64 = 4_242;

    assert_eq!(
        s.workflow
            .simulate_with_compliance(&unknown, &s.token_id, &s.merchant),
        SimulationResult::WouldFail(SimulationFailure::TreasuryWouldReject)
    );

    let err = s
        .workflow
        .try_execute_with_compliance(&unknown, &s.token_id, &s.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, settlement_workflow::TreasuryError::SettlementNotFound);
}

#[test]
fn simulation_reports_treasury_rejection_for_already_executed_settlement() {
    let s = setup();

    s.compliance.allow_address(&s.admin, &s.merchant);
    let settlement = s
        .treasury
        .propose_settlement(&s.admin, &s.merchant, &10_000_000, &0_u64);
    s.treasury.approve_settlement(&s.admin, &settlement);
    s.treasury.approve_settlement(&s.cosigner, &settlement);
    token::StellarAssetClient::new(&s.env, &s.token_id).mint(&s.treasury_id, &10_000_000);
    s.workflow
        .execute_with_compliance(&settlement, &s.token_id, &s.merchant);

    // Now already executed, the dry run must report a rejection.
    assert_eq!(
        s.workflow
            .simulate_with_compliance(&settlement, &s.token_id, &s.merchant),
        SimulationResult::WouldFail(SimulationFailure::TreasuryWouldReject)
    );
}

#[test]
fn simulation_reports_success_for_a_ready_settlement() {
    let s = setup();

    s.compliance.allow_address(&s.admin, &s.merchant);
    let settlement = s
        .treasury
        .propose_settlement(&s.admin, &s.merchant, &10_000_000, &0_u64);
    s.treasury.approve_settlement(&s.admin, &settlement);
    s.treasury.approve_settlement(&s.cosigner, &settlement);
    token::StellarAssetClient::new(&s.env, &s.token_id).mint(&s.treasury_id, &10_000_000);

    assert_eq!(
        s.workflow
            .simulate_with_compliance(&settlement, &s.token_id, &s.merchant),
        SimulationResult::WouldSucceed
    );

    // And the real path then succeeds, paying the merchant.
    s.workflow
        .execute_with_compliance(&settlement, &s.token_id, &s.merchant);
    assert_eq!(
        token::Client::new(&s.env, &s.token_id).balance(&s.merchant),
        10_000_000
    );
}

#[test]
fn simulation_makes_no_state_changes() {
    let s = setup();

    s.compliance.allow_address(&s.admin, &s.merchant);
    let settlement = s
        .treasury
        .propose_settlement(&s.admin, &s.merchant, &10_000_000, &0_u64);
    s.treasury.approve_settlement(&s.admin, &settlement);
    s.treasury.approve_settlement(&s.cosigner, &settlement);
    token::StellarAssetClient::new(&s.env, &s.token_id).mint(&s.treasury_id, &10_000_000);

    let balance_before = token::Client::new(&s.env, &s.token_id).balance(&s.merchant);

    // Simulating twice must be idempotent and must not move funds or settle.
    let first = s
        .workflow
        .simulate_with_compliance(&settlement, &s.token_id, &s.merchant);
    let second = s
        .workflow
        .simulate_with_compliance(&settlement, &s.token_id, &s.merchant);

    assert_eq!(first, SimulationResult::WouldSucceed);
    assert_eq!(first, second);
    assert_eq!(
        token::Client::new(&s.env, &s.token_id).balance(&s.merchant),
        balance_before,
        "simulation must not move funds"
    );
    // The settlement is still pending, so it can still be executed for real.
    assert_eq!(
        s.treasury.get_settlement(&settlement).status,
        settlement_workflow::SettlementStatus::Pending
    );
}
