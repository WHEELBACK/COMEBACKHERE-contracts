//! Cross-contract tests: Invoice contract → Treasury contract.
//!
//! A workflow contract mirrors the off-chain settlement initiator: it reads
//! invoice status via `InvoiceContractClient` and proposes a treasury
//! settlement only when the invoice is `Pending`.  Any terminal state
//! (`Paid`, `Cancelled`, `Expired`) must be rejected before reaching the
//! treasury.

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
pub enum InvoiceTreasuryError {
    InvoiceNotPending = 1,
}

/// Workflow contract: checks invoice status then proposes a treasury settlement.
#[contract]
pub struct InvoiceTreasuryWorkflow;

#[contractimpl]
impl InvoiceTreasuryWorkflow {
    pub fn propose_for_invoice(
        env: Env,
        invoice_contract: Address,
        treasury_contract: Address,
        invoice_id: u64,
    ) -> Result<u64, InvoiceTreasuryError> {
        let inv = InvoiceContractClient::new(&env, &invoice_contract).get_invoice(&invoice_id);
        if inv.status != InvoiceStatus::Pending {
            return Err(InvoiceTreasuryError::InvoiceNotPending);
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
    Address, // treasury_contract_id
    TreasuryContractClient<'static>,
    Address, // workflow_id
);

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    let workflow_id = env.register_contract(None, InvoiceTreasuryWorkflow);

    let invoice_contract_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &invoice_contract_id);
    invoice.initialize(&admin);

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
        treasury_contract_id,
        treasury,
        workflow_id,
    )
}

#[test]
fn pending_invoice_allows_settlement_proposal() {
    let (env, _admin, merchant, invoice_cid, invoice, treasury_cid, _treasury, wf_id) = setup();
    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Pending);

    let result = InvoiceTreasuryWorkflowClient::new(&env, &wf_id)
        .try_propose_for_invoice(&invoice_cid, &treasury_cid, &inv_id);
    assert!(result.is_ok(), "pending invoice must allow settlement proposal");
}

#[test]
fn paid_invoice_blocks_settlement_proposal() {
    let (env, admin, merchant, invoice_cid, invoice, treasury_cid, _treasury, wf_id) = setup();
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
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    let err = InvoiceTreasuryWorkflowClient::new(&env, &wf_id)
        .try_propose_for_invoice(&invoice_cid, &treasury_cid, &inv_id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceTreasuryError::InvoiceNotPending);
}

#[test]
fn cancelled_invoice_blocks_settlement_proposal() {
    let (env, _admin, merchant, invoice_cid, invoice, treasury_cid, _treasury, wf_id) = setup();
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
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Cancelled);

    let err = InvoiceTreasuryWorkflowClient::new(&env, &wf_id)
        .try_propose_for_invoice(&invoice_cid, &treasury_cid, &inv_id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceTreasuryError::InvoiceNotPending);
}

#[test]
fn expired_invoice_blocks_settlement_proposal() {
    let (env, admin, merchant, invoice_cid, invoice, treasury_cid, _treasury, wf_id) = setup();
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
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Expired);

    let err = InvoiceTreasuryWorkflowClient::new(&env, &wf_id)
        .try_propose_for_invoice(&invoice_cid, &treasury_cid, &inv_id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceTreasuryError::InvoiceNotPending);
}
