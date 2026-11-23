//! Fuzz target: `partially_execute_settlement` malicious-caller scenarios.
//!
//! This target exercises the treasury settlement partial-execution entry point
//! with arbitrary, potentially adversarial inputs. The goal is to ensure that
//! malformed or malicious caller input results in well-defined error returns
//! (or successful, invariant-preserving execution) rather than panics,
//! host aborts, or ledger-footprint blowups.
//!
//! Run with:
//! ```text
//! cargo +nightly fuzz run partially_execute_settlement
//! ```

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use treasury::{TreasuryContract, TreasuryContractClient};

/// Maximum number of settlement legs we will attempt to feed into the
/// contract from a single fuzz input. Keeping this bounded prevents the fuzz
/// harness itself from dominating runtime while still covering multi-leg
/// partial execution paths.
const MAX_LEGS: usize = 16;

/// Maximum number of arbitrary "malicious" callers we will register.
const MAX_CALLERS: usize = 8;

/// A single settlement leg as described by the fuzzer.
#[derive(Arbitrary, Debug, Clone)]
struct FuzzLeg {
    /// Index into the caller set that should authorize this leg.
    caller_idx: u8,
    /// Amount to settle for this leg.
    amount: i128,
    /// Whether the fuzzer wants this leg marked as already executed.
    pre_executed: bool,
}

/// Structured fuzz input for the partial settlement entry point.
#[derive(Arbitrary, Debug, Clone)]
struct FuzzInput {
    /// Number of callers to register (clamped to `MAX_CALLERS`).
    caller_count: u8,
    /// The settlement legs to attempt.
    legs: Vec<FuzzLeg>,
    /// Whether to attempt execution from an unregistered caller.
    use_unregistered_caller: bool,
    /// Whether to attempt execution with an empty leg set.
    empty_legs: bool,
    /// Arbitrary ledger sequence to advance to before execution.
    ledger_seq: u32,
    /// Arbitrary timestamp to advance to before execution.
    timestamp: u64,
}

impl FuzzInput {
    /// Normalize the raw fuzz input into bounded, safe-to-use values.
    fn normalized(&self) -> NormalizedInput {
        let caller_count = (self.caller_count as usize).min(MAX_CALLERS).max(1);

        let legs: std::vec::Vec<FuzzLeg> = if self.empty_legs {
            std::vec::Vec::new()
        } else {
            self.legs
                .iter()
                .take(MAX_LEGS)
                .cloned()
                .collect::<std::vec::Vec<_>>()
        };

        NormalizedInput {
            caller_count,
            legs,
            use_unregistered_caller: self.use_unregistered_caller,
            ledger_seq: self.ledger_seq,
            timestamp: self.timestamp,
        }
    }
}

struct NormalizedInput {
    caller_count: usize,
    legs: std::vec::Vec<FuzzLeg>,
    use_unregistered_caller: bool,
    ledger_seq: u32,
    timestamp: u64,
}

fuzz_target!(|input: FuzzInput| {
    let normalized = input.normalized();

    let env = Env::default();
    env.mock_all_auths();

    // Advance the ledger to the fuzzer-chosen point. This exercises any
    // time/sequence-dependent validation in the settlement logic.
    env.ledger().with_mut(|li| {
        li.sequence_number = normalized.ledger_seq;
        li.timestamp = normalized.timestamp;
    });

    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);

    // Register a bounded set of callers. These represent potentially
    // malicious actors that may attempt to authorize legs they do not own.
    let mut callers: std::vec::Vec<Address> = std::vec::Vec::new();
    for _ in 0..normalized.caller_count {
        callers.push(Address::generate(&env));
    }

    // Build the settlement legs as Soroban `Vec`s keyed by caller.
    let mut leg_callers: Vec<Address> = Vec::new(&env);
    let mut leg_amounts: Vec<i128> = Vec::new(&env);
    let mut leg_pre_executed: Vec<bool> = Vec::new(&env);

    for leg in &normalized.legs {
        let caller = if normalized.use_unregistered_caller {
            // Deliberately use an address that was never registered as a
            // caller to simulate an unauthorized settlement attempt.
            Address::generate(&env)
        } else {
            let idx = (leg.caller_idx as usize) % callers.len();
            callers[idx].clone()
        };

        leg_callers.push_back(caller);
        leg_amounts.push_back(leg.amount);
        leg_pre_executed.push_back(leg.pre_executed);
    }

    // Attempt the partial settlement. The contract must either succeed while
    // preserving invariants, or return a typed error. It must never panic.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.partially_execute_settlement(
            &leg_callers,
            &leg_amounts,
            &leg_pre_executed,
        )
    }));

    match result {
        Ok(Ok(())) => {
            // Successful execution: verify basic invariants hold.
            //
            // A successful partial settlement must not have executed more
            // legs than were provided, and every leg marked as pre-executed
            // must remain executed (idempotency).
            let executed = client.get_executed_leg_count();
            assert!(
                executed <= normalized.legs.len() as u32,
                "executed leg count exceeded provided legs",
            );

            for (i, leg) in normalized.legs.iter().enumerate() {
                if leg.pre_executed {
                    assert!(
                        client.is_leg_executed(&(i as u32)),
                        "pre-executed leg was not preserved as executed",
                    );
                }
            }
        }
        Ok(Err(_contract_error)) => {
            // A typed contract error is the expected outcome for malformed
            // or unauthorized inputs. Nothing further to assert here beyond
            // the fact that we did not panic.
        }
        Err(_panic) => {
            // A panic escaping the contract is a bug: the contract must
            // return typed errors for malicious input rather than aborting
            // the host. Surface this clearly to the fuzzer.
            panic!(
                "partially_execute_settlement panicked on malicious input: {:?}",
                normalized.legs,
            );
        }
    }
});