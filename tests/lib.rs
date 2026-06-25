//! Workspace-level cross-contract integration tests for COMEBACKHERE.
//!
//! The suite is organised into three scenario modules, each exercising a
//! distinct cross-contract interaction boundary:
//!
//! | Module                | Contracts under test                      |
//! |-----------------------|-------------------------------------------|
//! | [`invoice_treasury`]  | Invoice status gates settlement proposals |
//! | [`treasury_compliance`]| Compliance allowlist gates settlement     |
//! | [`full_lifecycle`]    | End-to-end: invoice → payment → settle    |
//!
//! Every module defines a minimal workflow contract that mirrors the
//! off-chain orchestration layer and exercises cross-contract call paths
//! inside a Soroban test environment.
//!
//! Run the suite with:
//! ```text
//! cargo test -p protocol-integration-tests
//! ```

#[cfg(test)]
mod invoice_treasury;
#[cfg(test)]
mod treasury_compliance;
#[cfg(test)]
mod full_lifecycle;
