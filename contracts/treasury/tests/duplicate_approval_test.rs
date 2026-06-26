use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient, Address, u64) {
    let admin = Address::generate(env);
    let merchant = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &2);
    let sid = client.propose_settlement(&admin, &merchant, &5_000_000);
    (client, admin, sid)
}

#[test]
fn duplicate_approval_does_not_grow_approvals() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sid) = setup(&env);

    let s1 = client.approve_settlement(&admin, &sid);
    let count_after_first = s1.approvals.len();

    let s2 = client.approve_settlement(&admin, &sid);
    assert_eq!(s2.approvals.len(), count_after_first);
}

#[test]
fn duplicate_approval_preserves_settlement_fields() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sid) = setup(&env);

    let s1 = client.approve_settlement(&admin, &sid);
    let s2 = client.approve_settlement(&admin, &sid);

    assert_eq!(s1.amount, s2.amount);
    assert_eq!(s1.id, s2.id);
    assert_eq!(s1.merchant_address, s2.merchant_address);
    assert_eq!(s2.status, SettlementStatus::Pending);
}

#[test]
fn independent_signer_still_appends_after_proposer_duplicate() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sid) = setup(&env);

    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    client.approve_settlement(&admin, &sid);
    let s = client.approve_settlement(&backup, &sid);

    assert_eq!(s.approvals.len(), 2);
    assert_eq!(s.status, SettlementStatus::Pending);
}

// #34 — weighted signer: duplicate approval must not double-count approval_weight
#[test]
fn weighted_signer_duplicate_approval_does_not_double_count_weight() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &3);

    // Register a signer with weight 5 — above-threshold weight to make the
    // double-count scenario meaningful.
    let heavy = Address::generate(&env);
    client.set_signer(&admin, &heavy, &5);

    let sid = client.propose_settlement(&heavy, &merchant, &5_000_000);

    // First approval records weight 5.
    let s1 = client.approve_settlement(&heavy, &sid);
    let weight_after_first = s1.approval_weight;

    // Second approval from the same signer must not increment the weight again.
    let s2 = client.approve_settlement(&heavy, &sid);
    assert_eq!(s2.approval_weight, weight_after_first,
        "approval_weight must not increase on duplicate approve from same signer");
    assert_eq!(s2.approvals.len(), 1,
        "approvals vec must not grow on duplicate approve");
}
