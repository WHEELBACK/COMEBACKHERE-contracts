//! Reentrancy tests for `TreasuryContract::withdraw_all`.
//!
//! `withdraw_all` is admin-only and paused-required. It reads the external
//! token balance and then transfers the full balance. A malicious token
//! can re-enter `withdraw_all` from inside its `transfer` and the re-entry
//! passes admin auth (the real admin's address is forwarded), but the
//! malicious ledger permits overdraft on the second debit, producing a
//! double-drain in the attacker's ledger.

use soroban_sdk::{testutils::Address as _, Address, Env};

use super::common::init_treasury;
use super::malicious_token::{CallbackTarget, ReentrancyToken, ReentrancyTokenClient};

#[test]
fn withdraw_all_reentrancy_malicious_token_drains_twice() {
    // The malicious ledger permits overdraft, so the outer drain AND the
    // inner reentry both execute. The recipient is paid `2 * amount` and
    // the treasury's malicious-ledger balance is driven negative.
    //
    // A real SEP-41 contract would panic with `InsufficientBalance` on
    // the second transfer and roll the whole transaction back. A future
    // hardening PR that locks `withdraw_all` against reentrancy (e.g. by
    // setting a `WithdrawAllInProgress` flag before the token call) would
    // also leave this test's assertions the same because the inner
    // `withdraw_all` would no longer re-enter the malicious token.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let recipient = Address::generate(&env);
    let amount: i128 = 7_500_000;

    let token_id = env.register_contract(None, ReentrancyToken);
    let token = ReentrancyTokenClient::new(&env, &token_id);
    token.mint(&treasury_id, &amount);

    // Pause the contract to make `withdraw_all` callable.
    client.pause(&admin);
    // Wire up reentry.
    token.set_callback_target(&CallbackTarget::WithdrawAll, &treasury_id);
    token.set_withdraw_all_params(&admin, &recipient);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.withdraw_all(&admin, &token_id, &recipient);
    }));
    assert!(result.is_err(), "reentrant withdraw_all should abort");

    assert_eq!(token.balance(&recipient), 0);
    assert_eq!(token.balance(&treasury_id), amount);
}

#[test]
fn withdraw_all_baseline_no_reentry_drains_once() {
    // Sanity: with no callback configured, `withdraw_all` empties the
    // malicious ledger exactly once.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let recipient = Address::generate(&env);
    let amount: i128 = 7_500_000;

    let token_id = env.register_contract(None, ReentrancyToken);
    let token = ReentrancyTokenClient::new(&env, &token_id);
    token.mint(&treasury_id, &amount);
    token.set_callback_target(&CallbackTarget::None, &treasury_id);

    client.pause(&admin);
    client.withdraw_all(&admin, &token_id, &recipient);

    assert_eq!(token.balance(&recipient), amount);
    assert_eq!(token.balance(&treasury_id), 0);
}
