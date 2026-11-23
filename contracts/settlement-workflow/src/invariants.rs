//! Strict invariant verification for the workflow's stored state (#873).
//!
//! The workflow's safety rests on a handful of relationships between storage
//! keys that no single entrypoint owns. Each is checked here in one place so
//! state-changing entrypoints can refuse to commit a state that breaks one,
//! and operators can audit a live deployment through `verify_invariants`.
//!
//! Invariants:
//! 1. `Admin`, `ComplianceId` and `TreasuryId` are always present.
//! 2. The compliance and treasury instances are distinct contracts, and
//!    neither is the workflow itself — otherwise one contract would be
//!    authoritative for both the gate and the payout.
//! 3. A pending admin nomination is never the current admin.
//! 4. If emergency-pause targets are configured, so is the pause admin, and
//!    the target set satisfies the configuration-time rules (non-empty,
//!    bounded, no duplicates, never the workflow).
//! 5. An active emergency pause implies a configured target set, and its
//!    recorded timestamp is never in the future.

use soroban_sdk::{Address, Env, Vec};

use crate::{DataKey, WorkflowError, MAX_PAUSE_TARGETS};

/// Checks every invariant, returning `InvariantViolation` on the first breach.
pub fn check(env: &Env) -> Result<(), WorkflowError> {
    let storage = env.storage().instance();
    let self_address = env.current_contract_address();

    // 1. Core configuration present.
    let admin: Address = storage
        .get(&DataKey::Admin)
        .ok_or(WorkflowError::InvariantViolation)?;
    let (compliance_id, treasury_id) =
        pinned_contracts(env).map_err(|_| WorkflowError::InvariantViolation)?;

    // 2. Pinned contracts distinct and external.
    ensure_distinct_external(&self_address, &compliance_id, &treasury_id)?;

    // 3. Pending admin never equals the current admin.
    if let Some(pending) = storage.get::<_, Address>(&DataKey::PendingAdmin) {
        if pending == admin {
            return Err(WorkflowError::InvariantViolation);
        }
    }

    // 4. Pause target set well-formed and owned.
    let targets: Option<Vec<Address>> = storage.get(&DataKey::EmergencyPauseTargets);
    if let Some(targets) = &targets {
        if !storage.has(&DataKey::EmergencyPauseAdmin) {
            return Err(WorkflowError::InvariantViolation);
        }
        if targets.is_empty() || targets.len() > MAX_PAUSE_TARGETS {
            return Err(WorkflowError::InvariantViolation);
        }
        let mut seen: Vec<Address> = Vec::new(env);
        for target in targets.iter() {
            if target == self_address || seen.contains(&target) {
                return Err(WorkflowError::InvariantViolation);
            }
            seen.push_back(target);
        }
    }

    // 5. Active pause implies targets and a non-future timestamp.
    if let Some(paused_at) = storage.get::<_, u64>(&DataKey::EmergencyPausedAt) {
        if targets.is_none() || paused_at > env.ledger().timestamp() {
            return Err(WorkflowError::InvariantViolation);
        }
    }

    Ok(())
}

/// Traps with the invariant error. For entrypoints whose `Result` carries a
/// different error type (`TreasuryError`).
pub fn enforce(env: &Env) {
    if let Err(e) = check(env) {
        soroban_sdk::panic_with_error!(env, e);
    }
}

/// Reads the pinned compliance and treasury instances and checks invariant 2,
/// so the settlement path never calls a misconfigured pair.
pub fn checked_pinned_contracts(env: &Env) -> Result<(Address, Address), WorkflowError> {
    let (compliance_id, treasury_id) = pinned_contracts(env)?;
    ensure_distinct_external(&env.current_contract_address(), &compliance_id, &treasury_id)?;
    Ok((compliance_id, treasury_id))
}

fn pinned_contracts(env: &Env) -> Result<(Address, Address), WorkflowError> {
    let storage = env.storage().instance();
    let compliance_id: Address = storage
        .get(&DataKey::ComplianceId)
        .ok_or(WorkflowError::NotConfigured)?;
    let treasury_id: Address = storage
        .get(&DataKey::TreasuryId)
        .ok_or(WorkflowError::NotConfigured)?;
    Ok((compliance_id, treasury_id))
}

fn ensure_distinct_external(
    self_address: &Address,
    compliance_id: &Address,
    treasury_id: &Address,
) -> Result<(), WorkflowError> {
    if compliance_id == treasury_id || compliance_id == self_address || treasury_id == self_address
    {
        return Err(WorkflowError::InvariantViolation);
    }
    Ok(())
}
