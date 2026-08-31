use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

// Same order of magnitude as MAX_BATCH_EXPIRE_INSTRUCTIONS in
// contracts/invoice/tests/invoice_load_test.rs — get_pending_settlements_page
// is a read-only scan, so this is a generous ceiling, not a tight one.
const PAGE_INSTRUCTION_BUDGET: u64 = 100_000_000;

fn setup_with_settlements(env: &Env, n: u64) -> (TreasuryContractClient<'_>, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &100, &soroban_sdk::Vec::new(env));
    for _ in 0..n {
        let merchant = Address::generate(env);
        client.propose_settlement(&admin, &merchant, &1_000_000);
    }
    (client, admin)
}

fn setup_with_interspersed_executions<'a>(
    env: &'a Env,
    pattern: &[bool],
) -> (TreasuryContractClient<'a>, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));

    let token_id = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(env, &token_id).mint(&contract_id, &1_000_000_000);

    let mut sid = 0u64;
    for &executed in pattern {
        let merchant = Address::generate(env);
        sid = client.propose_settlement(&admin, &merchant, &1_000_000);
        if executed {
            client.execute_settlement(&admin, &sid, &token_id);
        }
    }

    (client, admin)
}

// ── First page ──────────────────────────────────────────────────────────

#[test]
fn first_page_returns_prefix() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 10);

    let page = client.get_pending_settlements_page(&0, &4);
    assert_eq!(page.len(), 4);
    for i in 0..4 {
        assert_eq!(page.get(i).unwrap().id, (i as u64) + 1);
        assert_eq!(page.get(i).unwrap().status, SettlementStatus::Pending);
    }
}

#[test]
fn first_page_exact_fit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 3);

    let page = client.get_pending_settlements_page(&0, &3);
    assert_eq!(page.len(), 3);
}

// ── Last page with fewer items than limit ────────────────────────────────

#[test]
fn last_page_fewer_than_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 5);

    let page = client.get_pending_settlements_page(&3, &5);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().id, 4);
    assert_eq!(page.get(1).unwrap().id, 5);
}

#[test]
fn last_page_single_item() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 3);

    let page = client.get_pending_settlements_page(&2, &10);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap().id, 3);
}

// ── Empty result when start exceeds count ────────────────────────────────

#[test]
fn empty_page_when_start_exceeds_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 3);

    let page = client.get_pending_settlements_page(&10, &5);
    assert_eq!(page.len(), 0);
}

#[test]
fn empty_page_when_start_equals_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 5);

    let page = client.get_pending_settlements_page(&5, &5);
    assert_eq!(page.len(), 0);
}

#[test]
fn empty_contract_returns_empty_page() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 0);

    let page = client.get_pending_settlements_page(&0, &10);
    assert_eq!(page.len(), 0);
}

// ── Interspersed executed and pending settlements ────────────────────────

#[test]
fn interspersed_executed_skipped_first_page() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_interspersed_executions(&env, &[true, false, true, false, true]);

    let page = client.get_pending_settlements_page(&0, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().id, 2);
    assert_eq!(page.get(0).unwrap().status, SettlementStatus::Pending);
    assert_eq!(page.get(1).unwrap().id, 4);
    assert_eq!(page.get(1).unwrap().status, SettlementStatus::Pending);
}

#[test]
fn interspersed_executed_skipped_mid_page() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_interspersed_executions(&env, &[true, false, true, false, true]);

    // skip 1 pending (id 2), take 2 → ids 4
    let page = client.get_pending_settlements_page(&1, &2);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap().id, 4);
}

#[test]
fn interspersed_executed_skipped_trailing_executed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_interspersed_executions(&env, &[false, true, false, true, false]);

    let page = client.get_pending_settlements_page(&0, &10);
    assert_eq!(page.len(), 3);
    assert_eq!(page.get(0).unwrap().id, 1);
    assert_eq!(page.get(1).unwrap().id, 3);
    assert_eq!(page.get(2).unwrap().id, 5);
}

#[test]
fn interspersed_all_executed_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_interspersed_executions(&env, &[true, true, true]);

    let page = client.get_pending_settlements_page(&0, &10);
    assert_eq!(page.len(), 0);
}

#[test]
fn interspersed_start_exceeds_pending_not_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_interspersed_executions(&env, &[false, true, false, true, false]);

    // 3 pending (ids 1,3,5), request start=3 → 0 remaining
    let page = client.get_pending_settlements_page(&3, &5);
    assert_eq!(page.len(), 0);
}

// ── Stress: extreme start/limit values ────────────────────────────────────
//
// get_pending_settlements_page, this test file, and the future
// export_snapshot_page (#47) all need boundary coverage at very large
// start/limit values, not just "one past the end". The pagination loop is
// internally bounded by the actual settlement count rather than by `limit`
// (see contracts/treasury/src/settlements.rs), so these values must return
// cleanly rather than panicking or scanning proportionally to the input.

#[test]
fn start_far_beyond_total_count_returns_empty_not_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 5);

    let page = client.get_pending_settlements_page(&u64::MAX, &10);
    assert_eq!(page.len(), 0);
}

#[test]
fn start_and_limit_both_at_u64_max_returns_empty_not_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 5);

    let page = client.get_pending_settlements_page(&u64::MAX, &u64::MAX);
    assert_eq!(page.len(), 0);
}

#[test]
fn limit_of_u64_max_returns_all_pending_and_stays_under_instruction_budget() {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let (client, _) = setup_with_settlements(&env, 50);

    env.cost_estimate().budget().reset_tracker();
    let page = client.get_pending_settlements_page(&0, &u64::MAX);
    let instructions = env.cost_estimate().budget().cpu_instruction_cost();

    assert_eq!(page.len(), 50);
    assert!(
        instructions <= PAGE_INSTRUCTION_BUDGET,
        "get_pending_settlements_page(0, u64::MAX) over 50 settlements used \
         {instructions} instructions, expected <= {PAGE_INSTRUCTION_BUDGET}"
    );
}

#[test]
fn mid_range_start_with_limit_of_u64_max_returns_remaining_suffix() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 10);

    let page = client.get_pending_settlements_page(&7, &u64::MAX);
    assert_eq!(page.len(), 3);
    assert_eq!(page.get(0).unwrap().id, 8);
    assert_eq!(page.get(1).unwrap().id, 9);
    assert_eq!(page.get(2).unwrap().id, 10);
}
