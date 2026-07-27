#![no_std]

use compliance_client::ComplianceClient;
use soroban_sdk::{contract, contracterror, contractimpl, Address, Env};
use treasury::TreasuryContractClient;

/// Reference on-chain implementation of the `SettlementWorkflow` role described in
/// `ARCHITECTURE.md`: gates `Treasury::execute_settlement` behind
/// `Compliance::is_allowed`. Treasury does not consult compliance itself, so this
/// contract is the enforcement point for the compliance gate in the payment lifecycle.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum WorkflowError {
    ComplianceFailed = 1,
}

#[contract]
pub struct SettlementWorkflowContract;

#[contractimpl]
impl SettlementWorkflowContract {
    /// Checks `Compliance::is_allowed(merchant)` and, only if it passes, calls
    /// `Treasury::execute_settlement(..., settlement_id, token_contract)` using this
    /// contract's own address as the authorizing signer (it must be registered as a
    /// Treasury signer via `Treasury::set_signer` beforehand).
    /// Returns `Err(WorkflowError::ComplianceFailed)` without touching Treasury
    /// if the compliance check fails.
    pub fn execute_with_compliance(
        env: Env,
        compliance_id: Address,
        treasury_id: Address,
        settlement_id: u64,
        token_contract: Address,
        merchant: Address,
    ) -> Result<(), WorkflowError> {
        let compliance = ComplianceClient::new(&env, &compliance_id);
        if !compliance.is_allowed(&merchant) {
            return Err(WorkflowError::ComplianceFailed);
        }
        let treasury = TreasuryContractClient::new(&env, &treasury_id);
        treasury.execute_settlement(
            &env.current_contract_address(),
            &settlement_id,
            &token_contract,
        );
        Ok(())
    }
}
