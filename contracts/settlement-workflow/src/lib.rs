#![no_std]

use compliance_client::ComplianceClient;
use soroban_sdk::{contract, contractclient, contractimpl, Address, Env, Symbol};

/// Cross-contract call surface this crate needs from the treasury contract.
/// `#[contractclient]` on a bare trait generates only an invocation client, not
/// a dependency on the `comebackhere-treasury` implementation crate, so this
/// contract doesn't statically link treasury's wasm exports (`pause`,
/// `unpause`, ...) alongside its own. See the `compliance_client` crate for
/// the same pattern, and Cargo.toml for why `treasury` is dev-only here.
#[contractclient(name = "TreasuryOnlyClient")]
pub trait TreasuryInterface {
    fn execute_settlement(env: Env, signer: Address, settlement_id: u64, token_contract: Address);
}

/// Dedicated error type for settlement-workflow, mirroring the pattern used
/// by every other contract in the repo (#362). Lets callers distinguish
/// failures originating in this contract's own orchestration logic from
/// errors that genuinely came from Treasury's `execute_settlement`.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SettlementWorkflowError {
    AlreadyInitialized = 1,
    Unauthorized = 2,
    ContractPaused = 3,
    ComplianceCheckFailed = 4,
}

/// Operational configuration for the SettlementWorkflow contract.
/// Stored in instance storage at initialization and read back via
/// `get_config` (#365). Migration tooling and off-chain integrators use
/// this to discover the wired-up compliance and treasury contract IDs.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub admin: Address,
    pub compliance_id: Address,
    pub treasury_id: Address,
    pub paused: bool,
}

/// Instance-storage keys.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DataKey {
    Admin = 1,
    ComplianceId = 2,
    TreasuryId = 3,
    Paused = 4,
    Initialized = 5,
}

/// Reference on-chain implementation of the `SettlementWorkflow` role described in
/// `ARCHITECTURE.md`: gates `Treasury::execute_settlement` behind
/// `Compliance::is_allowed`. Treasury does not consult compliance itself, so this
/// contract is the enforcement point for the compliance gate in the payment lifecycle.
#[contract]
pub struct SettlementWorkflowContract;

#[contractimpl]
impl SettlementWorkflowContract {
    /// Stores `admin`, `compliance_id`, and `treasury_id` in instance storage
    /// so that subsequent calls to `execute_with_compliance` can read them
    /// without requiring them per-call (#365). Emits no events — the stored
    /// values are observable via `get_config`.
    pub fn initialize(
        env: Env,
        admin: Address,
        compliance_id: Address,
        treasury_id: Address,
    ) -> Result<(), SettlementWorkflowError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(SettlementWorkflowError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::ComplianceId, &compliance_id);
        env.storage().instance().set(&DataKey::TreasuryId, &treasury_id);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::Initialized, &true);
        Ok(())
    }

    /// Checks `Compliance::is_allowed(merchant)` and, only if it passes, calls
    /// `Treasury::execute_settlement(..., settlement_id, token_contract)` using this
    /// contract's own address as the authorizing signer (it must be registered as a
    /// Treasury signer via `Treasury::set_signer` beforehand).
    /// Returns `Err(SettlementWorkflowError::ComplianceCheckFailed)` without touching Treasury
    /// if the compliance check fails, instead of panicking or reusing a generic
    /// `Unauthorized` (see #74).
    pub fn execute_with_compliance(
        env: Env,
        settlement_id: u64,
        token_contract: Address,
        merchant: Address,
    ) -> Result<(), SettlementWorkflowError> {
        Self::require_not_paused(&env)?;
        let compliance_id: Address = env
            .storage()
            .instance()
            .get(&DataKey::ComplianceId)
            .unwrap();
        let treasury_id: Address = env
            .storage()
            .instance()
            .get(&DataKey::TreasuryId)
            .unwrap();
        let compliance = ComplianceClient::new(&env, &compliance_id);
        compliance.require_allowed(&merchant, SettlementWorkflowError::ComplianceCheckFailed)?;
        let treasury = TreasuryOnlyClient::new(&env, &treasury_id);
        treasury.execute_settlement(
            &env.current_contract_address(),
            &settlement_id,
            &token_contract,
        );
        Ok(())
    }

    /// Pauses the contract, blocking all state-mutating operations except
    /// admin functions (admin-only). Emits: `settlement_workflow_paused`.
    pub fn pause(env: Env, admin: Address) -> Result<(), SettlementWorkflowError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "settlement_workflow_paused"),), admin);
        Ok(())
    }

    /// Resumes normal operations after a pause (admin-only).
    /// Emits: `settlement_workflow_unpaused`.
    pub fn unpause(env: Env, admin: Address) -> Result<(), SettlementWorkflowError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "settlement_workflow_unpaused"),), admin);
        Ok(())
    }

    /// Returns the full operational configuration stored at initialization.
    /// Off-chain integrators and other contracts call this rather than guessing
    /// at storage keys (#365).
    pub fn get_config(env: Env) -> Config {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        let compliance_id: Address = env
            .storage()
            .instance()
            .get(&DataKey::ComplianceId)
            .unwrap();
        let treasury_id: Address = env
            .storage()
            .instance()
            .get(&DataKey::TreasuryId)
            .unwrap();
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        Config {
            admin,
            compliance_id,
            treasury_id,
            paused,
        }
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), SettlementWorkflowError> {
        admin.require_auth();
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored != *admin {
            return Err(SettlementWorkflowError::Unauthorized);
        }
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), SettlementWorkflowError> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            return Err(SettlementWorkflowError::ContractPaused);
        }
        Ok(())
    }
}
