use compliance::{ComplianceContract, ComplianceContractClient};
use invoice::{InvoiceContract, InvoiceContractClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient};

pub struct ProtocolFixture {
    pub admin: Address,
    pub merchant: Address,
    pub invoice_contract_id: Address,
    pub invoice: InvoiceContractClient<'static>,
    pub compliance_contract_id: Address,
    pub compliance: ComplianceContractClient<'static>,
    pub treasury_contract_id: Address,
    pub treasury: TreasuryContractClient<'static>,
}

/// Deploys and initializes invoice, compliance, and treasury with default
/// admin and threshold settings. Treasury signer storage keeps only active
/// non-zero signers; tests can add workflow contracts as signers after setup.
pub fn setup_full_protocol(env: &Env) -> ProtocolFixture {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let merchant = Address::generate(env);

    let invoice_contract_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(env, &invoice_contract_id);
    invoice.initialize(&admin);

    let compliance_contract_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(env, &compliance_contract_id);
    compliance.initialize(&admin);

    let treasury_contract_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(env, &treasury_contract_id);
    treasury.initialize(&admin, &1, &Vec::new(env));

    ProtocolFixture {
        admin,
        merchant,
        invoice_contract_id,
        invoice,
        compliance_contract_id,
        compliance,
        treasury_contract_id,
        treasury,
    }
}

pub struct WorkflowFixture {
    pub admin: Address,
    pub merchant: Address,
    pub compliance_contract_id: Address,
    pub compliance: ComplianceContractClient<'static>,
    pub treasury_contract_id: Address,
    pub treasury: TreasuryContractClient<'static>,
    pub workflow_contract_id: Address,
    pub workflow: SettlementWorkflowContractClient<'static>,
}

/// Deploys and initializes compliance, treasury, and settlement-workflow with
/// the workflow contract registered as a Treasury signer.
///
/// `register_workflow_signer` controls whether the workflow contract is
/// registered as a Treasury signer. Pass `false` to exercise the #370
/// precondition path where the workflow's own address has not yet been
/// registered via `Treasury::set_signer`.
pub fn setup_with_workflow(env: &Env, register_workflow_signer: bool) -> WorkflowFixture {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let merchant = Address::generate(env);

    let compliance_contract_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(env, &compliance_contract_id);
    compliance.initialize(&admin);

    let treasury_contract_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(env, &treasury_contract_id);
    treasury.initialize(&admin, &1, &Vec::new(env));

    let workflow_contract_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(env, &workflow_contract_id);
    // Pin the trusted compliance/treasury instances and the admin once at init
    // (#364, #621).
    workflow.initialize(&admin, &compliance_contract_id, &treasury_contract_id);
    // The workflow contract executes settlements as itself, so it must be an
    // authorized Treasury signer.
    if register_workflow_signer {
        treasury.set_signer(&admin, &workflow_contract_id, &1);
    }

    WorkflowFixture {
        admin,
        merchant,
        compliance_contract_id,
        compliance,
        treasury_contract_id,
        treasury,
        workflow_contract_id,
        workflow,
    }
}
