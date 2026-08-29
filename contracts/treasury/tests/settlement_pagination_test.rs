use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

fn setup_with_settlements(env: &Env, n: u64) -> (TreasuryContractClient<'_>, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    // threshold high enough that no settlement executes automatically
    client.initialize(&admin, &100);
    for _ in 0..n {
        let merchant = Address::generate(env);
        client.propose_settlement(&admin, &merchant, &1_000_000);
    }
    (client, admin)
}

#[test]
fn empty_page_when_start_exceeds_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 3);

    let page = client.get_pending_settlements_page(&10, &5);
    assert_eq!(page.len(), 0);
}

#[test]
fn mid_list_page_returns_correct_slice() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 5);

    // skip first 2, take 2 → ids 3 and 4
    let page = client.get_pending_settlements_page(&2, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().id, 3);
    assert_eq!(page.get(1).unwrap().id, 4);
}

#[test]
fn last_page_returns_remaining_items() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 5);

    // skip first 3, limit 5 → only 2 remain: ids 4 and 5
    let page = client.get_pending_settlements_page(&3, &5);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().id, 4);
    assert_eq!(page.get(1).unwrap().id, 5);
}

#[test]
fn overflow_limit_returns_all_items() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 4);

    // limit larger than total — returns everything
    let page = client.get_pending_settlements_page(&0, &100);
    assert_eq!(page.len(), 4);
    for i in 0..4u32 {
        assert_eq!(page.get(i).unwrap().id, (i as u64) + 1);
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

#[test]
fn empty_contract_returns_empty_page() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_with_settlements(&env, 0);

    let page = client.get_pending_settlements_page(&0, &10);
    assert_eq!(page.len(), 0);
}
