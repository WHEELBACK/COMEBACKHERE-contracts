use compliance::{ComplianceContract, ComplianceContractClient};
use invoice::{InvoiceContract, InvoiceContractClient};
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
