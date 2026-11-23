#![no_main]

//! Fuzz harness for the fee-manager contract covering malicious-caller scenarios.
//!
//! # What this harness tests (#899)
//!
//! A malicious caller can try to:
//! - Modify `FeeBps` without holding Admin or Operator (#897).
//! - Modify `FeeRecipient` without holding Admin (#897).
//! - Escalate to Admin via `add_operator` / `transfer_admin` (#897).
//! - Trigger arithmetic overflow/underflow via `compute_fee` or
//!   `compute_partial_fee` (#896).
//! - Accept a pending admin transfer they were not nominated for (#897).
//! - Pause or unpause the contract without Admin rights (#897).
//!
//! # Invariant checked after every operation
//!
//! `fee_bps` must always remain `<= 10_000` (BPS_DENOMINATOR). Any input that
//! makes `get_fee_bps` exceed that bound will trigger an assertion failure,
//! which libFuzzer treats as a crash and saves as a reproducer.
//!
//! # Design note
//!
//! The harness is intentionally simple: each `Op` variant captures the minimum
//! inputs needed to exercise one code path, the attacker and operator addresses
//! are fixed for the run, and `env.mock_all_auths()` is used so the SDK does not
//! reject calls on the missing-signature path before they reach contract logic.
//! This matches the approach in the compliance fuzz harness.

use arbitrary::Arbitrary;
use fee_manager::{FeeManagerContract, FeeManagerContractClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Maximum number of operations replayed from a single fuzz input.
///
/// Bounded so a single long input does not dominate the libFuzzer time budget.
const MAX_OPS: usize = 16;

/// One operation a malicious caller might attempt.
#[derive(Debug, Clone, Arbitrary)]
enum Op {
    /// Try to set `FeeBps`. When `caller_is_admin` is false the attacker calls.
    SetFeeBps { caller_is_admin: bool, bps: u32 },
    /// Try to set `FeeRecipient`. Attacker calls when `caller_is_admin` is false.
    SetFeeRecipient { caller_is_admin: bool },
    /// Try to add an operator. Attacker calls when `caller_is_admin` is false.
    AddOperator { caller_is_admin: bool },
    /// Try to remove the legitimate operator. Attacker calls when false.
    RemoveOperator { caller_is_admin: bool },
    /// Call `compute_fee` with an arbitrary amount (may be negative/zero/huge).
    ComputeFee { amount: i128 },
    /// Call `compute_partial_fee` with arbitrary inputs (may cause underflow).
    ComputePartialFee { total: i128, settled: i128 },
    /// Try to initiate an admin transfer. Attacker calls when false.
    TransferAdmin { caller_is_admin: bool },
    /// Try to accept the pending admin transfer.
    /// `correct_pending` = use the address that was actually nominated.
    AcceptAdmin { correct_pending: bool },
    /// Try to pause. Attacker calls when false.
    Pause { caller_is_admin: bool },
    /// Try to unpause. Attacker calls when false.
    Unpause { caller_is_admin: bool },
    /// Operator (not admin) tries to set `FeeBps` — this should succeed (#897).
    SetFeeBpsAsOperator { bps: u32 },
}

#[derive(Debug, Arbitrary)]
struct Input {
    ops: Vec<Op>,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    let operator = Address::generate(&env);
    let attacker = Address::generate(&env);

    let contract_id = env.register(FeeManagerContract, ());
    let client = FeeManagerContractClient::new(&env, &contract_id);

    // Initialize with 500 bps (5%)
    client.initialize(&admin, &fee_recipient, &500u32);
    // Register one legitimate operator so Op::SetFeeBpsAsOperator has a target.
    client.add_operator(&admin, &operator);

    for op in input.ops.iter().take(MAX_OPS) {
        match op {
            Op::SetFeeBps { caller_is_admin, bps } => {
                let caller = if *caller_is_admin {
                    admin.clone()
                } else {
                    attacker.clone()
                };
                let _ = client.try_set_fee_bps(&caller, bps);
            }

            Op::SetFeeRecipient { caller_is_admin } => {
                let caller = if *caller_is_admin {
                    admin.clone()
                } else {
                    attacker.clone()
                };
                let new_recipient = Address::generate(&env);
                let _ = client.try_set_fee_recipient(&caller, &new_recipient);
            }

            Op::AddOperator { caller_is_admin } => {
                let caller = if *caller_is_admin {
                    admin.clone()
                } else {
                    attacker.clone()
                };
                let _ = client.try_add_operator(&caller, &attacker);
            }

            Op::RemoveOperator { caller_is_admin } => {
                let caller = if *caller_is_admin {
                    admin.clone()
                } else {
                    attacker.clone()
                };
                let _ = client.try_remove_operator(&caller, &operator);
            }

            Op::ComputeFee { amount } => {
                let _ = client.try_compute_fee(amount);
            }

            Op::ComputePartialFee { total, settled } => {
                let _ = client.try_compute_partial_fee(total, settled);
            }

            Op::TransferAdmin { caller_is_admin } => {
                let caller = if *caller_is_admin {
                    admin.clone()
                } else {
                    attacker.clone()
                };
                // Attacker tries to nominate themselves; admin might nominate the attacker.
                let _ = client.try_transfer_admin(&caller, &attacker);
            }

            Op::AcceptAdmin { correct_pending } => {
                // If admin previously transferred to attacker, the attacker IS
                // the pending admin. `correct_pending = true` exercises the
                // happy path; `false` exercises the wrong-caller rejection.
                let caller = if *correct_pending {
                    attacker.clone()
                } else {
                    admin.clone()
                };
                let _ = client.try_accept_admin(&caller);
            }

            Op::Pause { caller_is_admin } => {
                let caller = if *caller_is_admin {
                    admin.clone()
                } else {
                    attacker.clone()
                };
                let _ = client.try_pause(&caller);
            }

            Op::Unpause { caller_is_admin } => {
                let caller = if *caller_is_admin {
                    admin.clone()
                } else {
                    attacker.clone()
                };
                let _ = client.try_unpause(&caller);
            }

            Op::SetFeeBpsAsOperator { bps } => {
                // Legitimate operator path (#897): should succeed for valid bps.
                let _ = client.try_set_fee_bps(&operator, bps);
            }
        }

        // ── Invariant: fee_bps must never exceed BPS_DENOMINATOR (10_000) ──────
        //
        // Invalid inputs (`bps > 10_000`) must be rejected by the contract with
        // `InvalidFeeBps` before the value is stored. Any input that violates
        // this invariant is a bug.
        let bps = client.get_fee_bps();
        assert!(
            bps <= 10_000,
            "fee_bps invariant violated: got {bps}, expected <= 10_000"
        );
    }
});
