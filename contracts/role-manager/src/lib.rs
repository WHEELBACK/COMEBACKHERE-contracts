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

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env,
};

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
}

/// Minimal call surface external contracts need from the role manager.
///
/// Declaring only the check entrypoint as a client means a consumer does not
/// link the role manager's own wasm exports (which would collide at build time
/// if the consumer were also compiled as a contract). The real contract's
/// `require_role` returns `Result<(), RoleManagerError>`; the host unwraps the
/// `Ok` and traps on `Err`, which is exactly what this `()`-returning client
/// observes.
#[contractclient(name = "RoleManagerClient")]
pub trait RoleManagerInterface {
    /// Aborts the invocation unless `caller` authenticated and holds `role`.
    fn require_role(env: Env, caller: Address, role: Role);
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
        env.storage()
            .persistent()
            .set(&DataKey::RoleMember(Role::Admin, admin), &true);
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
        env.storage()
            .persistent()
            .set(&DataKey::RoleMember(role, member), &true);
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
        env.storage()
            .persistent()
            .set(&DataKey::RoleMember(role, member), &false);
        Ok(())
    }

    /// Returns whether `member` currently holds `role`. Permissionless read.
    pub fn has_role(env: Env, role: Role, member: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::RoleMember(role, member))
            .unwrap_or(false)
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
}

impl RoleManagerContract {
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
