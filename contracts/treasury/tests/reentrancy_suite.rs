//! Entry point for the treasury reentrancy suite (issue #118).
//!
//! Each module below targets a specific token-touching entrypoint and
//! reuses the shared `malicious_token::ReentrancyToken` mock. The mock is
//! configured (per test) to re-enter one of `deposit`, `withdraw`,
//! `withdraw_all`, `execute_settlement`, or `partially_execute_settlement`
//! from inside its `transfer` callback so every entrypoint can be exercised
//! through a single harness.

#[path = "reentrancy_suite/malicious_token.rs"]
mod malicious_token;

#[path = "reentrancy_suite/common.rs"]
mod common;

#[path = "reentrancy_suite/deposit.rs"]
mod deposit;

#[path = "reentrancy_suite/withdraw.rs"]
mod withdraw;

#[path = "reentrancy_suite/withdraw_all.rs"]
mod withdraw_all;

#[path = "reentrancy_suite/execute_settlement.rs"]
mod execute_settlement;

#[path = "reentrancy_suite/partially_execute_settlement.rs"]
mod partially_execute_settlement;
