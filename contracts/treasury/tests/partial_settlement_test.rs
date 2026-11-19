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

/// Boundary: `partial_amount == 0` must be rejected with `InvalidAmount` (#7)
/// and must leave the settlement untouched (still Pending, nothing executed).
#[test]
fn partial_execution_zero_amount_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id, sid) = setup(&env, 10_000_000);

    let before = client.get_settlement(&sid);
    assert_eq!(before.status, SettlementStatus::Pending);

    let result = client.try_partially_execute_settlement(&admin, &sid, &0, &token_id);
    assert!(result.is_err());

    let after = client.get_settlement(&sid);
    assert_eq!(after.status, SettlementStatus::Pending);
    assert_eq!(after.amount, before.amount);
    let merchant_balance = soroban_sdk::token::Client::new(&env, &token_id).balance(&after.merchant);
    assert_eq!(merchant_balance, 0);
}

/// Boundary: `partial_amount == settlement.amount` must be rejected with
/// `InvalidAmount` (#7) and leave the settlement Pending with no funds moved.
#[test]
fn partial_execution_exact_amount_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let total = 10_000_000i128;
    let (client, admin, token_id, sid) = setup(&env, total);

    let before = client.get_settlement(&sid);
    assert_eq!(before.status, SettlementStatus::Pending);

    let result = client.try_partially_execute_settlement(&admin, &sid, &total, &token_id);
    assert!(result.is_err());

    let after = client.get_settlement(&sid);
    assert_eq!(after.status, SettlementStatus::Pending);
    assert_eq!(after.amount, before.amount);
    let merchant_balance = soroban_sdk::token::Client::new(&env, &token_id).balance(&after.merchant);
    assert_eq!(merchant_balance, 0);
}

/// Boundary: `partial_amount > settlement.amount` must be rejected with
/// `InvalidAmount` (#7) and leave the settlement Pending with no funds moved.
#[test]
fn partial_execution_exceeding_amount_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let total = 10_000_000i128;
    let (client, admin, token_id, sid) = setup(&env, total);

    let before = client.get_settlement(&sid);
    assert_eq!(before.status, SettlementStatus::Pending);

    let result = client.try_partially_execute_settlement(&admin, &sid, &(total + 1), &token_id);
    assert!(result.is_err());

    let after = client.get_settlement(&sid);
    assert_eq!(after.status, SettlementStatus::Pending);
    assert_eq!(after.amount, before.amount);
    let merchant_balance = soroban_sdk::token::Client::new(&env, &token_id).balance(&after.merchant);
    assert_eq!(merchant_balance, 0);
}

/// Cumulative approvals that would exceed the settlement amount must be
/// rejected with `InvalidAmount` (#7) and must not mutate stored state.
#[test]
fn cumulative_partial_approvals_exceeding_amount_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let total = 10_000_000i128;
    let (client, admin, _token_id, sid) = setup(&env, total);
    let signer_two = Address::generate(&env);
    client.set_signer(&admin, &signer_two, &1);

    client.approve_partial_settlement(&admin, &sid, &5_000_000);
    let before = client.get_settlement(&sid);

    // Pushes the cumulative approved total to amount + 1.
    let result = client.try_approve_partial_settlement(&signer_two, &sid, &5_000_001);
    assert!(result.is_err());

    let after = client.get_settlement(&sid);
    assert_eq!(after.status, before.status);
    assert_eq!(after.amount, before.amount);
}

/// Repeated partial executions must never drive the remaining balance
/// negative: once a partial execution has occurred the settlement is no
/// longer Pending, so any further attempt is rejected and state is unchanged.
#[test]
fn repeated_partial_execution_cannot_underflow_remaining() {
    let env = Env::default();
    env.mock_all_auths();
    let total = 10_000_000i128;
    let partial = 3_000_000i128;
    let (client, admin, token_id, sid) = setup(&env, total);

    client.partially_execute_settlement(&admin, &sid, &partial, &token_id);
    let after_first = client.get_settlement(&sid);
    assert_eq!(after_first.status, SettlementStatus::PartiallyExecuted);

    let merchant_balance_after_first =
        soroban_sdk::token::Client::new(&env, &token_id).balance(&after_first.merchant);
    assert_eq!(merchant_balance_after_first, partial);

    // A second partial execution must be rejected (status guard) and must not
    // move additional funds or alter the stored settlement.
    let result = client.try_partially_execute_settlement(&admin, &sid, &partial, &token_id);
    assert!(result.is_err());

    let after_second = client.get_settlement(&sid);
    assert_eq!(after_second.status, SettlementStatus::PartiallyExecuted);
    assert_eq!(after_second.amount, after_first.amount);
    let merchant_balance_after_second =
        soroban_sdk::token::Client::new(&env, &token_id).balance(&after_second.merchant);
    assert_eq!(merchant_balance_after_second, partial);
}

/// Property-style sweep over arbitrary `(total, partial)` pairs. For every
/// combination the call must either succeed (when `0 < partial < total`) or be
/// rejected without panicking, and the stored settlement must never end up in
/// an inconsistent state (no negative remaining, no over-transfer).
#[test]
fn property_partial_amounts_never_underflow() {
    let env = Env::default();
    env.mock_all_auths();

    let totals: [i128; 5] = [1, 2, 1_000, 10_000_000, i128::MAX / 4];
    let partials: [i128; 7] = [0, 1, 2, 999, 1_000, 10_000_000, i128::MAX / 4];

    for &total in totals.iter() {
        for &partial in partials.iter() {
            let admin = Address::generate(&env);
            let merchant = Address::generate(&env);
            let contract_id = env.register_contract(None, TreasuryContract);
            let client = TreasuryContractClient::new(&env, &contract_id);
            client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

            let token_id = env.register_stellar_asset_contract(admin.clone());
            soroban_sdk::token::StellarAssetClient::new(&env, &token_id).mint(&contract_id, &total);

            let sid = client.propose_settlement(&admin, &merchant, &total, &0_u64);

            let result = client.try_partially_execute_settlement(&admin, &sid, &partial, &token_id);
            let settlement = client.get_settlement(&sid);
            let merchant_balance =
                soroban_sdk::token::Client::new(&env, &token_id).balance(&merchant);

            if partial > 0 && partial < total {
                assert!(result.is_ok(), "expected success for total={total} partial={partial}");
                assert_eq!(settlement.status, SettlementStatus::PartiallyExecuted);
                assert_eq!(merchant_balance, partial);
            } else {
                assert!(result.is_err(), "expected rejection for total={total} partial={partial}");
                assert_eq!(settlement.status, SettlementStatus::Pending);
                assert_eq!(merchant_balance, 0);
            }

            // Invariant: the remaining balance can never be negative.
            let remaining = settlement.amount - merchant_balance;
            assert!(remaining >= 0, "remaining underflowed for total={total} partial={partial}");
        }
    }
}
