//! Exact-boundary coverage for the `MAX_BATCH_SIZE` cap on the three bulk
//! operations: `bulk_allow_addresses`, `bulk_block_addresses`, and
//! `bulk_check_addresses` (issue: cover max batch sizes for bulk operations).
//!
//! Each bulk entrypoint must:
//!   1. Accept a list of exactly `MAX_BATCH_SIZE` addresses without error.
//!   2. Reject a list of `MAX_BATCH_SIZE + 1` addresses with the typed error
//!      `ContractError::BatchTooLarge` before performing *any* state mutations.
//!
//! Testing both boundaries (cap and cap + 1) means the limit is explicitly
//! documented in the test suite and cannot silently change — a future refactor
//! that raises or removes the cap will cause these tests to fail, preventing an
//! unintentional budget overrun from reaching integrators.
//!
//! `MAX_BATCH_SIZE` is re-exported by the `compliance` crate so this suite
//! asserts against the real constant rather than a hand-mirrored copy that
//! could silently drift.
//!
//! Note on cooldown: `bulk_allow_addresses` and `bulk_block_addresses` enforce
//! `BULK_OP_COOLDOWN_SECS` between successive calls by the same admin. Where a
//! test needs to issue more than one such call it advances the ledger clock past
//! the cooldown window first, matching the pattern used in
//! `address_index_full_boundary_test.rs`.

use compliance::{
    ComplianceContract, ComplianceContractClient, ContractError, BULK_OP_COOLDOWN_SECS,
    MAX_BATCH_SIZE,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

/// Build a `soroban_sdk::Vec` of `n` freshly generated addresses.
fn gen_addresses(env: &Env, n: u32) -> soroban_sdk::Vec<Address> {
    let mut v = soroban_sdk::Vec::new(env);
    for _ in 0..n {
        v.push_back(Address::generate(env));
    }
    v
}

/// Advance the ledger clock past the bulk-op cooldown window so a second call
/// to the same bulk entrypoint by the same admin is not rejected with
/// `BulkOperationCooldown`.
fn advance_past_cooldown(env: &Env) {
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + BULK_OP_COOLDOWN_SECS + 1);
}

// ── bulk_allow_addresses ──────────────────────────────────────────────────────

/// Exactly `MAX_BATCH_SIZE` addresses must be accepted and every address must
/// be allowed afterward — no off-by-one rejection at the boundary.
#[test]
fn bulk_allow_exactly_max_batch_size_succeeds() {
    let (env, admin, client) = setup();
    let addresses = gen_addresses(&env, MAX_BATCH_SIZE);

    client.bulk_allow_addresses(&admin, &addresses);

    // Every address in the batch must now be allowed.
    for addr in addresses.iter() {
        assert!(
            client.is_allowed(&addr),
            "address should be allowed after a max-size bulk_allow_addresses call"
        );
    }
}

/// `MAX_BATCH_SIZE + 1` addresses must be rejected with `BatchTooLarge` before
/// any state mutations occur — none of the addresses in the oversized batch
/// must become allowed as a side-effect.
#[test]
fn bulk_allow_one_over_max_batch_size_is_rejected() {
    let (env, admin, client) = setup();
    let addresses = gen_addresses(&env, MAX_BATCH_SIZE + 1);

    let result = client.try_bulk_allow_addresses(&admin, &addresses);
    assert_eq!(
        result,
        Err(Ok(ContractError::BatchTooLarge)),
        "bulk_allow_addresses with MAX_BATCH_SIZE + 1 addresses must return BatchTooLarge"
    );

    // No state mutation must have occurred — every address remains not-allowed.
    for addr in addresses.iter() {
        assert!(
            !client.is_allowed(&addr),
            "no address in a rejected oversized batch should become allowed"
        );
    }
}

// ── bulk_block_addresses ──────────────────────────────────────────────────────

/// Exactly `MAX_BATCH_SIZE` addresses must be accepted by `bulk_block_addresses`
/// and every address must be blocked afterward.
#[test]
fn bulk_block_exactly_max_batch_size_succeeds() {
    let (env, admin, client) = setup();
    let addresses = gen_addresses(&env, MAX_BATCH_SIZE);

    client.bulk_block_addresses(&admin, &addresses);

    // Every address in the batch must now be blocked (and therefore not allowed).
    for addr in addresses.iter() {
        assert!(
            client.is_blocked(&addr),
            "address should be blocked after a max-size bulk_block_addresses call"
        );
        assert!(
            !client.is_allowed(&addr),
            "blocked address must not be allowed"
        );
    }
}

/// `MAX_BATCH_SIZE + 1` addresses must be rejected with `BatchTooLarge` before
/// any address is blocked.
#[test]
fn bulk_block_one_over_max_batch_size_is_rejected() {
    let (env, admin, client) = setup();
    let addresses = gen_addresses(&env, MAX_BATCH_SIZE + 1);

    let result = client.try_bulk_block_addresses(&admin, &addresses);
    assert_eq!(
        result,
        Err(Ok(ContractError::BatchTooLarge)),
        "bulk_block_addresses with MAX_BATCH_SIZE + 1 addresses must return BatchTooLarge"
    );

    // No state mutation must have occurred — every address remains unblocked.
    for addr in addresses.iter() {
        assert!(
            !client.is_blocked(&addr),
            "no address in a rejected oversized batch should become blocked"
        );
    }
}

// ── bulk_check_addresses ──────────────────────────────────────────────────────

/// Exactly `MAX_BATCH_SIZE` addresses must be accepted by `bulk_check_addresses`
/// and the returned vec must have exactly `MAX_BATCH_SIZE` entries.
///
/// Note: `bulk_check_addresses` is a read-only query (no auth, no state
/// mutations, no cooldown). If it does not yet enforce `MAX_BATCH_SIZE` this
/// test documents the *expected* behaviour and will fail until the cap is added,
/// serving as the tracking test for that work.
#[test]
fn bulk_check_exactly_max_batch_size_succeeds() {
    let (env, admin, client) = setup();
    let addresses = gen_addresses(&env, MAX_BATCH_SIZE);

    // Allow half of them so the result vector has a mix of true/false values.
    let half = MAX_BATCH_SIZE / 2;
    let allowed_batch = {
        let mut v = soroban_sdk::Vec::new(&env);
        for i in 0..half {
            v.push_back(addresses.get(i).unwrap());
        }
        v
    };
    client.bulk_allow_addresses(&admin, &allowed_batch);

    let results = client.bulk_check_addresses(&addresses);

    assert_eq!(
        results.len(),
        MAX_BATCH_SIZE,
        "bulk_check_addresses must return exactly one result per input address"
    );
    // Spot-check the first allowed and first non-allowed entries.
    assert!(results.get(0).unwrap(), "first address should be allowed");
    assert!(
        !results.get(half).unwrap(),
        "address past the allowed half should not be allowed"
    );
}

/// `MAX_BATCH_SIZE + 1` addresses must be rejected with `BatchTooLarge`.
///
/// `bulk_check_addresses` currently has no size guard. This test documents the
/// required behaviour: it will fail (and therefore block a merge) until the
/// cap is enforced, preventing unbounded read loops from exceeding the ledger
/// budget.
#[test]
fn bulk_check_one_over_max_batch_size_is_rejected() {
    let (env, _, client) = setup();
    let addresses = gen_addresses(&env, MAX_BATCH_SIZE + 1);

    let result = client.try_bulk_check_addresses(&addresses);
    assert_eq!(
        result,
        Err(Ok(ContractError::BatchTooLarge)),
        "bulk_check_addresses with MAX_BATCH_SIZE + 1 addresses must return BatchTooLarge"
    );
}

// ── cross-op symmetry ─────────────────────────────────────────────────────────

/// Both `bulk_allow_addresses` and `bulk_block_addresses` must reject at the
/// same threshold (`MAX_BATCH_SIZE + 1`), confirming the cap is applied
/// consistently across all mutating bulk operations.
#[test]
fn bulk_allow_and_bulk_block_share_the_same_cap() {
    let (env, admin, client) = setup();

    let allow_batch = gen_addresses(&env, MAX_BATCH_SIZE + 1);
    let allow_result = client.try_bulk_allow_addresses(&admin, &allow_batch);
    assert_eq!(allow_result, Err(Ok(ContractError::BatchTooLarge)));

    // Advance clock so the block call is not rejected by the cooldown instead.
    advance_past_cooldown(&env);

    let block_batch = gen_addresses(&env, MAX_BATCH_SIZE + 1);
    let block_result = client.try_bulk_block_addresses(&admin, &block_batch);
    assert_eq!(block_result, Err(Ok(ContractError::BatchTooLarge)));
}
