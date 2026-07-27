/// Integration tests for issue #276.
///
/// Extends pause-blocking coverage to the full cross-contract lifecycle.
///
/// Three sub-scenarios prove that pausing *any one* of the three deployed
/// contracts mid-lifecycle produces a clean, well-typed failure rather than a
/// stuck or inconsistent state across the others:
///
///   1. Invoice paused mid-flow  — `mark_paid` blocked; treasury is unaffected
///   2. Treasury paused mid-flow — `propose_settlement` / `execute_settlement`
///                                  blocked (panics with "ContractPaused");
///                                  invoice and compliance are unaffected
///   3. Compliance paused mid-flow — `allow_address` blocked so the compliance
///                                    gate always rejects; treasury settlement
///                                    remains Pending, invoice remains Paid
///
/// Each sub-scenario verifies:
///   * The paused contract returns the correct error (typed or panic message)
///   * All *other* contracts remain in their last known good state
///   * After unpause the blocked operation succeeds (recovery path)
///
/// ### Notes on treasury error handling
/// The treasury contract is `#[no_std]` and uses `panic!()` (abort-on-panic)
/// for error paths.  `try_*` client wrappers cannot catch abort panics, so
/// treasury pause-rejection is verified via `#[should_panic(expected = …)]`.
/// Invoice and compliance use `Result`-returning functions and are verified via
/// `try_*` wrappers as usual.
use compliance::{ComplianceContract, ComplianceContractClient};
use invoice::{InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeBytes};
use soroban_sdk::{
    contract, contracterror, contractimpl,
    testutils::Address as _,
    Address, Env,
};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

// ─── Workflow error type shared across scenarios ──────────────────────────────

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
        // no-op: token transfers are not the focus of pause tests
    }
}

// ─── Compliance-gated settlement workflow ─────────────────────────────────────

#[contract]
struct PauseTestWorkflow;

#[contractimpl]
impl PauseTestWorkflow {
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

// ─── Shared test fixture ──────────────────────────────────────────────────────

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    #[allow(dead_code)]
    invoice_id: Address,
    invoice: InvoiceContractClient<'static>,
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

    // Invoice contract
    let invoice_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);

    // Compliance contract
    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    // Treasury contract (threshold=1 so single admin approval is sufficient)
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1);

    // Stub token
    let token_id = env.register_contract(None, StubToken);

    // Workflow contract registered as a treasury signer
    let workflow_id = env.register_contract(None, PauseTestWorkflow);
    treasury.set_signer(&admin, &workflow_id, &1);

    Fixture {
        env,
        admin,
        merchant,
        invoice_id,
        invoice,
        compliance_id,
        compliance,
        treasury_id,
        treasury,
        token_id,
        workflow_id,
    }
}

/// Helper: create a minimal pending invoice.
fn create_pending_invoice(f: &Fixture) -> u64 {
    f.invoice.create_invoice(
        &f.merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sub-scenario 1 — Invoice paused mid-flow
// ═══════════════════════════════════════════════════════════════════════════════

/// Pausing the invoice contract mid-flow blocks `mark_paid` with a well-typed
/// `InvoiceError::ContractPaused` error.  The pending invoice retains its
/// Pending status; treasury and compliance are completely unaffected.
#[test]
fn scenario_invoice_paused_blocks_mark_paid_cleanly() {
    let f = setup();
    let payer = Address::generate(&f.env);

    // Step 1 – create invoice (succeeds; contract not yet paused)
    let inv_id = create_pending_invoice(&f);
    assert_eq!(f.invoice.get_invoice(&inv_id).status, InvoiceStatus::Pending);

    // Step 2 – allow merchant in compliance (unrelated to invoice pause)
    f.compliance.allow_address(&f.admin, &f.merchant);

    // Step 3 – propose a treasury settlement (also unaffected by invoice state)
    let sid = f.treasury.propose_settlement(&f.admin, &f.merchant, &10_000_000);
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Pending
    );

    // Step 4 – PAUSE invoice mid-flow
    let pause_result = f.invoice.try_pause(&f.admin);
    assert!(pause_result.is_ok(), "admin must be able to pause invoice");

    // Step 5 – mark_paid must fail with ContractPaused
    let err = f
        .invoice
        .try_mark_paid(&f.admin, &inv_id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        InvoiceError::ContractPaused,
        "mark_paid must return ContractPaused when invoice contract is paused"
    );

    // Step 6 – invoice stays Pending (no stuck/inconsistent state)
    assert_eq!(
        f.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Pending,
        "invoice must remain Pending after a blocked mark_paid"
    );

    // Step 7 – treasury and compliance are unaffected by invoice pause
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Pending,
        "treasury settlement must be unaffected by invoice pause"
    );
    assert!(
        f.compliance.is_allowed(&f.merchant),
        "compliance state must be unaffected by invoice pause"
    );

    // Step 8 – unpause and confirm recovery
    f.invoice.unpause(&f.admin);
    let mark_result = f.invoice.try_mark_paid(&f.admin, &inv_id, &payer);
    assert!(mark_result.is_ok(), "mark_paid must succeed after invoice is unpaused");
    assert_eq!(f.invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);
}

/// Creating a new invoice while the contract is paused is also blocked with the
/// correct typed error.
#[test]
fn scenario_invoice_paused_blocks_create_invoice() {
    let f = setup();

    f.invoice.pause(&f.admin);

    let err = f
        .invoice
        .try_create_invoice(
            &f.merchant,
            &10_000_000,
            &10_250_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        InvoiceError::ContractPaused,
        "create_invoice must return ContractPaused when invoice is paused"
    );
}

/// After unpausing invoice, the full lifecycle (create → mark_paid → propose
/// settlement) completes without errors; other contracts are always unaffected.
#[test]
fn scenario_invoice_pause_unpause_full_lifecycle_recovers() {
    let f = setup();
    let payer = Address::generate(&f.env);

    f.compliance.allow_address(&f.admin, &f.merchant);
    let inv_id = create_pending_invoice(&f);

    // Pause and confirm mark_paid is blocked
    f.invoice.pause(&f.admin);
    assert!(f.invoice.try_mark_paid(&f.admin, &inv_id, &payer).is_err());

    // Compliance and treasury remain operational
    let sid = f.treasury.propose_settlement(&f.admin, &f.merchant, &10_000_000);
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Pending
    );

    // Unpause invoice; full lifecycle completes
    f.invoice.unpause(&f.admin);
    f.invoice.mark_paid(&f.admin, &inv_id, &payer);
    assert_eq!(f.invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    // Treasury execution also succeeds
    let workflow = PauseTestWorkflowClient::new(&f.env, &f.workflow_id);
    assert!(workflow
        .try_execute_if_compliant(
            &f.compliance_id,
            &f.treasury_id,
            &sid,
            &f.token_id,
            &f.merchant,
        )
        .is_ok());
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Executed
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sub-scenario 2 — Treasury paused mid-flow
// ═══════════════════════════════════════════════════════════════════════════════
//
// Treasury uses non-unwinding panic!() for error paths (no_std).  The Soroban
// testutils sandbox traps the abort so tests don't crash the process, but the
// `try_*` wrappers cannot return a typed error — they re-abort.  Therefore
// treasury pause-rejection tests use `#[should_panic(expected = "ContractPaused")]`.

/// Pausing the treasury mid-flow panics with "ContractPaused" when
/// `propose_settlement` is called.
#[test]
#[should_panic(expected = "ContractPaused")]
fn scenario_treasury_paused_blocks_propose_settlement() {
    let f = setup();
    f.treasury.pause(&f.admin);
    // This must panic with "ContractPaused"
    f.treasury.propose_settlement(&f.admin, &f.merchant, &10_000_000);
}

/// Pausing treasury *after* proposal panics with "ContractPaused" on execute.
#[test]
#[should_panic(expected = "ContractPaused")]
fn scenario_treasury_paused_blocks_execute_settlement() {
    let f = setup();
    f.compliance.allow_address(&f.admin, &f.merchant);
    let sid = f.treasury.propose_settlement(&f.admin, &f.merchant, &10_000_000);
    // Pause AFTER proposal
    f.treasury.pause(&f.admin);
    // This must panic with "ContractPaused"
    f.treasury.execute_settlement(&f.admin, &sid, &f.token_id);
}

/// Invoice and compliance continue to work normally when treasury is paused.
/// This is the cross-contract isolation proof for sub-scenario 2.
#[test]
fn scenario_treasury_paused_invoice_and_compliance_unaffected() {
    let f = setup();
    let payer = Address::generate(&f.env);

    // Pre-condition: invoice and compliance work before pause
    let inv_id = create_pending_invoice(&f);
    f.compliance.allow_address(&f.admin, &f.merchant);

    // Pause treasury
    f.treasury.pause(&f.admin);

    // Invoice operations are unaffected — mark_paid succeeds
    let mark_result = f.invoice.try_mark_paid(&f.admin, &inv_id, &payer);
    assert!(
        mark_result.is_ok(),
        "mark_paid must succeed when only treasury is paused"
    );
    assert_eq!(f.invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    // Compliance read is unaffected
    assert!(
        f.compliance.is_allowed(&f.merchant),
        "compliance is_allowed must work when treasury is paused"
    );

    // Invoice escrow-release also works
    let release_result = f.invoice.try_release_escrow(&f.admin, &inv_id);
    assert!(
        release_result.is_ok(),
        "release_escrow must succeed when only treasury is paused"
    );
    assert_eq!(f.invoice.get_invoice(&inv_id).status, InvoiceStatus::Released);
}

/// After unpausing treasury, propose + execute succeed; confirms full recovery.
#[test]
fn scenario_treasury_pause_unpause_full_lifecycle_recovers() {
    let f = setup();

    f.compliance.allow_address(&f.admin, &f.merchant);

    // Pause treasury before any settlement
    f.treasury.pause(&f.admin);

    // Invoice and compliance remain usable
    let inv_id = create_pending_invoice(&f);
    let payer = Address::generate(&f.env);
    f.invoice.mark_paid(&f.admin, &inv_id, &payer);
    assert_eq!(f.invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    // Unpause treasury
    f.treasury.unpause(&f.admin);

    // Now propose and execute succeed
    let sid = f.treasury.propose_settlement(&f.admin, &f.merchant, &10_000_000);
    let workflow = PauseTestWorkflowClient::new(&f.env, &f.workflow_id);
    assert!(workflow
        .try_execute_if_compliant(
            &f.compliance_id,
            &f.treasury_id,
            &sid,
            &f.token_id,
            &f.merchant,
        )
        .is_ok());
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Executed
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sub-scenario 3 — Compliance paused mid-flow
// ═══════════════════════════════════════════════════════════════════════════════

/// Pausing compliance mid-flow freezes `allow_address` with
/// `ContractError::ContractPaused`.  A merchant who was not pre-allowed is
/// therefore rejected by the compliance gate, leaving the treasury settlement
/// Pending and the invoice in whatever state it was.
#[test]
fn scenario_compliance_paused_blocks_new_allow_so_gate_rejects() {
    let f = setup();
    let payer = Address::generate(&f.env);

    // Step 1 – invoice workflow succeeds (unrelated to compliance)
    let inv_id = create_pending_invoice(&f);
    f.invoice.mark_paid(&f.admin, &inv_id, &payer);
    assert_eq!(f.invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    // Step 2 – propose treasury settlement (treasury doesn't consult compliance)
    let sid = f.treasury.propose_settlement(&f.admin, &f.merchant, &10_000_000);
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Pending
    );

    // Step 3 – PAUSE compliance *before* allowing the merchant
    f.compliance.pause(&f.admin);

    // Step 4 – allow_address must be blocked while paused
    let allow_err = f
        .compliance
        .try_allow_address(&f.admin, &f.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(
        allow_err,
        compliance::ContractError::ContractPaused,
        "allow_address must return ContractPaused when compliance is paused"
    );

    // Step 5 – merchant is therefore not allowed; compliance gate rejects
    assert!(
        !f.compliance.is_allowed(&f.merchant),
        "merchant must not be allowed when compliance is paused and allow was blocked"
    );

    let workflow = PauseTestWorkflowClient::new(&f.env, &f.workflow_id);
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
        "workflow must return ComplianceFailed when compliance is paused and merchant not allowed"
    );

    // Step 6 – treasury settlement stays Pending — no stuck state
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Pending,
        "treasury settlement must remain Pending after compliance-paused execution attempt"
    );

    // Step 7 – invoice is unaffected
    assert_eq!(
        f.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Paid,
        "invoice state must be unaffected by compliance pause"
    );

    // Step 8 – recovery: unpause compliance, allow merchant, execute
    f.compliance.unpause(&f.admin);
    f.compliance.allow_address(&f.admin, &f.merchant);
    assert!(f.compliance.is_allowed(&f.merchant));

    let result = workflow.try_execute_if_compliant(
        &f.compliance_id,
        &f.treasury_id,
        &sid,
        &f.token_id,
        &f.merchant,
    );
    assert!(
        result.is_ok(),
        "execution must succeed after compliance unpaused and merchant allowed"
    );
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Executed
    );
}

/// Pre-allowed merchant executes even while compliance is paused; `is_allowed`
/// is a read that does not require the contract to be unpaused.
#[test]
fn scenario_compliance_paused_pre_allowed_merchant_can_still_execute() {
    let f = setup();

    // Allow merchant BEFORE pausing
    f.compliance.allow_address(&f.admin, &f.merchant);
    assert!(f.compliance.is_allowed(&f.merchant));

    let sid = f.treasury.propose_settlement(&f.admin, &f.merchant, &10_000_000);

    // Pause compliance after allow
    f.compliance.pause(&f.admin);

    // is_allowed is a read — it still works while paused
    assert!(
        f.compliance.is_allowed(&f.merchant),
        "is_allowed must return true for pre-allowed merchant even when compliance is paused"
    );

    // Execution via workflow succeeds because the merchant is already in the allowlist
    let workflow = PauseTestWorkflowClient::new(&f.env, &f.workflow_id);
    let result = workflow.try_execute_if_compliant(
        &f.compliance_id,
        &f.treasury_id,
        &sid,
        &f.token_id,
        &f.merchant,
    );
    assert!(
        result.is_ok(),
        "pre-allowed merchant must be able to execute even when compliance is paused"
    );
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Executed
    );
}

/// Block-while-paused: `block_address` is permitted when compliance is paused
/// (emergency remediation policy).  A blocked merchant who was previously allowed
/// is now rejected at the gate.
#[test]
fn scenario_compliance_paused_block_address_is_permitted() {
    let f = setup();

    // Allow merchant first
    f.compliance.allow_address(&f.admin, &f.merchant);

    let sid = f.treasury.propose_settlement(&f.admin, &f.merchant, &10_000_000);

    // Pause compliance
    f.compliance.pause(&f.admin);

    // block_address is allowed even while paused (emergency policy)
    let block_result = f.compliance.try_block_address(&f.admin, &f.merchant);
    assert!(
        block_result.is_ok(),
        "block_address must succeed even when compliance is paused"
    );
    assert!(
        !f.compliance.is_allowed(&f.merchant),
        "blocked merchant must not be allowed"
    );

    // Workflow execution now fails because merchant is blocked
    let workflow = PauseTestWorkflowClient::new(&f.env, &f.workflow_id);
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

    // Settlement stays Pending — no stuck state
    assert_eq!(
        f.treasury.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}
