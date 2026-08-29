//! Compliance contract — admin-managed allowlist and blocklist for the COMEBACKHERE protocol.
//!
//! # Module layout
//!
//! All types, storage keys, error codes, and entrypoint logic live in this single file.
//! A refactor to split the data types and `DataKey` variants into a dedicated `allowlist.rs`
//! submodule was drafted (see issue #43 / branch `docs/protocol-glossary-and-compliance-stub-intent`)
//! but has been deferred pending owner sign-off. Until that split lands, `allowlist.rs` does
//! **not** exist in this crate — this file is the sole source of truth. Do not create a
//! stub `allowlist.rs` without completing the migration described in #43.
//!
//! # Contract responsibilities
//!
//! - Maintain a per-address allow/block state under [`DataKey::Allowed`] / [`DataKey::Blocked`].
//! - Support time-bound allowances via [`DataKey::AllowedUntil`].
//! - Expose [`ComplianceContract::is_allowed`] as the compliance gate read by `SettlementWorkflow`.
//! - Admin operations (allow, block, clear, pause) are gated on [`DataKey::Admin`] auth.
//!
//! See `contracts/compliance/README.md` for the full entrypoint reference and `is_allowed`
//! precedence rules, and `docs/GLOSSARY.md` for term definitions.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Bytes, Env, Symbol, Vec,
};

pub use compliance_errors::ComplianceError;

#[contracttype]
#[derive(Clone)]
/// Storage key enum for the compliance contract.
///
/// These variants were intended to move to a dedicated `allowlist.rs` submodule
/// (see issue #43). Until that refactor lands they remain here. Add new variants
/// at the end only — reordering breaks any stored data keyed by ordinal position.
pub enum DataKey {
    /// The active administrator address (instance storage).
    Admin,
    /// Staged address for the two-step admin transfer (`transfer_admin` / `accept_admin`).
    PendingAdmin,
    /// Reserved for a future operator role; not yet used by any entrypoint.
    Operator,
    /// Persistent flag: address is on the protocol allowlist.
    Allowed(Address),
    /// Persistent flag: address is blocked; overrides `Allowed` in `is_allowed`.
    Blocked(Address),
    /// Optional UNIX timestamp after which a temporary allow expires.
    AllowedUntil(Address),
    /// Optional UNIX timestamp after which an auto-expiring block is lifted.
    BlockedUntil(Address),
    /// Optional human-readable reason stored when an address is blocked.
    BlockReason(Address),
    /// Monotonically incrementing schema version; used for future migrations.
    SchemaVersion,
    /// Circuit-breaker flag — when `true`, administrative mutations are rejected.
    Paused,
    /// Index of all tracked addresses for `export_snapshot`; bounded by `MAX_TRACKED_ADDRESSES`.
    AddressIndex,
    /// Running count of addresses that have ever been allowed.
    AllowCount,
    /// Running count of addresses that have ever been blocked.
    BlockCount,
    /// Compliance tier recorded via `allow_address_with_tier`; `0` (basic KYC) if unset.
    /// See `get_address_tier`.
    Tier(Address),
    /// Optional jurisdiction code (e.g. ISO 3166 alpha-2, such as `US` or `EU`) under
    /// which an address's allow/block determination applies. Set via `set_jurisdiction`;
    /// unset (`None`) for addresses tracked before this field existed. Purely metadata —
    /// does not affect `is_allowed`.
    Jurisdiction(Address),
}

/// Coarse classification of an address's compliance state.
///
/// Returned by internal helpers; external callers should prefer [`AddressStatus`] or
/// the `is_allowed` / `is_blocked` entrypoints for authoritative state.
#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum AddressState {
    Allowed,
    Blocked,
    /// A time-bound allow or block whose expiry timestamp has passed.
    Expired,
}

/// Rich compliance status for a single address, returned by `address_status`.
///
/// Aggregates the raw storage flags into a single queryable struct so callers
/// do not need to call `is_allowed` and `is_blocked` separately.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AddressStatus {
    /// Whether the address has an active `Allowed` entry.
    pub allowed: bool,
    /// Whether the address has an active `Blocked` entry.
    pub blocked: bool,
    /// The `AllowedUntil` expiry timestamp, if a temporary allow was set.
    pub expires_at: Option<u64>,
    /// Computed result equivalent to calling `is_allowed` — factors in block,
    /// expiry, and precedence rules.
    pub is_currently_allowed: bool,
}

/// Primary error type for the compliance contract.
///
/// Variants must only be appended at the end (highest numeric value) to preserve
/// on-chain backwards compatibility. Range: 1..=6 (see `ARCHITECTURE.md`).
#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    Unauthorized = 1,
    ContractPaused = 2,
    AlreadyInitialized = 3,
    BatchTooLarge = 4,
    AddressIndexFull = 5,
    /// A bulk allow/block call was made before [`BULK_OP_COOLDOWN_SECS`] elapsed since the
    /// caller's previous bulk call (see #454).
    BulkOperationCooldown = 6,
}

/// Upper bound on the number of distinct addresses tracked in `DataKey::AddressIndex`.
/// Once reached, operations that would track a *new* address are rejected with
/// [`ContractError::AddressIndexFull`] instead of growing the index further — this
/// caps unbounded storage-rent growth. Existing tracked addresses are unaffected.
/// See `track_address`.
const MAX_TRACKED_ADDRESSES: u32 = 50_000;

/// Maximum number of addresses accepted per batch admin call, consistent with
/// the batch caps used elsewhere in the workspace (see #8/#21/#29).
pub const MAX_BATCH_SIZE: u32 = 50;

/// Minimum time (seconds) a caller must wait between successive calls to the *same* bulk
/// entrypoint (`bulk_allow_addresses` or `bulk_block_addresses`). `MAX_BATCH_SIZE` bounds how
/// many addresses a single call can affect, but without a time dimension a compromised admin
/// key could still call one bulk entrypoint repeatedly in quick succession and affect an
/// unbounded number of addresses in aggregate (see #454). Tracked separately per entrypoint
/// (rather than shared) so that legitimate admin flows — e.g. allowing a batch and then
/// immediately blocking a different batch — are not penalized for using both in succession.
pub const BULK_OP_COOLDOWN_SECS: u64 = 60;

#[contract]
pub struct ComplianceContract;

#[contractimpl]
impl ComplianceContract {
    /// Initialize the compliance contract with an admin address.
    ///
    /// # Parameters
    /// - `admin`: The initial administrator. Must authorize this call.
    ///
    /// # Errors
    /// - [`ContractError::AlreadyInitialized`] if the contract has already been initialized.
    ///
    /// # Events
    /// None emitted on initialization.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::SchemaVersion, &1u32);
        Ok(())
    }

    pub fn bulk_allow_addresses(
        env: Env,
        admin: Address,
        addresses: Vec<Address>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        Self::check_bulk_op_cooldown(&env, DataKey::LastBulkAllow(admin.clone()))?;
        if addresses.len() > MAX_BATCH_SIZE {
            return Err(ContractError::BatchTooLarge);
        }
        for address in addresses.iter() {
            let was_allowed: bool = env
                .storage()
                .persistent()
                .get(&DataKey::Allowed(address.clone()))
                .unwrap_or(false);
            env.storage()
                .persistent()
                .set(&DataKey::Allowed(address.clone()), &true);
            env.storage()
                .persistent()
                .remove(&DataKey::AllowedUntil(address.clone()));
            if !was_allowed {
                let count: u64 = env
                    .storage()
                    .instance()
                    .get(&DataKey::AllowCount)
                    .unwrap_or(0u64);
                env.storage().instance().set(
                    &DataKey::AllowCount,
                    &(count
                        .checked_add(1)
                        .unwrap_or_else(|| panic!("ArithmeticOverflow"))),
                );
            }
            Self::track_address(&env, &address)?;
            env.events()
                .publish((Symbol::new(&env, "address_allowed"),), address);
        }
        Ok(())
    }

    pub fn bulk_check_addresses(env: Env, addresses: Vec<Address>) -> Vec<bool> {
        let mut results = Vec::new(&env);
        for address in addresses.iter() {
            results.push_back(Self::is_allowed(env.clone(), address));
        }
        results
    }

    pub fn is_allowed(env: Env, address: Address) -> bool {
        let blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(address.clone()))
            .unwrap_or(false);
        if blocked {
            // If there's a BlockedUntil timestamp, the block auto-expires once now >= unblock_at.
            if let Some(unblock_at) = env
                .storage()
                .persistent()
                .get::<_, u64>(&DataKey::BlockedUntil(address.clone()))
            {
                if env.ledger().timestamp() >= unblock_at {
                    // Block has expired — fall through to allow check below.
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        let allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(address.clone()))
            .unwrap_or(false);
        if !allowed {
            return false;
        }
        // Check optional expiry
        if let Some(expires_at) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::AllowedUntil(address))
        {
            return env.ledger().timestamp() < expires_at;
        }
        true
    }

    /// Returns whether `address` is explicitly blocked. No auth required.
    pub fn is_blocked(env: Env, address: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Blocked(address))
            .unwrap_or(false)
    }

    /// Permanently allow an address. Removes any existing expiry.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    /// - `address`: The address to allow.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    /// - [`ContractError::ContractPaused`] if the contract is paused.
    ///
    /// # Events
    /// Publishes `("address_allowed",) → address`.
    pub fn allow_address(env: Env, admin: Address, address: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        let was_allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(address.clone()))
            .unwrap_or(false);
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        // Remove any expiry so this becomes a permanent allow.
        env.storage()
            .persistent()
            .remove(&DataKey::AllowedUntil(address.clone()));
        if !was_allowed {
            let count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::AllowCount)
                .unwrap_or(0u64);
            env.storage().instance().set(
                &DataKey::AllowCount,
                &(count
                    .checked_add(1)
                    .unwrap_or_else(|| panic!("ArithmeticOverflow"))),
            );
        }
        Self::track_address(&env, &address)?;
        env.events()
            .publish((Symbol::new(&env, "address_allowed"),), address);
        Ok(())
    }

    /// Allow an address and record its compliance tier.
    ///
    /// Tier 0 = basic KYC, higher values = enhanced / institutional.
    /// The tier is stored independently and does not affect `is_allowed` logic;
    /// callers (e.g. the treasury contract) read it via `get_address_tier`.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    /// - [`ContractError::ContractPaused`] if the contract is paused.
    pub fn allow_address_with_tier(
        env: Env,
        admin: Address,
        address: Address,
        tier: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        env.storage()
            .persistent()
            .remove(&DataKey::AllowedUntil(address.clone()));
        env.storage()
            .persistent()
            .set(&DataKey::Tier(address.clone()), &tier);
        Self::track_address(&env, &address)?;
        env.events()
            .publish((Symbol::new(&env, "address_allowed"),), address);
        Ok(())
    }

    /// Returns the stored compliance tier for `address`, or `0` if none has been set.
    pub fn get_address_tier(env: Env, address: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::Tier(address))
            .unwrap_or(0u32)
    }

    /// Block a batch of addresses (admin-only).
    ///
    /// Like [`block_address`](Self::block_address), this is **not** gated behind
    /// [`require_not_paused`](Self::require_not_paused) — it is permitted while
    /// paused as part of the same emergency-remediation policy, so an admin can
    /// sanction an entire batch of compromised addresses without unpausing first.
    pub fn bulk_block_addresses(
        env: Env,
        admin: Address,
        addresses: Vec<Address>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::check_bulk_op_cooldown(&env, DataKey::LastBulkBlock(admin.clone()))?;
        if addresses.len() > MAX_BATCH_SIZE {
            return Err(ContractError::BatchTooLarge);
        }
        for address in addresses.iter() {
            env.storage()
                .persistent()
                .set(&DataKey::Blocked(address.clone()), &true);
            Self::track_address(&env, &address)?;
            env.events()
                .publish((Symbol::new(&env, "address_blocked"),), address);
        }
        Ok(())
    }

    // Emergency policy: block_address and clear_address are permitted while paused
    // so the admin can remediate compromised addresses without unpausing first.
    pub fn block_address(
        env: Env,
        admin: Address,
        address: Address,
        reason: Option<Bytes>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(address.clone()), &true);
        if let Some(r) = reason {
            env.storage()
                .persistent()
                .set(&DataKey::BlockReason(address.clone()), &r);
        }
        Self::track_address(&env, &address)?;
        env.events()
            .publish((Symbol::new(&env, "address_blocked"),), address);
        Ok(())
    }

    /// Block an address until a specific ledger timestamp. Permitted while paused (emergency policy).
    pub fn block_address_until(
        env: Env,
        admin: Address,
        address: Address,
        unblock_at: u64,
        reason: Option<Bytes>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(address.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::BlockedUntil(address.clone()), &unblock_at);
        if let Some(r) = reason {
            env.storage()
                .persistent()
                .set(&DataKey::BlockReason(address.clone()), &r);
        }
        Self::track_address(&env, &address)?;
        env.events().publish(
            (Symbol::new(&env, "address_blocked_until"),),
            (address, unblock_at),
        );
        Ok(())
    }

    /// Returns the stored block reason for an address, if any.
    pub fn get_block_reason(env: Env, address: Address) -> Option<Bytes> {
        env.storage()
            .persistent()
            .get(&DataKey::BlockReason(address))
    }

    /// Returns the schema version set at initialization.
    pub fn get_schema_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::SchemaVersion)
            .unwrap_or(1)
    }

    /// Allow an address until a specific ledger timestamp (seconds since epoch).
    ///
    /// After `expires_at`, [`is_allowed`](Self::is_allowed) returns `false` even if
    /// the `Allowed` flag is set.
    ///
    /// A `expires_at` that is already in the past (or equal to the current ledger
    /// timestamp) is accepted rather than rejected: the address is recorded as
    /// allowed, but [`is_allowed`](Self::is_allowed) evaluates the expiry lazily on
    /// every read, so it immediately reports `false`. This is a deliberate silent
    /// no-op-allow rather than an error, keeping the entrypoint idempotent for
    /// callers that pass a computed/stale timestamp.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    /// - `address`: The address to allow temporarily.
    /// - `expires_at`: Unix timestamp (seconds) after which the allowance expires.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    /// - [`ContractError::ContractPaused`] if the contract is paused.
    ///
    /// # Events
    /// Publishes `("address_allowed_until",) → (address, expires_at)`.
    pub fn allow_address_until(
        env: Env,
        admin: Address,
        address: Address,
        expires_at: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::AllowedUntil(address.clone()), &expires_at);
        Self::track_address(&env, &address)?;
        env.events().publish(
            (Symbol::new(&env, "address_allowed_until"),),
            (address, expires_at),
        );
        Ok(())
    }

    /// Initiate a two-step admin transfer. The pending admin must call
    /// [`accept_admin`](Self::accept_admin) to complete the handover.
    ///
    /// Calling this again before the pending admin accepts fully **supersedes** the
    /// previous nomination — `PendingAdmin` is a plain overwrite, not a queue. So a
    /// lost or compromised pending-admin key does not leave the contract stuck: the
    /// current admin can simply call `transfer_admin` again with a fresh address to
    /// replace it, with no separate expiry/timeout mechanism required.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    /// - `new_admin`: The address being nominated as the next administrator.
    ///
    /// Not gated behind [`require_not_paused`](Self::require_not_paused): admin-role
    /// management is orthogonal to the allow/block mutations that pause is meant to
    /// stop, and must remain available even while paused (e.g. to hand control to a
    /// recovery admin during an incident).
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    ///
    /// # Events
    /// Publishes `("admin_transfer_initiated",) → new_admin`.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        env.events()
            .publish((Symbol::new(&env, "admin_transfer_initiated"),), new_admin);
        Ok(())
    }

    /// Complete the admin transfer initiated by [`transfer_admin`](Self::transfer_admin).
    ///
    /// Must be called by the pending admin to activate the new admin role.
    /// Like `transfer_admin`, this is not gated behind `require_not_paused` and
    /// works while the contract is paused.
    ///
    /// # Parameters
    /// - `new_admin`: The pending administrator. Must authorize this call.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `new_admin` does not match the stored pending admin.
    ///
    /// # Panics
    /// Panics with `"NoPendingAdmin"` if [`transfer_admin`](Self::transfer_admin) was never called.
    ///
    /// # Events
    /// Publishes `("admin_transferred",) → new_admin`.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), ContractError> {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .expect("NoPendingAdmin");
        if pending != new_admin {
            return Err(ContractError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.events()
            .publish((Symbol::new(&env, "admin_transferred"),), new_admin);
        Ok(())
    }

    /// Remove the block flag and explicitly allow an address.
    ///
    /// Permitted even while paused (emergency policy). Does **not** remove an existing
    /// `AllowedUntil` expiry; call [`allow_address`](Self::allow_address) for a
    /// permanent, expiry-free allow.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    /// - `address`: The address to clear.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    ///
    /// # Events
    /// Publishes `("address_cleared",) → address`.
    pub fn clear_address(env: Env, admin: Address, address: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let was_blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(address.clone()))
            .unwrap_or(false);
        let was_allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(address.clone()))
            .unwrap_or(false);
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(address.clone()), &false);
        env.storage()
            .persistent()
            .remove(&DataKey::BlockedUntil(address.clone()));
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        if was_blocked {
            let count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::BlockCount)
                .unwrap_or(0u64);
            env.storage()
                .instance()
                .set(&DataKey::BlockCount, &count.saturating_sub(1));
        }
        if !was_allowed {
            let count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::AllowCount)
                .unwrap_or(0u64);
            env.storage().instance().set(
                &DataKey::AllowCount,
                &(count
                    .checked_add(1)
                    .unwrap_or_else(|| panic!("ArithmeticOverflow"))),
            );
        }
        Self::track_address(&env, &address)?;
        env.events()
            .publish((Symbol::new(&env, "address_cleared"),), address);
        Ok(())
    }

    /// Remove the allowed status for an address without blocking it.
    /// This is a soft de-listing: the address is removed from the allowlist
    /// but not placed on the blocklist, so it can be re-allowed later.
    pub fn revoke_allow(env: Env, admin: Address, address: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        env.storage()
            .persistent()
            .remove(&DataKey::Allowed(address.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::AllowedUntil(address.clone()));
        Self::track_address(&env, &address)?;
        env.events()
            .publish((Symbol::new(&env, "address_revoked"),), address);
        Ok(())
    }

    pub fn pause(env: Env, admin: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "compliance_paused"),), admin);
        Ok(())
    }

    /// Resume normal operation after a pause.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    ///
    /// # Events
    /// Publishes `("compliance_unpaused",) → admin`.
    pub fn unpause(env: Env, admin: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "compliance_unpaused"),), admin);
        Ok(())
    }

    /// Assign an operator address. Only admin may call this.
    /// Not gated behind `require_not_paused` — role assignment, like admin
    /// transfer, is permitted while paused.
    pub fn set_operator(env: Env, admin: Address, operator: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Operator, &operator);
        env.events()
            .publish((Symbol::new(&env, "operator_set"),), operator);
        Ok(())
    }

    /// Returns the raw expiry timestamp (seconds since epoch) for `address`, or
    /// `None` if the address has no time-limited allow entry (permanent allow or no allow).
    pub fn get_allow_expiry(env: Env, address: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get::<_, u64>(&DataKey::AllowedUntil(address))
    }

    /// Sweep tracked addresses for lapsed time-bound allow entries.
    ///
    /// `is_allowed` checks `AllowedUntil` lazily on every read, so there is
    /// otherwise no discrete moment at which an expiry "happens" and no event is
    /// emitted when a time-bound allow naturally lapses. This entrypoint gives
    /// callers/indexers an explicit point to trigger and observe that transition:
    /// for every tracked address whose `AllowedUntil` has passed, it clears the
    /// `Allowed` flag, removes the expiry, and publishes `("address_allow_expired",) → address`.
    ///
    /// Returns the number of addresses swept.
    ///
    /// Not gated behind `require_not_paused`: sweeping only clears already-lapsed
    /// time-bound allows, so it is treated as bookkeeping rather than a new grant
    /// of access, and admins may run it even while paused.
    pub fn sweep_expired(env: Env, admin: Address) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;
        let index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AddressIndex)
            .unwrap_or(Vec::new(&env));
        let now = env.ledger().timestamp();
        let mut swept = 0u32;
        for addr in index.iter() {
            let allowed: bool = env
                .storage()
                .persistent()
                .get(&DataKey::Allowed(addr.clone()))
                .unwrap_or(false);
            if !allowed {
                continue;
            }
            let expires_at: Option<u64> = env
                .storage()
                .persistent()
                .get(&DataKey::AllowedUntil(addr.clone()));
            if let Some(expires_at) = expires_at {
                if now >= expires_at {
                    env.storage()
                        .persistent()
                        .set(&DataKey::Allowed(addr.clone()), &false);
                    env.storage()
                        .persistent()
                        .remove(&DataKey::AllowedUntil(addr.clone()));
                    let count: u64 = env
                        .storage()
                        .instance()
                        .get(&DataKey::AllowCount)
                        .unwrap_or(0u64);
                    env.storage()
                        .instance()
                        .set(&DataKey::AllowCount, &count.saturating_sub(1));
                    env.events()
                        .publish((Symbol::new(&env, "address_allow_expired"),), addr.clone());
                    swept += 1;
                }
            }
        }
        Ok(swept)
    }

    /// Returns a paginated snapshot of all tracked addresses and their current state.
    /// Pass `offset=0, limit=0` to return all entries.
    pub fn export_snapshot(
        env: Env,
        admin: Address,
        offset: u32,
        limit: u32,
    ) -> Vec<(Address, AddressState)> {
        Self::require_admin(&env, &admin).unwrap();
        let index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AddressIndex)
            .unwrap_or(Vec::new(&env));
        let mut result = Vec::new(&env);
        let start = offset as usize;
        let end = if limit == 0 {
            index.len() as usize
        } else {
            (start + limit as usize).min(index.len() as usize)
        };
        for i in start..end {
            let addr = index.get(i as u32).unwrap();
            let state = Self::address_state(&env, &addr);
            result.push_back((addr, state));
        }
        result
    }

    /// Returns a page of tracked addresses and their current state: skips the first
    /// `start` entries and returns up to `limit`, following the same pagination
    /// convention as treasury's `get_pending_settlements_page`.
    pub fn export_snapshot_page(
        env: Env,
        admin: Address,
        start: u64,
        limit: u64,
    ) -> Vec<(Address, AddressState)> {
        Self::require_admin(&env, &admin).unwrap();
        let index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AddressIndex)
            .unwrap_or(Vec::new(&env));
        let mut result = Vec::new(&env);
        let total = index.len() as u64;
        let mut i = start;
        while i < total && (result.len() as u64) < limit {
            let addr = index.get(i as u32).unwrap();
            let state = Self::address_state(&env, &addr);
            result.push_back((addr, state));
            i += 1;
        }
        result
    }

    /// Returns the compliance state for `address` (Allowed, Blocked, or Expired).
    /// Requires admin or operator authentication.
    pub fn address_status(
        env: Env,
        caller: Address,
        address: Address,
    ) -> Result<AddressState, ContractError> {
        Self::require_admin_or_operator(&env, &caller)?;
        let blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(address.clone()))
            .unwrap_or(false);
        if blocked {
            return Ok(AddressState::Blocked);
        }
        let allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(address.clone()))
            .unwrap_or(false);
        if !allowed {
            return Ok(AddressState::Blocked);
        }
        if let Some(expires_at) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::AllowedUntil(address))
        {
            if env.ledger().timestamp() < expires_at {
                Ok(AddressState::Allowed)
            } else {
                Ok(AddressState::Expired)
            }
        } else {
            Ok(AddressState::Allowed)
        }
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        admin.require_auth();
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored != *admin {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }

    fn require_admin_or_operator(env: &Env, caller: &Address) -> Result<(), ContractError> {
        caller.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored_admin == *caller {
            return Ok(());
        }
        if let Some(operator) = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::Operator)
        {
            if operator == *caller {
                return Ok(());
            }
        }
        Err(ContractError::Unauthorized)
    }

    /// Enforces [`BULK_OP_COOLDOWN_SECS`] between successive calls keyed by `key`
    /// (a `LastBulkAllow`/`LastBulkBlock` variant), and records `now` as the new
    /// last-call timestamp on success.
    fn check_bulk_op_cooldown(env: &Env, key: DataKey) -> Result<(), ContractError> {
        let now = env.ledger().timestamp();
        if let Some(last) = env.storage().instance().get::<_, u64>(&key) {
            if now < last.saturating_add(BULK_OP_COOLDOWN_SECS) {
                return Err(ContractError::BulkOperationCooldown);
            }
        }
        env.storage().instance().set(&key, &now);
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), ContractError> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            return Err(ContractError::ContractPaused);
        }
        Ok(())
    }

    /// Compute the current [`AddressState`] for a single address without auth.
    fn address_state(env: &Env, addr: &Address) -> AddressState {
        let blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(addr.clone()))
            .unwrap_or(false);
        if blocked {
            return AddressState::Blocked;
        }
        let allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(addr.clone()))
            .unwrap_or(false);
        if !allowed {
            return AddressState::Blocked;
        }
        if let Some(expires_at) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::AllowedUntil(addr.clone()))
        {
            if env.ledger().timestamp() < expires_at {
                AddressState::Allowed
            } else {
                AddressState::Expired
            }
        } else {
            AddressState::Allowed
        }
    }

    /// Adds `address` to the instance-level AddressIndex if not already present.
    ///
    /// # Errors
    /// - [`ContractError::AddressIndexFull`] if `address` is new and the index has
    ///   already reached [`MAX_TRACKED_ADDRESSES`].
    fn track_address(env: &Env, address: &Address) -> Result<(), ContractError> {
        let mut index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AddressIndex)
            .unwrap_or(Vec::new(env));
        if !index.contains(address) {
            if index.len() >= MAX_TRACKED_ADDRESSES {
                return Err(ContractError::AddressIndexFull);
            }
            index.push_back(address.clone());
            env.storage().instance().set(&DataKey::AddressIndex, &index);
        }
        Ok(())
    }
}

#[cfg(test)]
extern crate std;
