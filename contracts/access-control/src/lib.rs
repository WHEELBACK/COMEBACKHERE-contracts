#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, Symbol,
};

/// Storage keys for the access-control contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Maps (role, account) -> bool indicating whether the account holds the role.
    HasRole(Symbol, Address),
    /// Maps role -> Address of the admin account that manages the role.
    RoleAdmin(Symbol),
    /// Maps role -> Symbol of the admin role that manages the role.
    RoleAdminRole(Symbol),
}

/// Typed errors returned by the access-control contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AccessControlError {
    /// The caller is not authorized to perform the operation.
    Unauthorized = 1,
    /// The provided role is invalid (e.g. empty or self-administering).
    InvalidRole = 2,
    /// The account already holds the role.
    RoleAlreadyGranted = 3,
    /// The account does not hold the role.
    RoleNotGranted = 4,
    /// The role admin configuration is missing.
    RoleAdminNotSet = 5,
    /// The role admin role configuration is missing.
    RoleAdminRoleNotSet = 6,
    /// The contract has not been initialized.
    NotInitialized = 7,
    /// The contract has already been initialized.
    AlreadyInitialized = 8,
}

const DEFAULT_ADMIN_ROLE: &str = "admin";

#[contract]
pub struct AccessControl;

#[contractimpl]
impl AccessControl {
    /// Initializes the contract, granting the default admin role to `admin`.
    ///
    /// # Errors
    /// - `AlreadyInitialized` if the contract has already been initialized.
    /// - `InvalidRole` if the default admin role symbol is invalid.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::RoleAdminRole(Symbol::new(&env, DEFAULT_ADMIN_ROLE)))
        {
            panic_with_error!(&env, AccessControlError::AlreadyInitialized);
        }

        let admin_role = Symbol::new(&env, DEFAULT_ADMIN_ROLE);
        Self::validate_role(&env, &admin_role);

        // The default admin role administers itself.
        env.storage()
            .instance()
            .set(&DataKey::RoleAdminRole(admin_role.clone()), &admin_role);
        env.storage()
            .instance()
            .set(&DataKey::RoleAdmin(admin_role.clone()), &admin.clone());

        env.storage()
            .persistent()
            .set(&DataKey::HasRole(admin_role, admin), &true);
    }

    /// Returns whether `account` holds `role`.
    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::HasRole(role, account))
            .unwrap_or(false)
    }

    /// Returns the admin role that controls `role`.
    pub fn get_role_admin(env: Env, role: Symbol) -> Symbol {
        env.storage()
            .instance()
            .get(&DataKey::RoleAdminRole(role))
            .unwrap_or_else(|| panic_with_error!(&env, AccessControlError::RoleAdminRoleNotSet))
    }

    /// Returns the admin account that manages `role`.
    pub fn get_role_admin_account(env: Env, role: Symbol) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::RoleAdmin(role))
            .unwrap_or_else(|| panic_with_error!(&env, AccessControlError::RoleAdminNotSet))
    }

    /// Grants `role` to `account`. Only the role's admin account may call this.
    ///
    /// # Errors
    /// - `Unauthorized` if the caller is not the role admin.
    /// - `InvalidRole` if the role is invalid.
    /// - `RoleAlreadyGranted` if the account already holds the role.
    pub fn grant_role(env: Env, role: Symbol, account: Address) {
        Self::validate_role(&env, &role);
        Self::require_role_admin(&env, &role);

        let key = DataKey::HasRole(role.clone(), account.clone());
        if env.storage().persistent().get(&key).unwrap_or(false) {
            panic_with_error!(&env, AccessControlError::RoleAlreadyGranted);
        }

        env.storage().persistent().set(&key, &true);
    }

    /// Revokes `role` from `account`. Only the role's admin account may call this.
    ///
    /// # Errors
    /// - `Unauthorized` if the caller is not the role admin.
    /// - `InvalidRole` if the role is invalid.
    /// - `RoleNotGranted` if the account does not hold the role.
    pub fn revoke_role(env: Env, role: Symbol, account: Address) {
        Self::validate_role(&env, &role);
        Self::require_role_admin(&env, &role);

        let key = DataKey::HasRole(role.clone(), account.clone());
        if !env.storage().persistent().get(&key).unwrap_or(false) {
            panic_with_error!(&env, AccessControlError::RoleNotGranted);
        }

        env.storage().persistent().remove(&key);
    }

    /// Renounces `role` for the calling account.
    ///
    /// # Errors
    /// - `InvalidRole` if the role is invalid.
    /// - `RoleNotGranted` if the caller does not hold the role.
    pub fn renounce_role(env: Env, role: Symbol, account: Address) {
        Self::validate_role(&env, &role);
        account.require_auth();

        let key = DataKey::HasRole(role.clone(), account.clone());
        if !env.storage().persistent().get(&key).unwrap_or(false) {
            panic_with_error!(&env, AccessControlError::RoleNotGranted);
        }

        env.storage().persistent().remove(&key);
    }

    /// Sets the admin role that controls `role`. Only the current admin may call this.
    ///
    /// # Errors
    /// - `Unauthorized` if the caller is not the current role admin.
    /// - `InvalidRole` if either role is invalid.
    pub fn set_role_admin(env: Env, role: Symbol, admin_role: Symbol) {
        Self::validate_role(&env, &role);
        Self::validate_role(&env, &admin_role);
        Self::require_role_admin(&env, &role);

        env.storage()
            .instance()
            .set(&DataKey::RoleAdminRole(role), &admin_role);
    }

    /// Sets the admin account that manages `role`. Only the current admin may call this.
    ///
    /// # Errors
    /// - `Unauthorized` if the caller is not the current role admin.
    /// - `InvalidRole` if the role is invalid.
    pub fn set_role_admin_account(env: Env, role: Symbol, admin: Address) {
        Self::validate_role(&env, &role);
        Self::require_role_admin(&env, &role);

        env.storage()
            .instance()
            .set(&DataKey::RoleAdmin(role), &admin);
    }

    /// Asserts that `account` holds `role`, panicking with `Unauthorized` otherwise.
    /// Intended for cross-contract invocation by other contracts.
    pub fn require_role(env: Env, role: Symbol, account: Address) {
        if !Self::has_role(env, role, account) {
            panic_with_error!(&env, AccessControlError::Unauthorized);
        }
    }

    /// Asserts that `account` holds at least one of the provided roles.
    pub fn require_any_role(env: Env, roles: soroban_sdk::Vec<Symbol>, account: Address) {
        for role in roles.iter() {
            if Self::has_role(env.clone(), role, account.clone()) {
                return;
            }
        }
        panic_with_error!(&env, AccessControlError::Unauthorized);
    }

    /// Asserts that `account` holds all of the provided roles.
    pub fn require_all_roles(env: Env, roles: soroban_sdk::Vec<Symbol>, account: Address) {
        for role in roles.iter() {
            if !Self::has_role(env.clone(), role, account.clone()) {
                panic_with_error!(&env, AccessControlError::Unauthorized);
            }
        }
    }

    // --- Internal helpers ---

    fn validate_role(env: &Env, role: &Symbol) {
        // Reject empty roles defensively; Symbol::new panics on empty input,
        // so we guard by checking the length of the underlying representation.
        if role.to_string().len() == 0 {
            panic_with_error!(env, AccessControlError::InvalidRole);
        }
    }

    fn require_role_admin(env: &Env, role: &Symbol) {
        let admin_role: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::RoleAdminRole(role.clone()))
            .unwrap_or_else(|| panic_with_error!(env, AccessControlError::RoleAdminRoleNotSet));

        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::RoleAdmin(role.clone()))
            .unwrap_or_else(|| panic_with_error!(env, AccessControlError::RoleAdminNotSet));

        admin.require_auth();

        // The admin must hold the admin role for this role.
        let has_admin = env
            .storage()
            .persistent()
            .get(&DataKey::HasRole(admin_role, admin))
            .unwrap_or(false);

        if !has_admin {
            panic_with_error!(env, AccessControlError::Unauthorized);
        }
    }
}

#[cfg(test)]
mod test;
```

```toml
[package]
name = "access-control"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
soroban-sdk = { workspace = true }

[dev-dependencies]
soroban-sdk = { workspace = true, features = ["testutils"] }

[features]
testutils = ["soroban-sdk/testutils"]