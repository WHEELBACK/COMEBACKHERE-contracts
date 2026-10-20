#![no_std]

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Error codes for the compliance contract.
    ///
    /// Variants are append-only and must not be renumbered; discriminants are
    /// matched by on-chain callers. All compliance error variants live here;
    /// the compliance contract no longer defines a separate `ContractError`.
    ///
    /// New compliance error variants belong here, not in the contract crate.
    pub enum ComplianceError {
        AlreadyInitialized = 1,
        Unauthorized = 2,
        ContractPaused = 3,
        BatchTooLarge = 4,
        AddressIndexFull = 5,
        /// A bulk allow/block call was made before [`BULK_OP_COOLDOWN_SECS`] elapsed since the
        /// caller's previous bulk call (see #454).
        BulkOperationCooldown = 6,
        /// `migrate` was called while storage is at a schema version it has no
        /// migration path from (see #610).
        UnexpectedSchemaVersion = 7,
        /// An operator tried to clear or override a block placed by the admin.
        OperatorCannotClearAdminBlock = 8,
        /// `add_operator` called when `MAX_OPERATORS` operators are already set.
        OperatorSetFull = 9,
        /// `set_jurisdiction` was given a code that is not 2-3 uppercase ASCII letters or digits.
        InvalidJurisdictionCode = 10,
        /// `accept_admin` or `cancel_admin_transfer` called with no pending admin transfer.
        NoPendingAdmin = 11,
    }
}
