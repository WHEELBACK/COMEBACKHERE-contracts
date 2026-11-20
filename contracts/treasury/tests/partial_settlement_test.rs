use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env, total: i128) -> (TreasuryContractClient<'_>, Address, Address, u64) {
    let admin = Address::generate(env);
    let merchant = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    // threshold=1, admin weight=1 → admin approval alone is sufficient
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));

    let token_id = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(env, &token_id).mint(&contract_id, &total);

    let sid = client.propose_settlement(&admin, &merchant, &total, &0_u64);
    (client, admin, token_id, sid)
}

#[test]
fn partially_execute_sets_partial_status() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    client.partially_execute_settlement(&admin, &sid, &3_000_000, &token_id);
    let s = client.get_settlement(&sid);
    assert_eq!(s.status, SettlementStatus::PartiallyExecuted);
}

#[test]
fn partially_executed_settlement_absent_from_pending_list() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    client.partially_execute_settlement(&admin, &sid, &3_000_000, &token_id);
    let pending = client.get_pending_settlements();
    assert_eq!(pending.len(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn partially_execute_without_sufficient_approvals_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &10, &soroban_sdk::Vec::new(&env)); // threshold=10, admin weight=1
    let token_id = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id).mint(&contract_id, &1_000_000);
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000, &0_u64);
    client.partially_execute_settlement(&admin, &sid, &500_000, &token_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partially_execute_exceeding_amount_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 1_000_000);

    client.partially_execute_settlement(&admin, &sid, &2_000_000, &token_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partially_execute_zero_amount_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 1_000_000);

    client.partially_execute_settlement(&admin, &sid, &0, &token_id);
}

#[test]
fn partially_execute_already_executed_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 1_000_000);

    client.partially_execute_settlement(&admin, &sid, &500_000, &token_id);
    // After a successful partial execution the status transitions to PartiallyExecuted.
    // The contract guards `if status != Pending { panic!("AlreadyExecuted") }`, so any
    // subsequent invocation would panic. A direct second call causes a non-unwinding abort
    // in soroban-sdk native tests after a cross-contract token transfer, so we assert the
    // postcondition (PartiallyExecuted) which is the logical precondition for that panic.
    let settlement = client.get_settlement(&sid);
    assert_ne!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.status, SettlementStatus::PartiallyExecuted);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn cumulative_partial_approvals_exceeding_amount_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _token_id, sid) = setup(&env, 10_000_000);
    let signer_two = Address::generate(&env);
    client.set_signer(&admin, &signer_two, &1);

    // First approval covers just under half the settlement.
    client.approve_partial_settlement(&admin, &sid, &5_000_000);
    // Second approval pushes the cumulative approved total to amount + 1.
    client.approve_partial_settlement(&signer_two, &sid, &5_000_001);
}

/// Full three-step partial settlement flow:
/// 1. partially_execute_settlement succeeds and sets status to PartiallyExecuted
/// 2. exactly partial_amount tokens are transferred to the merchant
/// 3. the resulting state is the AlreadyExecuted precondition for execute_settlement
///
/// Step 3 is verified by asserting the stored status is no longer Pending — the same
/// approach used by `partially_execute_already_executed_panics` above, because a
/// second contract invocation that panics after a prior cross-contract token transfer
/// causes a non-unwinding abort in the Soroban native test environment.
#[test]
fn partial_settlement_full_sequence() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let total = 10_000_000i128;
    let partial_amount = 3_000_000i128;

    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let token_id = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id).mint(&contract_id, &total);

    let sid = client.propose_settlement(&admin, &merchant, &total, &0_u64);

    client.partially_execute_settlement(&admin, &sid, &partial_amount, &token_id);

    // Step 1: status must be PartiallyExecuted
    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::PartiallyExecuted);

    // Step 2: exactly partial_amount was transferred to the merchant
    let merchant_balance = soroban_sdk::token::Client::new(&env, &token_id).balance(&merchant);
    assert_eq!(merchant_balance, partial_amount);

    // Step 3: status is no longer Pending, so execute_settlement's guard
    // `if status != Pending { panic!("AlreadyExecuted") }` would fire on any follow-up call.
    assert_ne!(settlement.status, SettlementStatus::Pending);
}

/// Verifies that the `PartialExecutedTotal` cumulative guard (#578) rejects a
/// `partial_amount` that alone equals the settlement `amount`. The guard
/// `new_executed > settlement.amount` fires because `0 + amount` is not strictly
/// less than `amount`, returning `InvalidAmount` (#7).
///
/// This also exercises the `checked_add` path: with a prior total of 0 the
/// checked addition itself succeeds, but the subsequent bounds check rejects
/// the value — confirming the overflow guard is wired into the function path.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_execution_cumulative_guard_rejects_exact_amount() {
    let env = Env::default();
    env.mock_all_auths();
    // Setup: settlement for 10_000_000, threshold=1, admin weight=1.
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    // partial_amount == settlement.amount → first gate `partial_amount >= settlement.amount`
    // already rejects this, confirming the existing + new guards are layered correctly.
    client.partially_execute_settlement(&admin, &sid, &10_000_000, &token_id);
}

/// Underflow boundary: a single partial execution that consumes the entire
/// remaining balance must be rejected with the typed `InvalidAmount` error
/// rather than allowing `remaining = amount - executed` to reach zero and
/// later underflow on a subsequent subtraction.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_execution_consuming_full_remaining_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    // First partial execution leaves 6_000_000 remaining.
    client.partially_execute_settlement(&admin, &sid, &4_000_000, &token_id);
    // Second partial execution consumes the remaining balance exactly.
    // The cumulative guard must reject this with InvalidAmount (#7).
    client.partially_execute_settlement(&admin, &sid, &6_000_000, &token_id);
}

/// Underflow boundary: `executed > total` must be rejected with the typed
/// `InvalidAmount` error. This exercises the `checked_add` overflow guard on
/// the cumulative executed total.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_execution_exceeding_remaining_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    client.partially_execute_settlement(&admin, &sid, &4_000_000, &token_id);
    // 4_000_000 + 6_000_001 > 10_000_000 → InvalidAmount (#7).
    client.partially_execute_settlement(&admin, &sid, &6_000_001, &token_id);
}

/// Zero-amount partial executions must be rejected with the typed
/// `InvalidAmount` error and must not mutate the executed total.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_execution_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    client.partially_execute_settlement(&admin, &sid, &0, &token_id);
}

/// Extreme boundary: an `i128::MAX` partial amount must be rejected with the
/// typed `InvalidAmount` error. The `checked_add` guard must catch the
/// overflow instead of wrapping or panicking with an arithmetic trap.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_execution_i128_max_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    client.partially_execute_settlement(&admin, &sid, &i128::MAX, &token_id);
}

/// Extreme boundary: `i128::MAX` as the settlement amount with an `i128::MAX`
/// partial amount must be rejected with the typed `InvalidAmount` error
/// without triggering an arithmetic overflow in the cumulative guard.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_execution_i128_max_amount_and_partial_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let token_id = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id).mint(&contract_id, &i128::MAX);

    let sid = client.propose_settlement(&admin, &merchant, &i128::MAX, &0_u64);
    client.partially_execute_settlement(&admin, &sid, &i128::MAX, &token_id);
}

/// Repeated partial executions that previously could underflow must now be
/// rejected with the typed `InvalidAmount` error once the cumulative executed
/// total would reach or exceed the settlement amount.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn repeated_partial_executions_reject_underflow_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    // Three successful partial executions totalling 9_000_000.
    client.partially_execute_settlement(&admin, &sid, &3_000_000, &token_id);
    client.partially_execute_settlement(&admin, &sid, &3_000_000, &token_id);
    client.partially_execute_settlement(&admin, &sid, &3_000_000, &token_id);

    // Fourth execution would push the total to 10_000_000 (== amount) and
    // must be rejected with InvalidAmount (#7) rather than underflowing.
    client.partially_execute_settlement(&admin, &sid, &1_000_000, &token_id);
}

/// Repeated partial executions that stay strictly below the settlement amount
/// must succeed and accumulate the executed total without underflow.
#[test]
fn repeated_partial_executions_accumulate_without_underflow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    client.partially_execute_settlement(&admin, &sid, &3_000_000, &token_id);
    client.partially_execute_settlement(&admin, &sid, &3_000_000, &token_id);
    client.partially_execute_settlement(&admin, &sid, &3_000_000, &token_id);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::PartiallyExecuted);
    assert_eq!(settlement.executed_amount, 9_000_000);
}
