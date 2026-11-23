//! Timelocked signer and threshold-configuration changes (issue #447).
//!
//! # Design rationale
//!
//! `set_signer`, `remove_signer`, and `update_threshold` previously took effect
//! immediately upon a single admin signature. For a protocol whose entire purpose
//! is guarding settlement funds behind a multisig quorum, an instantaneous
//! single-key change means a compromised admin key can silently redirect who
//! controls that quorum with zero reaction time.
//!
//! This module introduces a **propose → wait → execute** pattern:
//!
//! 1. `propose_signer_change` — admin proposes a change; the proposal is stored
//!    with a `proposed_at` timestamp and an `executable_at = proposed_at +
//!    SIGNER_CHANGE_TIMELOCK_SECS`. The change is **not** applied yet.
//! 2. During the delay window (default 24 h), other signers or off-chain
//!    monitoring systems can observe the pending proposal via
//!    `get_signer_change` and, if the change looks malicious, an admin can call
//!    `cancel_signer_change` to permanently block it.
//! 3. Once `env.ledger().timestamp() >= executable_at`, any admin may call
//!    `execute_signer_change` to atomically apply the change.
//!
//! The immediate `set_signer` / `remove_signer` / `update_threshold` entry
//! points in `signers.rs` are preserved for backward compatibility but
//! documented as bypassing the timelock; production deployments should prefer
//! the timelocked flow for signer-configuration changes.
//!
//! # Storage
//! Proposals are written to **persistent** storage under `DataKey::SignerChange(id)`.
//! The `DataKey::SignerChangeCount` instance counter is the source of monotonically-
//! increasing IDs.
//!
//! # Events
//! - `signer_change_proposed` (topic: `(symbol, id)`, data: `SignerChangeProposal`)
//! - `signer_change_executed` (topic: `(symbol, id)`, data: `SignerChangeProposal`)
//! - `signer_change_cancelled` (topic: `(symbol, id)`, data: `SignerChangeProposal`)

use crate::{
    require_admin, DataKey, SignerChangeKind, SignerChangeProposal, SignerChangeStatus,
    TreasuryContract, TreasuryContractArgs, TreasuryContractClient, TreasuryError,
};
use soroban_sdk::{contractimpl, Address, Env, Symbol, Vec};

/// Minimum delay (in seconds) between a signer/threshold-change proposal and
/// when it may be executed. Set to 24 hours so that other signers and off-chain
/// monitors have a meaningful reaction window.
pub(crate) const SIGNER_CHANGE_TIMELOCK_SECS: u64 = 24 * 60 * 60;

/// Hard cap on the number of proposals `list_pending_signer_changes` returns in a
/// single call, to stay within Soroban budget limits.
pub(crate) const MAX_SIGNER_CHANGE_PAGE: u32 = 50;

/// Minimum allowed value for the admin-configurable timelock delay (#732): one hour.
/// Prevents the delay from being shortened to the point of defeating the reaction
/// window the timelock exists to provide.
pub const MIN_TIMELOCK_DELAY_SECS: u64 = 60 * 60;

/// Maximum allowed value for the admin-configurable timelock delay (#732): 30 days.
/// Prevents an operator from locking signer changes behind an impractically long wait.
pub const MAX_TIMELOCK_DELAY_SECS: u64 = 30 * 24 * 60 * 60;

/// Returns the currently-configured signer/threshold-change timelock delay (#732),
/// falling back to `SIGNER_CHANGE_TIMELOCK_SECS` when it has never been changed.
pub(crate) fn current_timelock_delay(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::TimelockDelaySecs)
        .unwrap_or(SIGNER_CHANGE_TIMELOCK_SECS)
}

#[contractimpl]
impl TreasuryContract {
    /// Queues a timelocked signer or threshold-configuration change (admin-only).
    ///
    /// The change described by `kind` is **not** applied immediately. It is
    /// stored as a `Pending` `SignerChangeProposal` and becomes executable only
    /// after `SIGNER_CHANGE_TIMELOCK_SECS` (24 h) have elapsed, giving other
    /// signers and off-chain monitoring systems time to observe and react.
    ///
    /// Returns the unique `change_id` that identifies the proposal in subsequent
    /// `execute_signer_change` / `cancel_signer_change` / `get_signer_change` calls.
    ///
    /// Errors: `ArithmeticOverflow`.
    /// Emits: `signer_change_proposed`.
    pub fn propose_signer_change(
        env: Env,
        admin: Address,
        kind: SignerChangeKind,
    ) -> Result<u64, TreasuryError> {
        require_admin(&env, &admin);

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::SignerChangeCount)
            .unwrap_or(0);
        let id = count
            .checked_add(1)
            .ok_or(TreasuryError::ArithmeticOverflow)?;

        if let SignerChangeKind::UpdateTimelockDelay(seconds) = kind {
            if seconds < MIN_TIMELOCK_DELAY_SECS || seconds > MAX_TIMELOCK_DELAY_SECS {
                return Err(TreasuryError::InvalidTimelockDelay);
            }
        }

        let now = env.ledger().timestamp();
        let executable_at = now
            .checked_add(current_timelock_delay(&env))
            .ok_or(TreasuryError::ArithmeticOverflow)?;

        let proposal = SignerChangeProposal {
            id,
            kind: kind.clone(),
            proposed_at: now,
            executable_at,
            status: SignerChangeStatus::Pending,
        };

        env.storage()
            .persistent()
            .set(&DataKey::SignerChange(id), &proposal);
        env.storage()
            .instance()
            .set(&DataKey::SignerChangeCount, &id);

        env.events()
            .publish((Symbol::new(&env, "signer_change_proposed"), id), proposal);

        Ok(id)
    }

    /// Applies a previously-queued signer/threshold-change (admin-only).
    ///
    /// Succeeds only if:
    /// - A `Pending` proposal with `change_id` exists.
    /// - `env.ledger().timestamp() >= proposal.executable_at` (the 24 h delay has elapsed).
    ///
    /// On success the underlying storage is updated identically to the direct
    /// `set_signer` / `remove_signer` / `update_threshold` calls and the proposal
    /// is marked `Executed`.
    ///
    /// Errors: `SignerChangeNotFound`, `SignerChangeAlreadyFinalised`,
    ///         `SignerChangeTooEarly`, `ZeroThreshold`, `ThresholdUnreachable`.
    /// Emits: `signer_change_executed`.
    pub fn execute_signer_change(
        env: Env,
        admin: Address,
        change_id: u64,
    ) -> Result<SignerChangeProposal, TreasuryError> {
        require_admin(&env, &admin);

        let mut proposal: SignerChangeProposal = env
            .storage()
            .persistent()
            .get(&DataKey::SignerChange(change_id))
            .ok_or(TreasuryError::SignerChangeNotFound)?;

        if proposal.status != SignerChangeStatus::Pending {
            return Err(TreasuryError::SignerChangeAlreadyFinalised);
        }

        let now = env.ledger().timestamp();
        if now < proposal.executable_at {
            return Err(TreasuryError::SignerChangeTooEarly);
        }

        // Apply the change.
        match proposal.kind.clone() {
            SignerChangeKind::SetSigner(signer, weight) => {
                env.storage()
                    .instance()
                    .set(&DataKey::Signer(signer.clone()), &weight);
                // Maintain the SignerList exactly as set_signer does.
                let mut list: Vec<Address> = env
                    .storage()
                    .instance()
                    .get(&DataKey::SignerList)
                    .unwrap_or_else(|| Vec::new(&env));
                if weight > 0 {
                    if !list.contains(&signer) {
                        list.push_back(signer.clone());
                        env.storage().instance().set(&DataKey::SignerList, &list);
                    }
                } else {
                    let mut updated = Vec::new(&env);
                    for s in list.iter() {
                        if s != signer {
                            updated.push_back(s);
                        }
                    }
                    env.storage().instance().set(&DataKey::SignerList, &updated);
                }
            }
            SignerChangeKind::RemoveSigner(signer) => {
                env.storage()
                    .instance()
                    .remove(&DataKey::Signer(signer.clone()));
                let list: Vec<Address> = env
                    .storage()
                    .instance()
                    .get(&DataKey::SignerList)
                    .unwrap_or_else(|| Vec::new(&env));
                let mut updated = Vec::new(&env);
                for s in list.iter() {
                    if s != signer {
                        updated.push_back(s);
                    }
                }
                env.storage().instance().set(&DataKey::SignerList, &updated);
            }
            SignerChangeKind::UpdateThreshold(new_threshold) => {
                if new_threshold == 0 {
                    return Err(TreasuryError::ZeroThreshold);
                }
                let total_weight: u32 = TreasuryContract::get_all_signers(env.clone())
                    .iter()
                    .map(|(_, weight, _)| weight)
                    .sum();
                if new_threshold > total_weight {
                    return Err(TreasuryError::ThresholdUnreachable);
                }
                env.storage()
                    .instance()
                    .set(&DataKey::Threshold, &new_threshold);
            }
            SignerChangeKind::UpdateTimelockDelay(seconds) => {
                // Re-checked here (not just at proposal time) so the bound can never
                // be bypassed, even if a future change alters the bounds between
                // proposal and execution.
                if seconds < MIN_TIMELOCK_DELAY_SECS || seconds > MAX_TIMELOCK_DELAY_SECS {
                    return Err(TreasuryError::InvalidTimelockDelay);
                }
                env.storage()
                    .instance()
                    .set(&DataKey::TimelockDelaySecs, &seconds);
            }
        }

        proposal.status = SignerChangeStatus::Executed;
        env.storage()
            .persistent()
            .set(&DataKey::SignerChange(change_id), &proposal);

        env.events().publish(
            (Symbol::new(&env, "signer_change_executed"), change_id),
            proposal.clone(),
        );

        Ok(proposal)
    }

    /// Cancels a pending timelocked signer/threshold-change before it is executed (admin-only).
    ///
    /// Once cancelled a proposal is permanently terminal — it cannot be executed or
    /// uncancelled. An admin wishing to proceed after cancellation must create a new
    /// proposal via `propose_signer_change`.
    ///
    /// Errors: `SignerChangeNotFound`, `SignerChangeAlreadyFinalised`.
    /// Emits: `signer_change_cancelled`.
    pub fn cancel_signer_change(
        env: Env,
        admin: Address,
        change_id: u64,
    ) -> Result<SignerChangeProposal, TreasuryError> {
        require_admin(&env, &admin);

        let mut proposal: SignerChangeProposal = env
            .storage()
            .persistent()
            .get(&DataKey::SignerChange(change_id))
            .ok_or(TreasuryError::SignerChangeNotFound)?;

        if proposal.status != SignerChangeStatus::Pending {
            return Err(TreasuryError::SignerChangeAlreadyFinalised);
        }

        proposal.status = SignerChangeStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::SignerChange(change_id), &proposal);

        env.events().publish(
            (Symbol::new(&env, "signer_change_cancelled"), change_id),
            proposal.clone(),
        );

        Ok(proposal)
    }

    /// Returns the `SignerChangeProposal` with the given `change_id`, or `None`
    /// if no proposal with that id exists.
    pub fn get_signer_change(env: Env, change_id: u64) -> Option<SignerChangeProposal> {
        env.storage()
            .persistent()
            .get(&DataKey::SignerChange(change_id))
    }

    /// Returns a page of `Pending` signer/threshold-change proposals (i.e. proposed but
    /// not yet executed or cancelled), in ascending `change_id` order, along with each
    /// one's `executable_at` timestamp — so monitoring tools can see every outstanding
    /// change and when it becomes executable without walking the full proposal history.
    ///
    /// `start_after` skips every id `<= start_after` (pass `0` to start from the
    /// beginning). `limit` is capped at `MAX_SIGNER_CHANGE_PAGE` to stay within Soroban
    /// budget limits.
    pub fn list_pending_signer_changes(
        env: Env,
        start_after: u64,
        limit: u32,
    ) -> Vec<SignerChangeProposal> {
        let capped_limit = if limit > MAX_SIGNER_CHANGE_PAGE {
            MAX_SIGNER_CHANGE_PAGE
        } else {
            limit
        };
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::SignerChangeCount)
            .unwrap_or(0);
        let mut page = Vec::new(&env);
        let mut id = start_after.saturating_add(1);
        while id <= count && (page.len() as u32) < capped_limit {
            if let Some(proposal) = env
                .storage()
                .persistent()
                .get::<DataKey, SignerChangeProposal>(&DataKey::SignerChange(id))
            {
                if proposal.status == SignerChangeStatus::Pending {
                    page.push_back(proposal);
                }
            }
            id += 1;
        }
        page
    }

    /// Returns the currently-configured signer/threshold-change timelock delay, in
    /// seconds (#732). Falls back to the 24 h default when it has never been changed
    /// via `propose_signer_change(UpdateTimelockDelay(..))` /
    /// `execute_signer_change`.
    pub fn get_timelock_delay(env: Env) -> u64 {
        current_timelock_delay(&env)
    }
}
