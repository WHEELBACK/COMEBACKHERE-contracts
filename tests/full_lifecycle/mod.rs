//! End-to-end integration tests spanning all three contracts.
//!
//! A workflow contract enforces two sequential gates:
//!
//! 1. **Invoice gate** – invoice must be `Pending` (not paid, expired, or cancelled).
//! 2. **Compliance gate** – merchant must be allowed by the compliance contract.
//!
//! Only when both gates pass is a treasury settlement proposal emitted.  The
//! tests cover the happy path and every relevant failure combination.

use compliance::{ComplianceContract, ComplianceContractClient};
use invoice::{InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeBytes};
use soroban_sdk::{
    contract, contracterror, contractimpl,
    testutils::{Address as _, Ledger},
    Address, Env,
};
use treasury::{TreasuryContract, TreasuryContractClient};

extern crate std;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FullLifecycleError {
    InvoiceNotPending = 1,
    ComplianceDenied = 2,
}

/// Workflow contract covering the complete settlement initiation pipeline.
#[contract]
pub struct FullLifecycleWorkflow;

#[contractimpl]
impl FullLifecycleWorkflow {
    /// Gate 1: invoice must be Pending.
    /// Gate 2: merchant must be compliance-allowed.
    /// On success: proposes a treasury settlement and returns its ID.
    pub fn process_payment(
        env: Env,
        invoice_contract: Address,
        compliance_contract: Address,
        treasury_contract: Address,
        invoice_id: u64,
        merchant: Address,
    ) -> Result<u64, FullLifecycleError> {
        let inv =
            InvoiceContractClient::new(&env, &invoice_contract).get_invoice(&invoice_id);
        if inv.status != InvoiceStatus::Pending {
            return Err(FullLifecycleError::InvoiceNotPending);
        }
        if !ComplianceContractClient::new(&env, &compliance_contract).is_allowed(&merchant) {
            return Err(FullLifecycleError::ComplianceDenied);
        }
        let signer = env.current_contract_address();
        Ok(TreasuryContractClient::new(&env, &treasury_contract)
            .propose_settlement(&signer, &inv.merchant, &inv.amount_usdc))
    }
}

type Setup = (
    Env,
    Address, // admin
    Address, // merchant
    Address, // invoice_contract_id
    InvoiceContractClient<'static>,
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

    let workflow_id = env.register_contract(None, FullLifecycleWorkflow);

    let invoice_contract_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &invoice_contract_id);
    invoice.initialize(&admin);

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
        invoice_contract_id,
        invoice,
        compliance_contract_id,
        compliance,
        treasury_contract_id,
        treasury,
        workflow_id,
    )
}

#[test]
fn pending_and_compliant_merchant_settlement_succeeds() {
    let (
        env,
        admin,
        merchant,
        invoice_cid,
        invoice,
        compliance_cid,
        compliance,
        treasury_cid,
        _treasury,
        wf_id,
    ) = setup();

    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    compliance.allow_address(&admin, &merchant);

    let result = FullLifecycleWorkflowClient::new(&env, &wf_id).try_process_payment(
        &invoice_cid,
        &compliance_cid,
        &treasury_cid,
        &inv_id,
        &merchant,
    );
    assert!(
        result.is_ok(),
        "pending invoice with compliant merchant must produce a settlement proposal"
    );
}

#[test]
fn pending_but_non_compliant_merchant_blocked() {
    let (
        env,
        _admin,
        merchant,
        invoice_cid,
        invoice,
        compliance_cid,
        _compliance,
        treasury_cid,
        _treasury,
        wf_id,
    ) = setup();

    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    // merchant is never added to the compliance allowlist

    let err = FullLifecycleWorkflowClient::new(&env, &wf_id)
        .try_process_payment(
            &invoice_cid,
            &compliance_cid,
            &treasury_cid,
            &inv_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, FullLifecycleError::ComplianceDenied);
}

#[test]
fn paid_invoice_blocked_before_compliance_check() {
    let (
        env,
        admin,
        merchant,
        invoice_cid,
        invoice,
        compliance_cid,
        compliance,
        treasury_cid,
        _treasury,
        wf_id,
    ) = setup();

    let payer = Address::generate(&env);
    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    invoice.mark_paid(&admin, &inv_id, &payer);
    compliance.allow_address(&admin, &merchant);

    // Invoice gate fires first; compliance is irrelevant.
    let err = FullLifecycleWorkflowClient::new(&env, &wf_id)
        .try_process_payment(
            &invoice_cid,
            &compliance_cid,
            &treasury_cid,
            &inv_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, FullLifecycleError::InvoiceNotPending);
}

#[test]
fn expired_invoice_blocked_before_compliance_check() {
    let (
        env,
        admin,
        merchant,
        invoice_cid,
        invoice,
        compliance_cid,
        compliance,
        treasury_cid,
        _treasury,
        wf_id,
    ) = setup();

    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &1,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    env.ledger().with_mut(|l| l.timestamp += 2);
    invoice.batch_expire(&admin, &soroban_sdk::vec![&env, inv_id]);
    compliance.allow_address(&admin, &merchant);

    let err = FullLifecycleWorkflowClient::new(&env, &wf_id)
        .try_process_payment(
            &invoice_cid,
            &compliance_cid,
            &treasury_cid,
            &inv_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, FullLifecycleError::InvoiceNotPending);
}

#[test]
fn cancelled_invoice_blocked_before_compliance_check() {
    let (
        env,
        admin,
        merchant,
        invoice_cid,
        invoice,
        compliance_cid,
        compliance,
        treasury_cid,
        _treasury,
        wf_id,
    ) = setup();

    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    invoice.cancel_invoice(&merchant, &inv_id);

    // allow merchant in compliance — still should fail at invoice gate
    compliance.allow_address(&admin, &merchant);

    let err = FullLifecycleWorkflowClient::new(&env, &wf_id)
        .try_process_payment(
            &invoice_cid,
            &compliance_cid,
            &treasury_cid,
            &inv_id,
            &merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, FullLifecycleError::InvoiceNotPending);
}
