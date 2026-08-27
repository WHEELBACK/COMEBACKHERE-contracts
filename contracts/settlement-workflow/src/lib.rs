#![no_std]

use compliance_client::ComplianceClient;
use multisig::TreasuryError;
use soroban_sdk::{contract, contractclient, contractimpl, Address, Env};

/// Cross-contract call surface this crate needs from the treasury contract.
/// `#[contractclient]` on a bare trait generates only an invocation client, not
/// a dependency on the `comebackhere-treasury` implementation crate, so this
/// contract doesn't statically link treasury's wasm exports (`pause`,
/// `unpause`, ...) alongside its own. See the `compliance_client` crate for
/// the same pattern, and Cargo.toml for why `treasury` is dev-only here.
#[contractclient(name = "TreasuryOnlyClient")]
pub trait TreasuryInterface {
    fn execute_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
        token_contract: Address,
    ) -> Result<(), TreasuryError>;
}

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
        let treasury = TreasuryOnlyClient::new(&env, &treasury_id);
        treasury.execute_settlement(
            &env.current_contract_address(),
            &settlement_id,
            &token_contract,
        );
        Ok(())
    }
}
