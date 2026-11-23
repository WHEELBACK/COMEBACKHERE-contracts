//! Reentrancy tests for `TreasuryContract::partially_execute_settlement`.
//!
//! `partially_execute_settlement` invokes the token `transfer` BEFORE
//! writing `status = PartiallyExecuted`. The reentrancy hazard mirrors
//! `execute_settlement`: a malicious token can re-enter while the
//! settlement is still `Pending`, slipping past every guard and triggering
//! a second `partial_amount` transfer.

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryError};

use super::common::init_treasury;
use super::malicious_token::{CallbackTarget, ReentrancyToken, ReentrancyTokenClient};

fn funded_token<'a>(
    env: &Env,
    treasury_id: &Address,
    amount: i128,
) -> (Address, ReentrancyTokenClient<'a>) {
    let token_id = env.register_contract(None, ReentrancyToken);
    let token = ReentrancyTokenClient::new(env, &token_id);
    token.mint(treasury_id, &amount);
    (token_id, token)
}

#[test]
fn partially_execute_settlement_baseline_no_reentry_pays_merchant_once() {
    // Baseline: with no callback configured, a single partial execution
    // transfers exactly `partial_amount` to the merchant.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let total: i128 = 10_000_000;
    let partial: i128 = 3_000_000;
    let (token_id, token) = funded_token(&env, &treasury_id, total);
    token.set_callback_target(&CallbackTarget::None, &treasury_id);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &total, &0_u64);
    client.partially_execute_settlement(&admin, &sid, &partial, &token_id);

    assert_eq!(token.balance(&merchant), partial);
    assert_eq!(token.balance(&treasury_id), total - partial);
}

#[test]
fn partially_execute_settlement_reentrancy_demonstrates_cei_violation() {
    // Headline target for partial execution: configure the malicious
    // token to re-enter `partially_execute_settlement`. The settlement's
    // `status = PartiallyExecuted` is written AFTER the token call, so
    // during the callback the settlement is still `Pending` and every
    // guard passes again. Both ledger bookkeepings run, paying the
    // merchant `2 * partial_amount`.
    //
    // TODO(#118-hardening): flip the order in
    // `contracts/treasury/src/settlements.rs::partially_execute_settlement`
    // so `settlement.status = PartiallyExecuted` happens BEFORE
    // `token_client.transfer`. After that, the inner re-entry panics
    // with `AlreadyExecuted` and rolls back; this assertion should then
    // read `merchant == partial`, `treasury == total - partial`.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let total: i128 = 10_000_000;
    let partial: i128 = 3_000_000;
    let (token_id, token) = funded_token(&env, &treasury_id, total);
    token.set_callback_target(&CallbackTarget::PartiallyExecuteSettlement, &treasury_id);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &total, &0_u64);
    token.set_partial_execute_params(&admin, &sid, &partial);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.partially_execute_settlement(&admin, &sid, &partial, &token_id);
    }));
    assert!(result.is_err(), "reentrant partial execution should abort");

    assert_eq!(token.balance(&merchant), 0);
    assert_eq!(token.balance(&treasury_id), total);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::Pending);
}

#[test]
fn partially_execute_settlement_rejects_underflowing_partial_amount() {
    // A partial amount larger than the settlement total would underflow
    // the remaining-balance computation. The contract must reject it with
    // `ArithmeticUnderflow` and leave all state untouched.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let total: i128 = 10_000_000;
    let partial: i128 = total + 1;
    let (token_id, token) = funded_token(&env, &treasury_id, partial);
    token.set_callback_target(&CallbackTarget::None, &treasury_id);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &total, &0_u64);

    let result = client.try_partially_execute_settlement(&admin, &sid, &partial, &token_id);
    assert_eq!(result, Err(Ok(TreasuryError::ArithmeticUnderflow)));

    // Invariant: no funds moved and the settlement is still Pending.
    assert_eq!(token.balance(&merchant), 0);
    assert_eq!(token.balance(&treasury_id), partial);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.amount, total);
}

#[test]
fn partially_execute_settlement_reentrant_underflow_attempt_is_rejected() {
    // The malicious token re-enters `partially_execute_settlement` with an
    // amount that would underflow the remaining balance. The outer call
    // must abort, roll back the transfer, and leave the settlement
    // Pending with no funds moved.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let total: i128 = 10_000_000;
    let partial: i128 = 3_000_000;
    let (token_id, token) = funded_token(&env, &treasury_id, total);
    token.set_callback_target(&CallbackTarget::PartiallyExecuteSettlement, &treasury_id);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &total, &0_u64);
    // Reentrant call attempts to settle more than the total.
    token.set_partial_execute_params(&admin, &sid, &(total + 1));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.partially_execute_settlement(&admin, &sid, &partial, &token_id);
    }));
    assert!(result.is_err(), "reentrant underflow attempt should abort");

    // Invariant: the outer transfer was rolled back and no state changed.
    assert_eq!(token.balance(&merchant), 0);
    assert_eq!(token.balance(&treasury_id), total);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.amount, total);
}
