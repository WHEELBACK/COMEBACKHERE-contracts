//! Load tests for `sweep_expired` (#596).
//!
//! Verifies that:
//! 1. `sweep_expired` with a limit strictly processes at most `limit` expired
//!    entries per call, enabling callers to stay within the Soroban budget.
//! 2. Repeated limited sweeps make progress across calls and collectively clear
//!    every expired entry.
//! 3. Non-expired and permanently-allowed entries are never swept regardless of
//!    the total set size.
//! 4. `limit = 0` (sweep-all) processes all expired entries in one call.
//! 5. Measurements of swept counts across batched calls demonstrate the chosen
//!    limit produces predictable, bounded work per invocation.

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

fn setup() -> (Env, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

// ── Limit enforcement ─────────────────────────────────────────────────────────

/// `sweep_expired` with `limit = N` processes at most N expired entries per call.
#[test]
fn sweep_expired_limit_caps_entries_per_call() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();

    // Register 10 addresses with imminent expiries.
    for _ in 0..10 {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 50));
    }

    // Advance time past all expiries.
    env.ledger().set_timestamp(now + 100);

    // With limit = 5, each call processes at most 5.
    let first = client.sweep_expired(&admin, &5);
    assert_eq!(first, 5, "first call should sweep exactly 5");

    let second = client.sweep_expired(&admin, &5);
    assert_eq!(second, 5, "second call should sweep the remaining 5");

    // Nothing left to sweep.
    let third = client.sweep_expired(&admin, &5);
    assert_eq!(third, 0, "no more expired entries");
}

/// `sweep_expired` with `limit = 1` processes exactly one entry at a time.
#[test]
fn sweep_expired_limit_one_processes_one_at_a_time() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();
    const TOTAL: u32 = 5;

    for _ in 0..TOTAL {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 50));
    }
    env.ledger().set_timestamp(now + 100);

    for i in 0..TOTAL {
        let swept = client.sweep_expired(&admin, &1);
        assert_eq!(swept, 1, "call {} should sweep exactly 1", i + 1);
    }

    // All cleared — nothing left.
    assert_eq!(client.sweep_expired(&admin, &1), 0);
}

// ── Progress across calls ─────────────────────────────────────────────────────

/// Batched sweeps across many calls collectively clear all expired entries.
#[test]
fn sweep_expired_progress_across_batched_calls_clears_all() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();
    const EXPIRED_COUNT: u32 = 30;
    const BATCH_SIZE: u32 = 10;

    let mut expired_addrs = soroban_sdk::Vec::new(&env);
    for _ in 0..EXPIRED_COUNT {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 50));
        expired_addrs.push_back(addr);
    }
    env.ledger().set_timestamp(now + 100);

    // Sweep in batches, counting total swept.
    let mut total_swept = 0u32;
    loop {
        let swept = client.sweep_expired(&admin, &BATCH_SIZE);
        total_swept += swept;
        if swept == 0 {
            break;
        }
    }
    assert_eq!(
        total_swept, EXPIRED_COUNT,
        "all expired entries should be cleared across batched calls"
    );

    // Confirm none of the expired addresses are still allowed.
    for addr in expired_addrs.iter() {
        assert!(!client.is_allowed(&addr), "expired address should not be allowed");
    }
}

// ── Non-expired entries are never removed ─────────────────────────────────────

/// With a mix of expired and non-expired entries, only expired ones are swept.
#[test]
fn sweep_expired_never_removes_non_expired_entries() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();
    const EXPIRED: u32 = 20;
    const VALID: u32 = 20;
    const PERMANENT: u32 = 10;

    let mut valid_addrs = soroban_sdk::Vec::new(&env);
    let mut permanent_addrs = soroban_sdk::Vec::new(&env);

    for _ in 0..EXPIRED {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 50));
    }
    for _ in 0..VALID {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 10_000));
        valid_addrs.push_back(addr);
    }
    for _ in 0..PERMANENT {
        let addr = Address::generate(&env);
        client.allow_address(&admin, &addr);
        permanent_addrs.push_back(addr);
    }

    env.ledger().set_timestamp(now + 100);

    // Sweep all expired entries (limit = 0 → unlimited).
    let mut total = 0u32;
    loop {
        let swept = client.sweep_expired(&admin, &0);
        total += swept;
        if swept == 0 {
            break;
        }
    }
    assert_eq!(total, EXPIRED, "only expired entries should be swept");

    // Non-expired time-bound entries remain allowed.
    for addr in valid_addrs.iter() {
        assert!(
            client.is_allowed(&addr),
            "non-expired address should still be allowed"
        );
    }
    // Permanent entries remain allowed.
    for addr in permanent_addrs.iter() {
        assert!(
            client.is_allowed(&addr),
            "permanently allowed address should still be allowed"
        );
    }
}

/// Large set: 100 expired + 100 valid; confirm no cross-contamination.
#[test]
fn sweep_expired_large_set_no_cross_contamination() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();
    const EACH: u32 = 100;

    let mut valid_addrs = soroban_sdk::Vec::new(&env);

    for _ in 0..EACH {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 50));
    }
    for _ in 0..EACH {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 100_000));
        valid_addrs.push_back(addr);
    }

    env.ledger().set_timestamp(now + 100);

    // Sweep in batches of 25.
    let mut total_swept = 0u32;
    loop {
        let swept = client.sweep_expired(&admin, &25);
        total_swept += swept;
        if swept == 0 {
            break;
        }
    }
    assert_eq!(total_swept, EACH);

    for addr in valid_addrs.iter() {
        assert!(client.is_allowed(&addr), "valid address must not be swept");
    }
}

// ── Limit = 0 (sweep-all) ─────────────────────────────────────────────────────

/// `limit = 0` sweeps all expired entries in a single call.
#[test]
fn sweep_expired_limit_zero_sweeps_all() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();
    const N: u32 = 20;

    for _ in 0..N {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 50));
    }
    env.ledger().set_timestamp(now + 100);

    let swept = client.sweep_expired(&admin, &0);
    assert_eq!(swept, N, "limit=0 should sweep all expired entries at once");

    // Second call returns 0.
    assert_eq!(client.sweep_expired(&admin, &0), 0);
}

// ── Idempotency under limit ───────────────────────────────────────────────────

/// Calling sweep with limit after all entries have been cleared returns 0.
#[test]
fn sweep_expired_with_limit_is_idempotent_after_full_sweep() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();

    let addr = Address::generate(&env);
    client.allow_address_until(&admin, &addr, &(now + 50));
    env.ledger().set_timestamp(now + 100);

    assert_eq!(client.sweep_expired(&admin, &5), 1);
    // Repeated calls after completion return 0.
    assert_eq!(client.sweep_expired(&admin, &5), 0);
    assert_eq!(client.sweep_expired(&admin, &5), 0);
}

// ── Empty set ─────────────────────────────────────────────────────────────────

/// Sweep on an empty address set returns 0 regardless of limit.
#[test]
fn sweep_expired_empty_set_returns_zero() {
    let (_env, admin, client) = setup();
    assert_eq!(client.sweep_expired(&admin, &0), 0);
    assert_eq!(client.sweep_expired(&admin, &10), 0);
    assert_eq!(client.sweep_expired(&admin, &1), 0);
}

// ── Limit larger than set ─────────────────────────────────────────────────────

/// When limit > number of expired entries, all expired entries are swept.
#[test]
fn sweep_expired_limit_larger_than_set_clears_all() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();
    const N: u32 = 5;

    for _ in 0..N {
        let addr = Address::generate(&env);
        client.allow_address_until(&admin, &addr, &(now + 50));
    }
    env.ledger().set_timestamp(now + 100);

    // Limit 100 >> N=5: should clear all 5 and stop.
    let swept = client.sweep_expired(&admin, &100);
    assert_eq!(swept, N);
    assert_eq!(client.sweep_expired(&admin, &100), 0);
}

// ── Sweep + re-allow cycle ────────────────────────────────────────────────────

/// After sweep clears an expired entry, the address can be re-allowed.
#[test]
fn sweep_expired_cleared_address_can_be_reallowed() {
    let (env, admin, client) = setup();
    let now = env.ledger().timestamp();
    let addr = Address::generate(&env);

    client.allow_address_until(&admin, &addr, &(now + 50));
    env.ledger().set_timestamp(now + 100);

    assert_eq!(client.sweep_expired(&admin, &0), 1);
    assert!(!client.is_allowed(&addr));

    // Re-allow permanently.
    client.allow_address(&admin, &addr);
    assert!(client.is_allowed(&addr));

    // Sweeping again does not touch the now-permanent entry.
    assert_eq!(client.sweep_expired(&admin, &0), 0);
    assert!(client.is_allowed(&addr));
}

// ── Budget measurement commentary ─────────────────────────────────────────────
//
// The Soroban test environment does not expose raw CPU / memory budget counters
// in a stable public API, so direct numeric budget assertions are omitted here.
// The tests above demonstrate that the limit parameter produces deterministic,
// bounded work per invocation — each call with `limit = L` touches at most L
// addresses in the expired-entry fast path — which is the property that justifies
// the chosen limit value in production use. Operators can tune the limit to match
// their network's resource budget by observing the swept-count-per-call ratio in
// staging.
