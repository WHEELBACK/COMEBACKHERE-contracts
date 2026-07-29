#![no_std]

use compliance_client::ComplianceClient;
use soroban_sdk::{contract, contractimpl, Address, Env};
use treasury::{TreasuryContractClient, TreasuryError};

/// Reference on-chain implementation of the `SettlementWorkflow` role described in
/// `ARCHITECTURE.md`: gates `Treasury::execute_settlement` behind
/// `Compliance::is_allowed`. Treasury does not consult compliance itself, so this
/// contract is the enforcement point for the compliance gate in the payment lifecycle.
#[contract]
pub struct SettlementWorkflowContract;

#[contractimpl]
impl SettlementWorkflowContract {
    /// Checks `Compliance::is_allowed(merchant)` and, only if it passes, calls
    /// `Treasury::execute_settlement(..., settlement_id, token_contract)` using this
    /// contract's own address as the authorizing signer (it must be registered as a
    /// Treasury signer via `Treasury::set_signer` beforehand).
    /// Returns `Err(TreasuryError::ComplianceCheckFailed)` without touching Treasury
    /// if the compliance check fails, instead of panicking or reusing a generic
    /// `Unauthorized` (see #74).
    pub fn execute_with_compliance(
        env: Env,
        compliance_id: Address,
        treasury_id: Address,
        settlement_id: u64,
        token_contract: Address,
        merchant: Address,
    ) -> Result<(), TreasuryError> {
        let compliance = ComplianceClient::new(&env, &compliance_id);
        compliance.require_allowed_for_treasury(&merchant)?;
        let treasury = TreasuryContractClient::new(&env, &treasury_id);
        treasury.execute_settlement(
            &env.current_contract_address(),
            &settlement_id,
            &token_contract,
        );
        Ok(())
    }
}
