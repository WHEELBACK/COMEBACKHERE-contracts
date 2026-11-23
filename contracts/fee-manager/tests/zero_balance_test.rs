/// Issue #903 — Zero-balance / edge-case tests.
///
/// Every test in this file verifies that the contract handles zero/uninitialized
/// storage gracefully and that zero-amount operations return
/// `FeeManagerError::ZeroAmount` instead of panicking.
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

// ── get_balance on uninitialized storage ──────────────────────────────────────

/// Issue #903: `get_balance` must return 0 for an address that has never deposited,
/// without panicking.
#[test]
fn get_balance_returns_zero_for_never_deposited_address() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let stranger = Address::generate(&env);

    // No deposit ever made — must not panic.
    assert_eq!(client.get_balance(&stranger, &token), 0);
}

/// Issue #903: `get_accumulated_fees` must return 0 for a token with no fee
/// history, without panicking.
#[test]
fn get_accumulated_fees_returns_zero_for_uninitialized_token() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);

    assert_eq!(client.get_accumulated_fees(&token), 0);
}

// ── Deposit of zero ───────────────────────────────────────────────────────────

/// Issue #903: depositing 0 must return `ZeroAmount`, not panic or silently succeed.
#[test]
fn deposit_zero_returns_zero_amount_error() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);

    let result = client.try_deposit(&depositor, &token, &0);
    assert_eq!(result, Err(Ok(FeeManagerError::ZeroAmount)));
}

// ── Collect fees when balance is zero ────────────────────────────────────────

/// Issue #903: calling `collect_fees` when there are no accumulated fees must
/// return `ZeroBalance` rather than transferring 0 tokens or panicking.
#[test]
fn collect_fees_on_zero_balance_returns_zero_balance_error() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let recipient = Address::generate(&env);

    let result = client.try_collect_fees(&admin, &token, &recipient);
    assert_eq!(result, Err(Ok(FeeManagerError::ZeroBalance)));
}

/// Issue #903: `collect_fees` after fees have been collected once resets the
/// accumulator to 0, and a second call returns `ZeroBalance`.
#[test]
fn collect_fees_twice_second_call_returns_zero_balance_error() {
    let (env, admin, client) = setup(500); // 5%
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token, &depositor, 10_000);

    client.deposit(&depositor, &token, &10_000);
    client.collect_fees(&admin, &token, &recipient); // first collection — succeeds
    let result = client.try_collect_fees(&admin, &token, &recipient); // second — zero balance
    assert_eq!(result, Err(Ok(FeeManagerError::ZeroBalance)));
}

// ── Zero fee rate ─────────────────────────────────────────────────────────────

/// Issue #903: with a zero fee rate, depositing produces a zero fee and a full
/// net balance.  `get_accumulated_fees` must return 0 (not panic on 0 storage).
#[test]
fn deposit_with_zero_fee_rate_accumulates_zero_fees() {
    let (env, admin, client) = setup(0);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 5_000);

    client.deposit(&depositor, &token, &5_000);

    assert_eq!(client.get_accumulated_fees(&token), 0);
    assert_eq!(client.get_balance(&depositor, &token), 5_000);
}

/// Issue #903: even with a zero fee rate, depositing 0 is still rejected.
#[test]
fn deposit_zero_with_zero_fee_rate_still_errors() {
    let (env, admin, client) = setup(0);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);

    let result = client.try_deposit(&depositor, &token, &0);
    assert_eq!(result, Err(Ok(FeeManagerError::ZeroAmount)));
}

// ── Multiple depositors, independent balances ─────────────────────────────────

/// Issue #903: each depositor's balance is stored independently.  A stranger's
/// balance must remain 0 when only another depositor has deposited.
#[test]
fn balance_per_depositor_is_independent() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let depositor_a = Address::generate(&env);
    let depositor_b = Address::generate(&env);
    mint(&env, &token, &depositor_a, 1_000);

    client.deposit(&depositor_a, &token, &1_000);

    assert!(client.get_balance(&depositor_a, &token) > 0);
    assert_eq!(client.get_balance(&depositor_b, &token), 0);
}

// ── get_fee_rate before and after update ─────────────────────────────────────

/// Issue #903: `get_fee_rate` must not panic on a freshly-initialized contract.
#[test]
fn get_fee_rate_never_panics() {
    let (_, _, client) = setup(777);
    assert_eq!(client.get_fee_rate(), 777);
}
