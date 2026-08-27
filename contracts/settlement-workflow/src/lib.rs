#![no_std]

use compliance_client::ComplianceClient;
use multisig::TreasuryError;
use soroban_sdk::{contract, contractclient, contractimpl, Address, Env, Symbol, Vec};

/// Cross-contract call surface this crate needs from the treasury contract.
/// `#[contractclient]` on a bare trait generates only an invocation client, not
/// a dependency on the `comebackhere-treasury` implementation crate, so this
/// contract doesn't statically link treasury's wasm exports (`pause`,
/// `unpause`, ...) alongside its own. See the `compliance_client` crate for
/// the same pattern, and Cargo.toml for why `treasury` is dev-only here.
#[contractclient(name = "TreasuryOnlyClient")]
pub trait TreasuryInterface {
    fn execute_settlement(env: Env, signer: Address, settlement_id: u64, token_contract: Address);
    fn get_signer_weight(env: Env, signer: Address) -> u32;
}

/// Storage key for the ordered list of settlement IDs executed through this
/// workflow contract (as opposed to executed directly against treasury, bypassing
/// the compliance gate). See `get_executed_settlement_ids_page` (#373).
#[contracttype]
pub enum DataKey {
    ExecutedSettlements,
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
    /// contract's own address as the authorizing signer.
    ///
    /// # Precondition: this contract must be a registered Treasury signer
    /// Before `execute_with_compliance` is ever called, the deployer must register
    /// this contract's own address as a Treasury signer with non-zero weight via
    /// `Treasury::set_signer(treasury_admin, this_contract_address, weight)`.
    ///
    /// If that setup step is skipped, the nested `Treasury::execute_settlement`
    /// would reject the call with a generic `TreasuryError::UnauthorizedSigner`
    /// that gives no indication of the missing step. To make the failure
    /// actionable, this entrypoint checks its own signer registration up front and
    /// returns `Err(TreasuryError::WorkflowNotRegisteredSigner)` instead, so the fix
    /// is unambiguous: register this contract as a treasury signer. This is the
    /// single most likely misconfiguration for a first-time integrator (#370).
    ///
    /// # Errors
    /// - `TreasuryError::ComplianceCheckFailed` — compliance disallows `merchant`
    ///   (checked before touching treasury; never reuses a generic `Unauthorized`).
    /// - `TreasuryError::WorkflowNotRegisteredSigner` — this contract is not
    ///   registered as a treasury signer (see precondition above).
    /// - Any error propagated from the nested `Treasury::execute_settlement`, e.g.
    ///   `SettlementNotFound` for an unknown/mistyped `settlement_id`, or
    ///   `AlreadyExecuted` if the settlement was already executed (possibly via a
    ///   direct Treasury call that bypassed this workflow).
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
        if treasury.get_signer_weight(&env.current_contract_address()) == 0 {
            return Err(TreasuryError::WorkflowNotRegisteredSigner);
        }

        treasury.execute_settlement(
            &env.current_contract_address(),
            &settlement_id,
            &token_contract,
        );

        let mut executed: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::ExecutedSettlements)
            .unwrap_or_else(|| Vec::new(&env));
        executed.push_back(settlement_id);
        env.storage()
            .persistent()
            .set(&DataKey::ExecutedSettlements, &executed);
        env.events().publish(
            (Symbol::new(&env, "workflow_settlement_executed"),),
            settlement_id,
        );

        Ok(())
    }

    /// Returns a page of settlement IDs that were executed through this workflow
    /// contract (i.e. compliance-gated), in execution order. Skips the first `start`
    /// entries and returns up to `limit`, mirroring `Treasury::get_pending_settlements_page`.
    ///
    /// Auditing use case: cross-reference the returned IDs against Treasury's full
    /// settlement history (e.g. via `Treasury::get_settlement`) to confirm that every
    /// executed settlement passed the compliance gate, and to spot any that were
    /// executed directly against Treasury — bypassing this workflow — leaving a gap.
    pub fn get_executed_settlement_ids_page(env: Env, start: u64, limit: u64) -> Vec<u64> {
        let executed: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::ExecutedSettlements)
            .unwrap_or_else(|| Vec::new(&env));
        let mut page = Vec::new(&env);
        let mut skipped: u64 = 0;
        for id in executed.iter() {
            if skipped < start {
                skipped += 1;
            } else if (page.len() as u64) < limit {
                page.push_back(id);
            } else {
                break;
            }
        }
        page
    }
}
