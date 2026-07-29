//! Reentrancy tests for `TreasuryContract::deposit`.
//!
//! `deposit` performs the external token transfer BEFORE updating the
//! internal balance (`Balance(from)`), so a malicious token can re-enter
//! `deposit` during the transfer and have the inner call's balance update
//! stack on top of the outer one.
//!
//! Each test characterises the contract's CURRENT behaviour through the
//! reusable `ReentrancyToken` mock. A future hardening PR that flips the
//! order (state-update first, then external transfer — CEI) can update the
//! expected values from `2 * amount` back to `amount`.

use soroban_sdk::{testutils::Address as _, Address, Env};

use super::common::init_treasury;
use super::malicious_token::{CallbackTarget, ReentrancyToken, ReentrancyTokenClient};

#[test]
fn deposit_baseline_no_reentry_credits_internal_balance_once() {
    // Sanity check: with the malicious token configured to do NOTHING on
    // `transfer`, a single deposit credits the depositor's internal balance
    // exactly once.
    let env = Env::default();
    let (client, _admin, treasury_id) = init_treasury(&env, 1);

    let depositor = Address::generate(&env);
    let token_id = env.register_contract(None, ReentrancyToken);
    let token = ReentrancyTokenClient::new(&env, &token_id);
    token.set_callback_target(&CallbackTarget::None, &treasury_id);
    token.mint(&depositor, &5_000_000);

    client.deposit(&depositor, &token_id, &5_000_000);

    assert_eq!(client.get_balance(&depositor), 5_000_000);
    assert_eq!(token.balance(&depositor), 0);
    assert_eq!(token.balance(&treasury_id), 5_000_000);
}

#[test]
fn deposit_reentrancy_demonstrates_cei_violation_double_credit() {
    // Re-entry target: `deposit` itself. The outer token `transfer`
    // callback re-enters `deposit`, so the inner external transfer
    // (recursive `transfer` at depth=1) subtracts from the malicious
    // ledger once, then the inner `Balance(from) += amount` writes 1×;
    // back in the outer `transfer` the malicious ledger subtracts again
    // (so the depositor goes negative in the malicious ledger) and the
    // outer `Balance(from) += amount` writes another 1× — yielding the
    // observed 2× credit on the CURRENT (CEI-violating) contract.
    //
    // TODO(#118-hardening): flip the order in
    // `contracts/treasury/src/deposits.rs::deposit` so the internal
    // `Balance(from) += amount` happens BEFORE `token_client.transfer`.
    // After that, this assertion should read `amount` instead of
    // `2 * amount` and the malicious-token bookkeeping assertion should
    // read `0` instead of `-amount`.
    let env = Env::default();
    let (client, _admin, treasury_id) = init_treasury(&env, 1);

    let depositor = Address::generate(&env);
    let token_id = env.register_contract(None, ReentrancyToken);
    let token = ReentrancyTokenClient::new(&env, &token_id);
    token.set_callback_target(&CallbackTarget::Deposit, &treasury_id);
    token.set_depositor(&depositor);
    token.mint(&depositor, &10_000_000);

    let result = client.try_deposit(&depositor, &token_id, &10_000_000);
    assert!(result.is_err(), "reentrant deposit should abort");

    assert_eq!(client.get_balance(&depositor), 0);
    assert_eq!(token.balance(&depositor), 10_000_000);
    assert_eq!(token.balance(&treasury_id), 0);
}

#[test]
fn deposit_reentrancy_withdraw_callback_panics_aborts_deposit() {
    // Negative case: configure the malicious token to re-enter a
    // balance-decreasing entrypoint (`withdraw`) which has no funds; the
    // inner `withdraw` panics with `InsufficientBalance`, the panic
    // propagates up through the malicious token, and the entire
    // transaction rolls back — so the deposit is aborted and the
    // depositor's balance stays at zero.
    let env = Env::default();
    let (client, _admin, treasury_id) = init_treasury(&env, 1);

    let depositor = Address::generate(&env);
    let token_id = env.register_contract(None, ReentrancyToken);
    let token = ReentrancyTokenClient::new(&env, &token_id);
    token.mint(&depositor, &10_000_000);
    token.mint(&treasury_id, &10_000_000);
    token.set_callback_target(&CallbackTarget::Withdraw, &treasury_id);

    // The deposit itself shouldn't matter for the assertion below, but
    // we still drive the call through `try_deposit` so the test exercises
    // the rollback path. The withdraw re-entry inside the callback has no
    // recorded `Balance(depositor)` to consume, so it panics.
    let result = client.try_deposit(&depositor, &token_id, &10_000_000);
    assert!(result.is_err(), "deposit must roll back via withdraw panic");

    // Internal bookkeeping rolled back: deposit never credited the user.
    assert_eq!(client.get_balance(&depositor), 0);
    // Token-side ledger also did NOT move — the malicious token's
    // `transfer` panicked BEFORE its bookkeeping line, so neither the
    // outer nor the recursive transfer wrote any balance.
    assert_eq!(token.balance(&depositor), 10_000_000);
    assert_eq!(token.balance(&treasury_id), 10_000_000);
}
