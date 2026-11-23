//! Role manager contract for the COMEBACKHERE protocol.
//!
//! The contract is the single on-chain registry for protocol roles. Other
//! contracts call [`RoleManagerInterface::require_role`] (or, once granular
//! permissions are layered on top, the permission-equivalent entrypoint) before
//! they mutate a parameter, so the authorization policy lives in one auditable
//! place instead of being duplicated in every consumer.
//!
//! Role membership is persistent storage: it survives contract-instance
//! upgrades and is keyed by both [`Role`] and member [`Address`], so granting a
//! role never has to rewrite a monolithic member list.
//!
//! See `contracts/role-manager/README.md` for the entrypoint and auth reference.

#![no_std]

use core::fmt::Debug;
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env, IntoVal,
    Symbol, TryFromVal, Val,
};

/// Ledger lifetime remaining below which a persistent key is refreshed.
///
/// The network's default lifetime for a newly written persistent entry is 4096
/// ledgers, so the threshold has to be at least that high for a fresh key to be
/// extended on its first write. At ~5s per ledger this is about 5.7 hours.
pub const PERSISTENT_TTL_THRESHOLD: u32 = 4_096;

/// New lifetime applied when a persistent key falls below the threshold.
///
/// Ten thousand ledgers (~14 hours) keeps hot authorization entries alive while
/// remaining far below the network's maximum TTL, so a key that stops being
/// used can still age out naturally instead of being pinned forever.
pub const PERSISTENT_TTL_EXTEND_TO: u32 = 10_000;

/// Instance threshold, kept in step with persistent keys so the admin pointer
/// cannot expire while the role entries it controls are still alive.
pub const INSTANCE_TTL_THRESHOLD: u32 = PERSISTENT_TTL_THRESHOLD;

/// Instance lifetime applied on state-changing entrypoints.
pub const INSTANCE_TTL_EXTEND_TO: u32 = PERSISTENT_TTL_EXTEND_TO;

/// Protocol roles tracked by the registry.
///
/// Roles are deliberately coarse at this layer: each role is a capability
/// grouping that consumers check with [`RoleManagerContract::require_role`].
/// Fine-grained parameter permissions are derived from these roles by the
/// permission layer so a role change is the only write needed to revoke a whole
/// class of parameter changes.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    /// Can administer the registry itself (grant/revoke roles).
    Admin,
    /// Can perform day-to-day operational parameter changes.
    Operator,
    /// Can change compliance-related parameters.
    ComplianceOfficer,
    /// Can change treasury signing parameters.
    TreasurySigner,
    /// Can pause protocol contracts.
    Pauser,
}

/// Granular capabilities that a role may grant.
///
/// Every parameter-changing entrypoint in a consumer should name exactly one
/// of these, rather than accepting "some admin-ish role". That keeps the blast
/// radius of a leaked key to a single operation: holding `Pauser` does not let
/// somebody rewrite settlement thresholds, and holding `Admin` does not bypass
/// the parameter-specific checks.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    /// Grant and revoke roles in this registry.
    ManageRoles,
    /// Change operational thresholds (e.g. settlement quorum).
    SetThreshold,
    /// Change treasury signer weights.
    SetSignerWeight,
    /// Change compliance tier limits.
    SetTierLimit,
    /// Pause or unpause protocol contracts.
    PauseProtocol,
}

/// Storage keys for the role manager.
///
/// `Admin` lives in instance storage because it is a single small value.
/// Role memberships live in persistent storage so an instance upgrade cannot
/// silently drop authorizations. New variants must only be appended.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The administrator allowed to grant and revoke roles.
    Admin,
    /// `true` when `member` holds `role`; absent means "does not hold".
    RoleMember(Role, Address),
    /// Non-negative action budget for `member` in `role`; absent means zero.
    ///
    /// Budgets are persistent because they represent an issued allowance that
    /// must survive an instance upgrade, just like role membership.
    Budget(Role, Address),
}

/// Errors returned by the role manager.
///
/// Append-only: existing codes are part of the on-chain ABI and clients may
/// match on them by number.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RoleManagerError {
    /// `initialize` was called more than once.
    AlreadyInitialized = 1,
    /// The caller is not the stored administrator.
    Unauthorized = 2,
    /// The caller does not hold the role a consumer required.
    RoleNotAssigned = 3,
    /// A budget amount was negative or otherwise unusable.
    InvalidAmount = 4,
    /// Crediting a budget would overflow `i128`.
    BudgetOverflow = 5,
    /// The caller's budget is zero or smaller than the requested debit.
    InsufficientBudget = 6,
    /// No role held by the caller grants the requested permission.
    PermissionDenied = 7,
    /// The target contract rejected or failed the parameter write.
    ParameterTargetRejected = 8,
}

/// Minimal call surface external contracts need from the role manager.
///
/// Declaring only the two check entrypoints as a client means a consumer does
/// not link the role manager's own wasm exports (which would collide at build
/// time if the consumer were also compiled as a contract). The real methods
/// return `Result<(), RoleManagerError>`; the host unwraps the `Ok` and traps on
/// `Err`, which is exactly what these `()`-returning clients observe.
#[contractclient(name = "RoleManagerClient")]
pub trait RoleManagerInterface {
    /// Aborts the invocation unless `caller` authenticated and holds `role`.
    fn require_role(env: Env, caller: Address, role: Role);

    /// Aborts unless `caller` authenticated and one of its roles grants
    /// `permission`. This is the granular check parameter setters should use.
    fn require_permission(env: Env, caller: Address, permission: Permission);
}

/// Cross-contract surface the role manager uses to push an authorized
/// parameter change to the contract that owns the parameter.
#[contractclient(name = "ParameterTargetClient")]
pub trait ParameterTargetInterface {
    fn set_parameter(env: Env, key: Symbol, value: u32);
}

#[contract]
pub struct RoleManagerContract;

#[contractimpl]
impl RoleManagerContract {
    /// Installs `admin` as the one-shot administrator and first `Admin` member.
    ///
    /// The administrator must authorize the call so a typo in the argument
    /// cannot hand control of the registry to an address nobody controls.
    pub fn initialize(env: Env, admin: Address) -> Result<(), RoleManagerError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RoleManagerError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Self::touch_instance(&env);
        Self::persist_set(&env, &DataKey::RoleMember(Role::Admin, admin), &true);
        Ok(())
    }

    /// Grants `role` to `member`. Administrator-only.
    pub fn grant_role(
        env: Env,
        admin: Address,
        role: Role,
        member: Address,
    ) -> Result<(), RoleManagerError> {
        Self::require_admin(&env, &admin)?;
        Self::persist_set(&env, &DataKey::RoleMember(role, member), &true);
        Self::touch_instance(&env);
        Ok(())
    }

    /// Revokes `role` from `member`. Administrator-only.
    ///
    /// Revocation writes `false` instead of deleting the key so indexers can
    /// distinguish "never granted" from "granted then revoked" without an
    /// event stream, while `has_role` treats both identically.
    pub fn revoke_role(
        env: Env,
        admin: Address,
        role: Role,
        member: Address,
    ) -> Result<(), RoleManagerError> {
        Self::require_admin(&env, &admin)?;
        Self::persist_set(&env, &DataKey::RoleMember(role, member), &false);
        Self::touch_instance(&env);
        Ok(())
    }

    /// Returns whether `member` currently holds `role`. Permissionless read.
    pub fn has_role(env: Env, role: Role, member: Address) -> bool {
        Self::persist_get::<_, bool>(&env, &DataKey::RoleMember(role, member)).unwrap_or(false)
    }

    /// Aborts unless `caller` authenticated and holds `role`.
    ///
    /// This is the entrypoint external contracts call through
    /// [`RoleManagerInterface`]; returning a typed error (rather than panicking
    /// with a string) lets callers and tests distinguish a missing role from an
    /// authentication failure or an uninitialized registry.
    pub fn require_role(env: Env, caller: Address, role: Role) -> Result<(), RoleManagerError> {
        caller.require_auth();
        if !Self::has_role(env.clone(), role, caller) {
            return Err(RoleManagerError::RoleNotAssigned);
        }
        Ok(())
    }

    /// Returns whether any role held by `address` grants `permission`.
    ///
    /// The check is intentionally role-driven: there is no separate
    /// "permission storage" to drift out of sync with role grants. Revoking a
    /// role therefore revokes every permission derived from it immediately.
    pub fn has_permission(env: Env, address: Address, permission: Permission) -> bool {
        for role in Self::ALL_ROLES {
            if Self::role_permits(role, permission)
                && Self::persist_get::<_, bool>(&env, &DataKey::RoleMember(role, address.clone()))
                    .unwrap_or(false)
            {
                return true;
            }
        }
        false
    }

    /// Aborts unless `caller` authenticated and has `permission`.
    ///
    /// Consumers call this (through [`RoleManagerInterface`]) at the start of
    /// any parameter setter, so an unauthorized caller never reaches the write.
    pub fn require_permission(
        env: Env,
        caller: Address,
        permission: Permission,
    ) -> Result<(), RoleManagerError> {
        caller.require_auth();
        if !Self::has_permission(env.clone(), caller, permission) {
            return Err(RoleManagerError::PermissionDenied);
        }
        Ok(())
    }

    /// Applies an authorized parameter change to `target`.
    ///
    /// The permission check happens *before* the cross-contract call, and the
    /// target is only invoked after it succeeds, so a rejected caller leaves
    /// both this contract and the target untouched. If the target rejects the
    /// write for its own reasons, the typed `ParameterTargetRejected` error is
    /// returned and the whole invocation rolls back.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_parameter_change(
        env: Env,
        caller: Address,
        permission: Permission,
        target: Address,
        key: Symbol,
        value: u32,
    ) -> Result<(), RoleManagerError> {
        Self::require_permission(env.clone(), caller, permission)?;
        let target_client = ParameterTargetClient::new(&env, &target);
        if target_client.try_set_parameter(&key, &value).is_err() {
            return Err(RoleManagerError::ParameterTargetRejected);
        }
        Self::touch_instance(&env);
        Ok(())
    }

    /// Returns `member`'s current budget for `role`, or `0` when it was never
    /// credited (and after it is fully spent).
    ///
    /// Reads are deliberately tolerant of a missing key: an unwritten budget
    /// is a legitimate zero-balance state, not a corrupted storage entry.
    pub fn get_budget(env: Env, role: Role, member: Address) -> i128 {
        Self::read_budget(&env, &DataKey::Budget(role, member))
    }

    /// Adds `amount` to `member`'s budget for `role`. Administrator-only.
    ///
    /// A zero `amount` is an intentional no-op that does not create a storage
    /// entry — "nobody has funded this budget" and "a zero-byte budget entry"
    /// must remain indistinguishable. Negative amounts are rejected with a
    /// typed error rather than silently inverting a credit into a debit.
    /// Returns the resulting balance.
    pub fn credit_budget(
        env: Env,
        admin: Address,
        role: Role,
        member: Address,
        amount: i128,
    ) -> Result<i128, RoleManagerError> {
        Self::require_admin(&env, &admin)?;
        if amount < 0 {
            return Err(RoleManagerError::InvalidAmount);
        }
        let key = DataKey::Budget(role, member);
        if amount == 0 {
            return Ok(Self::read_budget(&env, &key));
        }
        let current = Self::read_budget(&env, &key);
        let updated = current
            .checked_add(amount)
            .ok_or(RoleManagerError::BudgetOverflow)?;
        Self::persist_set(&env, &key, &updated);
        Self::touch_instance(&env);
        Ok(updated)
    }

    /// Consumes `amount` from `caller`'s own budget for `role`.
    ///
    /// Zero-balance handling is the whole point of this entrypoint:
    ///
    /// - a budget that was never credited reads as `0`, so the first debit is a
    ///   typed [`RoleManagerError::InsufficientBudget`], never a host panic;
    /// - `amount == 0` succeeds and leaves storage untouched, even on a missing
    ///   budget;
    /// - a debit that exactly exhausts the budget **removes** the key, so no
    ///   zero-value entry is left to pay rent or confuse indexers;
    /// - negative amounts and arithmetic failures are typed errors.
    ///
    /// The caller must both authenticate and hold `role`, so an arbitrary
    /// address cannot drain somebody else's budget.
    /// Returns the remaining balance.
    pub fn debit_budget(
        env: Env,
        caller: Address,
        role: Role,
        amount: i128,
    ) -> Result<i128, RoleManagerError> {
        caller.require_auth();
        if !Self::has_role(env.clone(), role, caller.clone()) {
            return Err(RoleManagerError::RoleNotAssigned);
        }
        let key = DataKey::Budget(role, caller);
        let current = Self::read_budget(&env, &key);
        if amount < 0 {
            return Err(RoleManagerError::InvalidAmount);
        }
        if amount == 0 {
            return Ok(current);
        }
        if current < amount {
            return Err(RoleManagerError::InsufficientBudget);
        }
        let remaining = current
            .checked_sub(amount)
            .ok_or(RoleManagerError::InsufficientBudget)?;
        if remaining == 0 {
            Self::persist_remove(&env, &key);
        } else {
            Self::persist_set(&env, &key, &remaining);
        }
        Self::touch_instance(&env);
        Ok(remaining)
    }
}

impl RoleManagerContract {
    /// Every role, in a fixed order used by the permission scan. Adding a role
    /// requires updating this array — a compile-time-visible list is safer than
    /// discovering at runtime that a new role was never considered.
    const ALL_ROLES: [Role; 5] = [
        Role::Admin,
        Role::Operator,
        Role::ComplianceOfficer,
        Role::TreasurySigner,
        Role::Pauser,
    ];

    /// The single source of truth for role → permission mapping.
    ///
    /// Matches are explicit rather than "admin can do everything" so a role's
    /// capabilities are reviewable line by line during an audit.
    fn role_permits(role: Role, permission: Permission) -> bool {
        matches!(
            (role, permission),
            (Role::Admin, Permission::ManageRoles)
                | (Role::Operator, Permission::SetThreshold)
                | (Role::ComplianceOfficer, Permission::SetTierLimit)
                | (Role::TreasurySigner, Permission::SetSignerWeight)
                | (Role::Pauser, Permission::PauseProtocol)
        )
    }

    /// Reads a persistent key and refreshes its TTL when it is close to
    /// eviction. Every persistent read in the contract goes through here, so a
    /// newly added key cannot accidentally be left on a bare `get`.
    ///
    /// The host applies its own `threshold` check before writing the extended
    /// TTL, so a read of a healthy entry costs no storage write.
    fn persist_get<K, V>(env: &Env, key: &K) -> Option<V>
    where
        K: IntoVal<Env, Val>,
        V: TryFromVal<Env, Val>,
        V::Error: Debug,
    {
        let value = env.storage().persistent().get(key)?;
        env.storage().persistent().extend_ttl(
            key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
        Some(value)
    }

    /// Writes a persistent key and refreshes its TTL in the same operation.
    /// Centralising the pair keeps "write without rent top-up" impossible.
    fn persist_set<K, V>(env: &Env, key: &K, value: &V)
    where
        K: IntoVal<Env, Val>,
        V: IntoVal<Env, Val>,
    {
        env.storage().persistent().set(key, value);
        env.storage().persistent().extend_ttl(
            key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    /// Removes a persistent key. No TTL call: eviction of a dead key cannot
    /// happen while it is absent, and an `extend_ttl` on it would be wasteful.
    fn persist_remove<K: IntoVal<Env, Val>>(env: &Env, key: &K) {
        env.storage().persistent().remove(key);
    }

    /// Refreshes the contract instance on state-changing entrypoints. Without
    /// this, a heavily used registry could lose its admin pointer even though
    /// all of its persistent role entries are still alive.
    fn touch_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

    /// Reads a budget key, treating an absent entry as the zero balance it
    /// represents. Centralised so no caller needs an `unwrap` on storage.
    fn read_budget(env: &Env, key: &DataKey) -> i128 {
        Self::persist_get::<_, i128>(env, key).unwrap_or(0)
    }

    /// Authenticates `admin` and checks it is the stored administrator.
    fn require_admin(env: &Env, admin: &Address) -> Result<(), RoleManagerError> {
        admin.require_auth();
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(RoleManagerError::Unauthorized)?;
        if stored != *admin {
            return Err(RoleManagerError::Unauthorized);
        }
        Ok(())
    }
}
