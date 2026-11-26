//! Execution-agreement tests for `settlement-workflow`: verifies compliance
//! rejection, unknown settlement rejection, already-executed rejection, and
//! successful execution without leaking state on failure.

use compliance::{ComplianceContract, ComplianceContractClient};
use multisig::SettlementStatus;
use settlement_workflow::{
    SettlementWorkflowContract, SettlementWorkflowContractClient, TreasuryError,
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

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register(TreasuryContract, ());
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    let mut signers = Vec::new(&env);
    signers.push_back((admin.clone(), 1u32));
    signers.push_back((cosigner.clone(), 1u32));
    treasury.initialize(&admin, &2, &signers);

    let workflow_id = env.register(
        SettlementWorkflowContract,
        (admin.clone(), compliance_id.clone(), treasury_id.clone()),
    );
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    treasury.set_signer(&admin, &workflow_id, &1);

    let token_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

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
fn execution_rejects_unlisted_merchant_without_moving_funds() {
    let s = setup();
    let settlement = s
        .treasury
        .propose_settlement(&s.admin, &s.merchant, &10_000_000, &0_u64);

    let err = s
        .workflow
        .try_execute_with_compliance(&settlement, &s.token_id, &s.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed);
    assert_eq!(
        s.treasury.get_settlement(&settlement).status,
        SettlementStatus::Pending
    );
}

#[test]
fn execution_rejects_unknown_settlement() {
    let s = setup();
    s.compliance.allow_address(&s.admin, &s.merchant);
    let unknown: u64 = 4_242;

    let err = s
        .workflow
        .try_execute_with_compliance(&unknown, &s.token_id, &s.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::SettlementNotFound);
}

#[test]
fn execution_succeeds_once_and_rejects_replay() {
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
    assert_eq!(
        token::Client::new(&s.env, &s.token_id).balance(&s.merchant),
        10_000_000
    );
    assert_eq!(
        s.treasury.get_settlement(&settlement).status,
        SettlementStatus::Executed
    );

    let replay = s
        .workflow
        .try_execute_with_compliance(&settlement, &s.token_id, &s.merchant);
    assert!(replay.is_err());
}
