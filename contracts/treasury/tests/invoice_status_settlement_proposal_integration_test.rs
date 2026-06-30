use invoice::{InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeBytes};
use soroban_sdk::{
    contract, contracterror, contractimpl,
    testutils::{Address as _, Ledger},
    Address, Env,
};
use treasury::{TreasuryContract, TreasuryContractClient};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProposalError {
    InvalidInvoiceStatus = 1,
}

#[contract]
struct SettlementProposalWorkflow;

#[contractimpl]
impl SettlementProposalWorkflow {
    // A settlement proposal is only valid when the invoice is in `InvoiceStatus::Paid`.
    // Any non-Paid state (e.g. Pending, Released) must be rejected.
    pub fn propose_settlement_for_invoice(
        env: Env,
        invoice_id: Address,
        treasury_id: Address,
        invoice_num: u64,
    ) -> Result<u64, ProposalError> {
        let invoice = InvoiceContractClient::new(&env, &invoice_id).get_invoice(&invoice_num);
        if invoice.status != InvoiceStatus::Paid {
            return Err(ProposalError::InvalidInvoiceStatus);
        }
        let treasury = TreasuryContractClient::new(&env, &treasury_id);
        let signer = env.current_contract_address();
        Ok(treasury.propose_settlement(&signer, &invoice.merchant, &invoice.amount_usdc))
    }
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    InvoiceContractClient<'static>,
    Address,
    TreasuryContractClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    let wf_id = env.register_contract(None, SettlementProposalWorkflow);

    let invoice_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    assert!(invoice.try_initialize(&admin).is_ok());

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    assert!(treasury.try_initialize(&admin, &1, &soroban_sdk::Vec::new(&env)).is_ok());
    treasury.set_signer(&admin, &wf_id, &1);

    (
        env,
        admin,
        merchant,
        invoice_id,
        invoice,
        treasury_id,
        treasury,
        wf_id,
    )
}

#[test]
fn settlement_proposal_succeeds_when_invoice_paid() {
    let (env, admin, merchant, invoice_id, invoice, treasury_id, _treasury, wf_id) = setup();
    let payer = Address::generate(&env);
    let inv_id = invoice
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .unwrap()
        .unwrap();
    assert!(invoice.try_mark_paid(&admin, &inv_id, &payer).unwrap().is_ok());

    let wf = SettlementProposalWorkflowClient::new(&env, &wf_id);
    assert!(wf
        .try_propose_settlement_for_invoice(&invoice_id, &treasury_id, &inv_id)
        .is_ok());
}

#[test]
fn settlement_proposal_rejected_when_invoice_pending() {
    let (env, _admin, merchant, invoice_id, invoice, treasury_id, _treasury, wf_id) = setup();
    let inv_id = invoice
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .unwrap()
        .unwrap();
    // Invoice is still Pending — proposal must be rejected.
    let wf = SettlementProposalWorkflowClient::new(&env, &wf_id);
    assert!(wf
        .try_propose_settlement_for_invoice(&invoice_id, &treasury_id, &inv_id)
        .is_err());
}

#[test]
fn settlement_proposal_rejected_when_invoice_released() {
    let (env, admin, merchant, invoice_id, invoice, treasury_id, _treasury, wf_id) = setup();
    let payer = Address::generate(&env);
    let inv_id = invoice
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .unwrap()
        .unwrap();
    assert!(invoice.try_mark_paid(&admin, &inv_id, &payer).unwrap().is_ok());
    invoice.release_escrow(&admin, &inv_id);
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Released);

    let wf = SettlementProposalWorkflowClient::new(&env, &wf_id);
    assert!(wf
        .try_propose_settlement_for_invoice(&invoice_id, &treasury_id, &inv_id)
        .is_err());
}

#[test]
fn settlement_proposal_boundary_at_expiry_transition() {
    let (env, admin, merchant, invoice_id, invoice, treasury_id, _treasury, wf_id) = setup();
    let payer = Address::generate(&env);
    let inv_id = invoice
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &1,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .unwrap()
        .unwrap();
    let inv = invoice.get_invoice(&inv_id);

    // Boundary: at exact expiry timestamp mark_paid is rejected with Expired.
    env.ledger().with_mut(|l| l.timestamp = inv.expires_at);
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Pending);
    let wf = SettlementProposalWorkflowClient::new(&env, &wf_id);
    let err = invoice
        .try_mark_paid(&admin, &inv_id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, invoice::InvoiceError::Expired);

    // Before expiry (timestamp reset to 0) payment succeeds.
    env.ledger().with_mut(|l| l.timestamp = 0);
    assert!(invoice
        .try_mark_paid(&admin, &inv_id, &payer)
        .unwrap()
        .is_ok());
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    // Once the invoice is Paid, a settlement proposal must succeed.
    assert!(wf
        .try_propose_settlement_for_invoice(&invoice_id, &treasury_id, &inv_id)
        .is_ok());
}
