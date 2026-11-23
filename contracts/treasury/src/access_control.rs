//! Internal role helpers backing treasury's granular access-control checks (#969, #972).

use crate::admin::{ROLE_ADMIN, ROLE_OPERATOR};
use crate::DataKey;
use soroban_sdk::{Address, Env, Symbol};

/// Returns the symbol representing the admin role.
pub(crate) fn role_admin(env: &Env) -> Symbol {
    Symbol::new(env, "admin")
}

/// Grants `role` to `account` in persistent role storage.
pub(crate) fn grant_role(env: &Env, account: &Address, role: &Symbol) {
    let code = if *role == role_admin(env) {
        ROLE_ADMIN
    } else {
        ROLE_OPERATOR
    };
    env.storage()
        .persistent()
        .set(&DataKey::Role(account.clone()), &code);
}

/// Returns `true` if `caller` holds `role` (or is the stored admin).
pub(crate) fn has_role(env: &Env, caller: &Address, role: &Symbol) -> bool {
    let stored_admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
    if stored_admin.as_ref() == Some(caller) {
        return true;
    }
    let assigned: Option<u32> = env
        .storage()
        .persistent()
        .get(&DataKey::Role(caller.clone()));
    if *role == role_admin(env) {
        matches!(assigned, Some(ROLE_ADMIN))
    } else {
        matches!(assigned, Some(ROLE_ADMIN | ROLE_OPERATOR))
    }
}

/// Verifies basic access-control invariants (admin exists once initialized).
pub(crate) fn invariants_hold(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}
