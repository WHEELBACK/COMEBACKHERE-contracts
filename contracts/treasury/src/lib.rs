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
        env.events()
            .publish((Symbol::new(&env, "treasury_initialized"),), admin);
        Ok(())
    }
}

/// Maximum number of tokens allowed in the allowlist to prevent unbounded storage growth.
pub(crate) const MAX_ALLOWED_TOKENS: u32 = 20;

/// Default TTL (in ledgers) for a deposit reference id before it can be reused (#747).
pub(crate) const DEPOSIT_REF_TTL_LEDGERS: u32 = 30 * 24 * 60 * 60 / 5;

/// Ledger-count threshold below which a persistent storage entry is considered
/// "at risk" of eviction and gets its TTL bumped (#965). Roughly 30 days at
/// 5-second ledgers.
pub(crate) const STORAGE_LAYOUT_TTL_THRESHOLD: u32 = 30 * 24 * 60 * 60 / 5;

/// Ledger-count the TTL is extended to when a persistent entry is bumped (#965).
/// Roughly 90 days at 5-second ledgers, giving operators ample time to act on
/// long-lived settlements and disputes before they can be archived.
pub(crate) const STORAGE_LAYOUT_TTL_EXTEND_TO: u32 = 90 * 24 * 60 * 60 / 5;

/// Bumps the TTL of a persistent storage entry so open records cannot be
/// archived out from under the contract (#965). Called on every persistent
/// `Settlement`/`Dispute` read/write path. Idempotent and cheap when the entry
/// is already above `STORAGE_LAYOUT_TTL_THRESHOLD`.
pub(crate) fn bump_persistent_ttl(env: &Env, key: &DataKey) {
    env.storage().persistent().extend_ttl(
        key,
        STORAGE_LAYOUT_TTL_THRESHOLD,
        STORAGE_LAYOUT_TTL_EXTEND_TO,
    );
}

pub(crate) fn require_admin(env: &Env, admin: &Address) {
    admin.require_auth();
    // An uninitialized contract has no admin, so it fails closed as Unauthorized.
    let stored: Option<Address> = env.storage().instance().get(&DataKey::Admin);
    if stored.as_ref() != Some(admin) {
        soroban_sdk::panic_with_error!(env, TreasuryError::Unauthorized);
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

/// Writes `settlement` to storage and keeps `DataKey::PendingSettlementIndex` in sync
/// with its `status` (#572). This is the *only* place a `Settlement` should be persisted
/// in this crate — every write site (propose, approve, execute, cancel, expire, hold,
/// dispute-driven hold/release, force-cancel) goes through here so the index can never
/// drift from what `settlement.status == Pending` actually says, without having to
/// duplicate that add/remove bookkeeping at each call site individually.
pub(crate) fn write_settlement(env: &Env, id: u64, settlement: &Settlement) {
    let key = DataKey::Settlement(id);
    env.storage().persistent().set(&key, settlement);
    bump_persistent_ttl(env, &key);

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

/// Reads a persistent `Settlement`, bumping its TTL so an open record cannot be
/// archived while it is still being acted upon (#965).
pub(crate) fn read_settlement(env: &Env, id: u64) -> Option<Settlement> {
    let key = DataKey::Settlement(id);
    let settlement: Option<Settlement> = env.storage().persistent().get(&key);
    if settlement.is_some() {
        bump_persistent_ttl(env, &key);
    }
    settlement
}

/// Reads a persistent `Dispute`, bumping its TTL so an open record cannot be
/// archived while it is still being acted upon (#965).
pub(crate) fn read_dispute(env: &Env, id: u64) -> Option<Dispute> {
    let key = DataKey::Dispute(id);
    let dispute: Option<Dispute> = env.storage().persistent().get(&key);
    if dispute.is_some() {
        bump_persistent_ttl(env, &key);
    }
    dispute
}

/// Writes a persistent `Dispute` and bumps its TTL (#965).
pub(crate) fn write_dispute(env: &Env, id: u64, dispute: &Dispute) {
    let key = DataKey::Dispute(id);
    env.storage().persistent().set(&key, dispute);
    bump_persistent_ttl(env, &key);
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

#[cfg(test)]
mod ttl_tests {
    use super::*;
    use soroban_sdk::testutils::storage::Persistent as _;
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::{Address, Env};

    fn setup() -> (Env, TreasuryContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(TreasuryContract, ());
        let client = TreasuryContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin, &1u32, &Vec::new(&env));
        (env, client, admin)
    }

    #[test]
    fn write_settlement_bumps_persistent_ttl() {
        let (env, _client, _admin) = setup();
        let id = 1u64;
        let settlement = Settlement {
            id,
            status: SettlementStatus::Pending,
            ..Default::default()
        };
        env.as_contract(&env.current_contract_address(), || {
            write_settlement(&env, id, &settlement);
            let ttl = env.storage().persistent().get_ttl(&DataKey::Settlement(id));
            assert!(ttl >= STORAGE_LAYOUT_TTL_THRESHOLD);
        });
    }

    #[test]
    fn read_settlement_bumps_persistent_ttl() {
        let (env, _client, _admin) = setup();
        let id = 2u64;
        let settlement = Settlement {
            id,
            status: SettlementStatus::Pending,
            ..Default::default()
        };
        env.as_contract(&env.current_contract_address(), || {
            write_settlement(&env, id, &settlement);
            // Advance the ledger so the TTL drops below the threshold.
            env.ledger()
                .set_sequence_number(env.ledger().sequence() + STORAGE_LAYOUT_TTL_THRESHOLD + 1);
            let before = env.storage().persistent().get_ttl(&DataKey::Settlement(id));
            assert!(before < STORAGE_LAYOUT_TTL_THRESHOLD);
            let _ = read_settlement(&env, id);
            let after = env.storage().persistent().get_ttl(&DataKey::Settlement(id));
            assert!(after >= STORAGE_LAYOUT_TTL_THRESHOLD);
        });
    }

    #[test]
    fn read_dispute_bumps_persistent_ttl() {
        let (env, _client, _admin) = setup();
        let id = 3u64;
        let dispute = Dispute {
            id,
            status: DisputeStatus::Open,
            ..Default::default()
        };
        env.as_contract(&env.current_contract_address(), || {
            write_dispute(&env, id, &dispute);
            env.ledger()
                .set_sequence_number(env.ledger().sequence() + STORAGE_LAYOUT_TTL_THRESHOLD + 1);
            let before = env.storage().persistent().get_ttl(&DataKey::Dispute(id));
            assert!(before < STORAGE_LAYOUT_TTL_THRESHOLD);
            let _ = read_dispute(&env, id);
            let after = env.storage().persistent().get_ttl(&DataKey::Dispute(id));
            assert!(after >= STORAGE_LAYOUT_TTL_THRESHOLD);
        });
    }

    #[test]
    fn bump_persistent_ttl_is_idempotent() {
        let (env, _client, _admin) = setup();
        let id = 4u64;
        let settlement = Settlement {
            id,
            status: SettlementStatus::Pending,
            ..Default::default()
        };
        env.as_contract(&env.current_contract_address(), || {
            write_settlement(&env, id, &settlement);
            let first = env.storage().persistent().get_ttl(&DataKey::Settlement(id));
            bump_persistent_ttl(&env, &DataKey::Settlement(id));
            let second = env.storage().persistent().get_ttl(&DataKey::Settlement(id));
            // A second bump when already above the threshold must not shrink the TTL.
            assert!(second >= first);
        });
    }
}
