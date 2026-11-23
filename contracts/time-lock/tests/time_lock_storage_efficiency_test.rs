//! Issue #943: streamlined storage deserialization in `time-lock`.
//!
//! Proposal status checks read the stored map once and decode `status` before
//! the timestamps/target. Finalized records therefore avoid unnecessary host
//! conversions, while active records retain the strict #945 checks. These
//! tests pin the observable behaviour, the ledger footprint, and a generous
//! CPU/memory ceiling so a deserialization regression is caught.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use time_lock::{
    DataKey, TimeLockContract, TimeLockContractClient, TimeLockError, TimelockProposal,
    TimelockStatus,
};

/// Generous ceiling for 50 repeated status queries. A status projection must
/// stay far below a full proposal lifecycle; this catches accidental
/// per-record scans or duplicate storage reads without pinning SDK constants.
const MAX_STATUS_QUERY_INSTRUCTIONS: u64 = 500_000;
const MAX_STATUS_QUERY_MEMORY: u64 = 500_000;

struct Fixture<'a> {
    env: &'a Env,
    client: TimeLockContractClient<'a>,
    admin: Address,
    target: Address,
    cid: Address,
}

fn setup(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let target = Address::generate(env);
    let cid = env.register(TimeLockContract, ());
    let client = TimeLockContractClient::new(env, &cid);
    client.initialize(&admin);
    Fixture {
        env,
        client,
        admin,
        target,
        cid,
    }
}

fn proposal(target: &Address, delay_secs: u64, status: TimelockStatus) -> TimelockProposal {
    TimelockProposal {
        target: target.clone(),
        delay_secs,
        proposed_at: 1_000,
        executable_at: 1_000u64.saturating_add(delay_secs),
        status,
    }
}

fn advance_to(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|li| li.timestamp = timestamp);
}

fn write_proposal(f: &Fixture<'_>, value: &TimelockProposal) {
    f.env.as_contract(&f.cid, || {
        f.env
            .storage()
            .instance()
            .set(&DataKey::TimelockProposal(f.target.clone()), value);
    });
}

#[test]
fn status_projection_matches_full_lifecycle() {
    let env = Env::default();
    let f = setup(&env);

    assert!(!f.client.has_active_timelock(&f.target));
    f.client.propose_timelock(&f.admin, &f.target, &10);
    assert!(f.client.has_active_timelock(&f.target));

    advance_to(&env, 1_010);
    let executed = f.client.execute_timelock(&f.admin, &f.target);
    assert_eq!(executed.status, TimelockStatus::Executed);
    assert!(!f.client.has_active_timelock(&f.target));
    assert_eq!(
        f.client.try_execute_timelock(&f.admin, &f.target),
        Err(Ok(TimeLockError::TimelockInactive))
    );

    // A finalized target can be re-proposed without decoding the old record.
    f.client.propose_timelock(&f.admin, &f.target, &20);
    assert!(f.client.has_active_timelock(&f.target));
    f.client.cancel_timelock(&f.admin, &f.target);
    assert!(!f.client.has_active_timelock(&f.target));
    assert_eq!(
        f.client.try_cancel_timelock(&f.admin, &f.target),
        Err(Ok(TimeLockError::TimelockInactive))
    );
}

#[test]
fn active_records_are_still_strictly_verified_after_projection() {
    let env = Env::default();
    let f = setup(&env);

    let mut corrupt = proposal(&f.target, 10, TimelockStatus::Active);
    corrupt.target = Address::generate(&env);
    corrupt.executable_at = 1_001; // inconsistent with proposed_at + delay
    write_proposal(&f, &corrupt);

    assert!(!f.client.has_active_timelock(&f.target));
    assert_eq!(
        f.client.try_propose_timelock(&f.admin, &f.target, &10),
        Err(Ok(TimeLockError::StateInconsistency))
    );
    assert_eq!(
        f.client.try_execute_timelock(&f.admin, &f.target),
        Err(Ok(TimeLockError::StateInconsistency))
    );
    assert_eq!(
        f.client.try_cancel_timelock(&f.admin, &f.target),
        Err(Ok(TimeLockError::StateInconsistency))
    );
    assert!(f.client.get_timelock(&f.target).is_none());
}

#[test]
fn fuzz_projection_short_circuits_only_final_statuses() {
    let env = Env::default();
    let f = setup(&env);

    for delay in [1u64, 10, u64::MAX - 100] {
        for status in [
            TimelockStatus::Active,
            TimelockStatus::Executed,
            TimelockStatus::Cancelled,
        ] {
            let record = proposal(&f.target, delay, status.clone());
            write_proposal(&f, &record);

            let expected_active = status == TimelockStatus::Active
                && record
                    .proposed_at
                    .checked_add(record.delay_secs)
                    .is_some_and(|at| at == record.executable_at);
            assert_eq!(
                f.client.has_active_timelock(&f.target),
                expected_active,
                "delay={delay} status={status:?}"
            );

            if status != TimelockStatus::Active {
                assert_eq!(
                    f.client.try_execute_timelock(&f.admin, &f.target),
                    Err(Ok(TimeLockError::TimelockInactive))
                );
                assert_eq!(
                    f.client.try_propose_timelock(&f.admin, &f.target, &10),
                    Ok(Ok(())),
                    "a final status must allow re-proposal"
                );
                // Restore the finalized record for the next assertion set.
                write_proposal(&f, &record);
            }
        }
    }
}

#[test]
fn repeated_status_queries_stay_within_metering_and_footprint_bounds() {
    let env = Env::default();
    let f = setup(&env);
    f.client.propose_timelock(&f.admin, &f.target, &10);
    advance_to(&env, 1_010);
    f.client.execute_timelock(&f.admin, &f.target);

    // Warm up so one-time contract instantiation is excluded.
    assert!(!f.client.has_active_timelock(&f.target));

    let mut budget = env.cost_estimate().budget();
    budget.reset_unlimited();
    budget.reset_tracker();
    let cpu_before = budget.cpu_instruction_cost();
    let mem_before = budget.memory_bytes_cost();

    for _ in 0..50 {
        assert!(!f.client.has_active_timelock(&f.target));
    }

    let cpu = budget.cpu_instruction_cost().saturating_sub(cpu_before);
    let mem = budget.memory_bytes_cost().saturating_sub(mem_before);
    std::println!("50 status queries: cpu={cpu} mem={mem}");

    assert!(
        cpu <= MAX_STATUS_QUERY_INSTRUCTIONS,
        "status queries used {cpu} CPU instructions, expected <= {MAX_STATUS_QUERY_INSTRUCTIONS}"
    );
    assert!(
        mem <= MAX_STATUS_QUERY_MEMORY,
        "status queries used {mem} memory bytes, expected <= {MAX_STATUS_QUERY_MEMORY}"
    );
}

#[test]
fn missing_and_final_queries_do_not_allocate_proposal_payloads() {
    let env = Env::default();
    let f = setup(&env);

    assert!(!f.client.has_active_timelock(&f.target));
    assert_eq!(
        f.client.try_execute_timelock(&f.admin, &f.target),
        Err(Ok(TimeLockError::TimelockNotFound))
    );

    f.client.propose_timelock(&f.admin, &f.target, &10);
    advance_to(&env, 1_200);
    f.client.execute_timelock(&f.admin, &f.target);

    assert!(!f.client.has_active_timelock(&f.target));
    assert_eq!(
        f.client.try_cancel_timelock(&f.admin, &f.target),
        Err(Ok(TimeLockError::TimelockInactive))
    );
}
