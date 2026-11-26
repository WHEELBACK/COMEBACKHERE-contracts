//! Role storage and grant/revoke helpers.
//!
//! All helpers are `pub(crate)`: the public contract surface is
//! [`RoleManagerContract`](crate::RoleManagerContract).

use crate::errors::RoleManagerError;
use crate::invariants::{self, MAX_ROLES_PER_ACCOUNT};
use crate::DataKey;
use soroban_sdk::{Address, Env, Symbol, Vec};

/// Returns the raw role symbols currently granted to `account`.
pub(crate) fn stored_roles(env: &Env, account: &Address) -> Vec<Symbol> {
    env.storage()
        .instance()
        .get(&DataKey::RoleGrants(account.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

/// Reads a grant list and verifies its invariants. Used by authorization
/// queries, which must never trust a corrupt list (#936).
pub(crate) fn checked_roles(env: &Env, account: &Address) -> Result<Vec<Symbol>, RoleManagerError> {
    let roles = stored_roles(env, account);
    invariants::verify_grants(env, &roles)?;
    Ok(roles)
}

/// Reads a grant list and repairs any corruption in memory, so the next
/// mutation writes back a well-formed list (defensive recovery, #936).
pub(crate) fn sanitized_roles(env: &Env, account: &Address) -> Vec<Symbol> {
    let roles = stored_roles(env, account);
    invariants::sanitize_grants(env, &roles)
}

/// Stores the granted role list for `account`.
pub(crate) fn set_stored_roles(env: &Env, account: &Address, roles: &Vec<Symbol>) {
    env.storage()
        .instance()
        .set(&DataKey::RoleGrants(account.clone()), roles);
}

/// Returns `true` if `account` holds `role`, and `false` if its stored grant
/// list is corrupt rather than treating corrupt data as authorization.
pub(crate) fn has_granted(env: &Env, account: &Address, role: &Symbol) -> bool {
    checked_roles(env, account)
        .map(|roles| roles.contains(role))
        .unwrap_or(false)
}

/// Checked variant for entrypoints that can surface `StateInconsistency`.
pub(crate) fn has_granted_checked(
    env: &Env,
    account: &Address,
    role: &Symbol,
) -> Result<bool, RoleManagerError> {
    Ok(checked_roles(env, account)?.contains(role))
}

/// Returns whether the account is at the role capacity (#936).
pub(crate) fn is_full(roles: &Vec<Symbol>) -> bool {
    roles.len() >= MAX_ROLES_PER_ACCOUNT
}

/// Requires `admin` to authenticate and to be the stored admin.
///
/// Defensive: no partial state is written; returns `Err(Unauthorized)` instead
/// of panicking so clients get a typed failure.
pub(crate) fn require_admin(env: &Env, admin: &Address) -> Result<(), RoleManagerError> {
    admin.require_auth();
    match env.storage().instance().get::<_, Address>(&DataKey::Admin) {
        Some(stored) if stored == *admin => Ok(()),
        _ => Err(RoleManagerError::Unauthorized),
    }
}

/// Returns the stored pause flag, failing if the instance was never
/// initialized. `is_paused` keeps its lenient boolean surface for clients.
pub(crate) fn paused_flag(env: &Env) -> Result<bool, RoleManagerError> {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .ok_or(RoleManagerError::StateInconsistency)
}

/// Returns `true` if the contract is paused.
pub(crate) fn is_paused(env: &Env) -> bool {
    paused_flag(env).unwrap_or(false)
}

/// Requires the contract to be unpaused (admin mutations are blocked while paused).
pub(crate) fn require_not_paused(env: &Env) -> Result<(), RoleManagerError> {
    if paused_flag(env)? {
        return Err(RoleManagerError::ContractPaused);
    }
    Ok(())
}
