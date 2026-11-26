#![no_std]
#![allow(deprecated)]

mod errors;
mod invariants;
mod roles;

pub use errors::{RoleManagementError, RoleManagerError};

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol, Vec};

/// Storage keys for the role-manager contract.
///
/// Used as keys for Soroban instance storage. Variants must not be reordered
/// or removed once deployed; new variants should be appended at the end so
/// that existing on-chain data (keyed by XDR-encoded discriminants) continues
/// to decode correctly.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The stored administrator (instance storage).
    Admin,
    /// Pause flag: admin grant/revoke mutations are blocked while `true`
    /// (instance storage).
    Paused,
    /// Granted role symbols per account (instance storage).
    RoleGrants(Address),
}

/// Role-based access control for COMEBACKHERE contracts.
///
/// Manages on-chain roles (grants, revocations) and provides the
/// cross-contract enforcement point (`require_role`) other contracts use to
/// gate privileged operations behind roles instead of ad-hoc address
/// checks.
#[contract]
pub struct RoleManagerContract;

#[contractimpl]
impl RoleManagerContract {
    /// One-time initialization: stores `admin` and clears the pause flag.
    ///
    /// Errors: `AlreadyInitialized`.
    /// Emits: `role_manager_initialized`.
    pub fn initialize(env: Env, admin: Address) -> Result<(), RoleManagerError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RoleManagerError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        invariants::check(&env)?;
        env.events()
            .publish((Symbol::new(&env, "role_manager_initialized"),), admin);
        Ok(())
    }

    /// Grants `role` to `account` (admin-only, non-paused).
    ///
    /// Strict validation: `role` must be non-empty (`EmptyRole`); the account
    /// must not already hold the role (`RoleAlreadyGranted`); a paused contract
    /// rejects the mutation (`ContractPaused`). Defensive recovery: no
    /// partial state is written when validation fails.
    ///
    /// Emits: `role_granted`.
    pub fn grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), RoleManagerError> {
        roles::require_admin(&env, &admin)?;
        roles::require_not_paused(&env)?;
        if role == Symbol::new(&env, "") {
            return Err(RoleManagementError::EmptyRole.into());
        }
        // Repair legacy corruption in memory, then enforce the strict
        // invariants before committing the new list (#936).
        let mut granted = roles::sanitized_roles(&env, &account);
        if roles::is_full(&granted) {
            return Err(RoleManagerError::TooManyRoles);
        }
        if granted.contains(&role) {
            return Err(RoleManagementError::RoleAlreadyGranted.into());
        }
        granted.push_back(role.clone());
        invariants::verify_grants(&env, &granted)?;
        roles::set_stored_roles(&env, &account, &granted);
        env.events()
            .publish((Symbol::new(&env, "role_granted"), account), role);
        Ok(())
    }

    /// Revokes `role` from `account` (admin-only, non-paused).
    ///
    /// Strict validation: `role` must be non-empty (`EmptyRole`); the account
    /// must hold the role (`RoleNotGranted`). Defensive recovery: the stored
    /// role list is rewritten without the role, so a revoked role is cleared
    /// even if the storage contained duplicates.
    ///
    /// Emits: `role_revoked`.
    pub fn revoke_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), RoleManagerError> {
        roles::require_admin(&env, &admin)?;
        roles::require_not_paused(&env)?;
        if role == Symbol::new(&env, "") {
            return Err(RoleManagementError::EmptyRole.into());
        }
        let granted = roles::sanitized_roles(&env, &account);
        if !granted.contains(&role) {
            return Err(RoleManagementError::RoleNotGranted.into());
        }
        let mut updated = Vec::new(&env);
        for granted_role in granted.iter() {
            if granted_role != role {
                updated.push_back(granted_role.clone());
            }
        }
        invariants::verify_grants(&env, &updated)?;
        roles::set_stored_roles(&env, &account, &updated);
        env.events()
            .publish((Symbol::new(&env, "role_revoked"), account), role);
        Ok(())
    }

    /// Returns `true` if `account` currently holds `role`.
    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        roles::has_granted(&env, &account, &role)
    }

    /// Cross-contract enforcement point: authenticates `account` and fails
    /// with `MissingRole` if it does not hold `role`.
    pub fn require_role(env: Env, role: Symbol, account: Address) -> Result<(), RoleManagerError> {
        if role == Symbol::new(&env, "") {
            return Err(RoleManagementError::EmptyRole.into());
        }
        account.require_auth();
        if !roles::has_granted_checked(&env, &account, &role)? {
            return Err(RoleManagerError::MissingRole);
        }
        Ok(())
    }

    /// Sets the pause flag (admin-only). While paused, `grant_role` and
    /// `revoke_role` are rejected with `ContractPaused`.
    pub fn set_paused(env: Env, admin: Address, paused: bool) -> Result<(), RoleManagerError> {
        roles::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &paused);
        Ok(())
    }

    /// Returns the current pause flag.
    pub fn is_paused(env: Env) -> bool {
        roles::is_paused(&env)
    }

    /// Returns the stored admin, if `initialize` has been called.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Admin)
    }

    // -------------------------------------------------------------------------
    // Invariant audit (#936)
    // -------------------------------------------------------------------------

    /// Read-only audit of the instance-level invariants. Operators and
    /// monitors can detect a half-initialized instance before mutating it.
    pub fn verify_invariants(env: Env) -> Result<(), RoleManagerError> {
        invariants::check(&env)
    }

    /// Read-only audit of one account's grant list. Corrupt lists (empty
    /// symbols, duplicates, or more than the configured maximum) are reported
    /// as `StateInconsistency` / `TooManyRoles` instead of being trusted.
    pub fn verify_account_roles(env: Env, account: Address) -> Result<(), RoleManagerError> {
        invariants::verify_grants(&env, &roles::stored_roles(&env, &account))
    }
}
