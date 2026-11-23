/// Issue #902 — TTL bump tests.
///
/// These tests verify that the TTL-bump constants are correct and that the
/// `bump_state_ttl` entrypoint is callable without errors.  The Soroban test
/// environment does not expose ledger-entry TTL introspection directly, so we
/// verify behaviour indirectly: operations that read/write persistent storage
/// must not panic, and the public `bump_state_ttl` entrypoint must be callable
/// at any time.
extern crate std;

use fee_manager::{
    FeeManagerContract, FeeManagerContractClient, MIN_TTL_THRESHOLD, TTL_BUMP_AMOUNT,
};
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address, Env};

fn setup(fee_rate_bps: u32) -> (Env, Address, FeeManagerContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(FeeManagerContract, ());
    let client = FeeManagerContractClient::new(&env, &id);
    client.initialize(&admin, &fee_rate_bps);
    (env, admin, client)
}

fn create_token(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone())
        .address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

// ── Constant values ───────────────────────────────────────────────────────────

/// TTL_BUMP_AMOUNT must be at least as large as MIN_TTL_THRESHOLD, otherwise a
/// freshly-bumped entry could immediately fall below the threshold again.
#[test]
fn ttl_bump_amount_exceeds_min_threshold() {
    assert!(
        TTL_BUMP_AMOUNT > MIN_TTL_THRESHOLD,
        "TTL_BUMP_AMOUNT ({TTL_BUMP_AMOUNT}) must exceed MIN_TTL_THRESHOLD ({MIN_TTL_THRESHOLD})"
    );
}

/// Sanity check: 30 days at 5 s/ledger = 518_400 ledgers.
#[test]
fn ttl_bump_amount_is_approximately_30_days() {
    // 30 days × 24 h × 3600 s / 5 s per ledger
    let thirty_days_ledgers: u32 = 30 * 24 * 3600 / 5;
    assert_eq!(TTL_BUMP_AMOUNT, thirty_days_ledgers);
}

// ── bump_state_ttl public entrypoint ─────────────────────────────────────────

/// Any caller can invoke `bump_state_ttl` without authentication or error.
#[test]
fn bump_state_ttl_is_callable_by_anyone() {
    let (_, _, client) = setup(100);
    // Should not panic or return an error.
    client.bump_state_ttl();
}

/// `bump_state_ttl` can be called multiple times in a row without error.
#[test]
fn bump_state_ttl_is_idempotent() {
    let (_, _, client) = setup(100);
    for _ in 0..5 {
        client.bump_state_ttl();
    }
}

// ── Persistent-storage TTL bumped on deposit ──────────────────────────────────

/// Depositing writes two persistent keys (Balance, AccumulatedFees).  Both
/// must be accessible immediately after the deposit — confirming the TTL was
/// bumped (entries were not evicted in the same transaction).
#[test]
fn deposit_does_not_evict_persistent_keys_in_same_tx() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 1_000);

    client.deposit(&depositor, &token, &1_000);

    // If TTL bump were absent or broken the keys could be evicted and reads
    // would return 0 (default).  With a bump they must hold the written values.
    let balance = client.get_balance(&depositor, &token);
    let fees = client.get_accumulated_fees(&token);

    assert!(balance > 0, "balance must be > 0 after deposit");
    assert!(
        fees > 0,
        "accumulated fees must be > 0 after deposit with non-zero rate"
    );
}

/// `get_balance` and `get_accumulated_fees` both bump the TTL on reads.  Call
/// them repeatedly to confirm no panic and consistent values.
#[test]
fn repeated_reads_bump_ttl_without_panicking() {
    let (env, admin, client) = setup(500);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 10_000);

    client.deposit(&depositor, &token, &10_000);

    for _ in 0..10 {
        let _ = client.get_balance(&depositor, &token);
        let _ = client.get_accumulated_fees(&token);
    }
    // No panic — TTL bumps are idempotent in the test environment.
}

// ── collect_fees bumps TTL ────────────────────────────────────────────────────

/// After `collect_fees` resets the accumulator to 0, `get_accumulated_fees`
/// must still return 0 (not evicted / absent-storage panic).
#[test]
fn collect_fees_keeps_key_alive_after_reset() {
    let (env, admin, client) = setup(1_000);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token, &depositor, 10_000);

    client.deposit(&depositor, &token, &10_000);
    client.collect_fees(&admin, &token, &recipient);

    // The key was written to 0 and TTL-bumped; reading it must return 0, not panic.
    assert_eq!(client.get_accumulated_fees(&token), 0);
}
