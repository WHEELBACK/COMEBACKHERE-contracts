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

    client.withdraw_all(&admin, &token_id, &recipient);

    // Inner + outer drain: recipient receives twice the balance.
    assert_eq!(token.balance(&recipient), 2 * amount);
    // Treasury's malicious-ledger balance goes negative (overdraft).
    assert_eq!(token.balance(&treasury_id), -amount);
    // NOTE: `withdraw_all` does not have a CEI ordering hazard in the
    // same sense as the other entrypoints because it doesn't write any
    // treasury-side state — it only reads the external token balance,
    // transfers, and emits an event. The double-drain demonstrated here
    // is purely a property of an attacker-controlled token that allows
    // overdraft; a real SEP-41 contract would panic with
    // `InsufficientBalance` on the second `transfer` and roll the
    // whole transaction back.
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
