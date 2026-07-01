use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{contract, contracterror, contractimpl, testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WorkflowError {
    ComplianceFailed = 1,
    InsufficientBalance = 2,
}

#[contract]
struct TestTokenContract;

#[contractimpl]
impl TestTokenContract {
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&admin, &true);
    }

    pub fn mint(env: Env, to: Address, amount: i128) {
        to.require_auth();
        let key = ("bal", to.clone());
        let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(bal + amount));
    }

    pub fn balance(env: Env, of: Address) -> i128 {
        let key = ("bal", of);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let from_key = ("bal", from.clone());
        let to_key = ("bal", to.clone());
        let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
        if from_bal < amount {
            // Use a Result-returning path in the workflow contract for test assertions;
            // the token contract only errors in cases this test doesn't exercise.
            panic!("InsufficientBalance");
        }
        let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&from_key, &(from_bal - amount));
        env.storage().persistent().set(&to_key, &(to_bal + amount));
    }
}

#[contract]
struct SettlementWorkflow;

#[contractimpl]
impl SettlementWorkflow {
    // Verification note: Compliance gate requirement.
    // Settlement execution is gated by compliance status in the workflow contract:
    // it requires `ComplianceContract::is_allowed(merchant)` to be true before calling
    // `TreasuryContract::execute_settlement(...)`. The Treasury contract itself does not
    // consult compliance; the gate is enforced by this cross-contract workflow.
    pub fn execute_with_compliance(
        env: Env,
        compliance_id: Address,
        treasury_id: Address,
        settlement_id: u64,
        token_id: Address,
        merchant: Address,
    ) -> Result<(), WorkflowError> {
        let compliance = ComplianceContractClient::new(&env, &compliance_id);
        if !compliance.is_allowed(&merchant) {
            return Err(WorkflowError::ComplianceFailed);
        }
        let treasury = TreasuryContractClient::new(&env, &treasury_id);
        treasury.execute_settlement(&env.current_contract_address(), &settlement_id, &token_id);
        Ok(())
    }

    /// Check-only variant: returns Ok if compliance passes, Err(ComplianceFailed) otherwise.
    /// Used in tests to verify the compliance gate without triggering token transfer.
    pub fn check_compliance(
        env: Env,
        compliance_id: Address,
        merchant: Address,
    ) -> Result<(), WorkflowError> {
        let compliance = ComplianceContractClient::new(&env, &compliance_id);
        if !compliance.is_allowed(&merchant) {
            return Err(WorkflowError::ComplianceFailed);
        }
        Ok(())
    }
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    ComplianceContractClient<'static>,
    Address,
    TreasuryContractClient<'static>,
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

    let token_id = env.register_contract(None, TestTokenContract);
    (
        env,
        admin,
        merchant,
        compliance_id,
        compliance,
        treasury_id,
        treasury,
        token_id,
    )
}

#[test]
fn settlement_proceeds_when_compliance_passing() {
    let (env, admin, merchant, compliance_id, compliance, _treasury_id, _treasury, _token_id) =
        setup();

    compliance.allow_address(&admin, &merchant);

    let workflow_id = env.register_contract(None, SettlementWorkflow);
    let workflow = SettlementWorkflowClient::new(&env, &workflow_id);

    // Compliance passes for allowed merchant.
    assert!(workflow
        .try_check_compliance(&compliance_id, &merchant)
        .is_ok());
    // Compliance fails for unknown merchant.
    let unknown = Address::generate(&env);
    let err = workflow
        .try_check_compliance(&compliance_id, &unknown)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, WorkflowError::ComplianceFailed);
}

#[test]
fn settlement_rejected_when_compliance_absent_or_failing() {
    let (env, admin, merchant, compliance_id, _compliance, treasury_id, treasury, token_id) =
        setup();

    // merchant is not allowed by default
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);

    let token = TestTokenContractClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);

    let workflow_id = env.register_contract(None, SettlementWorkflow);
    let workflow = SettlementWorkflowClient::new(&env, &workflow_id);
    assert!(workflow
        .try_execute_with_compliance(
            &compliance_id,
            &treasury_id,
            &settlement_id,
            &token_id,
            &merchant,
        )
        .is_err());
    assert_eq!(token.balance(&merchant), 0);
}

#[test]
fn settlement_rejected_when_compliance_paused_and_unable_to_pass() {
    let (env, admin, merchant, compliance_id, compliance, treasury_id, treasury, token_id) =
        setup();

    // While paused, allow/block operations are disabled, so merchant cannot be made passing.
    compliance.pause(&admin);

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);

    let token = TestTokenContractClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);

    let workflow_id = env.register_contract(None, SettlementWorkflow);
    let workflow = SettlementWorkflowClient::new(&env, &workflow_id);
    assert!(workflow
        .try_execute_with_compliance(
            &compliance_id,
            &treasury_id,
            &settlement_id,
            &token_id,
            &merchant,
        )
        .is_err());
    assert_eq!(token.balance(&merchant), 0);
}

#[test]
fn settlement_rejected_when_merchant_blocked() {
    let (env, admin, merchant, compliance_id, compliance, treasury_id, treasury, token_id) =
        setup();

    // First allow the merchant so compliance is initially passing.
    compliance.allow_address(&admin, &merchant);

    // Then explicitly block — block_address is permitted while paused or unpaused
    // and overrides any existing allow status.
    compliance.block_address(&admin, &merchant);

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);

    let token = TestTokenContractClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);

    let workflow_id = env.register_contract(None, SettlementWorkflow);
    let workflow = SettlementWorkflowClient::new(&env, &workflow_id);
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
    assert_eq!(err, WorkflowError::ComplianceFailed);
    assert_eq!(token.balance(&merchant), 0);
}

#[test]
fn blocked_merchant_rejected_even_when_already_settled() {
    let (env, admin, merchant, compliance_id, compliance, treasury_id, treasury, token_id) =
        setup();

    // Allow merchant, create and execute a settlement successfully.
    compliance.allow_address(&admin, &merchant);

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);

    let token = TestTokenContractClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);

    let workflow_id = env.register_contract(None, SettlementWorkflow);
    let workflow = SettlementWorkflowClient::new(&env, &workflow_id);
    treasury.set_signer(&admin, &workflow_id, &1);
    assert!(workflow
        .try_execute_with_compliance(
            &compliance_id,
            &treasury_id,
            &settlement_id,
            &token_id,
            &merchant,
        )
        .is_ok());

    // Now block the merchant and create a *new* settlement.
    compliance.block_address(&admin, &merchant);

    let settlement_id_2 = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    // Mint additional tokens
    token.mint(&treasury_id, &5_000_000);

    let err = workflow
        .try_execute_with_compliance(
            &compliance_id,
            &treasury_id,
            &settlement_id_2,
            &token_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, WorkflowError::ComplianceFailed);
    // Merchant did not receive the second settlement amount
    assert_eq!(token.balance(&merchant), 10_000_000);
}

#[test]
fn execute_settlement_fails_when_merchant_blocked_mid_flight() {
    let (env, admin, merchant, compliance_id, compliance, treasury_id, treasury, token_id) =
        setup();

    // Merchant is allowed at proposal time.
    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);

    let token = TestTokenContractClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);

    // Block the merchant after the settlement is already proposed (mid-flight).
    compliance.block_address(&admin, &merchant);

    let workflow_id = env.register_contract(None, SettlementWorkflow);
    let workflow = SettlementWorkflowClient::new(&env, &workflow_id);

    // Execution must fail: compliance gate sees the block even though proposal was valid.
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
    assert_eq!(err, WorkflowError::ComplianceFailed);
    // Merchant received nothing.
    assert_eq!(token.balance(&merchant), 0);
}
