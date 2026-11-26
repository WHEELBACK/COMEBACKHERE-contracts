extern crate std;

use fee_manager::{FeeManagerContract, FeeManagerContractClient, FeeManagerError};
use soroban_sdk::{
    testutils::{Address as _, Events},
    token::{Client as TokenClient, StellarAssetClient},
    xdr::{ContractEvent, ContractEventBody, ScVal},
    Address, Env,
};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Deploy and initialize a fee-manager, returning (env, admin, client).
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

/// Extract the event topic symbol name from a ContractEvent.
fn event_symbol(event: &ContractEvent) -> std::string::String {
    match &event.body {
        ContractEventBody::V0(v0) => match v0.topics.first() {
            Some(ScVal::Symbol(sym)) => sym.to_string(),
            _ => std::string::String::new(),
        },
    }
}

// ── Initialization ────────────────────────────────────────────────────────────

#[test]
fn initialize_stores_admin_and_fee_rate() {
    let (_env, admin, client) = setup(250);
    assert_eq!(client.get_fee_rate(), 250);
    // Admin is not directly queryable but we verify indirectly by calling an
    // admin-only function without error.
    client.update_fee_rate(&admin, &300);
    assert_eq!(client.get_fee_rate(), 300);
}

#[test]
fn initialize_rejects_already_initialized() {
    let (_env, admin, client) = setup(100);
    let result = client.try_initialize(&admin, &100);
    assert_eq!(result, Err(Ok(FeeManagerError::AlreadyInitialized)));
}

#[test]
fn initialize_rejects_fee_rate_above_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(FeeManagerContract, ());
    let client = FeeManagerContractClient::new(&env, &id);
    let result = client.try_initialize(&admin, &10_001);
    assert_eq!(result, Err(Ok(FeeManagerError::InvalidFeeRate)));
}

#[test]
fn initialize_accepts_boundary_fee_rates() {
    // 0 bps — no fee.
    let (_, _, c0) = setup(0);
    assert_eq!(c0.get_fee_rate(), 0);

    // 10_000 bps — full fee (100%).
    let (_, _, c10k) = setup(10_000);
    assert_eq!(c10k.get_fee_rate(), 10_000);
}

// ── Deposit ───────────────────────────────────────────────────────────────────

#[test]
fn deposit_records_net_balance_and_accumulates_fees() {
    let (env, admin, client) = setup(500); // 5% fee
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 10_000);

    client.deposit(&depositor, &token, &10_000);

    // fee = 10_000 * 500 / 10_000 = 500
    // net = 10_000 - 500 = 9_500
    assert_eq!(client.get_balance(&depositor, &token), 9_500);
    assert_eq!(client.get_accumulated_fees(&token), 500);
}

#[test]
fn deposit_with_zero_fee_rate_records_full_amount() {
    let (env, admin, client) = setup(0);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 1_000);

    client.deposit(&depositor, &token, &1_000);

    assert_eq!(client.get_balance(&depositor, &token), 1_000);
    assert_eq!(client.get_accumulated_fees(&token), 0);
}

#[test]
fn deposit_emits_deposited_event() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 500);

    client.deposit(&depositor, &token, &500);

    let events = env.events().all();
    let last = events.events().last().unwrap();
    assert_eq!(event_symbol(last), "deposited");
}

#[test]
fn deposit_multiple_times_accumulates_balances() {
    let (env, admin, client) = setup(1_000); // 10%
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 20_000);

    client.deposit(&depositor, &token, &10_000);
    client.deposit(&depositor, &token, &10_000);

    // Each deposit: fee = 1_000, net = 9_000
    assert_eq!(client.get_balance(&depositor, &token), 18_000);
    assert_eq!(client.get_accumulated_fees(&token), 2_000);
}

#[test]
fn deposit_while_paused_returns_contract_paused() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 100);

    client.pause(&admin);
    let result = client.try_deposit(&depositor, &token, &100);
    assert_eq!(result, Err(Ok(FeeManagerError::ContractPaused)));
}

// ── Collect fees ──────────────────────────────────────────────────────────────

#[test]
fn collect_fees_transfers_accumulated_to_recipient() {
    let (env, admin, client) = setup(200); // 2%
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token, &depositor, 50_000);

    client.deposit(&depositor, &token, &50_000);
    // fee = 50_000 * 200 / 10_000 = 1_000
    assert_eq!(client.get_accumulated_fees(&token), 1_000);

    client.collect_fees(&admin, &token, &recipient);

    // Fees reset to 0 after collection.
    assert_eq!(client.get_accumulated_fees(&token), 0);

    // Verify recipient received tokens.
    let token_client = TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&recipient), 1_000);
}

#[test]
fn collect_fees_emits_fees_collected_event() {
    let (env, admin, client) = setup(500);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint(&env, &token, &depositor, 10_000);

    client.deposit(&depositor, &token, &10_000);
    client.collect_fees(&admin, &token, &recipient);

    let events = env.events().all();
    let last = events.events().last().unwrap();
    assert_eq!(event_symbol(last), "fees_collected");
}

#[test]
fn collect_fees_non_admin_returns_unauthorized() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    let non_admin = Address::generate(&env);
    mint(&env, &token, &depositor, 10_000);
    client.deposit(&depositor, &token, &10_000);

    let result = client.try_collect_fees(&non_admin, &token, &non_admin);
    // Non-admin panics with Unauthorized via panic_with_error!
    assert!(result.is_err());
}

// ── Update fee rate ───────────────────────────────────────────────────────────

#[test]
fn update_fee_rate_changes_stored_rate() {
    let (_, admin, client) = setup(100);
    client.update_fee_rate(&admin, &250);
    assert_eq!(client.get_fee_rate(), 250);
}

#[test]
fn update_fee_rate_rejects_above_10000() {
    let (_, admin, client) = setup(100);
    let result = client.try_update_fee_rate(&admin, &10_001);
    assert_eq!(result, Err(Ok(FeeManagerError::InvalidFeeRate)));
}

#[test]
fn update_fee_rate_accepts_boundary_10000() {
    let (_, admin, client) = setup(0);
    client.update_fee_rate(&admin, &10_000);
    assert_eq!(client.get_fee_rate(), 10_000);
}

#[test]
fn update_fee_rate_emits_event() {
    let (env, admin, client) = setup(50);
    client.update_fee_rate(&admin, &75);

    let events = env.events().all();
    let last = events.events().last().unwrap();
    assert_eq!(event_symbol(last), "fee_rate_updated");
}

// ── Pause / unpause ───────────────────────────────────────────────────────────

#[test]
fn pause_and_unpause_cycle() {
    let (env, admin, client) = setup(100);
    let token = create_token(&env, &admin);
    let depositor = Address::generate(&env);
    mint(&env, &token, &depositor, 1_000);

    client.pause(&admin);
    // Deposit should fail.
    assert!(client.try_deposit(&depositor, &token, &1_000).is_err());

    client.unpause(&admin);
    // Deposit should succeed after unpausing.
    client.deposit(&depositor, &token, &1_000);
    // fee = 1_000 * 100 / 10_000 = 10, net = 990
    assert_eq!(client.get_balance(&depositor, &token), 990);
}

#[test]
fn pause_emits_event() {
    let (env, admin, client) = setup(0);
    client.pause(&admin);

    let events = env.events().all();
    let last = events.events().last().unwrap();
    assert_eq!(event_symbol(last), "fee_manager_paused");
}

#[test]
fn unpause_emits_event() {
    let (env, admin, client) = setup(0);
    client.pause(&admin);
    client.unpause(&admin);

    let events = env.events().all();
    let last = events.events().last().unwrap();
    assert_eq!(event_symbol(last), "fee_manager_unpaused");
}

// ── bump_state_ttl ────────────────────────────────────────────────────────────

#[test]
fn bump_state_ttl_does_not_panic() {
    let (_, _, client) = setup(100);
    // Should complete without error — keepers call this to prevent eviction.
    client.bump_state_ttl();
}
