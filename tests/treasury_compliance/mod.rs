//! Cross-contract tests: Compliance contract gates Treasury settlement.
//!
//! A workflow contract checks `ComplianceContractClient::is_allowed(merchant)`
//! before forwarding a settlement proposal to the treasury.  Merchants that are
//! absent from, or explicitly blocked on, the compliance allowlist must be
//! rejected without touching the treasury.

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    contract, contracterror, contractimpl, testutils::Address as _, Address, Env,
};
use treasury::{TreasuryContract, TreasuryContractClient};

extern crate std;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TreasuryComplianceError {
    ComplianceDenied = 1,
}

/// Workflow contract: checks compliance allowlist before proposing a settlement.
#[contract]
pub struct TreasuryComplianceWorkflow;

#[contractimpl]
impl TreasuryComplianceWorkflow {
    pub fn check_and_propose(
        env: Env,
        compliance_contract: Address,
        treasury_contract: Address,
        merchant: Address,
        amount: i128,
    ) -> Result<u64, TreasuryComplianceError> {
        if !ComplianceContractClient::new(&env, &compliance_contract).is_allowed(&merchant) {
            return Err(TreasuryComplianceError::ComplianceDenied);
        }
        let signer = env.current_contract_address();
        Ok(TreasuryContractClient::new(&env, &treasury_contract)
            .propose_settlement(&signer, &merchant, &amount))
    }
}

type Setup = (
    Env,
    Address, // admin
    Address, // merchant
    Address, // compliance_contract_id
    ComplianceContractClient<'static>,
    Address, // treasury_contract_id
    TreasuryContractClient<'static>,
    Address, // workflow_id
);

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    let workflow_id = env.register_contract(None, TreasuryComplianceWorkflow);

    let compliance_contract_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_contract_id);
    compliance.initialize(&admin);

    let treasury_contract_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_contract_id);
    treasury.initialize(&admin, &1);
    treasury.set_signer(&admin, &workflow_id, &1);

    (
        env,
        admin,
        merchant,
        compliance_contract_id,
        compliance,
        treasury_contract_id,
        treasury,
        workflow_id,
    )
}

#[test]
fn allowed_merchant_settlement_proceeds() {
    let (env, admin, merchant, compliance_cid, compliance, treasury_cid, _treasury, wf_id) =
        setup();
    compliance.allow_address(&admin, &merchant);
    assert!(compliance.is_allowed(&merchant));

    let result = TreasuryComplianceWorkflowClient::new(&env, &wf_id)
        .try_check_and_propose(&compliance_cid, &treasury_cid, &merchant, &10_000_000);
    assert!(result.is_ok(), "allowed merchant must proceed to settlement proposal");
}

#[test]
fn unknown_merchant_settlement_blocked() {
    let (env, _admin, merchant, compliance_cid, _compliance, treasury_cid, _treasury, wf_id) =
        setup();
    // merchant was never added to the allowlist
    let err = TreasuryComplianceWorkflowClient::new(&env, &wf_id)
        .try_check_and_propose(&compliance_cid, &treasury_cid, &merchant, &10_000_000)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryComplianceError::ComplianceDenied);
}

#[test]
fn blocked_merchant_settlement_rejected() {
    let (env, admin, merchant, compliance_cid, compliance, treasury_cid, _treasury, wf_id) =
        setup();
    compliance.allow_address(&admin, &merchant);
    compliance.block_address(&admin, &merchant);
    assert!(!compliance.is_allowed(&merchant));

    let err = TreasuryComplianceWorkflowClient::new(&env, &wf_id)
        .try_check_and_propose(&compliance_cid, &treasury_cid, &merchant, &10_000_000)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryComplianceError::ComplianceDenied);
}

#[test]
fn paused_compliance_blocks_settlement_when_merchant_not_yet_allowed() {
    let (env, admin, merchant, compliance_cid, compliance, treasury_cid, _treasury, wf_id) =
        setup();
    // Pausing prevents allow_address from being called; merchant stays denied.
    compliance.pause(&admin);

    let err = TreasuryComplianceWorkflowClient::new(&env, &wf_id)
        .try_check_and_propose(&compliance_cid, &treasury_cid, &merchant, &10_000_000)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryComplianceError::ComplianceDenied);
}
