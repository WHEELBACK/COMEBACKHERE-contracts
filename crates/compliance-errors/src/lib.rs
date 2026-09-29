#![no_std]

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Error codes for the compliance contract.
    ///
    /// Variants are append-only and must not be renumbered; discriminants are
    /// matched by on-chain callers. Currently only `AlreadyInitialized` is
    /// defined because the compliance contract primarily relies on
    /// `require_auth()` panics for authorization failures.
    ///
    /// New compliance error variants belong in the contract's own
    /// `ContractError` (see `contracts/compliance/src/lib.rs`), not here — see
    /// `crates/compliance-errors/README.md`.
    pub enum ComplianceError {
        AlreadyInitialized = 1,
    }
}
