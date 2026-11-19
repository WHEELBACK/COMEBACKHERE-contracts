#![no_std]

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
        Ok(())
    }
}

/// Maximum number of tokens allowed in the allowlist to prevent unbounded storage growth.
pub(crate) const MAX_ALLOWED_TOKENS: u32 = 20;

/// Default TTL (in ledgers) for a deposit reference id before it can be reused (#747).
pub(crate) const DEPOSIT_REF_TTL_LEDGERS: u32 = 30 * 24 * 60 * 60 / 5;

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
pub(crate) fn require_role(env: &Env, caller: &Address, role: &Symbol) {
    caller.require_auth();
    if !access_control::has_role(env, caller, role) {
        soroban_sdk::panic_with_error!(env, TreasuryError::Unauthorized);
    }
}

/// Verifies the treasury's core storage invariants after a mutation (#972).
/// Panics with `TreasuryError::StateInvariantViolated` when the persisted state
/// is inconsistent so a bad write can never be committed silently.
pub(crate) fn verify_invariants(env: &Env) {
    if !access_control::invariants_hold(env) {
        soroban_sdk::panic_with_error!(env, TreasuryError::StateInvariantViolated);
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
    let client: Option<Address> = env
        .storage()
        .instance()
        .get(&DataKey::ComplianceId);
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
/// `TreasuryError::ArithmeticUnderflow` instead of wrapping/panicking when
/// `amount` exceeds `available`, keeping partial settlement math defensive.
pub(crate) fn checked_sub_amount(
    available: i128,
    amount: i128,
) -> Result<i128, TreasuryError> {
    available
        .checked_sub(amount)
        .ok_or(TreasuryError::ArithmeticUnderflow)
}

/// Writes `settlement` to storage and keeps `DataKey::PendingSettlementIndex` in sync
/// with its `status` (#572). This is the *only* place a `Settlement` should be persisted
/// in this crate — every write site (propose, approve, execute, cancel, expire, hold,
/// dispute-driven hold/release, force-cancel) goes through here so the index can never
/// drift from what `settlement.status == Pending` actually says, without having to
/// duplicate that add/remove bookkeeping at each call site individually.
pub(crate) fn write_settlement(env: &Env, id: u64, settlement: &Settlement) {
    env.storage()
        .persistent()
        .set(&DataKey::Settlement(id), settlement);

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
