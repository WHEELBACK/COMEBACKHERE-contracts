//! Modular error enums for the role-manager contract.
//!
//! - [`RoleManagerError`] — the contract-level error type returned by the
//!   `RoleManagerContract` entrypoints: lifecycle, authorization, and
//!   enforcement failures.
//! - [`RoleManagementError`] — a separate validation-error enum for the role
//!   grant/revoke operations, so clients can distinguish structural role
//!   validation failures from lifecycle and enforcement errors.
//!
//! Every variant carries a human-readable `Display` message ("custom error
//! messages") for client debugging.

use soroban_sdk::contracterror;

/// Contract-level errors for the role-manager contract.
///
/// Discriminants are part of the on-chain ABI: variants are append-only and
/// must never be renumbered or removed (see `scripts/check-enum-ordering.sh`).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RoleManagerError {
    /// `initialize` was called more than once.
    AlreadyInitialized = 1,
    /// The caller is not the stored admin (or failed authentication).
    Unauthorized = 2,
    /// The contract is paused; admin mutations are blocked.
    ContractPaused = 3,
    /// `require_role`: the caller does not hold the requested role.
    MissingRole = 4,
    /// A role symbol is empty (e.g. `Symbol::new(&env, "")`).
    EmptyRole = 5,
    /// `grant_role` on an account that already holds the role.
    RoleAlreadyGranted = 6,
    /// `revoke_role` on an account that does not hold the role.
    RoleNotGranted = 7,
    /// Stored state failed an invariant check (#936).
    StateInconsistency = 8,
    /// An account already holds the maximum number of roles (#936).
    TooManyRoles = 9,
}

/// Validation errors for role grant/revoke operations.
///
/// Returned by the role helpers in `roles` and converted into
/// [`RoleManagerError`] at the contract boundary via `From`.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RoleManagementError {
    /// The role symbol is empty.
    EmptyRole = 1,
    /// Granting a role that is already granted to the account.
    RoleAlreadyGranted = 2,
    /// Revoking a role that is not granted to the account.
    RoleNotGranted = 3,
}

impl From<RoleManagementError> for RoleManagerError {
    fn from(e: RoleManagementError) -> Self {
        match e {
            RoleManagementError::EmptyRole => RoleManagerError::EmptyRole,
            RoleManagementError::RoleAlreadyGranted => RoleManagerError::RoleAlreadyGranted,
            RoleManagementError::RoleNotGranted => RoleManagerError::RoleNotGranted,
        }
    }
}

/// Human-readable messages for clients of the role-manager contract.
impl core::fmt::Display for RoleManagerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            RoleManagerError::AlreadyInitialized => "role-manager: already initialized",
            RoleManagerError::Unauthorized => "role-manager: not the stored admin",
            RoleManagerError::ContractPaused => "role-manager: paused",
            RoleManagerError::MissingRole => "role-manager: missing role",
            RoleManagerError::EmptyRole => "role-manager: empty role symbol",
            RoleManagerError::RoleAlreadyGranted => "role-manager: role already granted",
            RoleManagerError::RoleNotGranted => "role-manager: role not granted",
            RoleManagerError::StateInconsistency => "role-manager: stored state is inconsistent",
            RoleManagerError::TooManyRoles => "role-manager: account role limit reached",
        })
    }
}

impl core::fmt::Display for RoleManagementError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            RoleManagementError::EmptyRole => "role-manager: empty role symbol",
            RoleManagementError::RoleAlreadyGranted => "role-manager: role already granted",
            RoleManagementError::RoleNotGranted => "role-manager: role not granted",
        })
    }
}
