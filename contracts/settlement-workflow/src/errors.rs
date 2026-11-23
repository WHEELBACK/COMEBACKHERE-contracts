//! Error surface of the settlement-workflow contract (#874).
//!
//! The workflow returns two error enums, split by which part of the protocol
//! owns the failure:
//!
//! - [`WorkflowError`] — failures owned by this contract: emergency-pause
//!   coordination, configuration, invariant checks (#873).
//! - [`TreasuryError`] — failures on the settlement path and admin rotation,
//!   kept as the shared treasury ABI so existing clients matching on
//!   `ContractPaused` / `Unauthorized` / `ComplianceCheckFailed` keep working.
//!
//! Both are re-exported from here so clients import errors from one place, and
//! [`message_for`] turns a raw `WorkflowError` code into a human-readable
//! explanation for debugging failed transactions.

pub use multisig::TreasuryError;
pub use workflow_errors::WorkflowError;

/// Human-readable message for a `WorkflowError` code, or `None` for a code
/// this contract never returns.
pub fn message_for(code: u32) -> Option<&'static str> {
    WorkflowError::from_code(code).map(WorkflowError::message)
}
