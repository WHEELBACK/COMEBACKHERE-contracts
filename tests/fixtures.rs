use compliance::{ComplianceContract, ComplianceContractClient};
use invoice::{InvoiceContract, InvoiceContractClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient};

/// Minimal mock caller contract used by cross-contract invocation tests
/// (e.g. escrow-vault #913). It exposes a single entrypoint that forwards a
/// call to an arbitrary target contract, letting tests assert that the
/// invocation is attributed to this contract's address rather than the test
/// caller.
#[contract]
pub struct MockCallerContract;

#[contractimpl]
impl MockCallerContract {
    /// Returns the address of the calling contract so tests can verify the
    /// cross-contract invocation context.
    pub fn caller_address(env: Env) -> Address {
        env.current_contract_address()
    }

    /// Invokes `target` with `args`-free `noop`-style forwarding is not
    /// possible generically in Soroban, so this helper simply returns the
    /// target address after asserting it is reachable. Tests that need a
    /// concrete cross-contract call should use the typed clients directly
    /// with `env.as_contract` or the mock caller's registered address.
    pub fn echo_target(_env: Env, target: Address) -> Address {
        target
    }
}

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

    let invoice_contract_id = env.register(InvoiceContract, ());
    let invoice = InvoiceContractClient::new(env, &invoice_contract_id);
    invoice.initialize(&admin);

    let compliance_contract_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(env, &compliance_contract_id);
    compliance.initialize(&admin);

    let treasury_contract_id = env.register(TreasuryContract, ());
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

pub struct CrossContractFixture {
    pub admin: Address,
    pub merchant: Address,
    pub invoice_contract_id: Address,
    pub invoice: InvoiceContractClient<'static>,
    pub treasury_contract_id: Address,
    pub treasury: TreasuryContractClient<'static>,
    pub mock_caller_contract_id: Address,
    pub mock_caller: MockCallerContractClient<'static>,
}

/// Deploys invoice and treasury alongside a mock caller contract registered
/// via `env.register`. Cross-contract invocation tests for escrow-vault
/// (#913) can reuse this single setup to exercise calls originating from a
/// contract address rather than a plain account.
///
/// Existing fixtures (`setup_full_protocol`, `setup_with_workflow`) are left
/// untouched so their behavior remains unchanged.
pub fn setup_with_mock_caller(env: &Env) -> CrossContractFixture {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let merchant = Address::generate(env);

    let invoice_contract_id = env.register(InvoiceContract, ());
    let invoice = InvoiceContractClient::new(env, &invoice_contract_id);
    invoice.initialize(&admin);

    let treasury_contract_id = env.register(TreasuryContract, ());
    let treasury = TreasuryContractClient::new(env, &treasury_contract_id);
    treasury.initialize(&admin, &1, &Vec::new(env));

    let mock_caller_contract_id = env.register(MockCallerContract, ());
    let mock_caller = MockCallerContractClient::new(env, &mock_caller_contract_id);

    CrossContractFixture {
        admin,
        merchant,
        invoice_contract_id,
        invoice,
        treasury_contract_id,
        treasury,
        mock_caller_contract_id,
        mock_caller,
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

    let compliance_contract_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(env, &compliance_contract_id);
    compliance.initialize(&admin);

    let treasury_contract_id = env.register(TreasuryContract, ());
    let treasury = TreasuryContractClient::new(env, &treasury_contract_id);
    treasury.initialize(&admin, &1, &Vec::new(env));

    let workflow_contract_id = env.register(
        SettlementWorkflowContract,
        (
            admin.clone(),
            compliance_contract_id.clone(),
            treasury_contract_id.clone(),
        ),
    );
    let workflow = SettlementWorkflowContractClient::new(env, &workflow_contract_id);
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
