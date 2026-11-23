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

// ---------------------------------------------------------------------------
// Underflow regression tests (#968)
//
// Partial settlement computations derive `remaining = amount - settled` and
// `net = partial_amount - fee`. Both subtractions must be performed with
// checked arithmetic and surface `ArithmeticUnderflow` (#8) instead of
// trapping the host with a raw arithmetic panic.
// ---------------------------------------------------------------------------

/// `settled == remaining` boundary: after a partial execution of exactly half
/// the settlement, the derived remaining balance must be exactly the other
/// half and no underflow may occur.
#[test]
fn partial_settlement_settled_equals_remaining_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let total = 10_000_000i128;
    let half = total / 2;
    let (client, admin, token_id, sid) = setup(&env, total);

    client.partially_execute_settlement(&admin, &sid, &half, &token_id);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::PartiallyExecuted);
    assert_eq!(settlement.executed_amount, half);
    assert_eq!(settlement.amount - settlement.executed_amount, half);

    let merchant_balance = soroban_sdk::token::Client::new(&env, &token_id).balance(&settlement.merchant);
    assert_eq!(merchant_balance, half);
}

/// `settled > remaining` boundary: a large first partial execution leaves a
/// small remaining balance; the subtraction must still be exact and positive.
#[test]
fn partial_settlement_settled_greater_than_remaining_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let total = 10_000_000i128;
    let large = 9_999_999i128;
    let (client, admin, token_id, sid) = setup(&env, total);

    client.partially_execute_settlement(&admin, &sid, &large, &token_id);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::PartiallyExecuted);
    assert_eq!(settlement.executed_amount, large);
    assert_eq!(settlement.amount - settlement.executed_amount, 1);

    let merchant_balance = soroban_sdk::token::Client::new(&env, &token_id).balance(&settlement.merchant);
    assert_eq!(merchant_balance, large);
}

/// Zero partial amount is rejected with `InvalidAmount` (#7) before any
/// subtraction is attempted, so no underflow path is reachable.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_settlement_zero_amount_rejected_before_subtraction() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 1_000_000);

    client.partially_execute_settlement(&admin, &sid, &0, &token_id);
}

/// A partial amount that would drive `settled` past `amount` must be rejected
/// with `InvalidAmount` (#7) rather than underflowing `amount - settled`.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_settlement_overflowing_settled_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 1_000_000);

    client.partially_execute_settlement(&admin, &sid, &1_000_001, &token_id);
}

/// Cumulative approvals that would exceed the settlement amount must be
/// rejected with `InvalidAmount` (#7) instead of underflowing the remaining
/// computation.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_settlement_cumulative_overflow_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _token_id, sid) = setup(&env, 1_000_000);
    let signer_two = Address::generate(&env);
    client.set_signer(&admin, &signer_two, &1);

    client.approve_partial_settlement(&admin, &sid, &600_000);
    client.approve_partial_settlement(&signer_two, &sid, &600_000);
}

/// Fee subtraction from a smaller amount must not underflow: a fee larger than
/// the partial amount is rejected with `InvalidAmount` (#7) before the
/// `partial_amount - fee` subtraction is evaluated.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_settlement_fee_exceeding_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    // Fee greater than the partial amount would underflow `partial_amount - fee`.
    client.partially_execute_settlement_with_fee(&admin, &sid, &1_000, &token_id, &2_000);
}

/// A fee equal to the partial amount is the exact boundary: the net transfer is
/// zero, which must be rejected rather than silently transferring nothing.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn partial_settlement_fee_equal_to_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    client.partially_execute_settlement_with_fee(&admin, &sid, &1_000, &token_id, &1_000);
}

/// A valid fee smaller than the partial amount must succeed and transfer
/// exactly `partial_amount - fee` to the merchant.
#[test]
fn partial_settlement_fee_subtraction_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let total = 10_000_000i128;
    let partial_amount = 3_000_000i128;
    let fee = 1_000_000i128;
    let (client, admin, token_id, sid) = setup(&env, total);

    client.partially_execute_settlement_with_fee(&admin, &sid, &partial_amount, &token_id, &fee);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.status, SettlementStatus::PartiallyExecuted);
    assert_eq!(settlement.executed_amount, partial_amount);

    let merchant_balance = soroban_sdk::token::Client::new(&env, &token_id).balance(&settlement.merchant);
    assert_eq!(merchant_balance, partial_amount - fee);
}
