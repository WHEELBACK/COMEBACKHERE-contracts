#![no_std]

use soroban_sdk::contracterror;

/// Error codes for the compliance contract.
///
/// Variants are append-only and must not be renumbered; discriminants are
/// matched by on-chain callers. Currently only `AlreadyInitialized` is
/// defined because the compliance contract primarily relies on
/// `require_auth()` panics for authorization failures.
#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u32)]
pub enum ComplianceError {
    AlreadyInitialized = 1,
}
