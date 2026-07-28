//! Reentrancy tests for `TreasuryContract::withdraw`.
//!
//! `withdraw` performs its internal balance decrement BEFORE the external
//! token transfer (CEI-compliant), so any re-entry sees the post-decrement
//! balance. A malicious token that re-enters `withdraw` with the same amount
//! must panic with `InsufficientBalance`, aborting the whole transaction.

use soroban_sdk::{testutils::Address as _, Address, Env};

use super::common::init_treasury;
use super::malicious_token::{CallbackTarget, ReentrancyToken, ReentrancyTokenClient};

#[test]
fn withdraw_reentrancy_is_blocked_by_insufficient_balance() {
    // The internal `Balance(to)` is decremented (from `amount` to `0`)
    // BEFORE the cross-contract `transfer`. The re-entry then calls
    // `withdraw(to, token, amount)` again, finds `Balance(to) == 0`, and
    // panics with `InsufficientBalance`. Soroban's transaction-level
    // panic rollback restores the outer decrement.
    let env = Env::default();
    let (client, _admin, treasury_id) = init_treasury(&env, 1);

    let user = Address::generate(&env);
    let amount: i128 = 5_000_000;

    let token_id = env.register_contract(None, ReentrancyToken);
    let token = ReentrancyTokenClient::new(&env, &token_id);
    token.mint(&user, &amount);
    token.mint(&treasury_id, &amount);

    // Pre-fund internal balance via a normal deposit (no callback).
    token.set_callback_target(&CallbackTarget::None, &treasury_id);
    client.deposit(&user, &token_id, &amount);
    assert_eq!(client.get_balance(&user), amount);

    // Replace the malicious token's configuration with a `Withdraw`
    // re-entry target. The previous `CallbackTarget::None` setting is
    // overwritten here (not composed with) so the same harness instance
    // can drive multiple entrypoints within one test.
    token.set_callback_target(&CallbackTarget::Withdraw, &treasury_id);

    // The outer withdrawal attempts to send `amount`; the inner re-attempt
    // sees a zero balance and panics. The whole transaction rolls back.
    let result = client.try_withdraw(&user, &token_id, &amount);
    assert!(result.is_err(), "reentrant withdraw must panic");

    // State preserved by transaction-level rollback.
    assert_eq!(client.get_balance(&user), amount);
}

#[test]
fn withdraw_reentrancy_with_sufficient_balance_drains_twice() {
    // When the user pre-deposits twice the withdrawal amount, the inner
    // reentry has a non-negative remaining balance and the double-execution
    // is NOT blocked — the treasury treats it as two legitimate
    // withdrawals. This documents exactly the leak the CEI ordering
    // permits: one re-entry, one extra transfer of the same amount.
    //
    // Walk: `Balance(user)` starts at `2 * amount`. Outer `withdraw`
    // decrements to `amount` and triggers the malicious callback. The
    // re-entry `withdraw` decrements to `0` (and is allowed because
    // `amount >= amount`, not strictly less than). Both ledger
    // bookkeepings then run in their respective `transfer` calls.
    let env = Env::default();
    let (client, _admin, treasury_id) = init_treasury(&env, 1);

    let user = Address::generate(&env);
    let amount: i128 = 5_000_000;

    let token_id = env.register_contract(None, ReentrancyToken);
    let token = ReentrancyTokenClient::new(&env, &token_id);
    token.mint(&user, &(amount * 2));
    token.mint(&treasury_id, &(amount * 2));

    // User deposits exactly `2 * amount` so two `amount`-sized withdrawals
    // both survive the `Balance < amount` check.
    token.set_callback_target(&CallbackTarget::None, &treasury_id);
    client.deposit(&user, &token_id, &(amount * 2));
    assert_eq!(client.get_balance(&user), amount * 2);

    token.set_callback_target(&CallbackTarget::Withdraw, &treasury_id);

    // First call withdraws once AND triggers a re-entry that withdraws a
    // second time, draining the user's internal balance to zero.
    client.withdraw(&user, &token_id, &amount);

    // Both transfers' bookkeeping have run, so the malicious ledger shows
    // the user received `2 * amount` and the treasury shed `2 * amount`.
    assert_eq!(client.get_balance(&user), 0);
    assert_eq!(token.balance(&user), amount * 2);
    assert_eq!(token.balance(&treasury_id), amount * 2);
}
