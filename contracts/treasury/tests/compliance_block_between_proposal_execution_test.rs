/// Integration test for issue #275.
///
/// Scenario: compliance block applied *between* proposal and execution.
///
/// This is the cross-crate proof that the compliance gate correctly intercepts a
/// settlement whose merchant was *compliant at proposal time* but is *blocked
/// (or never allowed) at execution time*.  The treasury contract itself is
/// agnostic to compliance; the gate is enforced by a thin workflow contract that
/// calls `ComplianceContract::is_allowed` before forwarding to
/// `TreasuryContract::execute_settlement`.  Both contracts are deployed as
/// separate on-chain instances in the Soroban test environment, which is the
/// distinction from the treasury-internal unit tests.
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{contract, contracterror, contractimpl, testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

// ─── Error type for the workflow ─────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WorkflowError {
    /// Compliance gate rejected the merchant.
    ComplianceFailed = 1,
}

// ─── Minimal token stub ───────────────────────────────────────────────────────

#[contract]
struct StubToken;

#[contractimpl]
impl StubToken {
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {
        // no-op: we only care about settlement state transitions in these tests
    }
}

// ─── Compliance-gated settlement workflow ─────────────────────────────────────

#[contract]
struct ComplianceGatedWorkflow;

#[contractimpl]
impl ComplianceGatedWorkflow {
    /// Execute a settlement only when the merchant passes the compliance check.
    /// This is the cross-crate gate: treasury does not call compliance; the
    /// workflow contract does, then delegates to treasury on success.
    pub fn execute_if_compliant(
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
}

// ─── Shared setup ─────────────────────────────────────────────────────────────

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    compliance_id: Address,
    compliance: ComplianceContractClient<'static>,
    treasury_id: Address,
    treasury: TreasuryContractClient<'static>,
    token_id: Address,
    workflow_id: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    // Deploy compliance contract
    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    // Deploy treasury contract; threshold=1 so the proposer's single vote is enough
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    // Deploy stub token
    let token_id = env.register_contract(None, StubToken);

    // Deploy the workflow contract and register it as a treasury signer
    let workflow_id = env.register_contract(None, ComplianceGatedWorkflow);
    treasury.set_signer(&admin, &workflow_id, &1);

    Fixture {
        env,
        admin,
        merchant,
        compliance_id,
        compliance,
        treasury_id,
        treasury,
        token_id,
        workflow_id,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Baseline: merchant allowed at proposal time and still allowed at execution —
/// settlement executes successfully.
#[test]
fn execution_succeeds_when_merchant_allowed_throughout() {
    let f = setup();

    // Allow merchant before proposal
    f.compliance.allow_address(&f.admin, &f.merchant);

    // Propose settlement while compliant
    let sid = f
        .treasury
        .propose_settlement(&f.admin, &f.merchant, &10_000_000);

    // Execute via the compliance-gated workflow — merchant still allowed
    let workflow = ComplianceGatedWorkflowClient::new(&f.env, &f.workflow_id);
    let result = workflow.try_execute_if_compliant(
        &f.compliance_id,
        &f.treasury_id,
        &sid,
        &f.token_id,
        &f.merchant,
    );
    assert!(
        result.is_ok(),
        "expected execution to succeed for allowed merchant"
    );

    // Confirm the treasury records the settlement as Executed
    let settlement = f.treasury.get_settlement(&sid);
    assert_eq!(
        settlement.status,
        SettlementStatus::Executed,
        "settlement must be Executed after successful workflow call"
    );
}

/// Core scenario for #275: merchant is *blocked between proposal and execution*.
/// The workflow must return ComplianceFailed and the settlement must remain Pending.
#[test]
fn execution_blocked_when_merchant_blocked_after_proposal() {
    let f = setup();

    // Allow merchant so they can be proposed
    f.compliance.allow_address(&f.admin, &f.merchant);

    // Propose the settlement while merchant is compliant
    let sid = f
        .treasury
        .propose_settlement(&f.admin, &f.merchant, &10_000_000);

    // Simulate a compliance event: merchant is blocked *after* proposal
    f.compliance.block_address(&f.admin, &f.merchant, &None);
    assert!(
        !f.compliance.is_allowed(&f.merchant),
        "merchant must be blocked before execution attempt"
    );

    // Attempt to execute — the gate must reject
    let workflow = ComplianceGatedWorkflowClient::new(&f.env, &f.workflow_id);
    let err = workflow
        .try_execute_if_compliant(
            &f.compliance_id,
            &f.treasury_id,
            &sid,
            &f.token_id,
            &f.merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        WorkflowError::ComplianceFailed,
        "workflow must return ComplianceFailed when merchant is blocked at execution time"
    );

    // Treasury settlement must still be Pending — no stuck/inconsistent state
    let settlement = f.treasury.get_settlement(&sid);
    assert_eq!(
        settlement.status,
        SettlementStatus::Pending,
        "settlement must remain Pending after a blocked execution attempt"
    );
}

/// Merchant was never allowed in compliance; proposal goes through (treasury
/// does not consult compliance), but execution is blocked.
#[test]
fn execution_blocked_when_merchant_never_allowed() {
    let f = setup();

    // merchant is NOT allowed — propose anyway (treasury doesn't gate on compliance)
    let sid = f
        .treasury
        .propose_settlement(&f.admin, &f.merchant, &5_000_000);

    let workflow = ComplianceGatedWorkflowClient::new(&f.env, &f.workflow_id);
    let err = workflow
        .try_execute_if_compliant(
            &f.compliance_id,
            &f.treasury_id,
            &sid,
            &f.token_id,
            &f.merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, WorkflowError::ComplianceFailed);

    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}

/// After a block, the admin clears the merchant and execution succeeds — confirms
/// that the gate is stateless (re-reads compliance on every call) and recovery works.
#[test]
fn execution_succeeds_after_block_is_cleared() {
    let f = setup();

    f.compliance.allow_address(&f.admin, &f.merchant);
    let sid = f
        .treasury
        .propose_settlement(&f.admin, &f.merchant, &10_000_000);

    // Block mid-flight
    f.compliance.block_address(&f.admin, &f.merchant, &None);

    // First attempt fails
    let workflow = ComplianceGatedWorkflowClient::new(&f.env, &f.workflow_id);
    assert!(workflow
        .try_execute_if_compliant(
            &f.compliance_id,
            &f.treasury_id,
            &sid,
            &f.token_id,
            &f.merchant,
        )
        .is_err());

    // Admin clears the block
    f.compliance.clear_address(&f.admin, &f.merchant);
    assert!(f.compliance.is_allowed(&f.merchant));

    // Second attempt succeeds
    let result = workflow.try_execute_if_compliant(
        &f.compliance_id,
        &f.treasury_id,
        &sid,
        &f.token_id,
        &f.merchant,
    );
    assert!(
        result.is_ok(),
        "execution must succeed after block is cleared"
    );

    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Executed
    );
}

/// Multiple settlements: one merchant compliant, one blocked.  Confirms the gate
/// is per-merchant, not a global switch.
#[test]
fn compliance_gate_is_per_merchant() {
    let f = setup();

    let merchant_b = Address::generate(&f.env);

    // Allow only merchant A
    f.compliance.allow_address(&f.admin, &f.merchant);

    let sid_a = f
        .treasury
        .propose_settlement(&f.admin, &f.merchant, &10_000_000);
    let sid_b = f
        .treasury
        .propose_settlement(&f.admin, &merchant_b, &10_000_000);

    let workflow = ComplianceGatedWorkflowClient::new(&f.env, &f.workflow_id);

    // Merchant A executes
    assert!(workflow
        .try_execute_if_compliant(
            &f.compliance_id,
            &f.treasury_id,
            &sid_a,
            &f.token_id,
            &f.merchant,
        )
        .is_ok());

    // Merchant B is blocked
    let err = workflow
        .try_execute_if_compliant(
            &f.compliance_id,
            &f.treasury_id,
            &sid_b,
            &f.token_id,
            &merchant_b,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, WorkflowError::ComplianceFailed);

    assert_eq!(
        f.treasury.get_settlement(&sid_a).status,
        SettlementStatus::Executed
    );
    assert_eq!(
        f.treasury.get_settlement(&sid_b).status,
        SettlementStatus::Pending
    );
}
