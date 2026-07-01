use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeBytes,
};
use soroban_sdk::{
    contract, contracterror, contractimpl,
    testutils::Address as _,
    Address, Env,
};
use treasury::{TreasuryContract, TreasuryContractClient};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PauseWorkflowError {
    InvoicePaused = 1,
}

#[contract]
struct PauseAwareSettlementWorkflow;

#[contractimpl]
impl PauseAwareSettlementWorkflow {
    pub fn pay_and_propose(
        env: Env,
        admin: Address,
        invoice_id: Address,
        invoice_num: u64,
        treasury_id: Address,
        payer: Address,
    ) -> Result<u64, PauseWorkflowError> {
        let invoice = InvoiceContractClient::new(&env, &invoice_id);
        // Try to mark the invoice as paid. If the invoice contract is paused
        // this will fail and the settlement proposal is never submitted.
        if invoice.try_mark_paid(&admin, &invoice_num, &payer).is_err() {
            return Err(PauseWorkflowError::InvoicePaused);
        }
        let inv = invoice.get_invoice(&invoice_num);
        let treasury = TreasuryContractClient::new(&env, &treasury_id);
        let signer = env.current_contract_address();
        Ok(treasury.propose_settlement(&signer, &inv.merchant, &inv.amount_usdc))
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
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    let wf_id = env.register_contract(None, PauseAwareSettlementWorkflow);

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
        payer,
        invoice,
        invoice_id,
        treasury,
        treasury_id,
        wf_id,
    )
}

#[test]
fn pay_and_propose_succeeds_when_not_paused() {
    let (_env, admin, merchant, payer, invoice, invoice_id, _treasury, treasury_id, wf_id) =
        setup();

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

    let wf = PauseAwareSettlementWorkflowClient::new(&_env, &wf_id);
    assert!(wf
        .try_pay_and_propose(&admin, &invoice_id, &inv_id, &treasury_id, &payer)
        .is_ok());

    let inv = invoice.get_invoice(&inv_id);
    assert_eq!(inv.status, InvoiceStatus::Paid);
}

#[test]
fn pay_and_propose_fails_when_invoice_contract_paused() {
    let (_env, admin, merchant, payer, invoice, invoice_id, _treasury, treasury_id, wf_id) =
        setup();

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

    // Pause the invoice contract — this blocks mark_paid.
    invoice.pause(&admin);

    // mark_paid directly should fail with ContractPaused
    let err = invoice
        .try_mark_paid(&admin, &inv_id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::ContractPaused);

    // The cross-contract workflow also fails at the invoice level.
    let wf = PauseAwareSettlementWorkflowClient::new(&_env, &wf_id);
    let err = wf
        .try_pay_and_propose(&admin, &invoice_id, &inv_id, &treasury_id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, PauseWorkflowError::InvoicePaused);

    // Invoice status remains unchanged.
    assert_eq!(
        invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Pending
    );
}

#[test]
fn unpause_restores_workflow() {
    let (_env, admin, merchant, payer, invoice, invoice_id, _treasury, treasury_id, wf_id) =
        setup();

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

    // Pause then unpause
    invoice.pause(&admin);
    invoice.unpause(&admin);

    // Workflow should succeed after unpause
    let wf = PauseAwareSettlementWorkflowClient::new(&_env, &wf_id);
    assert!(wf
        .try_pay_and_propose(&admin, &invoice_id, &inv_id, &treasury_id, &payer)
        .is_ok());

    let inv = invoice.get_invoice(&inv_id);
    assert_eq!(inv.status, InvoiceStatus::Paid);
}
