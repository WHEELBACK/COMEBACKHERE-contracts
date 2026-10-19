#![no_std]

use compliance_client::ComplianceClient;
pub use multisig::{Settlement, SettlementStatus, TreasuryError};
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec,
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
    /// Read-only: used to attach the settled amount to the workflow's own summary
    /// event, so indexers don't have to correlate against `settlement_executed` to
    /// learn how much moved (#614).
    fn get_settlement(env: Env, settlement_id: u64) -> Settlement;
}

/// Why a simulated settlement would not succeed, mirroring the checks in
/// [`SettlementWorkflowContract::execute_with_compliance`] (#618).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimulationFailure {
    /// The workflow is paused, so `execute_with_compliance` would trap.
    WorkflowPaused,
    /// `Compliance::require_allowed_for_treasury(merchant)` rejected the merchant.
    ComplianceCheckFailed,
    /// The settlement is not in a state treasury would execute (unknown ID,
    /// already executed, cancelled, or on hold).
    TreasuryWouldReject,
}

/// Outcome of [`SettlementWorkflowContract::simulate_with_compliance`] (#618).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimulationResult {
    /// Every check the execution path runs from here would pass.
    WouldSucceed,
    /// Execution would fail, and this is the check that would fail first.
    WouldFail(SimulationFailure),
}

/// The pause surface shared by every protocol contract this workflow can
/// coordinate: the invoice, treasury and compliance contracts all expose
/// `pause(admin)` / `unpause(admin)` with the same shape, so one thin client
/// covers all three.
///
/// `#[contractclient]` is used for the same reason as [`TreasuryInterface`]:
/// depending on an implementation crate would statically link its wasm exports
/// and collide with this contract's own `pause` symbol. Only the two pause
/// entrypoints are declared, so the orchestrator's authority over the rest of the
/// protocol is exactly "stop it", and nothing more.
///
/// Both methods are declared as returning `()`, which is what the generated
/// client expects for `invoice::pause -> Result<(), InvoiceError>`,
/// `treasury::pause -> ()` and `compliance::pause -> Result<(), ComplianceError>`:
/// the host unwraps a `Result` return and traps on `Err`, so a refusal to pause
/// surfaces as a failed invocation rather than being silently ignored.
#[contractclient(name = "PausableContractClient")]
pub trait PausableInterface {
    fn pause(env: Env, admin: Address);
    fn unpause(env: Env, admin: Address);
    /// Read-only: reports whether the target is currently paused. Used by the
    /// sweep to distinguish targets paused independently (before this sweep)
    /// from targets paused by the sweep itself, so `resume_all` does not reverse
    /// an independent pause.
    fn is_paused(env: Env) -> bool;
}

/// Per-item outcome of `execute_with_compliance_batch` (#776).
///
/// One entry is returned per requested settlement ID, in the same order, so
/// callers can tell exactly which settlements executed and which were skipped
/// (and why) without diffing events or re-querying treasury.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettlementBatchResult {
    /// The settlement was executed and funds moved.
    Executed,
    /// No settlement exists with this ID.
    SkippedNotFound,
    /// The settlement is already executed (or otherwise terminal).
    SkippedAlreadyExecuted,
    /// The settlement is on hold and the hold has not expired.
    SkippedOnHold,
    /// The settlement's approval weight is below the threshold.
    SkippedThresholdNotMet,
    /// The settlement's execution deadline has passed.
    SkippedExecutionDeadlineExceeded,
    /// The token contract is the treasury itself (invalid).
    SkippedInvalidTokenContract,
    /// The token contract is not on the allowlist.
    SkippedTokenNotAllowed,
}

/// Maps a treasury execution error to the corresponding per-item skip status.
/// Errors that are pre-checked at batch level (threshold, token contract,
/// allowlist) are included for completeness even though they abort the whole
/// batch before the per-item loop.
fn treasury_error_to_batch_result(err: TreasuryError) -> SettlementBatchResult {
    use TreasuryError::*;
    match err {
        SettlementNotFound => SettlementBatchResult::SkippedNotFound,
        AlreadyExecuted => SettlementBatchResult::SkippedAlreadyExecuted,
        SettlementOnHold => SettlementBatchResult::SkippedOnHold,
        ThresholdNotMet => SettlementBatchResult::SkippedThresholdNotMet,
        ExecutionDeadlineExceeded => {
            SettlementBatchResult::SkippedExecutionDeadlineExceeded
        }
        InvalidTokenContract => SettlementBatchResult::SkippedInvalidTokenContract,
        TokenNotAllowed => SettlementBatchResult::SkippedTokenNotAllowed,
        // Batch-level pre-check failures; should not occur per-item once the
        // pre-checks have passed, but mapped defensively.
        ThresholdNotConfigured => SettlementBatchResult::SkippedThresholdNotMet,
        _ => SettlementBatchResult::SkippedAlreadyExecuted,
    }
}

/// Largest number of contracts a single `emergency_pause_all` may fan out to.
///
/// Bounded because the fan-out is a sequence of cross-contract invocations: an
/// unbounded one could exhaust the instruction budget part-way through, abandoning
/// the protocol mid-incident with an unknown pause state. Eight comfortably
/// covers this repository's contracts with room to grow.
pub const MAX_PAUSE_TARGETS: u32 = 8;

/// Errors raised by the workflow contract's own coordination role.
///
/// Distinct from [`TreasuryError`], which this crate returns from its settlement
/// path only because it already depends on `multisig` and had no error type of
/// its own. A coordination failure — a target would not pause, the workflow was
/// never configured, the caller is not the pause admin — is not a treasury
/// failure, and reporting it as one would send an operator debugging the wrong
/// contract during an incident.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum WorkflowError {
    /// The caller is not the address registered as the emergency-pause admin.
    Unauthorized = 1,
    /// `initialize_emergency_pause` has not been called, so there is no target
    /// set to coordinate.
    NotConfigured = 2,
    /// A target refused to pause, or the invocation itself failed. The whole
    /// fan-out is rolled back, so no target is left paused by a partial sweep.
    PauseTargetFailed = 3,
    /// A target refused to unpause. The whole fan-out is rolled back, so the
    /// protocol is never left partially resumed.
    UnpauseTargetFailed = 4,
    /// An empty target set was supplied; there would be nothing to coordinate.
    NoPauseTargets = 5,
    /// More targets than [`MAX_PAUSE_TARGETS`].
    TooManyPauseTargets = 6,
    /// The same address appears twice, which would pause it twice and make the
    /// second call the one that decides whether the fan-out succeeds.
    DuplicatePauseTarget = 7,
    /// The workflow contract itself was supplied as a target. Pausing itself
    /// part-way through the fan-out would make every remaining call fail,
    /// turning a recoverable incident into a workflow that cannot be resumed.
    SelfPauseTarget = 8,
    /// The target set cannot be reconfigured while an emergency pause is active:
    /// the new set would not match the state that is actually paused.
    EmergencyPauseActive = 9,
    /// `resume_all` was called while no emergency pause is active. Distinct from
    /// `NotConfigured` so an operator can tell "never set up" from "nothing to
    /// undo", and so the orchestrator cannot be used to unpause a protocol that
    /// somebody else paused through their own admin key.
    NotEmergencyPaused = 10,
    /// `initialize` was called more than once. Distinct from `NotConfigured`
    /// so an operator can tell "never initialized" from "already initialized".
    AlreadyInitialized = 11,
    /// The workflow is paused; `execute_with_compliance*` entrypoints are
    /// rejected while paused (#616).
    ContractPaused = 12,
    /// `accept_admin` was called with no pending admin transfer.
    NoPendingAdmin = 13,
}

/// Storage keys for the workflow contract.
///
/// `ComplianceId` / `TreasuryId` pin the compliance and treasury instances this
/// workflow trusts; they are set once at initialization (#364) so the contract
/// enforces which instances it uses rather than trusting whatever a caller
/// supplies per-call. `ExecutedSettlements` is the ordered list of settlement
/// IDs executed through this (compliance-gated) workflow, as opposed to executed
/// directly against treasury — see `get_executed_settlement_ids_page` (#373).
///
/// `EmergencyPauseAdmin` / `EmergencyPauseTargets` / `EmergencyPausedAt` back the
/// emergency pause coordination added in #73 and follow the same one-shot trust
/// model as `ComplianceId`/`TreasuryId`: the target set and the address allowed
/// to trigger it are pinned once and cannot then be redirected per-call.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    ExecutedSettlements,
    ComplianceId,
    TreasuryId,
    /// Address allowed to call `emergency_pause_all` and `resume_all`.
    EmergencyPauseAdmin,
    /// Ordered list of contract addresses `emergency_pause_all` fans out to.
    EmergencyPauseTargets,
    /// Ledger timestamp of the last successful `emergency_pause_all`, or absent
    /// while no emergency pause is active. Doubles as the flag that blocks
    /// reconfiguration and `resume_all` while an incident is open.
    EmergencyPausedAt,
    /// The active administrator. Set once at initialization and thereafter only
    /// changed through `accept_admin`. See #621.
    Admin,
    /// Staged address for the two-step admin transfer (`transfer_admin` /
    /// `accept_admin`). See #621.
    PendingAdmin,
    /// Circuit-breaker flag — when `true`, both `execute_with_compliance*`
    /// entrypoints are rejected (#616).
    Paused,
    /// Monotonically incrementing schema version written by `initialize` and
    /// targeted by `migrate` (#779). Mirrors the same key in compliance so
    /// future storage changes can be executed safely with a tested migration path.
    SchemaVersion,
}

/// Storage schema version written by `initialize` and targeted by `migrate` (#779).
///
/// Version 1 is the first version of the settlement-workflow that tracks schema
/// versioning. Future incompatible storage layout changes must bump this constant
/// and add a migration path in `migrate`.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

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
            soroban_sdk::panic_with_error!(env, WorkflowError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::ComplianceId, &compliance_id);
        env.storage()
            .instance()
            .set(&DataKey::TreasuryId, &treasury_id);
        env.storage()
            .instance()
            .set(&DataKey::SchemaVersion, &CURRENT_SCHEMA_VERSION);
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
    /// - Traps with `WorkflowError::Unauthorized` if `admin` is not the stored
    ///   administrator.
    ///
    /// Emits: `admin_transfer_initiated` so indexers can see a nomination is
    /// outstanding.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), WorkflowError> {
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
    /// - Traps with `WorkflowError::NoPendingAdmin` if no nomination is
    ///   outstanding — including after a completed handover, since
    ///   `PendingAdmin` is cleared on success and a transfer is not repeatable
    ///   until the admin initiates a new one.
    /// - Traps with `WorkflowError::Unauthorized` if `new_admin` is not the
    ///   nominee.
    ///
    /// Emits: `admin_transferred`.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), WorkflowError> {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(WorkflowError::NoPendingAdmin)?;
        if pending != new_admin {
            return Err(WorkflowError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.events()
            .publish((Symbol::new(&env, "admin_transferred"),), new_admin);
        Ok(())
    }

    /// Updates the pinned compliance and treasury contract instances (#771).
    ///
    /// Lets the admin point the workflow at upgraded compliance or treasury
    /// contracts without redeploying the workflow itself. `ComplianceId` and
    /// `TreasuryId` are fixed at initialization, so redeploying compliance
    /// previously meant redeploying the workflow too.
    ///
    /// Admin-only: `admin` must authorize and be the stored administrator.
    /// Emits: `contract_ids_updated`.
    pub fn update_contract_ids(
        env: Env,
        admin: Address,
        compliance_id: Address,
        treasury_id: Address,
    ) -> Result<(), TreasuryError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::ComplianceId, &compliance_id);
        env.storage()
            .instance()
            .set(&DataKey::TreasuryId, &treasury_id);
        env.events().publish(
            (Symbol::new(&env, "contract_ids_updated"),),
            (compliance_id.clone(), treasury_id.clone()),
        );
        Ok(())
    }

    /// Returns the pinned compliance contract instance.
    fn compliance_id(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::ComplianceId)
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, WorkflowError::NotConfigured))
    }

    /// Returns the pinned treasury contract instance.
    fn treasury_id(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::TreasuryId)
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, WorkflowError::NotConfigured))
    }

    /// Pauses the workflow (admin-only, #616): both `execute_with_compliance*`
    /// entrypoints trap with `ContractPaused` until `unpause` is called.
    /// Emits: `settlement_workflow_paused`.
    pub fn pause(env: Env, admin: Address) -> Result<(), WorkflowError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "settlement_workflow_paused"),), admin);
        Ok(())
    }

    /// Resumes normal operation after a pause (admin-only, #616).
    /// Emits: `settlement_workflow_unpaused`.
    pub fn unpause(env: Env, admin: Address) -> Result<(), WorkflowError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "settlement_workflow_unpaused"),), admin);
        Ok(())
    }

    /// Returns `true` while the workflow is paused (#616).
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Returns the current workflow admin.
    pub fn get_admin(env: Env) -> Result<Address, WorkflowError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(WorkflowError::Unauthorized)
    }

    /// Traps with [`WorkflowError::ContractPaused`] while the workflow is paused (#616).
    fn require_not_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            soroban_sdk::panic_with_error!(env, WorkflowError::ContractPaused);
        }
    }

    /// Requires `admin` to authorize and to be the stored administrator.
    ///
    /// Fails closed when no admin has been stored at all — that only happens on a
    /// contract that was never initialized, and treating it as "nobody is
    /// authorized" is safer and less surprising than panicking, since a missing
    /// admin can never be the caller's own address anyway.
    fn require_admin(env: &Env, admin: &Address) -> Result<(), WorkflowError> {
        admin.require_auth();
        let stored: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        if stored.as_ref() != Some(admin) {
            return Err(WorkflowError::Unauthorized);
        }
        Ok(())
    }

    /// Checks `Compliance::is_allowed(merchant)` and, only if it passes, calls
    /// `Treasury::execute_settlement(..., settlement_id, token_contract)` using this
    /// contract's own address as the authorizing signer (it must be registered as a
    /// Treasury signer via `Treasury::set_signer` beforehand).
    /// Returns `Err(SettlementWorkflowError::ComplianceCheckFailed)` without touching Treasury
    /// if the compliance check fails, instead of panicking or reusing a generic
    /// `Unauthorized` (see #74). Traps with `ContractPaused` while the workflow is
    /// paused (#616).
    ///
    /// Emits `settlement_workflow_executed` with `(merchant, token_contract, amount)`
    /// so this gated path is distinguishable from a direct `Treasury::execute_settlement`
    /// call and carries the amount in one place (#366, #614).
    ///
    /// Note on failures: a rejected call reverts the whole invocation, so no event is
    /// published for a compliance-blocked merchant — indexers must alert on the failed
    /// transaction itself (which carries `ComplianceCheckFailed`) rather than on a
    /// missing event. See `docs/event-schema.md`.
    pub fn execute_with_compliance(
        env: Env,
        settlement_id: u64,
        token_contract: Address,
        merchant: Address,
    ) -> Result<(), WorkflowError> {
        Self::require_not_paused(&env);
        let compliance = ComplianceClient::new(&env, &Self::compliance_id(&env));
        compliance.require_allowed_for_treasury(&merchant)?;
        let treasury = TreasuryOnlyClient::new(&env, &Self::treasury_id(&env));
        treasury.execute_settlement(
            &env.current_contract_address(),
            &settlement_id,
            &token_contract,
        );
        // Read the amount *after* execution, so a non-existent / already-executed
        // settlement still fails inside `execute_settlement` exactly as before this
        // event existed instead of trapping in this read first.
        let amount = treasury.get_settlement(&settlement_id).amount;
        env.events().publish(
            (
                Symbol::new(&env, "settlement_workflow_executed"),
                settlement_id,
            ),
            (merchant.clone(), token_contract.clone(), amount),
        );
        Ok(())
    }

    /// Read-only preview of [`Self::execute_with_compliance`] (#618).
    ///
    /// Performs no state changes and emits no events, so operators and frontends
    /// can tell whether a settlement will pass compliance before submitting it.
    /// The compliance leg calls the same `require_allowed_for_treasury` that
    /// `execute_with_compliance` calls, so the two cannot drift; the treasury leg
    /// uses the read-only `try_get_settlement` rather than `execute_settlement`.
    ///
    /// Quorum and treasury pause state are not checked here, so `WouldSucceed`
    /// means "the workflow is live, the compliance gate passes and the settlement
    /// is `Pending`"; execution can still fail with e.g. `ThresholdNotMet`. The
    /// negative results are exact.
    pub fn simulate_with_compliance(
        env: Env,
        settlement_id: u64,
        _token_contract: Address,
        merchant: Address,
    ) -> SimulationResult {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            return SimulationResult::WouldFail(SimulationFailure::WorkflowPaused);
        }
        let compliance = ComplianceClient::new(&env, &Self::compliance_id(&env));
        if compliance.require_allowed_for_treasury(&merchant).is_err() {
            return SimulationResult::WouldFail(SimulationFailure::ComplianceCheckFailed);
        }
        let treasury = TreasuryOnlyClient::new(&env, &Self::treasury_id(&env));
        match treasury.try_get_settlement(&settlement_id) {
            Ok(Ok(s)) if s.status == SettlementStatus::Pending => SimulationResult::WouldSucceed,
            _ => SimulationResult::WouldFail(SimulationFailure::TreasuryWouldReject),
        }
    }

    /// Batch variant of `execute_with_compliance` (#367). Runs the shared compliance
    /// gate for `merchant` once, then executes each settlement ID through the pinned
    /// treasury. Settlement IDs that don't exist, are already executed, or otherwise
    /// fail treasury execution are silently skipped (per treasury's batch precedent,
    /// #38) rather than aborting the whole batch; a per-item status is returned for
    /// every requested ID so callers can tell which settled and which were skipped.
    /// If the shared compliance gate fails, the whole batch is rejected with
    /// `ComplianceCheckFailed`. Traps with `ContractPaused` while the workflow is
    /// paused (#616).
    ///
    /// Emits `settlement_workflow_executed` for each settlement actually executed,
    /// each carrying `(merchant, token_contract, amount)`, followed by exactly one
    /// `workflow_batch_completed` summarising the outcome as `(requested, executed)`
    /// (#614). The summary is emitted even when every item was skipped, so a partial
    /// batch is one query away from being detected instead of having to be inferred by
    /// diffing per-item events.
    pub fn execute_with_compliance_batch(
        env: Env,
        settlement_ids: Vec<u64>,
        token_contract: Address,
        merchant: Address,
    ) -> Result<Vec<u64>, WorkflowError> {
        Self::require_not_paused(&env);
        let compliance = ComplianceClient::new(&env, &Self::compliance_id(&env));
        compliance.require_allowed_for_treasury(&merchant)?;
        let treasury = TreasuryOnlyClient::new(&env, &Self::treasury_id(&env));
        let requested = settlement_ids.len();
        let mut results = Vec::new(&env);
        let mut executed_count: u32 = 0;
        for id in settlement_ids.iter() {
            let result = treasury.try_execute_settlement(
                &env.current_contract_address(),
                &id,
                &token_contract,
            );
            match result {
                Ok(()) => {
                    // Read after execution: a skipped (non-existent) ID is never read,
                    // so `get_settlement`'s `SettlementNotFound` panic can't turn a
                    // skippable ID into a batch-wide abort.
                    let amount = treasury.get_settlement(&id).amount;
                    results.push_back(SettlementBatchResult::Executed);
                    executed_count += 1;
                    env.events().publish(
                        (Symbol::new(&env, "settlement_workflow_executed"), id),
                        (merchant.clone(), token_contract.clone(), amount),
                    );
                }
                Err(err) => {
                    results.push_back(treasury_error_to_batch_result(err));
                    // Invalid / already-executed / threshold-failed IDs are silently skipped.
                }
            }
        }
        // `workflow_batch_completed` (not `settlement_workflow_batch_completed`):
        // Soroban symbols are capped at 32 characters and the longer name is
        // rejected by the host at runtime with `InvalidInput`.
        env.events().publish(
            (Symbol::new(&env, "workflow_batch_completed"),),
            (requested, executed_count),
        );
        Ok(results)
    }

    // ── emergency pause coordination (#73) ───────────────────────────────────

    /// Pins the address allowed to trigger an emergency pause and the ordered set
    /// of contracts that pause fans out to.
    ///
    /// # Trust model
    ///
    /// The first caller wins, exactly as for `ComplianceId`/`TreasuryId` in
    /// `initialize` (#364): `EmergencyPauseAdmin` can be replaced but never
    /// silently redirected per-call, and there is no path by which a caller
    /// supplies a different target set to the one the protocol was deployed
    /// with. The first `initialize_emergency_pause` therefore has to be
    /// submitted by the deployer alongside `initialize` — a third party who
    /// front-ran it would own the emergency pause. That is the same bootstrap
    /// exposure `initialize` already has; reusing it keeps the contract to a
    /// single trust assumption instead of inventing a second one.
    ///
    /// Reconfiguration is allowed so a compromised or upgraded-away target can
    /// be replaced without redeploying, but only while no emergency pause is
    /// active: otherwise the new set would not match the contracts that are
    /// actually paused, and `resume_all` would strand them.
    ///
    /// The set is validated up front — non-empty, at most
    /// [`MAX_PAUSE_TARGETS`], no duplicates, never this contract itself — so a
    /// misconfiguration is rejected here rather than half-applied during an
    /// incident.
    ///
    /// Errors: `Unauthorized`, `NoPauseTargets`, `TooManyPauseTargets`,
    /// `DuplicatePauseTarget`, `SelfPauseTarget`, `EmergencyPauseActive`.
    /// Emits: `emergency_pause_configured`.
    pub fn initialize_emergency_pause(
        env: Env,
        admin: Address,
        targets: Vec<Address>,
    ) -> Result<(), WorkflowError> {
        admin.require_auth();

        if Self::emergency_paused_at(&env).is_some() {
            // Re-pointing the fan-out mid-incident would strand the contracts
            // that are already paused: `resume_all` would no longer know about
            // them, so they could never be resumed.
            return Err(WorkflowError::EmergencyPauseActive);
        }
        Self::validate_pause_targets(&env, &targets)?;

        env.storage()
            .instance()
            .set(&DataKey::EmergencyPauseAdmin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::EmergencyPauseTargets, &targets);
        env.events().publish(
            (Symbol::new(&env, "emergency_pause_configured"),),
            (admin, targets),
        );
        Ok(())
    }

    /// Pauses every configured target, or none of them (#73).
    ///
    /// This is the failure the issue is about. A naive fan-out calls `pause` on
    /// each contract in sequence and, when one refuses, either carries on or
    /// swallows the error — leaving the protocol split: invoices stopped while
    /// the treasury still moves money, or the reverse. During an incident that
    /// is worse than not pausing at all, because the operator can no longer tell
    /// which half of the protocol is live.
    ///
    /// The sweep is therefore **all-or-nothing**, and the guarantee is structural
    /// rather than best-effort:
    ///
    /// 1. **Failures are never collected.** Each `try_pause` is inspected and the
    ///    first that did not return `Ok` aborts the whole call with
    ///    `PauseTargetFailed`. This is deliberately not the
    ///    `try_pause(...); if result.is_ok() { ... }` shape used by
    ///    `execute_with_compliance_batch` — that shape is correct for skipping
    ///    individual settlements and is exactly what produces split state here.
    /// 2. **Aborting reverts the earlier pauses.** Returning `Err` fails this
    ///    contract's invocation, and the host unwinds the whole top-level
    ///    transaction — including the `pause` calls that already succeeded. The
    ///    targets reached before the failure are rolled back with them, so the
    ///    protocol is left exactly as it was: fully live, or, on a later retry,
    ///    fully paused.
    /// 3. **The timestamp is written last.** `EmergencyPausedAt` is set only after
    ///    the last target has confirmed, so its presence means "the sweep
    ///    completed", not "the sweep started". `resume_all` and
    ///    reconfiguration both key off it, and neither can be misled by a partial
    ///    sweep into treating the protocol as safe to touch.
    ///
    /// The observable consequence is the point of the whole design: an operator
    /// who sees `get_emergency_paused_at()` set knows *every* target is paused, and
    /// one who sees it unset knows *no* target was paused by this call. There is
    /// no in-between state to reason about.
    ///
    /// This contract must be registered as the `admin` on each target, because it
    /// passes its own address to their `pause(admin)`. That is the whole point of
    /// an emergency orchestrator: one key stops the protocol, and it holds no
    /// other authority over it.
    ///
    /// Returns the ordered list of contracts that are now paused, so the caller
    /// need not reconstruct it from the configuration it submitted.
    ///
    /// Errors: `Unauthorized`, `NotConfigured`, `PauseTargetFailed`.
    /// Emits: `emergency_pause_completed`.
    pub fn emergency_pause_all(env: Env, admin: Address) -> Result<Vec<Address>, WorkflowError> {
        admin.require_auth();
        Self::require_pause_admin(&env, &admin)?;

        let targets = Self::pause_targets(&env)?;
        let self_address = env.current_contract_address();
        let mut paused: Vec<Address> = Vec::new(&env);
        let mut already_paused: Vec<Address> = Vec::new(&env);

        for target in targets.iter() {
            let client = PausableContractClient::new(&env, &target);
            // Record whether the target was already paused before this sweep
            // touched it, so `resume_all` can skip targets paused independently.
            // Fail closed if we cannot read a target's state.
            let pause_state = client.try_is_paused();
            match pause_state {
                Ok(Ok(true)) => already_paused.push_back(target.clone()),
                Ok(Ok(false)) => {}
                _ => return Err(WorkflowError::PauseTargetFailed),
            }
            // Anything other than `Ok(Ok(()))` means this target did not pause.
            // Bailing out here reverts the targets already paused above, which is
            // the atomicity guarantee. An already-paused target pauses
            // idempotently, so a pre-paused target does not fail the sweep.
            if !matches!(client.try_pause(&self_address), Ok(Ok(()))) {
                return Err(WorkflowError::PauseTargetFailed);
            }
            paused.push_back(target.clone());
        }

        env.storage()
            .instance()
            .set(&DataKey::EmergencyPausedAt, &env.ledger().timestamp());
        env.storage().instance().set(&DataKey::AlreadyPaused, &already_paused);
        env.events().publish(
            (Symbol::new(&env, "emergency_pause_completed"),),
            (admin, paused.clone()),
        );
        Ok(paused)
    }

    /// Resumes every configured target, in reverse order, or none of them (#73).
    ///
    /// The mirror image of [`Self::emergency_pause_all`], with the same
    /// all-or-nothing discipline: a target that refuses to unpause aborts the
    /// sweep with `UnpauseTargetFailed` and the host unwinds the targets already
    /// resumed. There is no state in which this contract believes it resumed the
    /// protocol while part of the protocol is still paused.
    ///
    /// Reverse order — last paused, first resumed — is deliberate. The sweep
    /// stops the protocol from the front of the pipeline outwards, so recovery
    /// runs from the outside in: a downstream contract becomes reachable before
    /// the one feeding it, so a call landing mid-sweep is refused by the contract
    /// still paused rather than being accepted against a half-open pipeline.
    ///
    /// Refused when no emergency pause is active, so `resume_all` cannot be used
    /// to unpause a protocol somebody else paused through their own admin key:
    /// the orchestrator only ever reverses its own sweep.
    ///
    /// `min_delay_secs` enforces a minimum cooling-off period between the
    /// emergency pause and this resume call (#780). When non-zero, `resume_all`
    /// returns [`WorkflowError::ResumeTooEarly`] if fewer than `min_delay_secs`
    /// seconds have elapsed since `EmergencyPausedAt`. A value of `0` means no
    /// delay is required and the call proceeds immediately — preserving full
    /// backward compatibility with callers that pass `0`.
    ///
    /// Errors: `Unauthorized`, `NotConfigured`, `NotEmergencyPaused`,
    /// `ResumeTooEarly`, `UnpauseTargetFailed`.
    /// Emits: `emergency_pause_resumed`.
    pub fn resume_all(
        env: Env,
        admin: Address,
        min_delay_secs: u64,
    ) -> Result<Vec<Address>, WorkflowError> {
        admin.require_auth();
        Self::require_pause_admin(&env, &admin)?;

        let paused_at = Self::emergency_paused_at(&env).ok_or(WorkflowError::NotEmergencyPaused)?;

        if min_delay_secs > 0 {
            let now = env.ledger().timestamp();
            let earliest_resume = paused_at.saturating_add(min_delay_secs);
            if now < earliest_resume {
                return Err(WorkflowError::ResumeTooEarly);
            }
        }

        let targets = Self::pause_targets(&env)?;
        let already_paused: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AlreadyPaused)
            .unwrap_or(Vec::new(&env));
        let self_address = env.current_contract_address();
        let mut resumed: Vec<Address> = Vec::new(&env);

        for target in targets.iter().rev() {
            // Targets paused independently before the sweep are left paused: the
            // orchestrator only reverses what it paused itself.
            if already_paused.contains(&target) {
                continue;
            }
            let client = PausableContractClient::new(&env, &target);
            if !matches!(client.try_unpause(&self_address), Ok(Ok(()))) {
                return Err(WorkflowError::UnpauseTargetFailed);
            }
            resumed.push_back(target);
        }

        env.storage().instance().remove(&DataKey::EmergencyPausedAt);
        env.storage().instance().remove(&DataKey::AlreadyPaused);
        env.events().publish(
            (Symbol::new(&env, "emergency_pause_resumed"),),
            resumed.clone(),
        );
        Ok(resumed)
    }

    /// The contracts the emergency sweep covers, in the order they are paused.
    ///
    /// Errors: `NotConfigured`.
    pub fn get_emergency_pause_targets(env: Env) -> Result<Vec<Address>, WorkflowError> {
        Self::pause_targets(&env)
    }

    /// The address allowed to trigger an emergency pause or a resume.
    ///
    /// Errors: `NotConfigured`.
    pub fn get_emergency_pause_admin(env: Env) -> Result<Address, WorkflowError> {
        env.storage()
            .instance()
            .get(&DataKey::EmergencyPauseAdmin)
            .ok_or(WorkflowError::NotConfigured)
    }

    /// The ledger timestamp of the last completed emergency sweep, or `None` when
    /// no emergency pause is active.
    ///
    /// This is the flag an operator — or an off-chain monitor — should read to
    /// decide whether the protocol is mid-incident. It is set only after every
    /// target has confirmed the pause, so its presence means the whole protocol
    /// is stopped, and its absence means this contract did not stop any of it.
    pub fn get_emergency_paused_at(env: Env) -> Option<u64> {
        Self::emergency_paused_at(&env)
    }

    /// Rejects a caller that is not the pinned emergency-pause admin.
    fn require_pause_admin(env: &Env, admin: &Address) -> Result<(), WorkflowError> {
        let configured: Address = env
            .storage()
            .instance()
            .get(&DataKey::EmergencyPauseAdmin)
            .ok_or(WorkflowError::NotConfigured)?;
        if configured != *admin {
            return Err(WorkflowError::Unauthorized);
        }
        Ok(())
    }

    /// The configured emergency-pause target set, or `NotConfigured`.
    fn pause_targets(env: &Env) -> Result<Vec<Address>, WorkflowError> {
        env.storage()
            .instance()
            .get(&DataKey::EmergencyPauseTargets)
            .ok_or(WorkflowError::NotConfigured)
    }

    /// The ledger timestamp of the active emergency pause, if any. The single
    /// read behind both the public getter and the "is an incident open" checks.
    fn emergency_paused_at(env: &Env) -> Option<u64> {
        env.storage().instance().get(&DataKey::EmergencyPausedAt)
    }

    /// Rejects a target set that could not be swept safely.
    ///
    /// Runs before anything is written so a bad configuration leaves the contract
    /// exactly as it was, rather than half-applied during an incident.
    fn validate_pause_targets(env: &Env, targets: &Vec<Address>) -> Result<(), WorkflowError> {
        if targets.is_empty() {
            return Err(WorkflowError::NoPauseTargets);
        }
        if targets.len() > MAX_PAUSE_TARGETS {
            return Err(WorkflowError::TooManyPauseTargets);
        }

        let self_address = env.current_contract_address();
        let mut seen: Vec<Address> = Vec::new(env);
        for target in targets.iter() {
            if target == self_address {
                return Err(WorkflowError::SelfPauseTarget);
            }
            if seen.contains(&target) {
                return Err(WorkflowError::DuplicatePauseTarget);
            }
            seen.push_back(target);
        }
        Ok(())
    }
}
