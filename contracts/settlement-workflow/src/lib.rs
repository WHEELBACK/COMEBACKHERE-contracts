#![no_std]

use compliance_client::ComplianceClient;
use multisig::TreasuryError;
use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, Address, Env, Symbol, Vec,
};

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

/// Storage keys for the workflow contract.
///
/// `ComplianceId` / `TreasuryId` pin the compliance and treasury instances this
/// workflow trusts; they are set once at initialization (#364) so the contract
/// enforces which instances it uses rather than trusting whatever a caller
/// supplies per-call. `ExecutedSettlements` is the ordered list of settlement
/// IDs executed through this (compliance-gated) workflow, as opposed to executed
/// directly against treasury — see `get_executed_settlement_ids_page` (#373).
/// `Admin` / `PendingAdmin` back the two-step admin rotation from #621.
///
/// New variants must be appended at the end only: existing instances encode
/// these keys by their XDR discriminant, so reordering would corrupt stored data.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    ExecutedSettlements,
    ComplianceId,
    TreasuryId,
    /// The active administrator. Set once at initialization and thereafter only
    /// changed through `accept_admin`. See #621.
    Admin,
    /// Staged address for the two-step admin transfer (`transfer_admin` /
    /// `accept_admin`). See #621.
    PendingAdmin,
}

/// Reference on-chain implementation of the `SettlementWorkflow` role described in
/// `ARCHITECTURE.md`: gates `Treasury::execute_settlement` behind
/// `Compliance::is_allowed`. Treasury does not consult compliance itself, so this
/// contract is the enforcement point for the compliance gate in the payment lifecycle.
#[contract]
pub struct SettlementWorkflowContract;

#[contractimpl]
impl SettlementWorkflowContract {
    /// Pins the compliance and treasury contract instances this workflow trusts,
    /// and records `admin` as the initial administrator.
    ///
    /// Must be called exactly once before any `execute_with_compliance*` call; a
    /// second call traps with `AlreadyInitialized` (#364). Callers can no longer
    /// redirect the gate at an arbitrary compliance/treasury instance per-call.
    /// The same guard also covers the admin role (#621), so a repeat call cannot
    /// hand the admin key to an attacker either.
    /// Emits: `workflow_initialized`.
    ///
    /// The guard reads `ComplianceId` rather than `Admin`; the two are written in
    /// the same call and so are always present or absent together, but
    /// `ComplianceId` is what this check has always keyed on and what the pinned
    /// trust relationship is expressed in terms of.
    pub fn initialize(env: Env, admin: Address, compliance_id: Address, treasury_id: Address) {
        if env.storage().instance().has(&DataKey::ComplianceId) {
            soroban_sdk::panic_with_error!(env, TreasuryError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::ComplianceId, &compliance_id);
        env.storage()
            .instance()
            .set(&DataKey::TreasuryId, &treasury_id);
        env.events().publish(
            (Symbol::new(&env, "workflow_initialized"),),
            (admin, compliance_id, treasury_id),
        );
    }

    /// Nominates `new_admin` as the next administrator of this workflow (#621).
    ///
    /// Two-step, mirroring invoice's and compliance's `transfer_admin`: the
    /// handover does not take effect until `new_admin` calls
    /// [`accept_admin`](Self::accept_admin), so a typo in `new_admin` cannot
    /// permanently strand the contract with nobody able to administer it. That is
    /// the specific failure this avoids — the admin is fixed at initialization
    /// otherwise, so losing or leaking that key means redeploying.
    ///
    /// Calling this again before the nominee accepts **supersedes** the previous
    /// nomination (`PendingAdmin` is a plain overwrite, not a queue), so a lost or
    /// compromised nominee key does not leave the contract stuck.
    ///
    /// # Errors
    /// - Traps with `TreasuryError::Unauthorized` if `admin` is not the stored
    ///   administrator.
    ///
    /// Emits: `admin_transfer_initiated` so indexers can see a nomination is
    /// outstanding.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), TreasuryError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        env.events()
            .publish((Symbol::new(&env, "admin_transfer_initiated"),), new_admin);
        Ok(())
    }

    /// Completes the handover started by
    /// [`transfer_admin`](Self::transfer_admin) (#621).
    ///
    /// Must be called by the nominee, who must authorize it, and the stored
    /// nomination must match. Only then does the admin role move.
    ///
    /// # Errors
    /// - Traps with `TreasuryError::NoPendingAdmin` if no nomination is
    ///   outstanding — including after a completed handover, since
    ///   `PendingAdmin` is cleared on success and a transfer is not repeatable
    ///   until the admin initiates a new one.
    /// - Traps with `TreasuryError::Unauthorized` if `new_admin` is not the
    ///   nominee.
    ///
    /// Emits: `admin_transferred`.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), TreasuryError> {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(TreasuryError::NoPendingAdmin)?;
        if pending != new_admin {
            return Err(TreasuryError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.events()
            .publish((Symbol::new(&env, "admin_transferred"),), new_admin);
        Ok(())
    }

    /// Returns the pinned compliance contract instance.
    fn compliance_id(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::ComplianceId)
            .unwrap()
    }

    /// Returns the pinned treasury contract instance.
    fn treasury_id(env: &Env) -> Address {
        env.storage().instance().get(&DataKey::TreasuryId).unwrap()
    }

    /// Requires `admin` to authorize and to be the stored administrator.
    ///
    /// Fails closed when no admin has been stored at all — that only happens on a
    /// contract that was never initialized, and treating it as "nobody is
    /// authorized" is safer and less surprising than panicking, since a missing
    /// admin can never be the caller's own address anyway.
    fn require_admin(env: &Env, admin: &Address) -> Result<(), TreasuryError> {
        admin.require_auth();
        let stored: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        if stored.as_ref() != Some(admin) {
            return Err(TreasuryError::Unauthorized);
        }
        Ok(())
    }

    /// Checks `Compliance::is_allowed(merchant)` and, only if it passes, calls
    /// `Treasury::execute_settlement(..., settlement_id, token_contract)` using this
    /// contract's own address as the authorizing signer (it must be registered as a
    /// Treasury signer via `Treasury::set_signer` beforehand).
    /// Returns `Err(SettlementWorkflowError::ComplianceCheckFailed)` without touching Treasury
    /// if the compliance check fails, instead of panicking or reusing a generic
    /// `Unauthorized` (see #74).
    /// Emits: `settlement_workflow_executed` so indexers can distinguish this gated
    /// path from a direct `Treasury::execute_settlement` call (#366).
    pub fn execute_with_compliance(
        env: Env,
        settlement_id: u64,
        token_contract: Address,
        merchant: Address,
    ) -> Result<(), TreasuryError> {
        let compliance = ComplianceClient::new(&env, &Self::compliance_id(&env));
        compliance.require_allowed_for_treasury(&merchant)?;
        let treasury = TreasuryOnlyClient::new(&env, &Self::treasury_id(&env));
        treasury.execute_settlement(
            &env.current_contract_address(),
            &settlement_id,
            &token_contract,
        );
        env.events().publish(
            (
                Symbol::new(&env, "settlement_workflow_executed"),
                settlement_id,
            ),
            (merchant.clone(), token_contract.clone()),
        );
        Ok(())
    }

    /// Batch variant of `execute_with_compliance` (#367). Runs the shared compliance
    /// gate for `merchant` once, then executes each settlement ID through the pinned
    /// treasury. Settlement IDs that don't exist, are already executed, or otherwise
    /// fail treasury execution are silently skipped (per treasury's batch precedent,
    /// #38) rather than aborting the whole batch; only successfully executed IDs are
    /// returned and emitted. If the shared compliance gate fails, the whole batch is
    /// rejected with `ComplianceCheckFailed`.
    /// Emits: `settlement_workflow_executed` for each settlement actually executed.
    pub fn execute_with_compliance_batch(
        env: Env,
        settlement_ids: Vec<u64>,
        token_contract: Address,
        merchant: Address,
    ) -> Result<Vec<u64>, TreasuryError> {
        let compliance = ComplianceClient::new(&env, &Self::compliance_id(&env));
        compliance.require_allowed_for_treasury(&merchant)?;
        let treasury = TreasuryOnlyClient::new(&env, &Self::treasury_id(&env));
        let mut executed = Vec::new(&env);
        for id in settlement_ids.iter() {
            let result = treasury.try_execute_settlement(
                &env.current_contract_address(),
                &id,
                &token_contract,
            );
            if result.is_ok() {
                executed.push_back(id);
                env.events().publish(
                    (Symbol::new(&env, "settlement_workflow_executed"), id),
                    (merchant.clone(), token_contract.clone()),
                );
            }
            // Invalid / already-executed / threshold-failed IDs are silently skipped.
        }
        Ok(executed)
    }
}
