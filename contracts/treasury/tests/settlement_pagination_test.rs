use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup_with_settlements(env: &Env, n: u64) -> (TreasuryContractClient, Address) {
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

fn setup_with_interspersed_executions(
    env: &Env,
    pattern: &[bool],
) -> (TreasuryContractClient, Address) {
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
