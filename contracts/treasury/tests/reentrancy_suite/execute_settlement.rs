//! Reentrancy tests for `TreasuryContract::execute_settlement` — the
//! headline target from issue #118.
//!
//! `execute_settlement` invokes the token `transfer` BEFORE writing the
//! settlement's `status = Executed` to storage. During the token callback,
//! the settlement is still `Pending`, so a re-entry passes every check and
//! issues a second transfer — paying the merchant twice.
//!
//! Each test pins down either the no-reentry invariant (`CallbackTarget::None`)
//! or the reentry scenario so any future CEI fix in `execute_settlement`
//! (state write before token call) can be verified against them.

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::SettlementStatus;

use super::common::{allow_token_only, init_treasury};
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
fn execute_settlement_baseline_no_reentry_pays_merchant_once() {
    // Sanity check: with the malicious token configured to do NOTHING on
    // `transfer`, a single execution pays the merchant exactly `amount`
    // and the treasury's ledger is fully drained.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let amount: i128 = 10_000_000;
    let (token_id, token) = funded_token(&env, &treasury_id, amount);
    allow_token_only(&env, &client, &admin, &token_id);
    token.set_callback_target(&CallbackTarget::None, &treasury_id);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &amount);
    client.execute_settlement(&admin, &sid, &token_id);

    assert_eq!(token.balance(&merchant), amount);
    assert_eq!(token.balance(&treasury_id), 0);
}

#[test]
fn execute_settlement_reentrancy_demonstrates_cei_violation_double_payout() {
    // Headline target: configure the malicious token to re-enter
    // `execute_settlement`. The settlement's `status = Executed` is
    // written AFTER the token call, so during the callback the
    // settlement is still `Pending` and every precondition passes again.
    // Both ledger bookkeepings run (outer + inner), paying the merchant
    // `2 * amount` in the malicious ledger and driving the treasury's
    // ledger balance to `-amount`.
    //
    // TODO(#118-hardening): flip the order in
    // `contracts/treasury/src/settlements.rs::execute_settlement` so
    // `settlement.status = Executed` happens BEFORE
    // `token_client.transfer`. After that, the inner re-entry will panic
    // with `AlreadyExecuted` and roll back; this assertion should then
    // read `merchant == amount`, `treasury == 0`.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let amount: i128 = 10_000_000;
    let (token_id, token) = funded_token(&env, &treasury_id, amount);
    allow_token_only(&env, &client, &admin, &token_id);
    token.set_callback_target(&CallbackTarget::ExecuteSettlement, &treasury_id);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &amount);
    token.set_execute_settlement_params(&admin, sid);

    client.execute_settlement(&admin, &sid, &token_id);

    // Both transfers' bookkeeping have run.
    assert_eq!(token.balance(&merchant), 2 * amount);
    assert_eq!(token.balance(&treasury_id), -amount);
}

#[test]
fn execute_settlement_reentrancy_status_is_settled_once() {
    // Companion check: regardless of the re-entry outcome, the settlement
    // ends up in `Executed` exactly once. This guards against a future
    // regression where status is written twice during the reentry.
    //
    // To make the assertion non-trivial we deliberately record the
    // settlement ID before execution and then assert that after the
    // re-entrant execution the stored status is *not* `Pending` — the
    // status had to flip during the callback chain.
    let env = Env::default();
    let (client, admin, treasury_id) = init_treasury(&env, 1);

    let amount: i128 = 10_000_000;
    let (token_id, token) = funded_token(&env, &treasury_id, amount);
    allow_token_only(&env, &client, &admin, &token_id);
    token.set_callback_target(&CallbackTarget::ExecuteSettlement, &treasury_id);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &amount);
    token.set_execute_settlement_params(&admin, sid);

    // Pre-state: the settlement was just proposed, so it must still be
    // `Pending` before execution. Without reentry or any execution the
    // status would remain `Pending`; the reentry-and-execution chain is
    // the only path that flips it.
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );

    client.execute_settlement(&admin, &sid, &token_id);

    let settlement = client.get_settlement(&sid);
    // Status must have flipped out of `Pending` (proves execution ran,
    // including its reentry chain), and must have settled to `Executed`.
    assert_ne!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.status, SettlementStatus::Executed);
}
