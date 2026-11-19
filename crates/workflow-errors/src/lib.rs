#![no_std]

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Error codes for the storage-layout contract.
    ///
    /// Variants are append-only and must not be renumbered; discriminants are
    /// part of the on-chain ABI and are matched by callers and off-chain systems.
    /// New variants must be added at the end with an explicit discriminant one
    /// higher than the current maximum; see `scripts/check-enum-ordering.sh`.
    ///
    /// The range starts at 1000 to avoid colliding with the discriminants used by
    /// other contracts (for example `WorkflowError` uses 1..=10).
    pub enum StorageLayoutError {
        /// The caller is not authorized to perform the requested operation.
        Unauthorized = 1000,
        /// The contract has not been initialized yet.
        NotInitialized = 1001,
        /// The contract has already been initialized and cannot be re-initialized.
        AlreadyInitialized = 1002,
        /// A required storage key was not found in persistent storage.
        KeyNotFound = 1003,
        /// The supplied storage key is invalid or malformed.
        InvalidKey = 1004,
        /// The supplied value is invalid or fails validation.
        InvalidValue = 1005,
        /// The requested layout entry already exists.
        EntryAlreadyExists = 1006,
        /// The requested layout entry does not exist.
        EntryNotFound = 1007,
        /// The operation would exceed the maximum number of layout entries.
        TooManyEntries = 1008,
        /// The storage layout version is incompatible with the current contract.
        IncompatibleVersion = 1009,
        /// The storage layout migration failed and was rolled back.
        MigrationFailed = 1010,
        /// The operation is not permitted in the current contract state.
        InvalidState = 1011,
        /// The supplied input exceeds the maximum allowed size.
        InputTooLarge = 1012,
        /// An arithmetic operation overflowed or underflowed.
        ArithmeticError = 1013,
        /// The storage entry has expired and must be restored before use.
        EntryExpired = 1014,
    }
}