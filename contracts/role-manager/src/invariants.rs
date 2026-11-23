//! Strict invariant verification for the role-manager contract (#936).
//!
//! Grant state is a `Vec<Symbol>` per account. A corrupted list (empty role
//! symbols, duplicates, or an unbounded number of entries) can silently
//! change authorization decisions and inflate the ledger footprint of every
//! instance-storage read. These helpers make the rules explicit:
//!
//! 1. The instance is initialized: `Admin` and `Paused` are both present.
//! 2. A grant list contains only non-empty, unique role symbols.
//! 3. A grant list is bounded by [`MAX_ROLES_PER_ACCOUNT`], keeping the
//!    instance footprint (and therefore host memory/CPU cost) predictable.
//!
//! Queries verify strictly and refuse to act on corrupt lists; mutations use
//! [`sanitize_grants`] to recover from legacy corruption before writing a
//! well-formed list back.

use soroban_sdk::{Env, Symbol, Vec};

use crate::{DataKey, RoleManagerError};

/// Maximum number of distinct roles an account may hold. Bounding the list
/// keeps instance storage reads and event payloads predictable (#936).
pub const MAX_ROLES_PER_ACCOUNT: u32 = 32;

/// Checks the instance-level invariants shared by every entrypoint.
pub fn check(env: &Env) -> Result<(), RoleManagerError> {
    let instance = env.storage().instance();
    if instance
        .get::<_, soroban_sdk::Address>(&DataKey::Admin)
        .is_none()
        || instance.get::<_, bool>(&DataKey::Paused).is_none()
    {
        return Err(RoleManagerError::StateInconsistency);
    }
    Ok(())
}

/// Verifies that a grant list is well-formed and bounded.
pub fn verify_grants(env: &Env, grants: &Vec<Symbol>) -> Result<(), RoleManagerError> {
    if grants.len() > MAX_ROLES_PER_ACCOUNT {
        return Err(RoleManagerError::TooManyRoles);
    }
    let empty = Symbol::new(env, "");
    let mut seen: Vec<Symbol> = Vec::new(env);
    for role in grants.iter() {
        if role == empty || seen.contains(&role) {
            return Err(RoleManagerError::StateInconsistency);
        }
        seen.push_back(role);
    }
    Ok(())
}

/// Defensive recovery: rebuilds a grant list without empty/duplicate entries
/// and caps it at the configured maximum, preserving insertion order.
pub fn sanitize_grants(env: &Env, grants: &Vec<Symbol>) -> Vec<Symbol> {
    let empty = Symbol::new(env, "");
    let mut clean: Vec<Symbol> = Vec::new(env);
    for role in grants.iter() {
        if role == empty || clean.contains(&role) || clean.len() >= MAX_ROLES_PER_ACCOUNT {
            continue;
        }
        clean.push_back(role);
    }
    clean
}
