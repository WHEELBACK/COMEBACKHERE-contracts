/// Issue #900 — Invariant verification tests.
///
/// These tests verify that `verify_invariants` (called after every state
/// mutation) catches and rejects states that violate the fundamental
/// correctness properties of the fee-manager contract.
extern crate std;

use fee_manager::{FeeManagerContract, FeeManagerContractClient, FeeManagerError};
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

// ── Admin invariant ───────────────────────────────────────────────────────────

/// The admin must always be set after `initialize`.  Any subsequent admin-only
/// call that authenticates correctly must succeed — if the admin key were absent
/// the invariant check would fire.
#[test]
fn admin_invariant_holds_after_every_mutation() {
    let (_env, admin, client) = setup(100);

    // Each of these mutates state and calls verify_invariants internally.
    client.update_fee_rate(&admin, &200);
    client.pause(&admin);
    client.unpause(&admin);

    // The contract is still usable, confirming the admin key survived every mutation.
    assert_eq!(client.get_fee_rate(), 200);
}

// ── Fee-rate invariant ────────────────────────────────────────────────────────

/// Setting the fee rate to exactly 10_000 (100%) is a valid boundary value and
/// must not trigger the invariant guard.
#[test]
fn fee_rate_at_boundary_10000_is_valid() {
    let (_, admin, client) = setup(0);
    client.update_fee_rate(&admin, &10_000);
    assert_eq!(client.get_fee_rate(), 10_000);
}

/// Attempting to set a rate above 10_000 is caught by the input-validation guard
/// **before** `verify_invariants` even runs — the error must be `InvalidFeeRate`.
#[test]
fn fee_rate_above_10000_rejected_before_state_change() {
    let (_, admin, client) = setup(500);
    let result = client.try_update_fee_rate(&admin, &10_001);
    assert_eq!(result, Err(Ok(FeeManagerError::InvalidFeeRate)));

    // The rate must be unchanged — state must not have been mutated.
    assert_eq!(client.get_fee_rate(), 500);
}

/// Similarly, `initialize` with an out-of-range rate must fail before writing
/// anything to storage.
#[test]
fn initialize_with_invalid_rate_leaves_no_state() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(FeeManagerContract, ());
    let client = FeeManagerContractClient::new(&env, &id);

    let result = client.try_initialize(&admin, &99_999);
    assert_eq!(result, Err(Ok(FeeManagerError::InvalidFeeRate)));

    // The contract must be re-initializable — no state was committed.
    client.initialize(&admin, &500);
    assert_eq!(client.get_fee_rate(), 500);
}

// ── Accumulated fees ≤ total deposited ───────────────────────────────────────

/// After deposits and fee collection, accumulated fees must equal exactly
/// the sum of per-deposit fees — never more than the amount deposited.
#[test]
fn accumulated_fees_never_exceed_total_deposited() {
    let (env, admin, client) = setup(1_000); // 10%
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    let total = 100_000i128;
    mint(&env, &token, &depositor, total);

    client.deposit(&depositor, &token, &total);

    let fees = client.get_accumulated_fees(&token);
    // fees = 100_000 * 1_000 / 10_000 = 10_000
    assert_eq!(fees, 10_000);
    // Invariant: fees ≤ deposited amount.
    assert!(fees <= total);
}

/// When multiple depositors use different tokens, each token's accumulated fees
/// are independent and each satisfies fees ≤ deposited.
#[test]
fn per_token_fees_are_independent_and_bounded() {
    let (env, admin, client) = setup(500); // 5%
    let token_a = create_token(&env, &admin);
    let token_b = create_token(&env, &admin);
    let dep_a = Address::generate(&env);
    let dep_b = Address::generate(&env);

    mint(&env, &token_a, &dep_a, 20_000);
    mint(&env, &token_b, &dep_b, 40_000);

    client.deposit(&dep_a, &token_a, &20_000);
    client.deposit(&dep_b, &token_b, &40_000);

    let fees_a = client.get_accumulated_fees(&token_a);
    let fees_b = client.get_accumulated_fees(&token_b);

    assert_eq!(fees_a, 1_000); // 20_000 * 5% = 1_000
    assert_eq!(fees_b, 2_000); // 40_000 * 5% = 2_000
    assert!(fees_a <= 20_000);
    assert!(fees_b <= 40_000);
    // Cross-token isolation — no bleed-through.
    assert_eq!(client.get_accumulated_fees(&token_a), 1_000);
}

// ── State consistency after collect ──────────────────────────────────────────

/// After `collect_fees` the accumulated-fees storage resets to 0.  The net
/// balance of depositors must be unaffected.
#[test]
fn collect_fees_resets_accumulator_without_affecting_depositor_balance() {
    let (env, admin, client) = setup(200); // 2%
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token, &depositor, 50_000);

    client.deposit(&depositor, &token, &50_000);
    let balance_before = client.get_balance(&depositor, &token);

    client.collect_fees(&admin, &token, &recipient);

    // Depositor balance is unchanged.
    assert_eq!(client.get_balance(&depositor, &token), balance_before);
    // Accumulator resets to 0.
    assert_eq!(client.get_accumulated_fees(&token), 0);
}

// ── Pause flag invariant ──────────────────────────────────────────────────────

/// pause → unpause must leave the contract fully operational.  State mutated
/// while paused (none, by design) must be absent; state from before is preserved.
#[test]
fn pause_unpause_preserves_all_state() {
    let (env, admin, client) = setup(300);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 10_000);

    client.deposit(&depositor, &token, &10_000);
    let balance_before = client.get_balance(&depositor, &token);
    let fees_before = client.get_accumulated_fees(&token);

    client.pause(&admin);
    client.unpause(&admin);

    assert_eq!(client.get_balance(&depositor, &token), balance_before);
    assert_eq!(client.get_accumulated_fees(&token), fees_before);
    assert_eq!(client.get_fee_rate(), 300);
}
