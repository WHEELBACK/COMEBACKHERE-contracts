#![no_std]
#![allow(deprecated)]

/// Protocol-wide basis-point denominator (10 000 = 100.00%) and typed helpers,
/// re-exported from `crates/protocol-math` so clients building against the
/// treasury resolve ratios against the same value the contract validates them
/// with (#707).
pub use protocol_math::{apply_bps_i128, is_valid_bps, split_bps_i128, BPS_DENOMINATOR};

pub use multisig::{
    DataKey, Dispute, DisputeStatus, MaybeAddress, RotationStatus, Settlement,
    SettlementHoldReason, SettlementStatus, SignerChangeKind, SignerChangeProposal,
    SignerChangeStatus, SignerRotationProposal,
};
// Explicit re-export so external consumers can import TreasuryError from this
// crate directly. TreasuryError is canonically defined in `crates/multisig`
// (the shared error authority for all treasury operations) and re-exported here
// for convenience. See the doc comment on `TreasuryError` in that crate for the
// full rationale.
pub use multisig::TreasuryError;

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env, IntoVal, Symbol, Vec};

mod access_control;
mod admin;
mod deposits;
mod disputes;
pub mod errors;
mod holds;
mod settlements;
mod signers;
mod timelock;

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Initialises the treasury with `admin` as owner and `threshold` as the multisig approval
    /// weight required to execute settlements. Accepts an initial `signers` list of `(Address, u32)`
    /// pairs to bootstrap the full signer set in a single transaction.
    /// Errors: `AlreadyInitialized`, `ZeroThreshold`.
    /// Emits: `treasury_initialized`.
    pub fn initialize(
        env: Env,
        admin: Address,
        threshold: u32,
        signers: Vec<(Address, u32)>,
    ) -> Result<(), TreasuryError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TreasuryError::AlreadyInitialized);
        }
        if threshold == 0 {
            return Err(TreasuryError::ZeroThreshold);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &threshold);
        env.storage()
            .instance()
            .set(&DataKey::SettlementCount, &0u64);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::DisputeCount, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::SettlementExpirySecs, &(7u64 * 24 * 60 * 60));
        env.storage()
            .instance()
            .set(&DataKey::Signer(admin.clone()), &1u32);
        let mut signer_list = Vec::new(&env);
        signer_list.push_back(admin.clone());
        for (signer, weight) in signers.iter() {
            env.storage()
                .instance()
                .set(&DataKey::Signer(signer.clone()), &weight);
            if weight > 0 && !signer_list.contains(&signer) {
                signer_list.push_back(signer.clone());
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::SignerList, &signer_list);
        // Bootstrap the admin as the initial role holder so granular role checks
        // (#969) are usable immediately after initialization.
        access_control::grant_role(&env, &admin, &access_control::role_admin(&env));
        verify_invariants(&env);
        env.events()
            .publish((Symbol::new(&env, "treasury_initialized"),), admin);
        // Extend instance storage TTL on initialization (#866) so the contract's
        // core configuration is not evicted before the first real activity bumps it.
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL_LEDGERS, TARGET_TTL_LEDGERS);
        Ok(())
    }
}

/// Maximum number of tokens allowed in the allowlist to prevent unbounded storage growth.
pub(crate) const MAX_ALLOWED_TOKENS: u32 = 20;

/// Default TTL (in ledgers) for a deposit reference id before it can be reused (#747).
pub(crate) const DEPOSIT_REF_TTL_LEDGERS: u32 = 30 * 24 * 60 * 60 / 5;

// ── Persistent-storage TTL constants (#866) ──────────────────────────────────
//
// Soroban persistent entries are evicted from the ledger once their TTL reaches
// zero.  We pro-actively extend them on every live write so a settlement that
// sits in `Pending` or `OnHold` for weeks never disappears mid-approval.
//
// At ~5 s per ledger:
//   MIN_TTL_LEDGERS  = 100_000 ≈  5.78 days  (don't bump if already this fresh)
//   TARGET_TTL_LEDGERS = 535_000 ≈ 30.9 days  (extend to this target when bumped)
//
// Callers: `bump_settlement_ttl` (called from `write_settlement` for live
// settlements) and the `initialize` instance-TTL bump.
/// Default TTL (in ledgers) applied to persistent dispute keys (#920).
#[allow(dead_code)]
pub(crate) const DISPUTE_TTL_LEDGERS: u32 = 30 * 24 * 60 * 60 / 5;

/// Bumps the TTL of a persistent dispute-related key (#920).
#[allow(dead_code)]
pub(crate) fn bump_persistent_ttl(env: &Env, key: &DataKey) {
    if env.storage().persistent().has(key) {
        env.storage()
            .persistent()
            .extend_ttl(key, DISPUTE_TTL_LEDGERS, DISPUTE_TTL_LEDGERS);
    }
}

pub(crate) const MIN_TTL_LEDGERS: u32 = 100_000;
pub(crate) const TARGET_TTL_LEDGERS: u32 = 535_000;

/// Extends the TTL of all persistent storage entries associated with
/// `settlement_id` (#866). Only bumps each key when it currently exists in
/// storage so this never accidentally creates a stale entry for a key that was
/// never written (e.g. `HoldExpiry` before the first hold, or
/// `PartialApprovedTotal`/`PartialExecutedTotal` before the first partial step).
///
/// Called by [`write_settlement`] for every write where the settlement status is
/// still live (`Pending` or `OnHold`). Terminal writes (Executed, Cancelled,
/// Expired) intentionally skip this call — eviction is fine for finished
/// settlements.
pub(crate) fn bump_settlement_ttl(env: &Env, id: u64) {
    // The main settlement record always exists when this is called.
    let key = DataKey::Settlement(id);
    env.storage()
        .persistent()
        .extend_ttl(&key, MIN_TTL_LEDGERS, TARGET_TTL_LEDGERS);

    // Optional companion keys: only extend if the key was previously written.
    let hold_key = DataKey::HoldExpiry(id);
    if env.storage().persistent().has(&hold_key) {
        env.storage()
            .persistent()
            .extend_ttl(&hold_key, MIN_TTL_LEDGERS, TARGET_TTL_LEDGERS);
    }

    let partial_approved_key = DataKey::PartialApprovedTotal(id);
    if env.storage().persistent().has(&partial_approved_key) {
        env.storage().persistent().extend_ttl(
            &partial_approved_key,
            MIN_TTL_LEDGERS,
            TARGET_TTL_LEDGERS,
        );
    }

    let partial_executed_key = DataKey::PartialExecutedTotal(id);
    if env.storage().persistent().has(&partial_executed_key) {
        env.storage().persistent().extend_ttl(
            &partial_executed_key,
            MIN_TTL_LEDGERS,
            TARGET_TTL_LEDGERS,
        );
    }
}

/// Verifies runtime invariants for a live settlement before it is written to
/// storage (#864). Returns `Err` for any impossible/corrupt state so the caller
/// surfaces a typed error rather than silently persisting bad data.
///
/// Only called on `Pending` and `OnHold` writes — terminal writes (Executed,
/// Cancelled, Expired) are allowed to bypass the guards so cleanup paths are
/// never blocked by a corrupt field.
///
/// Invariants checked:
/// 1. `settlement.amount > 0` — a zero-amount settlement can never execute.
/// 2. `settlement.approval_weight == 0 || !settlement.approvals.is_empty()` —
///    positive weight implies at least one approver was recorded; a mismatch
///    means the approval accounting drifted.
pub(crate) fn verify_settlement_invariants(
    env: &Env,
    settlement: &Settlement,
) -> Result<(), TreasuryError> {
    if settlement.amount <= 0 {
        return Err(TreasuryError::InvalidAmount);
    }
    // weight > 0 must imply at least one recorded approver.
    if settlement.approval_weight > 0 && settlement.approvals.is_empty() {
        soroban_sdk::panic_with_error!(env, TreasuryError::Unauthorized);
    }
    Ok(())
}

pub(crate) fn require_admin(env: &Env, admin: &Address) {
    admin.require_auth();
    // An uninitialized contract has no admin, so it fails closed as Unauthorized.
    let stored: Option<Address> = env.storage().instance().get(&DataKey::Admin);
    if stored.as_ref() != Some(admin) {
        soroban_sdk::panic_with_error!(env, TreasuryError::Unauthorized);
    }
}

/// Granular role guard for parameter-modifying entrypoints (#969). Requires the
/// caller's authorization and verifies it holds `role` in the access-control
/// role table. Fails closed with `Unauthorized` when the role is not granted.
#[allow(dead_code)]
pub(crate) fn require_role(env: &Env, caller: &Address, role: &Symbol) {
    caller.require_auth();
    if !access_control::has_role(env, caller, role) {
        soroban_sdk::panic_with_error!(env, TreasuryError::Unauthorized);
    }
}

/// Verifies the treasury's core storage invariants after a mutation (#972).
/// Panics with `TreasuryError::InvalidAmount` when the persisted state
/// is inconsistent so a bad write can never be committed silently.
pub(crate) fn verify_invariants(env: &Env) {
    if !access_control::invariants_hold(env) {
        soroban_sdk::panic_with_error!(env, TreasuryError::InvalidAmount);
    }
}

pub(crate) fn require_not_paused(env: &Env) {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        soroban_sdk::panic_with_error!(env, TreasuryError::ContractPaused);
    }
}

/// Enforces the recipient's compliance tier transaction limit before a settlement
/// transfers funds (#757). Calls `is_allowed_for_amount` on the configured
/// `compliance-client` contract; if the recipient is not allowed for `amount` the
/// settlement execution fails closed with `TreasuryError::ComplianceCheckFailed`.
///
/// When no compliance client is configured the check is skipped so existing
/// deployments without compliance keep working.
pub(crate) fn require_compliance_allowed(
    env: &Env,
    recipient: &Address,
    amount: i128,
) -> Result<(), TreasuryError> {
    let client: Option<Address> = env.storage().instance().get(&DataKey::ComplianceId);
    let Some(compliance) = client else {
        return Ok(());
    };
    let allowed: bool = env.invoke_contract(
        &compliance,
        &Symbol::new(env, "is_allowed_for_amount"),
        soroban_sdk::vec![env, recipient.into_val(env), amount.into_val(env)],
    );
    if !allowed {
        return Err(TreasuryError::ComplianceCheckFailed);
    }
    Ok(())
}

/// Checked subtraction used by partial settlement computations (#968). Returns
/// `TreasuryError::ArithmeticOverflow` instead of wrapping/panicking when
/// `amount` exceeds `available`, keeping partial settlement math defensive.
#[allow(dead_code)]
pub(crate) fn checked_sub_amount(available: i128, amount: i128) -> Result<i128, TreasuryError> {
    available
        .checked_sub(amount)
        .ok_or(TreasuryError::ArithmeticOverflow)
}

/// Writes `settlement` to storage and keeps `DataKey::PendingSettlementIndex` in sync
/// with its `status` (#572). This is the *only* place a `Settlement` should be persisted
/// in this crate — every write site (propose, approve, execute, cancel, expire, hold,
/// dispute-driven hold/release, force-cancel) goes through here so the index can never
/// drift from what `settlement.status == Pending` actually says, without having to
/// duplicate that add/remove bookkeeping at each call site individually.
///
/// For live settlements (`Pending` or `OnHold`):
/// * Calls [`verify_settlement_invariants`] to catch impossible/corrupt states
///   early (#864).
/// * Calls [`bump_settlement_ttl`] to extend persistent-entry TTLs so ledger
///   eviction cannot remove an in-flight settlement (#866).
pub(crate) fn write_settlement(env: &Env, id: u64, settlement: &Settlement) {
    // Run invariant guards and TTL bump only for live (non-terminal) settlements
    // (#864, #866). Terminal writes are allowed through unconditionally so that
    // cleanup paths (cancel, execute, expire, force-cancel) are never blocked by
    // a pre-existing corrupt field.
    let is_live = matches!(
        settlement.status,
        SettlementStatus::Pending | SettlementStatus::OnHold
    );
    if is_live {
        // Invariant guard (#864): panics or returns Err for corrupt states.
        // Use unwrap_or_else with panic_with_error! so the error is typed.
        if let Err(e) = verify_settlement_invariants(env, settlement) {
            soroban_sdk::panic_with_error!(env, e);
        }
    }

    env.storage()
        .persistent()
        .set(&DataKey::Settlement(id), settlement);

    // TTL bump (#866): extend after the write so the entry definitely exists.
    if is_live {
        bump_settlement_ttl(env, id);
    }

    let mut index: Vec<u64> = env
        .storage()
        .instance()
        .get(&DataKey::PendingSettlementIndex)
        .unwrap_or_else(|| Vec::new(env));
    if matches!(
        settlement.status,
        SettlementStatus::Executed | SettlementStatus::Cancelled | SettlementStatus::Expired
    ) {
        clear_hold_state(env, id);
    }

    let is_pending = settlement.status == SettlementStatus::Pending;
    let already_indexed = index.contains(id);

    if is_pending && !already_indexed {
        index.push_back(id);
        env.storage()
            .instance()
            .set(&DataKey::PendingSettlementIndex, &index);
    } else if !is_pending && already_indexed {
        let mut updated = Vec::new(env);
        for existing in index.iter() {
            if existing != id {
                updated.push_back(existing);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::PendingSettlementIndex, &updated);
    }
}

/// Clears any hold-related storage for `id` (#745). Called by `write_settlement`
/// on every terminal settlement transition (execute, cancel, expire, force-cancel)
/// so a held settlement never leaves a `HoldExpiry` entry behind once it is no
/// longer live. Idempotent: safe to call when no hold was ever set.
pub(crate) fn clear_hold_state(env: &Env, id: u64) {
    env.storage().persistent().remove(&DataKey::HoldExpiry(id));
}

/// Rejects a deposit whose reference id was already seen within the TTL window
/// (#747). Records `ref_id` with a TTL so storage stays bounded and the id can
/// be reused once the entry expires.
/// Errors: `DuplicateReferenceId`.
pub(crate) fn record_deposit_reference(env: &Env, ref_id: &Bytes) -> Result<(), TreasuryError> {
    let key = DataKey::DepositRef(ref_id.clone());
    if env.storage().temporary().has(&key) {
        return Err(TreasuryError::DuplicateReferenceId);
    }
    env.storage().temporary().set(&key, &true);
    env.storage()
        .temporary()
        .extend_ttl(&key, DEPOSIT_REF_TTL_LEDGERS, DEPOSIT_REF_TTL_LEDGERS);
    Ok(())
}

#[cfg(test)]
extern crate std;
