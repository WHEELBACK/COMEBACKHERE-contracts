//! Role storage and grant/revoke helpers.
//!
//! All helpers are `pub(crate)`: the public contract surface is
//! [`RoleManagerContract`](crate::RoleManagerContract).

use crate::errors::RoleManagerError;
use crate::DataKey;
use soroban_sdk::{Address, Env, Symbol, Vec};

/// Returns the role symbols currently granted to `account`.
pub(crate) fn stored_roles(env: &Env, account: &Address) -> Vec<Symbol> {
    env.storage()
        .instance()
        .get(&DataKey::RoleGrants(account.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

/// Stores the granted role list for `account`.
pub(crate) fn set_stored_roles(env: &Env, account: &Address, roles: &Vec<Symbol>) {
    env.storage().instance().set(&DataKey::RoleGrants(account.clone()), roles);
}

/// Returns `true` if `account` currently holds `role`.
pub(crate) fn has_granted(env: &Env, account: &Address, role: &Symbol) -> bool {
    stored_roles(env, account).contains(role)
}

/// Requires `admin` to authenticate and to be the stored admin.
///
/// Defensive: no partial state is written; returns `Err(Unauthorized)` instead
/// of panicking so clients get a typed failure.
pub(crate) fn require_admin(env: &Env, admin: &Address) -> Result<(), RoleManagerError> {
    admin.require_auth();
    match env.storage().instance().get(&DataKey::Admin) {
        Some(stored) if stored == *admin => Ok(()),
        _ => Err(RoleManagerError::Unauthorized),
    }
}

/// Returns `true` if the contract is paused.
pub(crate) fn is_paused(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
}

/// Requires the contract to be unpaused (admin mutations are blocked while paused).
pub(crate) fn require_not_paused(env: &Env) -> Result<(), RoleManagerError> {
    if is_paused(env) {
        return Err(RoleManagerError::ContractPaused);
    }
    Ok(())
}
