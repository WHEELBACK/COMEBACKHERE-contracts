#![no_std]

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Error codes for the access-control contract.
    ///
    /// Variants are append-only and must not be renumbered; discriminants are
    /// matched by on-chain callers. All access-control error variants live here;
    /// the access-control contract no longer defines a separate `ContractError`.
    ///
    /// New access-control error variants belong here, not in the contract crate.
    pub enum AccessControlError {
        /// The contract has already been initialized.
        AlreadyInitialized = 1,
        /// The caller is not authorized to perform this operation.
        Unauthorized = 2,
        /// The contract is currently paused and cannot process this operation.
        ContractPaused = 3,
        /// The requested role does not exist in the role registry.
        RoleNotFound = 4,
        /// The account already holds the role being granted.
        RoleAlreadyGranted = 5,
        /// The account does not hold the role being revoked.
        RoleNotGranted = 6,
        /// The maximum number of members for a role has been reached.
        RoleMembersFull = 7,
        /// The maximum number of roles has been reached.
        RoleSetFull = 8,
        /// The provided role identifier is invalid.
        InvalidRole = 9,
        /// The provided account address is invalid.
        InvalidAccount = 10,
        /// `accept_admin` or `cancel_admin_transfer` called with no pending admin transfer.
        NoPendingAdmin = 11,
        /// An admin transfer is already pending and must be resolved first.
        PendingAdminExists = 12,
        /// `migrate` was called while storage is at a schema version it has no
        /// migration path from.
        UnexpectedSchemaVersion = 13,
        /// A bulk operation exceeded the maximum allowed batch size.
        BatchTooLarge = 14,
        /// A bulk operation was made before the cooldown period elapsed since the
        /// caller's previous bulk call.
        BulkOperationCooldown = 15,
    }
}